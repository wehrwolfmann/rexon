// Qt primitives of the Rexon system tray icon.
//
// There is NOT A SINGLE decision or label here: what the items are, what
// they are called, whether they are enabled and what a click does is known
// to the core (rexon-core::tray) and passed on by the executor
// (rust/rexon-gui/src/tray.rs), while this file only builds the
// QSystemTrayIcon with a QMenu and pokes the window.
//
// Why a shim and not pure Rust: QSystemTrayIcon, QMenu, QAction and
// intercepting the window close live in QtWidgets/QtGui, and cxx-qt-lib has
// no bindings for them, neither in 0.9 nor in 0.10. The same technique and
// the same boundary as for the preview placeholder (rexon_local_preview.h).
// QApplication ITSELF, however, does exist in Rust — in a neighbouring crate
// of the same release (cxx-qt-lib-extras), and without it a process with a
// QML window could not have an icon: Qt does not create widgets (QMenu)
// without a QApplication at all.
//
// Warning: the shim looks for the window in TWO ways and keeps both: the
// product's widget QMainWindow on PySide6 and the QQuickWindow of the Rust
// main window binary, where there is not a single widget in the process.
#pragma once

#include <cstdint>

extern "C" {

// ── callbacks into Rust ───────────────────────────────────────────────────
//
// They are set once before the icon is created. With a setter rather than
// a direct `extern` on a Rust symbol — as for launching the wallpaper tab's
// daemon (rexon_we_do_set_launch_hook in rexon_we_singletons.h): the shim
// stays buildable on its own.

// The menu item with this number was pressed.
typedef void (*RexonTrayClick)(int32_t id);
// The menu is about to open — the right moment to rebuild its contents.
typedef void (*RexonTrayVoid)();
// The main window is closing. true means swallow the close and hide the
// window.
typedef bool (*RexonTrayAsk)();

void rexon_tray_set_hooks(RexonTrayClick on_click, RexonTrayVoid on_menu_show,
                          RexonTrayVoid on_activate, RexonTrayAsk on_close);

// ── icon ──────────────────────────────────────────────────────────────────

// Whether the session has a tray to sit in.
bool rexon_tray_available();

// Create the icon. `icon_file` is the path to a picture ("" — take it from
// the theme), `icon_theme` is the icon name in the theme. A repeated call
// does nothing. false means the icon could not be created (including when
// the process has no QApplication: a QMenu is not created without it, and
// the attempt would kill the process).
bool rexon_tray_create(const char *icon_file, const char *icon_theme,
                       const char *tip_title, const char *tip_body);

// Icon of all windows of the process (`QGuiApplication::setWindowIcon`) —
// the same picture as the tray icon: from the theme, otherwise from the
// file. On X11 without it the window has no `_NET_WM_ICON`, and the task
// bar draws a generic icon instead of ours. false means neither the theme
// nor the file gave a picture. Call after the application is created and
// before the first window.
bool rexon_set_app_window_icon(const char *icon_file, const char *icon_theme);

// Rebuild the whole menu: begin → add… → end.
//
// `kind`: 0 — an action, 1 — a separator, 2 — a check box, 3 — a radio
// item in a group, 4 — start of a submenu, 5 — its end. The values are set
// by the core (rexon_core::tray::ItemKind); here they are just numbers.
void rexon_tray_menu_begin();
void rexon_tray_menu_add(int32_t id, int32_t kind, const char *label,
                         bool checked, bool enabled, bool is_default);
void rexon_tray_menu_end();

// A pop-up message from the icon.
void rexon_tray_notify(const char *title, const char *body);

// ── main window ───────────────────────────────────────────────────────────
//
// All four doors can be called FROM ANY THREAD: the shim itself moves the
// work with the window into the GUI thread and waits for the real answer.
// This is needed by the second-launch listener (rexon_core::single_instance)
// — it lives in its own thread, and without the move raising the window
// was impossible in principle.

// Remember the application's main window and intercept its close. false
// means no window was found (so there is nothing to hide either).
bool rexon_tray_guard_window();

// Raise the window and give it focus. false means there is no window.
bool rexon_tray_raise_window();

/// Capture the main window (`QQuickWindow::grabWindow`) into the PNG
/// `path`. Only from the GUI thread. Tour mode `--tour`; false means there
/// is no window or the file was not written.
bool rexon_tray_grab_window(const char *path);

// The QML main window is the root of the engine `engine`
// (`QQmlApplicationEngine *`). It is remembered BEFORE the window is
// shown: there are several hidden top-level windows in the process, and
// guessing among them is not allowed. false means there is no root window.
// Call from the GUI thread.
bool rexon_tray_adopt_qml_window(void *engine);

// Create the platform window of the main window without showing it.
// Returns its number (0 — no window) and in `rgb` the window background
// colour 0xRRGGBB. Call from the GUI thread.
uint64_t rexon_tray_window_native(uint32_t *rgb);

// Hide the window.
bool rexon_tray_hide_window();

// Switch the main window to the section with this number. false means the
// section strip was not found or there is no such number.
bool rexon_tray_select_tab(int32_t index);

// Quit the application for real (the close guard is removed).
void rexon_tray_quit();

// Open a section (and a sub-page) of the QML main window. false means
// there is no window, no such section, or the window is widget-based.
bool rexon_tray_open_section(const char *id, const char *subpage);

// Schedule quitting in delay_ms and return AT ONCE (handing over to the
// owner: the answer "leaving" must go out before quitting).
void rexon_tray_request_quit(int32_t delay_ms);

// ── wallpaper daemon on the bus ───────────────────────────────────────────

// Whether the daemon is visible on the session bus.
bool rexon_tray_daemon_alive();

// Daemon method without arguments / with one argument. false means the
// call failed.
bool rexon_tray_daemon_call(const char *method);
bool rexon_tray_daemon_call_u32(const char *method, uint32_t arg);
bool rexon_tray_daemon_call_bool(const char *method, bool arg);

// Daemon property: the string is copied into `buf` (NUL-terminated).
// Returns the number of bytes written without the NUL; -1 means the
// property is unavailable.
int32_t rexon_tray_daemon_prop(const char *name, char *buf, int32_t buf_len);
}
