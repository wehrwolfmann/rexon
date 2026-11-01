// Макет каркаса главного окна: в какую сторону Qt 6 смешивает QtWidgets и
// Qt Quick. Только замеры — приговор выносит Rust (src/main.rs).
//
// Сцена макета повторяет наш каркас: QTabWidget, у него страница на голых
// виджетах и страница-хост QML. Именно так выглядит окно Rexon в середине
// перехода: часть вкладок уже на QML, часть ещё на виджетах.
//
// Заведомо неверные условия (флаги mutate) нужны, чтобы стенд умел краснеть:
// зеленеющий всегда стенд ничего не проверяет.

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

// ── Флаги заведомо неверных условий ──────────────────────────────────
// Каждый ломает ровно одну проверку: если стенд после этого всё равно
// зелёный — проверка ничего не меряет и её надо переписывать.
enum MutateFlags {
    MUTATE_NONE            = 0,
    MUTATE_BLANK_QML       = 1 << 0,  // хост грузит пустую сцену → пикселей нет
    MUTATE_NO_FOCUS        = 1 << 1,  // хосту запрещён фокус → цепочка рвётся
    MUTATE_FAKE_EMBED      = 1 << 2,  // сцена покрашена в цвет виджета
    MUTATE_SKIP_PUSH       = 1 << 3,  // виджет не кладут в сцену вовсе
    MUTATE_PARENT_OF_ITEM  = 1 << 4,  // родителя спрашивают у настоящего Item
};

extern "C" struct TabHostReport {
    int qt_major;
    int qt_minor;
    // Вложение QML в виджеты
    int qml_host_ready;         // QQuickWidget::Status::Ready
    int qml_pixels_pct;         // доля пикселей цвета QML-сцены в снимке хоста
    int focus_by_setfocus;      // setFocus() на хосте доводит фокус до элемента QML
    int focus_by_tab;           // Tab с соседней кнопки доводит фокус до QML
    int focus_back_to_widget;   // Backtab возвращает фокус в виджет
    // Обратное вложение: виджет в сцену Qt Quick
    int widget_is_quickitem;    // QWidget можно привести к QQuickItem?
    int qml_data_before;        // объектов в data сцены до попытки
    int qml_data_after;         // …и после: принят ли виджет хотя бы ресурсом
    int qml_child_before;       // визуальных детей до попытки
    int qml_child_after;        // …и после: прибавился ли виджет
    int widget_pixels_pct;      // доля пикселей цвета виджета в снимке сцены
    int parent_undefined;       // у виджета в QML нет родителя-элемента вовсе
    int parent_assign_errors;   // отказов QML на попытку задать виджету parent
    int widget_parent_hijacked; // сцена перевесила виджету QObject-родителя
    int widget_became_toplevel; // show() у виджета рядом со сценой создал своё окно
    char focus_detail[96];      // что видно на стороне фокуса после setFocus()
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

// Ждать наступления условия, крутя цикл событий. Нужно потому, что фокус на
// Wayland приезжает не в том же такте, что вызов: композитор сначала обязан
// активировать окно. Ожидание НЕ делает стенд всегда зелёным — при запрете
// фокуса условие не наступает никогда и мы честно выходим по таймауту.
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

// Доля пикселей заданного цвета в снимке, в процентах. Допуск нужен:
// композитор и масштабирование дают краевое смешивание.
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
        // Заведомо неверное условие: сцена ничего не рисует.
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
        // ⚠️ `focus: true` на КОРНЕ не ставим намеренно: корень и вложенный\n
        // контрол объявляют фокус в одной области видимости, и активным\n
        // остаётся корень — управляемый элемент фокус не получает. Замер\n
        // этого макета показал ровно такую картину (item=probeRoot).\n
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
    // Сцена Qt Quick, которой ПЫТАЮТСЯ скормить настоящий QWidget.
    // Замеры: приняла ли `data`, прибавилось ли визуальных детей, знает ли
    // QML у виджета родителя-элемента.
    //
    // `realItem` — положительный контроль: настоящий Item в той же сцене. У
    // него родитель есть, и заведомо неверное условие PARENT_OF_ITEM
    // спрашивает родителя именно у него — так видно, что проверка отличает
    // элемент сцены от виджета, а не возвращает undefined всем подряд.
    //
    // Заведомо неверное условие FAKE_EMBED красит саму сцену в цвет виджета
    // (а не подкладывает лишний прямоугольник) — так ломается ровно счётчик
    // пикселей и ничего кроме него.
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

    // ── Каркас: ровно наш случай — QTabWidget, вкладка на виджетах и
    //    вкладка-хост QML в одном окне ────────────────────────────────
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

    // Сцена обратного вложения — третьей вкладкой того же окна, чтобы
    // мерить её на живом композиторе, а не в отрыве.
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

    // ── Замер 1: QML внутри вкладки виджетов ─────────────────────────
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

    // ── Замер 2: цепочка фокуса виджет → QML → виджет ────────────────
    // Условие одно на все три хода: активный элемент сцены — наше поле ввода.
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
        // Обратный ход: из QML фокус обязан вернуться в виджеты, иначе на
        // геймпаде и Tab-навигации вкладка станет ловушкой.
        QKeyEvent back(QEvent::KeyPress, Qt::Key_Backtab, Qt::ShiftModifier, QString());
        QApplication::sendEvent(host, &back);
        r.focus_back_to_widget =
            waitFor([&] { return QApplication::focusWidget() == neighbour; }, 2000) ? 1 : 0;
    }

    // ── Замер 3: попытка вложить виджет в сцену Qt Quick ─────────────
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

    // ⚠️ Что на самом деле сделал `data.push`: Qt Quick перевесил виджету
    // QObject-родителя на элемент сцены (QQuickItemPrivate::data_append →
    // `o->setParent(item)`), а `QWidget::parentWidget()` — это неснимаемый
    // static_cast от QObject::parent(). То есть у виджета теперь «родитель-
    // виджет», которого не существует. Проверено этим же макетом: вызов
    // `victim->show()` в таком состоянии валит процесс SIGSEGV внутри
    // QWidgetPrivate::createWinId() (Qt 6.11.2). Поэтому здесь только
    // фиксируем подмену и снимаем её, а показываем ОТДЕЛЬНЫЙ виджет.
    r.widget_parent_hijacked =
        (sceneRoot && static_cast<QObject *>(victim)->parent()
             == static_cast<QObject *>(sceneRoot)) ? 1 : 0;
    static_cast<QObject *>(victim)->setParent(nullptr);

    // Контрольный вопрос к тем, кто скажет «так виджет просто не показан»:
    // показываем чистый виджет, живущий рядом со сценой. Если бы вложение
    // работало, ему нашлось бы место внутри сцены. Вместо этого Qt делает
    // ему собственное окно верхнего уровня.
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
