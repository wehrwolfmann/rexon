#include "rexon_tray.h"

#include "rexon_we_socket.h"

#include <QtCore/QEvent>
#include <QtCore/QMetaObject>
#include <QtCore/QPointer>
#include <QtCore/QString>
#include <QtCore/QThread>
#include <QtCore/QTimer>
#include <QtCore/QVariant>
#include <QtDBus/QDBusConnection>
#include <QtDBus/QDBusConnectionInterface>
#include <QtDBus/QDBusMessage>
#include <QtGui/QGuiApplication>
#include <QtGui/QIcon>
#include <QtGui/QColor>
#include <QtGui/QWindow>
#include <QtGui/QImage>
#include <QtQuick/QQuickWindow>
#include <QtQml/QQmlApplicationEngine>
#include <QtWidgets/QApplication>
#include <QtWidgets/QMainWindow>
#include <QtWidgets/QMenu>
#include <QtWidgets/QSystemTrayIcon>
#include <QtWidgets/QTabWidget>

#include <cstring>
#include <functional>
#include <vector>

namespace {

// Names of the wallpaper daemon — the same as the core's
// (rexon_core::waywallen_client). Repeated here as strings because passing
// them through the C boundary would be needed on every call; a divergence
// is caught by the core executor's test.
const char *const DAEMON_SERVICE = "org.waywallen.waywallen.Daemon";
const char *const DAEMON_PATH = "/org/waywallen/waywallen/Daemon";
const char *const DAEMON_IFACE = "org.waywallen.waywallen.Daemon1";

// The daemon answers instantly (a local bus); a second is more than
// enough, while an endless wait would freeze the menu together with the
// whole interface.
const int DAEMON_TIMEOUT_MS = 1000;

RexonTrayClick g_on_click = nullptr;
RexonTrayVoid g_on_menu_show = nullptr;
RexonTrayVoid g_on_activate = nullptr;
RexonTrayAsk g_on_close = nullptr;

QSystemTrayIcon *g_icon = nullptr;
QMenu *g_menu = nullptr;
// The main window in two forms, because the Rexon process has two
// different hosts and both are alive: the PySide6 product builds a widget
// `QMainWindow`, while the Rust main window binary builds an
// `ApplicationWindow` entirely in QML, with NOT A SINGLE widget in it. A
// single pointer to a QWidget served only the first: for the second
// `topLevelWidgets()` is always empty, so "the close button hides the
// window", raising the window on a second launch and switching to a
// section from the icon menu did not work "badly" but not at all.
QPointer<QWidget> g_window;
QPointer<QWindow> g_qwindow;
// Close guard — so as not to install a second one on the same window. A
// QPointer, not findChild: the guard has no Q_OBJECT (it does not need
// one), and findChild without Q_OBJECT does not compile at all.
QPointer<QObject> g_guard;
// Quitting has already been ordered: the close guard must let the last
// close through, otherwise the application will NEVER quit — it would lock
// its own door.
bool g_quitting = false;

// Stack of open submenus while building: the top is where items are being
// added right now.
std::vector<QMenu *> g_stack;

/// Close guard of the main window: the close button hides the window rather
/// than shutting Rexon down.
///
/// An event filter rather than overriding closeEvent: the window is not
/// created by this file — but by PySide6 or QML — and there is nothing to
/// inherit its class from here. The class needs no Q_OBJECT: eventFilter is
/// virtual, moc has nothing to do with it.
///
/// One guard for both window forms: `QEvent::Close` arrives both at a widget
/// and at a `QWindow` (for the QML window — the `QQuickWindow` itself), so
/// the filter works equally well for both, and hiding must use whichever
/// method the object has.
class CloseGuard : public QObject
{
public:
    using QObject::QObject;

protected:
    bool eventFilter(QObject *watched, QEvent *event) override
    {
        if (event->type() == QEvent::Close && !g_quitting && g_on_close != nullptr
            && g_on_close()) {
            event->ignore();
            if (auto *w = qobject_cast<QWidget *>(watched))
                w->hide();
            else if (auto *win = qobject_cast<QWindow *>(watched))
                win->hide();
            return true;
        }
        return QObject::eventFilter(watched, event);
    }
};

/// Whether a real QApplication (not just a QGuiApplication) lives in the
/// process.
///
/// The question is mandatory, and `QApplication::instance()` will NOT do
/// for it: it is inherited from QCoreApplication and in a process with a
/// QGuiApplication honestly returns the application object. The tray icon
/// trusted it and went on to create a `QMenu` — while Qt does not create a
/// widget without a QApplication at all and kills the process ("QWidget:
/// Cannot create a QWidget without QApplication"). That is, the answer "no
/// icon" turned into a crash of the whole program.
bool have_widgets_app()
{
    return qobject_cast<QApplication *>(QCoreApplication::instance()) != nullptr;
}

/// The main window of a widget application: the only top-level QMainWindow.
///
/// Tabs are QMainWindows too, but they are inserted as child widgets and do
/// not get into `topLevelWidgets()`.
QWidget *find_main_widget()
{
    if (!have_widgets_app())
        return nullptr;
    const auto tops = QApplication::topLevelWidgets();
    for (QWidget *w : tops) {
        if (w->isWindow() && w->inherits("QMainWindow"))
            return w;
    }
    return nullptr;
}

/// The main window of a QML application: a top-level window without an
/// owner window.
///
/// Exactly this selection, not "the first in the list": next to the main
/// window `topLevelWindows()` contains the windows of pop-up lists,
/// tooltips and menus (`Qt::Popup`, `Qt::ToolTip`), and they appear before
/// the main one as soon as the user opens a menu. Taking the first one
/// would mean one day hiding a drop-down list with the close button
/// instead of the window.
QWindow *find_main_qwindow()
{
    QWindow *hidden = nullptr;
    const auto windows = QGuiApplication::topLevelWindows();
    for (QWindow *w : windows) {
        if (w == nullptr || w->transientParent() != nullptr)
            continue;
        if (w->flags().testFlag(Qt::ToolTip) || w->flags().testFlag(Qt::Popup))
            continue;
        if (w->isVisible())
            return w;
        // A hidden window is a fallback answer: a `--tray` start hides the window
        // at once, and after that there would be nothing to raise.
        if (hidden == nullptr)
            hidden = w;
    }
    return hidden;
}

/// Find the main window if not looked up yet. false means there is no
/// window in the process.
///
/// The widget is asked first: in the PySide6 product process there is both
/// it and its `QWindow`, but the widget has a `close()` with the drain of
/// the tabs' workers, while the `QWindow` does not.
bool resolve_window()
{
    if (!g_window.isNull() || !g_qwindow.isNull())
        return true;
    g_window = find_main_widget();
    if (!g_window.isNull())
        return true;
    g_qwindow = find_main_qwindow();
    return !g_qwindow.isNull();
}

QTabWidget *find_tabs()
{
    if (g_window.isNull())
        return nullptr;
    if (auto *mw = qobject_cast<QMainWindow *>(g_window.data())) {
        if (auto *tabs = qobject_cast<QTabWidget *>(mw->centralWidget()))
            return tabs;
    }
    return g_window->findChild<QTabWidget *>();
}

/// Section strip of the QML window.
///
/// By CLASS name, not by `objectName`: the strip has no name in QML (it is
/// declared there with `id: bar`, and an `id` is not visible from C++ at
/// all), but its class is real — `QQuickTabBar`, just as the widget strip
/// above is looked up by the `QTabWidget` type. Giving the strip an
/// `objectName` for this would mean editing the window markup for the
/// tray's needs.
QObject *find_qml_tabbar()
{
    if (g_qwindow.isNull())
        return nullptr;
    const auto children = g_qwindow->findChildren<QObject *>();
    for (QObject *child : children) {
        if (child != nullptr && child->inherits("QQuickTabBar"))
            return child;
    }
    return nullptr;
}

/// Icon: a name from the theme, and a file if the theme was not enough.
///
/// Warning: the order is exactly this, and it is NOT a matter of decoration.
/// The icon is laid out in the theme in eight DRAWN sizes (16…512), and the
/// small ones were drawn separately precisely because downscaling from a
/// large one gives mush. When Qt gets an icon from the theme, it hands the
/// tray the NAME (`IconName`), and the shell takes the needed size itself.
/// When Qt gets a single file, the icon has exactly one size, and below 22
/// px of the shell Qt shrinks it itself — the very downscaling the set of
/// sizes avoided.
///
/// Measured on the test bench 18.09.2026 (a headless session + our own
/// StatusNotifierWatcher): with the file the icon went onto the bus as
/// `IconName=""` and a 22×22 picture downscaled from 256×256 — a green blob
/// instead of a drawing. Hence the complaint "a skull in the tray": the
/// drawing is ours, but it is readable only at its drawn size.
///
/// The file remains a safeguard and cannot be removed: the developer tree
/// has no package, a portable build may have no theme at all, and an empty
/// QIcon must not go into the tray — the shell would show an empty spot,
/// and the customer would decide Rexon did not start.
QIcon build_icon(const char *icon_file, const char *icon_theme)
{
    if (icon_theme != nullptr && *icon_theme != '\0') {
        // Exactly hasThemeIcon, not "QIcon::fromTheme returned non-null": for a
        // missing name fromTheme returns a live but empty icon, and a null check
        // would let it into the tray.
        const QString name = QString::fromUtf8(icon_theme);
        if (QIcon::hasThemeIcon(name)) {
            QIcon from_theme = QIcon::fromTheme(name);
            if (!from_theme.isNull())
                return from_theme;
        }
    }
    if (icon_file != nullptr && *icon_file != '\0') {
        QIcon from_file(QString::fromUtf8(icon_file));
        if (!from_file.isNull())
            return from_file;
    }
    return QIcon();
}

bool daemon_call(const char *method, const QList<QVariant> &args)
{
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected())
        return false;
    QDBusMessage msg = QDBusMessage::createMethodCall(
        QString::fromUtf8(DAEMON_SERVICE), QString::fromUtf8(DAEMON_PATH),
        QString::fromUtf8(DAEMON_IFACE), QString::fromUtf8(method));
    msg.setArguments(args);
    const QDBusMessage reply = bus.call(msg, QDBus::Block, DAEMON_TIMEOUT_MS);
    return reply.type() == QDBusMessage::ReplyMessage;
}

}  // namespace

