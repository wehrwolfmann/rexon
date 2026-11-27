// Mock-up of the main window frame: in which direction Qt 6 mixes
// QtWidgets and Qt Quick. Measurements only — the verdict is given by Rust
// (src/main.rs).
//
// The mock-up scene repeats our frame: a QTabWidget with a page of bare
// widgets and a QML host page. That is exactly what the Rexon window looks
// like in the middle of the transition: some tabs already in QML, some
// still in widgets.
//
// Deliberately wrong conditions (the mutate flags) are needed so that the
// test bench can go red: a test bench that is always green checks nothing.

#include <QApplication>
#include <QColor>
#include <QDir>
#include <QElapsedTimer>
#include <QEvent>
#include <QFile>
#include <QImage>
#include <QKeyEvent>
#include <QPushButton>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQmlError>
#include <QQuickItem>
#include <QQuickWidget>
#include <QQuickWindow>
#include <QTabWidget>
#include <QTextStream>
#include <QThread>
#include <QVBoxLayout>
#include <QUrl>
#include <QVariant>
#include <QWidget>

#include <cstring>
#include <functional>

// ── Flags of deliberately wrong conditions ───────────────────────────
// Each breaks exactly one check: if the bench is still green after that,
// the check measures nothing and must be rewritten.
enum MutateFlags {
    MUTATE_NONE            = 0,
    MUTATE_BLANK_QML       = 1 << 0,  // the host loads an empty scene → no pixels
    MUTATE_NO_FOCUS        = 1 << 1,  // the host is denied focus → the chain breaks
    MUTATE_FAKE_EMBED      = 1 << 2,  // the scene is painted in the widget's colour
    MUTATE_SKIP_PUSH       = 1 << 3,  // the widget is not put into the scene at all
    MUTATE_PARENT_OF_ITEM  = 1 << 4,  // the parent is asked of a real Item
};

extern "C" struct TabHostReport {
    int qt_major;
    int qt_minor;
    // Embedding QML into widgets
    int qml_host_ready;         // QQuickWidget::Status::Ready
    int qml_pixels_pct;         // share of QML scene colour pixels in the host snapshot
    int focus_by_setfocus;      // setFocus() on the host brings focus to the QML element
    int focus_by_tab;           // Tab from the neighbouring button brings focus to QML
    int focus_back_to_widget;   // Backtab returns focus to the widget
    // The reverse embedding: a widget into a Qt Quick scene
    int widget_is_quickitem;    // can a QWidget be cast to a QQuickItem?
    int qml_data_before;        // objects in the scene's data before the attempt
    int qml_data_after;         // …and after: was the widget accepted at least as a resource
    int qml_child_before;       // visual children before the attempt
    int qml_child_after;        // …and after: was the widget added
    int widget_pixels_pct;      // share of widget colour pixels in the scene snapshot
    int parent_undefined;       // the widget in QML has no parent item at all
    int parent_assign_errors;   // QML refusals on attempting to set the widget's parent
    int widget_parent_hijacked; // the scene re-parented the widget's QObject parent
    int widget_became_toplevel; // show() of the widget next to the scene created its own window
    char focus_detail[96];      // what is visible on the focus side after setFocus()
    char error[256];
};

