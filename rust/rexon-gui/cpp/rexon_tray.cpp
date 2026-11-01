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
#include <QtGui/QWindow>
#include <QtWidgets/QApplication>
#include <QtWidgets/QMainWindow>
#include <QtWidgets/QMenu>
#include <QtWidgets/QSystemTrayIcon>
#include <QtWidgets/QTabWidget>

#include <cstring>
#include <functional>
#include <vector>

namespace {

// Имена демона обоев — те же, что у ядра (rexon_core::waywallen_client).
// Повторены здесь строками, потому что через C-границу их пришлось бы
// передавать на каждый вызов; расхождение ловит тест ядра-исполнителя.
const char *const DAEMON_SERVICE = "org.waywallen.waywallen.Daemon";
const char *const DAEMON_PATH = "/org/waywallen/waywallen/Daemon";
const char *const DAEMON_IFACE = "org.waywallen.waywallen.Daemon1";

// Демон отвечает мгновенно (локальная шина); секунды хватает с запасом, а
// вечное ожидание заморозило бы меню вместе со всем интерфейсом.
const int DAEMON_TIMEOUT_MS = 1000;

RexonTrayClick g_on_click = nullptr;
RexonTrayVoid g_on_menu_show = nullptr;
RexonTrayVoid g_on_activate = nullptr;
RexonTrayAsk g_on_close = nullptr;

QSystemTrayIcon *g_icon = nullptr;
QMenu *g_menu = nullptr;
// Главное окно двумя видами, потому что у процесса Rexon два разных хозяина
// и оба живые: продукт на PySide6 строит виджетное `QMainWindow`, а бинарь
// главного окна на Rust — `ApplicationWindow` целиком на QML, и виджетов в
// нём нет НИ ОДНОГО. Один указатель на QWidget обслуживал только первого:
// у второго `topLevelWidgets()` пуст всегда, поэтому «крестик прячет окно»,
// подъём окна вторым запуском и переход к разделу из меню значка не работали
// не «плохо», а никак.
QPointer<QWidget> g_window;
QPointer<QWindow> g_qwindow;
// Сторож закрытия — чтобы не поставить второй на то же окно. QPointer, а не
// findChild: у сторожа нет Q_OBJECT (он ему не нужен), а findChild без
// Q_OBJECT не компилируется вовсе.
QPointer<QObject> g_guard;
// Выход уже заказан: сторож закрытия обязан пропустить последнее закрытие,
// иначе приложение не выйдет ВООБЩЕ — сам себе запрёт дверь.
bool g_quitting = false;

// Стопка открытых вложенных меню при сборке: вершина — то, куда сейчас
// добавляются пункты.
std::vector<QMenu *> g_stack;

/// Сторож закрытия главного окна: крестик прячет окно, а не гасит Rexon.
///
/// Фильтр событий, а не подмена closeEvent: окно создаёт не этот файл —
/// PySide6 либо QML, — и наследовать его класс отсюда нечем. Q_OBJECT классу
/// не нужен: eventFilter виртуален, moc здесь не при чём.
///
/// Один сторож на оба вида окна: `QEvent::Close` приходит и виджету, и
/// `QWindow` (у окна на QML — самому `QQuickWindow`), поэтому фильтр
/// одинаково годен там и там, а прятать надо тем методом, который у объекта
/// есть.
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

/// Живёт ли в процессе настоящий QApplication (а не только QGuiApplication).
///
/// Спрашивать обязательно, и `QApplication::instance()` для этого НЕ годится:
/// он унаследован от QCoreApplication и в процессе с QGuiApplication честно
/// вернёт объект приложения. Значок в лотке этому верил, шёл создавать
/// `QMenu` — а виджет без QApplication Qt не создаёт вовсе и гасит процесс
/// («QWidget: Cannot create a QWidget without QApplication»). То есть ответ
/// «значка нет» превращался в падение всей программы.
bool have_widgets_app()
{
    return qobject_cast<QApplication *>(QCoreApplication::instance()) != nullptr;
}

/// Главное окно приложения-виджета: единственное верхнеуровневое QMainWindow.
///
/// Вкладки — тоже QMainWindow, но они вставлены дочерними виджетами и в
/// `topLevelWidgets()` не попадают.
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

/// Главное окно приложения на QML: верхнеуровневое окно без окна-хозяина.
///
/// Отбор именно такой, а не «первое из списка»: рядом с главным окном в
/// `topLevelWindows()` лежат окна всплывающих списков, подсказок и меню
/// (`Qt::Popup`, `Qt::ToolTip`), и они появляются раньше главного, стоит
/// человеку открыть меню. Взять первое подряд — значит однажды спрятать
/// крестиком выпадающий список вместо окна.
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
        // Спрятанное окно — запасной ответ: запуск с `--tray` прячет окно
        // сразу, и после этого поднимать было бы уже нечего.
        if (hidden == nullptr)
            hidden = w;
    }
    return hidden;
}

