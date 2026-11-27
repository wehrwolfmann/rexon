// ActionJournal — the journal of the user's actions and the window
// heartbeat.
//
// WHY. A live case on 15.11.2026: the window froze, the desktop offered
// "Close", the process was killed — and the log had neither what was
// pressed nor where the main thread was.
//
// Actions are written at ONE point for the whole application — by the
// input filter (the `rexon_install_input_journal` shim): any button, check
// box, list row, menu item, store card with a `TapHandler`/`MouseArea` of
// any tab, including the vendored wallpaper tab, gets into the journal by
// itself, without a line in the page. QML is left only with what the
// filter cannot know:
//   * a window section change (`section`, the section strip of
//     `MainWindow.qml`);
//   * a text field (`typing`/`entered`, the shared `RexonTextField`
//     control): key events do not tell what was typed, and a password
//     must not be written.
//
// There is no logic here (the crate rule): what counts as an action, the
// line texts, the current action and the watchdog belong to the core
// (`rexon_core::ui_watch`).
//
// The self-test and test benches do not set up the journal
// (`applog::is_active`), and the singleton stays silent in them: action
// lines must not get mixed into the output that the runs parse.

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
        #[qml_singleton]
        type ActionJournal = super::ActionJournalRust;

        /// The user is typing text into field `item` with placeholder `field`;
        /// `secret` is a password field (its `value` arrives empty).
        #[qinvokable]
        fn typing(self: &ActionJournal, item: &QVariant, field: &QString, value: &QString, secret: bool);

        /// Input into field `item` is finished (Enter or focus loss). → the number
        /// for `finish`, 0 means there was no line.
        #[qinvokable]
        fn entered(self: &ActionJournal, item: &QVariant, field: &QString, value: &QString, secret: bool) -> f64;

        /// The handler of action `id` returned control to the event loop.
        #[qinvokable]
        fn finish(self: &ActionJournal, id: f64);

        /// Window section `id` was opened.
        #[qinvokable]
        fn section(self: &ActionJournal, id: &QString);
    }
}

use std::os::raw::{c_char, c_int, c_void};

use rexon_core::applog;
use rexon_core::ui_watch;

extern "C" {
    fn rexon_start_ui_heartbeat(ms: c_int, tick: extern "C" fn()) -> bool;
    fn rexon_qml_ancestry(variant: *const c_void, buf: *mut c_char, len: c_int) -> c_int;
    fn rexon_install_input_journal(
        begin: extern "C" fn(*const c_char, c_int, c_int) -> u64,
        finish: extern "C" fn(u64),
    ) -> bool;
}

/// A press from the input filter (the shim) → the core. 0 means not an
/// action.
extern "C" fn input_begin(desc: *const c_char, len: c_int, via_key: c_int) -> u64 {
    if desc.is_null() || len <= 0 || !applog::is_active() {
        return 0;
    }
    let bytes = unsafe { std::slice::from_raw_parts(desc.cast::<u8>(), len as usize) };
    ui_watch::begin_input(&String::from_utf8_lossy(bytes), via_key != 0)
}

extern "C" fn input_finish(id: u64) {
    ui_watch::finish_deed(id);
}

#[derive(Default)]
pub struct ActionJournalRust;

/// Chain of ancestor classes of an object from QML (see the shim).
fn ancestry(item: &cxx_qt_lib::QVariant) -> String {
    let mut buf = vec![0u8; 4096];
    let n = unsafe {
        rexon_qml_ancestry((item as *const cxx_qt_lib::QVariant).cast(), buf.as_mut_ptr().cast(), buf.len() as c_int)
    };
    buf.truncate(n.max(0) as usize);
    String::from_utf8_lossy(&buf).into_owned()
}

fn area_of(item: &cxx_qt_lib::QVariant) -> String {
    ui_watch::area_from_chain(&ui_watch::current_section(), &ancestry(item))
}

impl qobject::ActionJournal {
    fn typing(&self, item: &cxx_qt_lib::QVariant, field: &cxx_qt_lib::QString, value: &cxx_qt_lib::QString, secret: bool) {
        if !applog::is_active() {
            return;
        }
        let value = if secret { String::new() } else { value.to_string() };
        ui_watch::note_typing(&area_of(item), &field.to_string(), &value, secret);
    }

    fn entered(
        &self,
        item: &cxx_qt_lib::QVariant,
        field: &cxx_qt_lib::QString,
        value: &cxx_qt_lib::QString,
        secret: bool,
    ) -> f64 {
        if !applog::is_active() || secret {
            return 0.0;
        }
        ui_watch::note_entered(&area_of(item), &field.to_string(), &value.to_string(), false) as f64
    }

    fn finish(&self, id: f64) {
        if id >= 1.0 {
            ui_watch::finish_deed(id as u64);
        }
    }

    fn section(&self, id: &cxx_qt_lib::QString) {
        if applog::is_active() {
            ui_watch::note_section(&id.to_string());
        }
    }
}

/// Set up the main thread watchdog, its heartbeat and the action journal's
/// input filter. Call from the GUI thread after the application is created
/// and before the event loop.
pub fn install_main_watchdog() {
    if !unsafe { rexon_install_input_journal(input_begin, input_finish) } {
        applog::write(applog::Level::Warn, ui_watch::AREA, "журнал действий не встал: приложения Qt ещё нет");
    }
    ui_watch::install_main_watchdog();
    let ok = unsafe { rexon_start_ui_heartbeat(ui_watch::HEARTBEAT_MS as c_int, ui_watch::main_heartbeat) };
    if !ok {
        applog::write(applog::Level::Warn, ui_watch::AREA, "пульс окна не встал: приложения Qt ещё нет");
    }
}