// ── callbacks ─────────────────────────────────────────────────────────────

void rexon_tray_set_hooks(RexonTrayClick on_click, RexonTrayVoid on_menu_show,
                          RexonTrayVoid on_activate, RexonTrayAsk on_close)
{
    g_on_click = on_click;
    g_on_menu_show = on_menu_show;
    g_on_activate = on_activate;
    g_on_close = on_close;
}

// ── icon ──────────────────────────────────────────────────────────────────

bool rexon_set_app_window_icon(const char *icon_file, const char *icon_theme)
{
    const QIcon icon = build_icon(icon_file, icon_theme);
    if (icon.isNull())
        return false;
    QGuiApplication::setWindowIcon(icon);
    return true;
}

bool rexon_tray_available()
{
    return QSystemTrayIcon::isSystemTrayAvailable();
}

bool rexon_tray_create(const char *icon_file, const char *icon_theme,
                       const char *tip_title, const char *tip_body)
{
    if (g_icon != nullptr)
        return true;
    // Exactly QApplication, not "there is an application object": the analysis
    // is at have_widgets_app().
    if (!have_widgets_app())
        return false;

    const QIcon icon = build_icon(icon_file, icon_theme);
    if (icon.isNull())
        return false;

    g_icon = new QSystemTrayIcon(icon, QApplication::instance());
    // A tooltip as a single line "Title — text": QSystemTrayIcon has no
    // separate title and body fields; only showMessage has them.
    QString tip = QString::fromUtf8(tip_title);
    const QString body = QString::fromUtf8(tip_body);
    if (!body.isEmpty())
        tip += QStringLiteral(" — ") + body;
    g_icon->setToolTip(tip);

    g_menu = new QMenu();
    g_icon->setContextMenu(g_menu);
    // The menu is rebuilt BEFORE every show: pause, sound, the interval and
    // daemon availability change bypassing us too (the wallpaper tab, the
    // daemon itself).
    QObject::connect(g_menu, &QMenu::aboutToShow, g_menu, [] {
        if (g_on_menu_show != nullptr)
            g_on_menu_show();
    });
    QObject::connect(g_icon, &QSystemTrayIcon::activated, g_icon,
                     [](QSystemTrayIcon::ActivationReason reason) {
                         // A right click is handled by the shell itself (the menu); the other ways
                         // of "poking the icon" open the window.
                         if (reason == QSystemTrayIcon::Context)
                             return;
                         if (g_on_activate != nullptr)
                             g_on_activate();
                     });

    g_icon->show();
    return true;
}