/// Найти главное окно, если ещё не искали. false — окна в процессе нет.
///
/// Виджет спрашивается первым: в процессе продукта на PySide6 есть и он, и
/// его `QWindow`, но у виджета есть `close()` со стоком воркеров вкладок, а
/// у `QWindow` — нет.
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

/// Полоса разделов окна на QML.
///
/// По имени КЛАССА, а не по `objectName`: имени у полосы в QML нет (там она
/// заведена под `id: bar`, а `id` в C++ не виден вовсе), зато класс у неё
/// настоящий — `QQuickTabBar`, ровно так же, как виджетная полоса выше
/// ищется по типу `QTabWidget`. Заводить полосе `objectName` ради этого
/// значило бы править разметку окна под нужды лотка.
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

/// Значок: имя из темы, а файл — если темы не хватило.
///
/// ⚠️ Порядок именно такой, и это НЕ мелочь оформления. Значок разложен по
/// теме восемью НАРИСОВАННЫМИ размерами (16…512), и мелкие рисовались
/// отдельно как раз потому, что пересчёт из крупного даёт кашу. Когда Qt
/// получает значок из темы, он отдаёт лотку ИМЯ (`IconName`), и нужный
/// размер берёт сама оболочка. Когда Qt получает один файл, размер у значка
/// ровно один, и до 22 px оболочки Qt сжимает его сам — тем самым пересчётом,
/// которого набор размеров и избегал.
///
/// Замер на стенде 18.09.2026 (headless-сеанс + свой StatusNotifierWatcher):
/// с файлом значок уезжал на шину как `IconName=""` и картинка 22×22,
/// пересчитанная из 256×256, — зелёное пятно вместо рисунка. Отсюда и жалоба
/// «в лотке череп»: рисунок наш, а читается он только в нарисованном размере.
///
/// Файл остаётся страховкой и снят быть не может: в дереве разработчика
/// пакета нет, у переносимой сборки темы может не быть вовсе, а пустой QIcon
/// в лоток становиться нельзя — оболочка покажет пустое место, и покупатель
/// решит, что Rexon не запустился.
QIcon build_icon(const char *icon_file, const char *icon_theme)
{
    if (icon_theme != nullptr && *icon_theme != '\0') {
        // Именно hasThemeIcon, а не «QIcon::fromTheme вернул не-null»:
        // на пропавшее имя fromTheme отдаёт живой, но пустой значок, и
        // проверка на null пропустила бы его в лоток.
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

// ── обратные вызовы ───────────────────────────────────────────────────────

void rexon_tray_set_hooks(RexonTrayClick on_click, RexonTrayVoid on_menu_show,
                          RexonTrayVoid on_activate, RexonTrayAsk on_close)
{
    g_on_click = on_click;
    g_on_menu_show = on_menu_show;
    g_on_activate = on_activate;
    g_on_close = on_close;
}

// ── значок ────────────────────────────────────────────────────────────────

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
    // Именно QApplication, а не «есть объект приложения»: разбор у
    // have_widgets_app().
    if (!have_widgets_app())
        return false;

    const QIcon icon = build_icon(icon_file, icon_theme);
    if (icon.isNull())
        return false;

    g_icon = new QSystemTrayIcon(icon, QApplication::instance());
    // Подсказка одной строкой «Заголовок — текст»: у QSystemTrayIcon нет
    // раздельных полей заголовка и тела, они есть только у showMessage.
    QString tip = QString::fromUtf8(tip_title);
    const QString body = QString::fromUtf8(tip_body);
    if (!body.isEmpty())
        tip += QStringLiteral(" — ") + body;
    g_icon->setToolTip(tip);

    g_menu = new QMenu();
    g_icon->setContextMenu(g_menu);
    // Меню пересобирается ПЕРЕД каждым показом: пауза, звук, интервал и
    // доступность демона меняются и мимо нас (вкладка обоев, сам демон).
    QObject::connect(g_menu, &QMenu::aboutToShow, g_menu, [] {
        if (g_on_menu_show != nullptr)
            g_on_menu_show();
    });
    QObject::connect(g_icon, &QSystemTrayIcon::activated, g_icon,
                     [](QSystemTrayIcon::ActivationReason reason) {
                         // Правое нажатие оболочка обслуживает сама (меню);
                         // остальные способы «ткнуть в значок» — окно.
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
    // Вложенные меню — дети g_menu, clear() снимает их вместе с действиями.
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
    case 1:  // разделитель
        into->addSeparator();
        return;
    case 4: {  // начало вложенного меню
        QMenu *sub = into->addMenu(text);
        sub->setEnabled(enabled);
        g_stack.push_back(sub);
        return;
    }
    case 5:  // конец вложенного меню
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
        // Жирный пункт: то же действие, что у щелчка по значку.
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

// ── главное окно ──────────────────────────────────────────────────────────

namespace {

/// Выполнить работу с окном в потоке GUI, откуда бы её ни попросили.
///
/// ⭐ Из-за этой прослойки двери окна ниже можно звать из любого потока, и
/// именно она снимает стену, из-за которой второй запуск не поднимал окно
/// первого. Слушатель второго запуска
/// (`rexon_core::single_instance::ActivationListener`) живёт в своём потоке,
/// а окно у Qt разрешено трогать только в потоке GUI — раньше это значило
/// «поднять нельзя», и подъём остался заглушкой.
///
/// Ждём ответа (BlockingQueuedConnection), а не «отправил и забыл»: по
/// ответу второй запуск решает, что сказать человеку, и соврать ему
/// «поднял» при пустом экране нельзя — это ровно тот дефект 06.09.2026,
/// из-за которого слушатель и появился. Взаимной блокировки нет: поток GUI
/// ответа слушателя не ждёт никогда.
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
    // Сторож — ребёнок окна: умрёт вместе с ним, второй раз не ставится.
    if (g_guard.isNull()) {
        QObject *target = g_window.isNull() ? static_cast<QObject *>(g_qwindow.data())
                                            : static_cast<QObject *>(g_window.data());
        auto *guard = new CloseGuard(target);
        target->installEventFilter(guard);
        g_guard = guard;
    }
    // Пока значок жив, закрытое окно приложение не гасит. Через
    // QGuiApplication, а не QApplication: настройка одна и та же (статика
    // унаследована), но виджетного приложения у окна на QML может не быть.
    QGuiApplication::setQuitOnLastWindowClosed(false);
    return true;
}

bool raise_window_here()
{
    if (!resolve_window())
        return false;

    if (QWidget *w = g_window.data()) {
        // Порядок важен: свёрнутое окно надо сперва развернуть, иначе show()
        // возвращает его свёрнутым же.
        w->setWindowState(w->windowState() & ~Qt::WindowMinimized);
        w->show();
        w->raise();
        w->activateWindow();
        // На Wayland фокус даёт не приложение, а оболочка: requestActivate()
        // отправляет запрос активации, который KWin исполняет по токену,
        // выданному лотку на щелчок. Без него окно всплывает без фокуса.
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
        // `count` у полосы разделов на QML тоже есть — проверяем по нему, а
        // не «поставим и посмотрим»: TabBar молча зажимает номер в границы,
        // и промах по разделу выглядел бы удачей.
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
    // Закрыть окно ПО-НАСТОЯЩЕМУ, а не просто выйти из цикла событий: в его
    // closeEvent главное окно осушает QThread-воркеры всех построенных
    // вкладок, и без этого шага «Выход» из меню значка обходил бы сток,
    // который закрытие крестиком проходило всегда. Сторож выше уже снят
    // (g_quitting), поэтому закрытие пройдёт, а не спрячет окно.
    if (!g_window.isNull())
        g_window->close();
    else if (!g_qwindow.isNull())
        g_qwindow->close();
    // Через QCoreApplication: выход один и тот же (статика унаследована), а
    // виджетного приложения у окна на QML нет.
    QCoreApplication::quit();
    return true;
}

/// Открыть раздел (и подраздел) главного окна на QML — его же функцией
/// `openSection(id, subpage)`. Есть ли такой раздел, решает окно; отсюда
/// только вызов. У виджетного окна на Python этой функции нет — там свой
/// слушатель, и ответ здесь честное false.
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
    // Через поток GUI, как и остальные двери окна: «Выход» приезжает из меню
    // значка (поток GUI), но им же гасит себя прогон стенда из своего потока,
    // а закрывать окно откуда попало нельзя.
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
    // НЕ ждём, в отличие от остальных дверей: зовёт слушатель передачи
    // хозяина, и его ответ «ухожу» обязан уйти раньше, чем процесс выйдет.
    // Выход ставится в очередь потока GUI с отсрочкой — к её концу ответ
    // отправлен, а новый процесс ждёт освобождения замка сам.
    QCoreApplication *app = QCoreApplication::instance();
    if (app == nullptr)
        return;
    QMetaObject::invokeMethod(
        app,
        [delay_ms] { QTimer::singleShot(delay_ms < 0 ? 0 : delay_ms, [] { quit_here(); }); },
        Qt::QueuedConnection);
}


// ── демон обоев ───────────────────────────────────────────────────────────

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
    // Чтение свойства демона в проекте ОДНО — то же, которым живёт вкладка
    // обоев; второй копии разбора QDBusVariant здесь не заводим.
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
