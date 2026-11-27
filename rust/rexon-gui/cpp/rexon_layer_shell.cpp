#include "rexon_layer_shell.h"

#include <QGuiApplication>
#include <QHash>
#include <QPointer>
#include <QString>
#include <QScreen>
#include <QWindow>

#include <LayerShellQt/Window>

namespace
{

// Bookkeeping of configured windows. It is not there for looks:
// LayerShellQt::Window::get() CREATES a layer object if there is none yet,
// so "reading the current state" through get() is not possible — reading
// would silently turn an ordinary window into a layer surface. So we read
// only what we configured ourselves.
//
// A QPointer rather than a raw pointer: the layer object dies together
// with the window, and the entry must reset itself, otherwise reuse of
// the address would give a false "the window is already in the layer".
QHash<QObject *, QPointer<LayerShellQt::Window>> &registry()
{
    static QHash<QObject *, QPointer<LayerShellQt::Window>> map;
    return map;
}

/// The layer of an already configured window, or nullptr. Creates nothing.
LayerShellQt::Window *known(QObject *window)
{
    const auto it = registry().constFind(window);
    if (it == registry().constEnd()) {
        return nullptr;
    }
    return it->data();
}

} // namespace

bool rexon_layer_shell_available()
{
    // Qt platform name: "wayland" or "wayland-egl" — under X11 there is no
    // layer at all, the wallpaper path is different there (xprop
    // _NET_WM_WINDOW_TYPE_DESKTOP).
    if (QGuiApplication::instance() == nullptr) {
        return false;
    }
    return QGuiApplication::platformName().startsWith(QStringLiteral("wayland"));
}

bool rexon_layer_shell_configure(QObject *window,
                                 std::int32_t layer,
                                 std::int32_t anchors,
                                 std::int32_t exclusion_zone,
                                 std::int32_t keyboard,
                                 const QString &scope)
{
    auto *w = qobject_cast<QWindow *>(window);
    if (w == nullptr) {
        return false;
    }

    // get() returns the already existing layer object if the window was
    // configured before — a repeated call is safe and simply rewrites the
    // properties.
    LayerShellQt::Window *shell = LayerShellQt::Window::get(w);
    if (shell == nullptr) {
        return false;
    }

    shell->setLayer(static_cast<LayerShellQt::Window::Layer>(layer));
    shell->setAnchors(static_cast<LayerShellQt::Window::Anchors>(anchors));
    // Warning: the method name is setExclusiveZone, while the property name is
    // exclusionZone. The discrepancy is in the KDE library itself; only the
    // latter is available in QML, and people have already tripped over this
    // (an entry in the project canon). Both forms are side by side here so
    // nobody has to look again.
    shell->setExclusiveZone(exclusion_zone);
    shell->setKeyboardInteractivity(
        static_cast<LayerShellQt::Window::KeyboardInteractivity>(keyboard));
    if (!scope.isEmpty()) {
        shell->setScope(scope);
    }

    if (!registry().contains(window)) {
        // The entry lives exactly as long as the window. Without this the map
        // would grow by every wallpaper window over a session — for the video
        // splash and window recreation on monitor changes this is a real leak,
        // not a theory.
        QObject::connect(w, &QObject::destroyed, w, [window]() { registry().remove(window); });
    }
    registry().insert(window, QPointer<LayerShellQt::Window>(shell));
    return true;
}

bool rexon_layer_shell_pin_to_window_screen(QObject *window)
{
    auto *w = qobject_cast<QWindow *>(window);
    if (w == nullptr) {
        return false;
    }
    // Warning: known(), not get(): get() WOULD CREATE a layer surface from an
    // ordinary window. Only what is already configured as a layer can be
    // targeted.
    LayerShellQt::Window *shell = known(window);
    if (shell == nullptr) {
        return false;
    }
    QScreen *screen = w->screen();
    if (screen == nullptr) {
        return false;
    }
    // setScreen itself clears "let the compositor decide"
    // (wantsToBeOnActiveScreen), so a second call is not needed.
    shell->setScreen(screen);
    return shell->screen() == screen;
}

QString rexon_layer_shell_screen_name(QObject *window)
{
    LayerShellQt::Window *shell = known(window);
    if (shell == nullptr) {
        return QString();
    }
    QScreen *screen = shell->screen();
    return screen == nullptr ? QString() : screen->name();
}

std::int32_t rexon_layer_shell_layer(QObject *window)
{
    LayerShellQt::Window *shell = known(window);
    return shell == nullptr ? -1 : static_cast<std::int32_t>(shell->layer());
}

std::int32_t rexon_layer_shell_anchors(QObject *window)
{
    LayerShellQt::Window *shell = known(window);
    return shell == nullptr ? -1 : static_cast<std::int32_t>(shell->anchors().toInt());
}

std::int32_t rexon_layer_shell_exclusion_zone(QObject *window)
{
    LayerShellQt::Window *shell = known(window);
    // -1 is a legitimate value of the wallpaper zone, so "the window is not
    // configured" here is not -1 but a number that is known to be impossible.
    return shell == nullptr ? INT32_MIN : static_cast<std::int32_t>(shell->exclusionZone());
}

std::int32_t rexon_layer_shell_keyboard(QObject *window)
{
    LayerShellQt::Window *shell = known(window);
    return shell == nullptr ? -1 : static_cast<std::int32_t>(shell->keyboardInteractivity());
}