void rexon_tray_menu_begin()
{
    if (g_menu == nullptr)
        return;
    // Submenus are children of g_menu; clear() removes them together with the
    // actions.
    g_menu->clear();
    g_stack.clear();
    g_stack.push_back(g_menu);
}

void rexon_tray_menu_add(int32_t id, int32_t kind, const char *label, bool checked,
                         bool enabled, bool is_default)
{
    if (g_stack.empty())
        return;
    QMenu *into = g_stack.back();
    const QString text = QString::fromUtf8(label);

    switch (kind) {
    case 1:  // separator
        into->addSeparator();
        return;
    case 4: {  // start of a submenu
        QMenu *sub = into->addMenu(text);
        sub->setEnabled(enabled);
        g_stack.push_back(sub);
        return;
    }
    case 5:  // end of a submenu
        if (g_stack.size() > 1)
            g_stack.pop_back();
        return;
    default:
        break;
    }

    QAction *action = into->addAction(text);
    action->setEnabled(enabled);
    if (kind == 2 || kind == 3) {
        action->setCheckable(true);
        action->setChecked(checked);
    }
    if (is_default) {
        // A bold item: the same action as a click on the icon.
        QFont bold = action->font();
        bold.setBold(true);
        action->setFont(bold);
        into->setDefaultAction(action);
    }
    QObject::connect(action, &QAction::triggered, action, [id] {
        if (g_on_click != nullptr)
            g_on_click(id);
    });
}