namespace {

const char *kQmlHostColor = "#2f7bd6";   // rgb(47,123,214)
const char *kWidgetColor  = "#d62f7b";   // rgb(214,47,123)

void setError(TabHostReport &r, const QString &text)
{
    const QByteArray utf8 = text.toUtf8();
    const int n = qMin(utf8.size(), int(sizeof(r.error)) - 1);
    std::memcpy(r.error, utf8.constData(), n);
    r.error[n] = '\0';
}

void pump(int ms)
{
    QElapsedTimer t;
    t.start();
    while (t.elapsed() < ms) {
        QCoreApplication::processEvents(QEventLoop::AllEvents, 5);
        QThread::msleep(1);
    }
}

// Wait for a condition to come true while spinning the event loop. Needed
// because on Wayland focus does not arrive in the same tick as the call:
// the compositor first has to activate the window. Waiting does NOT make
// the bench always green — with focus denied the condition never comes
// true and we honestly exit on the timeout.
bool waitFor(const std::function<bool()> &cond, int timeout_ms)
{
    QElapsedTimer t;
    t.start();
    while (t.elapsed() < timeout_ms) {
        if (cond())
            return true;
        QCoreApplication::processEvents(QEventLoop::AllEvents, 5);
        QThread::msleep(2);
    }
    return cond();
}

bool writeQml(const QString &path, const QString &body, TabHostReport &r)
{
    QFile f(path);
    if (!f.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        setError(r, QStringLiteral("не записан %1").arg(path));
        return false;
    }
    QTextStream out(&f);
    out.setEncoding(QStringConverter::Utf8);
    out << body;
    return true;
}

// Share of pixels of a given colour in a snapshot, in percent. A tolerance
// is needed: the compositor and scaling produce edge blending.
int colorShare(const QImage &shot, const QColor &want)
{
    if (shot.isNull())
        return -1;
    const QImage img = shot.convertToFormat(QImage::Format_RGB32);
    qint64 hit = 0;
    const qint64 total = qint64(img.width()) * img.height();
    if (total == 0)
        return -1;
    for (int y = 0; y < img.height(); ++y) {
        const QRgb *line = reinterpret_cast<const QRgb *>(img.constScanLine(y));
        for (int x = 0; x < img.width(); ++x) {
            const QRgb p = line[x];
            if (qAbs(qRed(p) - want.red()) <= 16
                && qAbs(qGreen(p) - want.green()) <= 16
                && qAbs(qBlue(p) - want.blue()) <= 16) {
                ++hit;
            }
        }
    }
    return int((hit * 100) / total);
}

QString qmlHostSource(int mutate)
{
    if (mutate & MUTATE_BLANK_QML) {
        // Deliberately wrong condition: the scene draws nothing.
        return QStringLiteral(
            "import QtQuick\n"
            "Item { objectName: \"probeRoot\" }\n");
    }
    return QStringLiteral(
        "import QtQuick\n"
        "\n"
        "Rectangle {\n"
        "    objectName: \"probeRoot\"\n"
        "    color: \"%1\"\n"
        // Warning: we deliberately do not put `focus: true` on the ROOT: the root\n
        // and the nested control declare focus in the same scope, and the root\n
        // remains active — the operable element does not get focus. A measurement\n
        // of this mock-up showed exactly that picture (item=probeRoot).\n
        "    TextInput {\n"
        "        objectName: \"probeInput\"\n"
        "        anchors.centerIn: parent\n"
        "        width: 220\n"
        "        color: \"white\"\n"
        "        text: \"фокус дошёл до QML\"\n"
        "        focus: true\n"
        "        activeFocusOnTab: true\n"
        "    }\n"
        "}\n").arg(QString::fromLatin1(kQmlHostColor));
}

QString qmlSceneSource(int mutate)
{
    // A Qt Quick scene that is being FED a real QWidget. Measurements: whether
    // `data` accepted it, whether visual children were added, whether QML
    // knows a parent item of the widget.
    //
    // `realItem` is a positive control: a real Item in the same scene. It has
    // a parent, and the deliberately wrong PARENT_OF_ITEM condition asks for
    // the parent precisely of it — that shows the check tells a scene item
    // from a widget rather than returning undefined for everyone.
    //
    // The deliberately wrong FAKE_EMBED condition paints the scene itself in
    // the widget's colour (rather than slipping in an extra rectangle) — that
    // way exactly the pixel counter breaks and nothing else.
    const QString background = (mutate & MUTATE_FAKE_EMBED)
        ? QString::fromLatin1(kWidgetColor)
        : QStringLiteral("#101010");
    return QStringLiteral(
        "import QtQuick\n"
        "\n"
        "Rectangle {\n"
        "    id: root\n"
        "    objectName: \"sceneRoot\"\n"
        "    color: \"%1\"\n"
        "    property int dataBefore: -1\n"
        "    property int dataAfter: -1\n"
        "    property int childBefore: -1\n"
        "    property int childAfter: -1\n"
        "    property int parentErrors: 0\n"
        "    property int parentUndefined: -1\n"
        "\n"
        "    Item { id: realItem; objectName: \"realItem\"; width: 1; height: 1 }\n"
        "\n"
        "    Component.onCompleted: {\n"
        "        root.dataBefore = root.data.length\n"
        "        root.childBefore = root.children.length\n"
        "        if (!probeSkipPush)\n"
        "            root.data.push(probeWidget)\n"
        "        root.dataAfter = root.data.length\n"
        "        root.childAfter = root.children.length\n"
        "        var target = probeCheckRealItem ? realItem : probeWidget\n"
        "        root.parentUndefined = (target.parent === undefined) ? 1 : 0\n"
        "        try {\n"
        "            probeWidget.parent = root\n"
        "        } catch (e) {\n"
        "            root.parentErrors += 1\n"
        "        }\n"
        "    }\n"
        "}\n").arg(background);
}

} // namespace

