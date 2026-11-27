// LayerShellQt shim: turning an ordinary QWindow into a wlr-layer-shell
// surface.
//
// Why a shim: LayerShellQt is a KDE C++ library (the layer-shell-qt
// package); it has no Rust bindings and will not have any. From Rust it
// can be seen only the same way cxx-qt sees its own QObjects — through a
// thin C++ file. The model factory (cpp/rexon_gui_factory.cpp) is made
// with exactly the same technique; this is a second shim of the same
// kind, not a new architecture.
//
// What the shim does NOT do: it creates no windows, does not decide what
// the layer should be and does not know the word "wallpaper". It executes
// what Rust told it — the decision "wallpaper layer = Background + four
// anchors + zone -1" lives in src/layer_shell.rs as a single instance
// (rule #12: a shared engine, not a copy per wallpaper kind).
//
// Warning: the order is mandatory: the layer properties are set BEFORE the
// window is first shown. LayerShellQt replaces the shell integration at
// the moment the platform window is created; if the window is already
// shown, it has already become an ordinary xdg_toplevel, and a late call
// will not move it into the layer. The test bench catches this violation
// by the protocol — see src/layer_shell.rs.
#pragma once

#include <cstdint>

#include <QString>

class QObject;

/// Whether a wallpaper layer is possible in the current session: the Qt
/// platform is Wayland. Whether the compositor has the wlr-layer-shell
/// protocol itself cannot be seen from here (Mutter/Muffin do not provide
/// it) — that is found out only when the window is shown.
bool rexon_layer_shell_available();

/// Move the window into the layer. `window` must be a QWindow (usually the
/// root object of a QML Window component), otherwise false is returned.
///
/// `layer` is LayerShellQt::Window::Layer (0 Background, 1 Bottom, 2 Top,
/// 3 Overlay); `anchors` is an Anchor bit mask (1 top, 2 bottom, 4 left,
/// 8 right); `exclusion_zone` is the very "exclusion zone" (-1 = do not push
/// panels away and reserve no space oneself, the wallpaper value);
/// `keyboard` is KeyboardInteractivity (0 None, 1 Exclusive, 2 OnDemand).
/// `scope` is the surface identifier for the compositor (an empty string =
/// leave the LayerShellQt default alone).
bool rexon_layer_shell_configure(QObject *window,
                                 std::int32_t layer,
                                 std::int32_t anchors,
                                 std::int32_t exclusion_zone,
                                 std::int32_t keyboard,
                                 const QString &scope);

/// Aim the layer surface at THE SAME screen the window itself is on
/// (`QWindow::screen()`), and clear "let the compositor decide".
///
/// Why a separate call. Without it LayerShellQt picks the screen itself:
/// with `screen() == nullptr` and `wantsToBeOnActiveScreen() == false` it
/// reads `QWindow::screen()`, but both defaults are its internal business,
/// and a wallpaper on the second monitor must not depend on whether KDE
/// changes them in the next version. Here the surface output is named
/// explicitly.
///
/// Call AFTER `rexon_layer_shell_configure` and BEFORE the window is first
/// shown: the output goes into the `get_layer_surface` request, and after
/// showing it is too late to change it. Returns false if the window is not
/// a QWindow, no layer was set on it or the window has no screen.
bool rexon_layer_shell_pin_to_window_screen(QObject *window);

// Reading back — for the self-test. Return -1 if the object is not a
// QWindow.
//
// Warning: this reads OUR OWN state in LayerShellQt, not the compositor's
// answer: it only proves that the setters were not silently ignored. That
// the window really became a background surface is proven by the Wayland
// protocol in the test bench.
std::int32_t rexon_layer_shell_layer(QObject *window);
std::int32_t rexon_layer_shell_anchors(QObject *window);
std::int32_t rexon_layer_shell_exclusion_zone(QObject *window);
std::int32_t rexon_layer_shell_keyboard(QObject *window);
/// Name of the screen the layer surface is aimed at; empty means no screen
/// is named (the window is not in a layer, or the compositor chooses the
/// output).
QString rexon_layer_shell_screen_name(QObject *window);