void rexon_tray_menu_end()
{
    g_stack.clear();
}

void rexon_tray_notify(const char *title, const char *body)
{
    if (g_icon == nullptr)
        return;
    g_icon->showMessage(QString::fromUtf8(title), QString::fromUtf8(body),
                        g_icon->icon(), 8000);
}

// ── main window ───────────────────────────────────────────────────────────

namespace {

/// Do the work with the window in the GUI thread, wherever it was requested
/// from.
///
/// ⭐ Thanks to this layer the window doors below can be called from any
/// thread, and it is what removes the wall that kept a second launch from
/// raising the first one's window. The second-launch listener
/// (`rexon_core::single_instance::ActivationListener`) lives in its own
/// thread, while Qt allows touching a window only in the GUI thread —
/// previously that meant "cannot be raised", and raising remained a stub.
///
/// We wait for the answer (BlockingQueuedConnection) rather than "send and
/// forget": the second launch decides from the answer what to tell the
/// user, and lying "raised" over an empty screen is not allowed — that is
/// exactly the defect of 06.09.2026 because of which the listener appeared.
/// There is no deadlock: the GUI thread never waits for the listener's
/// answer.
bool on_gui_thread(const std::function<bool()> &work)
{
    QCoreApplication *app = QCoreApplication::instance();
    if (app == nullptr)
        return false;
    if (QThread::currentThread() == app->thread())
        return work();
    bool done = false;
    QMetaObject::invokeMethod(
        app, [&work, &done] { done = work(); }, Qt::BlockingQueuedConnection);
    return done;
}

bool guard_window_here()
{
    if (!resolve_window())
        return false;
    // The guard is a child of the window: it dies with it and is not installed
    // twice.
    if (g_guard.isNull()) {
        QObject *target = g_window.isNull() ? static_cast<QObject *>(g_qwindow.data())
                                            : static_cast<QObject *>(g_window.data());
        auto *guard = new CloseGuard(target);
        target->installEventFilter(guard);
        g_guard = guard;
    }
    // While the icon is alive, a closed window does not shut the application
    // down. Through QGuiApplication, not QApplication: the setting is the same
    // (the static is inherited), but the QML window may have no widget
    // application.
    QGuiApplication::setQuitOnLastWindowClosed(false);
    return true;
}

bool raise_window_here()
{
    if (!resolve_window())
        return false;

    if (QWidget *w = g_window.data()) {
        // Order matters: a minimized window must first be restored, otherwise
        // show() returns it still minimized.
        w->setWindowState(w->windowState() & ~Qt::WindowMinimized);
        w->show();
        w->raise();
        w->activateWindow();
        // On Wayland focus is given not by the application but by the shell:
        // requestActivate() sends an activation request that KWin executes with
        // the token issued to the tray for the click. Without it the window pops
        // up without focus.
        if (QWindow *handle = w->windowHandle())
            handle->requestActivate();
        return true;
    }

    QWindow *win = g_qwindow.data();
    if (win == nullptr)
        return false;
    win->setWindowStates(win->windowStates() & ~Qt::WindowMinimized);
    win->show();
    win->raise();
    win->requestActivate();
    return true;
}

bool hide_window_here()
{
    if (!resolve_window())
        return false;
    if (QWidget *w = g_window.data()) {
        w->hide();
        return true;
    }
    g_qwindow->hide();
    return true;
}

bool select_tab_here(int32_t index)
{
    if (index < 0)
        return false;
    if (QTabWidget *tabs = find_tabs()) {
        if (index >= tabs->count())
            return false;
        tabs->setCurrentIndex(index);
        return true;
    }
    if (QObject *bar = find_qml_tabbar()) {
        // The QML section strip has `count` too — we check against it rather than
        // "set it and see": TabBar silently clamps the number into range, and
        // missing the section would look like success.
        const int count = bar->property("count").toInt();
        if (index >= count)
            return false;
        return bar->setProperty("currentIndex", index);
    }
    return false;
}

bool quit_here()
{
    g_quitting = true;
    if (g_icon != nullptr)
        g_icon->hide();
    // Close the window FOR REAL, not just leave the event loop: in its
    // closeEvent the main window drains the QThread workers of all built tabs,
    // and without this step "Quit" from the icon menu would bypass the drain
    // that closing with the close button always went through. The guard above
    // is already removed (g_quitting), so the close goes through rather than
    // hiding the window.
    if (!g_window.isNull())
        g_window->close();
    else if (!g_qwindow.isNull())
        g_qwindow->close();
    // Through QCoreApplication: quitting is the same (the static is inherited),
    // and the QML window has no widget application.
    QCoreApplication::quit();
    return true;
}

/// Open a section (and a sub-page) of the QML main window — with its own
/// function `openSection(id, subpage)`. Whether such a section exists is
/// decided by the window; from here there is only the call. The Python
/// widget window has no such function — it has its own listener, and the
/// answer here is an honest false.
bool open_section_here(const QString &id, const QString &subpage)
{
    if (!resolve_window() || g_qwindow.isNull())
        return false;
    QVariant opened;
    const bool called = QMetaObject::invokeMethod(
        g_qwindow.data(), "openSection", Q_RETURN_ARG(QVariant, opened),
        Q_ARG(QVariant, QVariant(id)), Q_ARG(QVariant, QVariant(subpage)));
    return called && opened.toBool();
}

}  // namespace