extern "C" TabHostReport rexon_tab_host_probe(int mutate, const char *out_dir)
{
    TabHostReport r;
    std::memset(&r, 0, sizeof(r));
    r.qml_pixels_pct = -1;
    r.widget_pixels_pct = -1;
    r.qml_data_before = -1;
    r.qml_data_after = -1;
    r.qml_child_before = -1;
    r.qml_child_after = -1;
    r.parent_undefined = -1;

    static int argc = 1;
    static char arg0[] = "tab-host-probe";
    static char *argv[] = {arg0, nullptr};
    QApplication app(argc, argv);

    r.qt_major = QT_VERSION_MAJOR;
    r.qt_minor = QT_VERSION_MINOR;

    const QString tmp = QDir::tempPath();
    const QString hostQml = tmp + QStringLiteral("/rexon_tab_host_page.qml");
    const QString sceneQml = tmp + QStringLiteral("/rexon_tab_host_scene.qml");
    if (!writeQml(hostQml, qmlHostSource(mutate), r))
        return r;
    if (!writeQml(sceneQml, qmlSceneSource(mutate), r))
        return r;

    // ── The frame: exactly our case — a QTabWidget, a widget tab and a
    //    QML host tab in one window ──────────────────────────────────
    QTabWidget tabs;
    tabs.resize(720, 520);

    QWidget widgetPage;
    QVBoxLayout widgetLay(&widgetPage);
    widgetLay.addWidget(new QPushButton(QStringLiteral("обычная вкладка")));

    QWidget qmlPage;
    QVBoxLayout qmlLay(&qmlPage);
    qmlLay.setContentsMargins(0, 0, 0, 0);
    QPushButton *neighbour = new QPushButton(QStringLiteral("сосед-виджет"));
    neighbour->setFocusPolicy(Qt::StrongFocus);
    QQuickWidget *host = new QQuickWidget();
    host->setResizeMode(QQuickWidget::SizeRootObjectToView);
    host->setFocusPolicy((mutate & MUTATE_NO_FOCUS) ? Qt::NoFocus
                                                    : Qt::StrongFocus);
    qmlLay.addWidget(neighbour);
    qmlLay.addWidget(host, 1);

    // The reverse embedding scene is a third tab of the same window, so that it
    // is measured on a live compositor rather than in isolation.
    QPushButton *victim = new QPushButton(QStringLiteral("я виджет"));
    victim->setStyleSheet(QStringLiteral("background: %1; color: %1; border: none;")
                              .arg(QString::fromLatin1(kWidgetColor)));
    victim->resize(400, 300);
    r.widget_is_quickitem =
        qobject_cast<QQuickItem *>(static_cast<QObject *>(victim)) ? 1 : 0;

    QQuickWidget *scene = new QQuickWidget();
    scene->setResizeMode(QQuickWidget::SizeRootObjectToView);
    scene->rootContext()->setContextProperty(QStringLiteral("probeWidget"), victim);
    scene->rootContext()->setContextProperty(
        QStringLiteral("probeSkipPush"), bool(mutate & MUTATE_SKIP_PUSH));
    scene->rootContext()->setContextProperty(
        QStringLiteral("probeCheckRealItem"), bool(mutate & MUTATE_PARENT_OF_ITEM));

    tabs.addTab(&widgetPage, QStringLiteral("Виджеты"));
    tabs.addTab(&qmlPage, QStringLiteral("QML-вкладка"));
    tabs.addTab(scene, QStringLiteral("Виджет в QML"));
    tabs.show();
    tabs.raise();
    tabs.activateWindow();
    pump(400);

    // ── Measurement 1: QML inside a widget tab ───────────────────────
    tabs.setCurrentIndex(1);
    host->setSource(QUrl::fromLocalFile(hostQml));
    waitFor([&] { return host->status() != QQuickWidget::Status::Loading; }, 5000);
    pump(300);
    r.qml_host_ready = (host->status() == QQuickWidget::Status::Ready) ? 1 : 0;
    if (host->status() == QQuickWidget::Status::Error) {
        const auto errs = host->errors();
        if (!errs.isEmpty())
            setError(r, errs.first().toString());
    }
    const QImage hostShot = host->grabFramebuffer();
    r.qml_pixels_pct = colorShare(hostShot, QColor(QString::fromLatin1(kQmlHostColor)));
    if (out_dir && *out_dir && !hostShot.isNull())
        hostShot.save(QString::fromUtf8(out_dir) + QStringLiteral("/probe-qml-in-widget.png"));

    // ── Measurement 2: focus chain widget → QML → widget ─────────────
    // One condition for all three moves: the scene's active item is our text
    // field.
    auto qmlHasFocus = [&] {
        QQuickItem *f = host->quickWindow() ? host->quickWindow()->activeFocusItem()
                                            : nullptr;
        return host->hasFocus() && f
               && f->objectName() == QLatin1String("probeInput");
    };

    neighbour->setFocus(Qt::OtherFocusReason);
    waitFor([&] { return QApplication::focusWidget() == neighbour; }, 2000);
    host->setFocus(Qt::OtherFocusReason);
    r.focus_by_setfocus = waitFor(qmlHasFocus, 2000) ? 1 : 0;
    {
        QQuickItem *f = host->quickWindow() ? host->quickWindow()->activeFocusItem()
                                            : nullptr;
        const QString d = QStringLiteral("active=%1 hostFocus=%2 focusWidget=%3 item=%4")
            .arg(tabs.isActiveWindow() ? 1 : 0)
            .arg(host->hasFocus() ? 1 : 0)
            .arg(QApplication::focusWidget()
                     ? QString::fromLatin1(QApplication::focusWidget()->metaObject()->className())
                     : QStringLiteral("null"))
            .arg(f ? (f->objectName().isEmpty()
                          ? QString::fromLatin1(f->metaObject()->className())
                          : f->objectName())
                   : QStringLiteral("null"));
        const QByteArray u = d.toUtf8();
        const int n = qMin(u.size(), int(sizeof(r.focus_detail)) - 1);
        std::memcpy(r.focus_detail, u.constData(), n);
        r.focus_detail[n] = '\0';
    }

    neighbour->setFocus(Qt::OtherFocusReason);
    waitFor([&] { return QApplication::focusWidget() == neighbour; }, 2000);
    {
        QKeyEvent tab(QEvent::KeyPress, Qt::Key_Tab, Qt::NoModifier, QStringLiteral("\t"));
        QApplication::sendEvent(neighbour, &tab);
        r.focus_by_tab = waitFor(qmlHasFocus, 2000) ? 1 : 0;
    }
    {
        // The way back: from QML focus must return to the widgets, otherwise with
        // a gamepad and Tab navigation the tab becomes a trap.
        QKeyEvent back(QEvent::KeyPress, Qt::Key_Backtab, Qt::ShiftModifier, QString());
        QApplication::sendEvent(host, &back);
        r.focus_back_to_widget =
            waitFor([&] { return QApplication::focusWidget() == neighbour; }, 2000) ? 1 : 0;
    }

    // ── Measurement 3: an attempt to embed a widget into a Qt Quick scene ─
    tabs.setCurrentIndex(2);
    scene->setSource(QUrl::fromLocalFile(sceneQml));
    waitFor([&] { return scene->status() != QQuickWidget::Status::Loading; }, 5000);
    pump(300);
    if (scene->status() == QQuickWidget::Status::Error) {
        const auto errs = scene->errors();
        if (!errs.isEmpty() && r.error[0] == '\0')
            setError(r, errs.first().toString());
    }
    QQuickItem *sceneRoot = scene->rootObject();
    if (sceneRoot) {
        r.qml_data_before = sceneRoot->property("dataBefore").toInt();
        r.qml_data_after = sceneRoot->property("dataAfter").toInt();
        r.qml_child_before = sceneRoot->property("childBefore").toInt();
        r.qml_child_after = sceneRoot->property("childAfter").toInt();
        r.parent_assign_errors = sceneRoot->property("parentErrors").toInt();
        r.parent_undefined = sceneRoot->property("parentUndefined").toInt();
    }
    const QImage sceneShot = scene->grabFramebuffer();
    r.widget_pixels_pct = colorShare(sceneShot, QColor(QString::fromLatin1(kWidgetColor)));
    if (out_dir && *out_dir && !sceneShot.isNull())
        sceneShot.save(QString::fromUtf8(out_dir) + QStringLiteral("/probe-widget-in-qml.png"));

    // Warning: what `data.push` actually did: Qt Quick re-parented the widget's
    // QObject parent to a scene item (QQuickItemPrivate::data_append →
    // `o->setParent(item)`), while `QWidget::parentWidget()` is an unremovable
    // static_cast from QObject::parent(). So the widget now has a "parent
    // widget" that does not exist. Verified with this same mock-up: calling
    // `victim->show()` in such a state crashes the process with SIGSEGV inside
    // QWidgetPrivate::createWinId() (Qt 6.11.2). So here we only record the
    // substitution and undo it, and show a SEPARATE widget.
    r.widget_parent_hijacked =
        (sceneRoot && static_cast<QObject *>(victim)->parent()
             == static_cast<QObject *>(sceneRoot)) ? 1 : 0;
    static_cast<QObject *>(victim)->setParent(nullptr);

    // A control question for anyone who says "the widget is just not shown":
    // we show a clean widget living next to the scene. If embedding worked, it
    // would find a place inside the scene. Instead Qt gives it its own
    // top-level window.
    QPushButton shown(QStringLiteral("я отдельное окно"));
    shown.resize(200, 80);
    shown.show();
    pump(200);
    r.widget_became_toplevel = shown.isWindow() ? 1 : 0;
    shown.hide();

    tabs.hide();
    pump(60);
    delete victim;
    return r;
}
