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

// Учёт настроенных окон. Нужен не для красоты: LayerShellQt::Window::get()
// СОЗДАЁТ объект слоя, если его ещё нет, поэтому «прочитать текущее
// состояние» через get() нельзя — чтение молча превратило бы обычное окно в
// поверхность слоя. Поэтому читаем только то, что сами и настроили.
//
// QPointer, а не сырой указатель: объект слоя умирает вместе с окном, и
// запись обязана обнулиться сама, иначе повторное использование адреса дало
// бы ложное «окно уже в слое».
QHash<QObject *, QPointer<LayerShellQt::Window>> &registry()
{
    static QHash<QObject *, QPointer<LayerShellQt::Window>> map;
    return map;
}

/// Слой уже настроенного окна либо nullptr. Ничего не создаёт.
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
    // Имя платформы Qt: «wayland» либо «wayland-egl» — под X11 слоя нет
    // вообще, там путь обоев другой (xprop _NET_WM_WINDOW_TYPE_DESKTOP).
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

    // get() возвращает уже существующий объект слоя, если окно настраивали
    // раньше, — повторный вызов безопасен и просто переписывает свойства.
    LayerShellQt::Window *shell = LayerShellQt::Window::get(w);
    if (shell == nullptr) {
        return false;
    }

    shell->setLayer(static_cast<LayerShellQt::Window::Layer>(layer));
    shell->setAnchors(static_cast<LayerShellQt::Window::Anchors>(anchors));
    // ⚠️ Имя метода — setExclusiveZone, а имя свойства — exclusionZone.
    // Расхождение в самой библиотеке KDE; в QML доступно только второе, и
    // на этом уже спотыкались (запись в каноне проекта). Здесь оба вида
    // рядом, чтобы больше не искать.
    shell->setExclusiveZone(exclusion_zone);
    shell->setKeyboardInteractivity(
        static_cast<LayerShellQt::Window::KeyboardInteractivity>(keyboard));
    if (!scope.isEmpty()) {
        shell->setScope(scope);
    }

    if (!registry().contains(window)) {
        // Запись живёт ровно столько, сколько окно. Без этого карта росла бы
        // на каждое окно обоев за сеанс — для видеозаставки и пересозданий
        // окна при смене монитора это настоящая утечка, а не теория.
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
    // ⚠️ known(), а не get(): get() СОЗДАЛ бы поверхность слоя из обычного
    // окна. Прицеливать можно только то, что уже настроено как слой.
    LayerShellQt::Window *shell = known(window);
    if (shell == nullptr) {
        return false;
    }
    QScreen *screen = w->screen();
    if (screen == nullptr) {
        return false;
    }
    // setScreen сам снимает «пусть решает композитор»
    // (wantsToBeOnActiveScreen), поэтому второго вызова не нужно.
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
    // -1 — законное значение зоны обоев, поэтому «окно не настроено» здесь
    // не -1, а заведомо невозможное число.
    return shell == nullptr ? INT32_MIN : static_cast<std::int32_t>(shell->exclusionZone());
}

std::int32_t rexon_layer_shell_keyboard(QObject *window)
{
    LayerShellQt::Window *shell = known(window);
    return shell == nullptr ? -1 : static_cast<std::int32_t>(shell->keyboardInteractivity());
}