bool rexon_tray_guard_window()
{
    return on_gui_thread(guard_window_here);
}

bool rexon_tray_raise_window()
{
    return on_gui_thread(raise_window_here);
}

bool rexon_tray_adopt_qml_window(void *engine)
{
    auto *qml = static_cast<QQmlApplicationEngine *>(engine);
    if (qml == nullptr)
        return false;
    const auto roots = qml->rootObjects();
    QWindow *win = roots.isEmpty() ? nullptr : qobject_cast<QWindow *>(roots.first());
    if (win == nullptr)
        return false;
    g_qwindow = win;
    return true;
}

uint64_t rexon_tray_window_native(uint32_t *rgb)
{
    if (!resolve_window() || g_qwindow.isNull())
        return 0;
    QWindow *win = g_qwindow.data();
    // The QML window is a `QQuickWindow`, and its background is the `color`
    // property (in the markup this is a skin colour). Through the property, not
    // the type: there is no QtQuick here.
    const QColor color = win->property("color").value<QColor>();
    if (rgb != nullptr)
        *rgb = color.isValid() ? (color.rgb() & 0xffffffu) : 0u;
    win->create();
    return static_cast<uint64_t>(win->winId());
}

bool rexon_tray_hide_window()
{
    return on_gui_thread(hide_window_here);
}

