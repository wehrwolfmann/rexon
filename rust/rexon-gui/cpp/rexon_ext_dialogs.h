// Dialogs of the QML process: choosing a file, a "yes/no" question,
// choosing a font, opening a directory in the file manager and a page in
// the browser.
//
// Warning: there is ONE dialog shim in the project — as with the gadget
// shim (see build.rs). It started with the "Extensions" tab (hence the
// historical `rexon_ext_` prefix), but "Settings" use it too: choosing a
// Windows theme file and choosing a system font. A second file just like
// it next to this one would mean a second copy of the nested event loop and
// a second set of `parentWindow` traps (rule #12).
//
// WHY THIS IS A SEPARATE SHIM AND NOT A DIRECT PORT OF QFileDialog.
//
// In Python these three calls were one-liners from QtWidgets:
// `QFileDialog.getOpenFileName`, `QMessageBox.question`,
// `QDesktopServices.openUrl`. In a QML process none of them can be used
// that way:
//
//   1. QFileDialog and QMessageBox are WIDGETS. They need a `QApplication`,
//      while the Rust window host brings up a `QGuiApplication` (there are no
//      widgets in the process at all — that is the whole point of leaving
//      PySide6). Creating a QWidget without a QApplication crashes the
//      process rather than drawing a window.
//   2. The standard QML replacement is `QtQuick.Dialogs.FileDialog` and
//      `MessageDialog`. They are ASYNCHRONOUS: `open()` returns at once, and
//      the answer arrives with the `accepted`/`rejected` signal. They have
//      no public C++ class — only a QML type.
//   3. And the page waits for a SYNCHRONOUS answer:
//      `const path = actions.installDialog()` and
//      `if (!actions.confirm(...)) return;`. Rewriting it with callbacks
//      would mean two control flows — one for the Python host, another for
//      the Rust host — that is, putting into the page exactly the second
//      copy of the rules that drifts apart silently (rule #12).
//
// Hence the solution: the dialog is created from QML text through
// `QQmlComponent`, and waiting for the answer is done by a NESTED
// `QEventLoop` — exactly what `QDialog::exec()` does for widgets. From
// outside the call is synchronous, inside it is an honest asynchronous QML
// dialog which on Wayland goes to the desktop portal and looks native. The
// technique is not new in the tree: the wallpaper tab's transport waits
// for the daemon's answer with the same nested loop
// (`rexon_we_socket.cpp`).
//
// The dialogs deliberately have no QML file of their own: thirty lines of
// markup that nobody looks at would be, in the module's static resource,
// just another reason to drift apart from their only call.
#pragma once

#include <cstdint>

#include <QtCore/QString>

// ── choosing a file ───────────────────────────────────────────────────────

/// Window for choosing one file. Returns a LOCAL path or an empty string if
/// the user changed their mind (or the dialog did not come up).
///
/// `filter` is a filter in Qt Widgets notation ("Name (*.ext);;Other (*)"):
/// the same text the widget tab showed, coming from the core. Splitting it
/// into the `nameFilters` list happens here, so that the core does not get
/// a second notation.
QString rexon_ext_pick_file(const QString& title, const QString& filter,
                            const QString& folder);

// ── saving a file ─────────────────────────────────────────────────────────

/// File save window with the path `path` filled in (directory and name).
/// Returns a LOCAL path or an empty string if the user changed their mind
/// (or the dialog did not come up). `filter` is as for `rexon_ext_pick_file`.
QString rexon_ext_save_file(const QString& title, const QString& filter,
                            const QString& path);

// ── a "yes/no" question ───────────────────────────────────────────────────

/// Confirmation of an irreversible action. `true` means the user answered
/// "Yes".
///
/// Failing to bring up the dialog means `false`: a silent "yes" would delete
/// an extension without asking (rule #13 — foolproofing).
/// `skin` is the skin colours and the button labels, separated by U+001F:
/// background, card, text, accent, border, "Yes", "No"
/// (`extensions_actions::confirm_skin`).
bool rexon_ext_confirm(const QString& title, const QString& text, const QString& skin);

// ── a directory in the file manager ───────────────────────────────────────

/// `QDesktopServices::openUrl(QUrl::fromLocalFile(path))`. It is in QtGui,
/// no widgets are needed; cxx-qt-lib 0.9 has no binding for it.
bool rexon_ext_open_folder(const QString& path);

// ── choosing a font ───────────────────────────────────────────────────────

/// Font chooser window. Returns the chosen font as a string
/// "Family,size,weight,italic" — it is parsed by the core
/// (`qt_settings::parse_font_spec`, `parse_font_face`). So the shim has no
/// parsing of its own at all.
///
/// `family`/`size` — what to show as selected on opening (an empty family
/// means "no font set", size `0` means "it was not in the setting");
/// `weight` (the Qt 6 scale, `0` — not set) and `italic` — the current
/// style.
///
/// `outcome` — WHAT EXACTLY happened, and it cannot be done without: "the
/// user changed their mind" and "the window did not come up" give the same
/// empty string, but the user must be told different things (rule #13):
///   *  1 — a font was chosen (the string is non-empty);
///   *  0 — the user closed the window without choosing;
///   * -1 — the window did not come up at all (the QML did not compile, no
///     engine).
QString rexon_ext_pick_font(const QString& title, const QString& family,
                            std::int32_t size, std::int32_t weight,
                            bool italic, std::int32_t& outcome);

// ── an item page in the browser ───────────────────────────────────────────

/// `QDesktopServices::openUrl(QUrl(url))` — a SITE address, not a local
/// path. An empty or schemeless address gives `false`: a silent button
/// reads as a breakage, and the tab must learn about the refusal.
bool rexon_ext_open_url(const QString& url);