bool rexon_tray_select_tab(int32_t index)
{
    return on_gui_thread([index] { return select_tab_here(index); });
}

void rexon_tray_quit()
{
    // Through the GUI thread, like the other window doors: "Quit" comes from
    // the icon menu (the GUI thread), but a bench run also shuts itself down
    // with it from its own thread, and the window must not be closed from just
    // anywhere.
    on_gui_thread(quit_here);
}

bool rexon_tray_open_section(const char *id, const char *subpage)
{
    const QString section = QString::fromUtf8(id != nullptr ? id : "");
    const QString sub = QString::fromUtf8(subpage != nullptr ? subpage : "");
    return on_gui_thread([&section, &sub] { return open_section_here(section, sub); });
}

void rexon_tray_request_quit(int32_t delay_ms)
{
    // We do NOT wait, unlike the other doors: the caller is the owner handover
    // listener, and its answer "leaving" must go out before the process exits.
    // Quitting is queued in the GUI thread with a delay — by its end the answer
    // is sent, and the new process waits for the lock to be released by
    // itself.
    QCoreApplication *app = QCoreApplication::instance();
    if (app == nullptr)
        return;
    QMetaObject::invokeMethod(
        app,
        [delay_ms] { QTimer::singleShot(delay_ms < 0 ? 0 : delay_ms, [] { quit_here(); }); },
        Qt::QueuedConnection);
}


// ── wallpaper daemon ──────────────────────────────────────────────────────

bool rexon_tray_daemon_alive()
{
    QDBusConnection bus = QDBusConnection::sessionBus();
    if (!bus.isConnected() || bus.interface() == nullptr)
        return false;
    return bus.interface()->isServiceRegistered(QString::fromUtf8(DAEMON_SERVICE)).value();
}

bool rexon_tray_daemon_call(const char *method)
{
    return daemon_call(method, {});
}

bool rexon_tray_daemon_call_u32(const char *method, uint32_t arg)
{
    return daemon_call(method, {QVariant(arg)});
}

bool rexon_tray_daemon_call_bool(const char *method, bool arg)
{
    return daemon_call(method, {QVariant(arg)});
}

int32_t rexon_tray_daemon_prop(const char *name, char *buf, int32_t buf_len)
{
    if (buf == nullptr || buf_len <= 0)
        return -1;
    bool ok = false;
    // There is ONE daemon property read in the project — the same one the
    // wallpaper tab lives on; we do not create a second copy of QDBusVariant
    // parsing here.
    const QString value = rexon_we_sock_daemon_property(
        QString::fromUtf8(DAEMON_SERVICE), QString::fromUtf8(DAEMON_PATH),
        QString::fromUtf8(DAEMON_IFACE), QString::fromUtf8(name), DAEMON_TIMEOUT_MS, ok);
    if (!ok)
        return -1;
    const QByteArray utf8 = value.toUtf8();
    if (utf8.size() + 1 > buf_len)
        return -1;
    memcpy(buf, utf8.constData(), utf8.size());
    buf[utf8.size()] = '\0';
    return utf8.size();
}

bool rexon_tray_grab_window(const char *path)
{
    // `grabToImage` from QML will not do here: the window's root element does
    // not belong to the QML engine, and the whole window (the section strip,
    // the menu) can only be captured from the QQuickWindow itself.
    auto *window = qobject_cast<QQuickWindow *>(find_main_qwindow());
    if (window == nullptr || path == nullptr)
        return false;
    const QImage image = window->grabWindow();
    return !image.isNull() && image.save(QString::fromUtf8(path), "PNG");
}
