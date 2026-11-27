#include "rexon_gui_factory.h"

#include <algorithm>
#include <cstring>

#include <QtCore/QCoreApplication>
#include <QtCore/QDeadlineTimer>
#include <QtCore/QHash>
#include <QtCore/QMutex>
#include <QtCore/QEventLoop>
#include <QtCore/QJsonDocument>
#include <QtCore/QLibraryInfo>
#include <QtCore/QThread>
#include <QtCore/QThreadPool>
#include <QtCore/QTimer>
#include <QtCore/QList>
#include <QtCore/QPointer>
#include <QtCore/QTranslator>
#include <QtCore/QSet>
#include <QtCore/QUrl>
#include <QtCore/QVariant>
#include <QtGui/QFontDatabase>
#include <QtGui/QGuiApplication>
#include <QtGui/QKeyEvent>
#include <QtGui/QStyleHints>
#include <QtGui/QMouseEvent>
#include <QtGui/QPalette>
#include <QtGui/QWindow>
#include <QtMultimedia/QMediaPlayer>
#include <QtQml/QJSValue>
#include <QtQuick/QQuickItem>
#include <QtQuick/QQuickWindow>

// Мета-рефлексия Qt-Protobuf-гаджетов — шим общий на проект, живёт в
// крейте модели редактора фильтров.
#include "rexon_gadget_shim.h"

// Заголовки генерирует cxx-qt из соответствующих мостов; путь внутри
// include-дерева повторяет путь файла в крейте.
#include "rexon-gui/src/discover_queries.cxxqt.h"
#include "rexon-gui/src/playlist_queries.cxxqt.h"
#include "rexon-gui/src/remote_list_model.cxxqt.h"
#include "rexon-gui/src/wallpaper_actions.cxxqt.h"
#include "rexon-gui/src/wallpaper_list_model.cxxqt.h"
#include "rexon-gui/src/wallpaper_queries.cxxqt.h"

QObject* rexon_new_remote_list_model(QObject* parent)
{
    auto* model = new RemoteListModel();
    if (parent != nullptr)
        model->setParent(parent);
    return model;
}

QByteArray rexon_variant_to_json(const QVariant& value)
{
    // ⚠️ QML кладёт в свойство типа QVariant не готовую карту, а обёртку
    // QJSValue — движок разворачивает её только по требованию. Без этого
    // шага QJsonDocument::fromVariant отдавал бы null на любом объекте,
    // пришедшем из QML, то есть свойства-карты приезжали бы пустыми.
    const QVariant unwrapped = value.metaType() == QMetaType::fromType<QJSValue>()
        ? value.value<QJSValue>().toVariant()
        : value;
    // ⚠️ И второй слой: внутри карты могут лежать Qt-Protobuf-гаджеты
    // (правила сортировки и фильтров). Для QJsonDocument гаджет —
    // неизвестный тип, он даёт null, и настройка уезжала бы демону пустой.
    // Разворачивает их мета-рефлексия гаджетного шима — одна на проект.
    return QJsonDocument::fromVariant(rexon_plain_variant(unwrapped))
        .toJson(QJsonDocument::Compact);
}

QObject* rexon_new_search_result_model(QObject* parent)
{
    return rexon_new_remote_list_model(parent);
}

// Запросы страницы Discover. Тело у всех одно, поэтому шаблон-помощник —
// но экспортируемые функции остаются отдельными: через cxx шаблоны не ходят.
namespace {
template <typename T>
QObject* make_query(QObject* parent)
{
    auto* query = new T();
    if (parent != nullptr)
        query->setParent(parent);
    return query;
}
}  // namespace

QObject* rexon_new_remote_availability_query(QObject* parent)
{
    return make_query<RemoteAvailabilityQuery>(parent);
}

QObject* rexon_new_remote_search_query(QObject* parent)
{
    return make_query<RemoteSearchQuery>(parent);
}

QObject* rexon_new_remote_details_query(QObject* parent)
{
    return make_query<RemoteDetailsQuery>(parent);
}

QObject* rexon_new_remote_download_query(QObject* parent)
{
    return make_query<RemoteDownloadQuery>(parent);
}

QObject* rexon_as_wallpaper_list_model(QObject* object)
{
    return qobject_cast<WallpaperListModel*>(object);
}

QObject* rexon_new_wallpaper_list_model(QObject* parent)
{
    auto* model = new WallpaperListModel();
    if (parent != nullptr)
        model->setParent(parent);
    return model;
}

QObject* rexon_new_playlist_wallpaper_list_model(QObject* parent)
{
    return rexon_new_wallpaper_list_model(parent);
}

QObject* rexon_as_playlist_wallpaper_list_model(QObject* object)
{
    return rexon_as_wallpaper_list_model(object);
}

// Страница «Обои» целиком: запрос списка и семь запросов-действий. Тот же
// шаблон-помощник, что у Discover, — тела одинаковые, а экспортируемые
// функции обязаны быть отдельными: через cxx шаблоны не ходят.
QObject* rexon_new_wallpaper_list_query(QObject* parent)
{
    return make_query<WallpaperListQuery>(parent);
}

QObject* rexon_new_wallpaper_get_query(QObject* parent)
{
    return make_query<WallpaperGetQuery>(parent);
}

QObject* rexon_new_wallpaper_apply_query(QObject* parent)
{
    return make_query<WallpaperApplyQuery>(parent);
}

QObject* rexon_new_wallpaper_remove_query(QObject* parent)
{
    return make_query<WallpaperRemoveQuery>(parent);
}

QObject* rexon_new_wallpaper_scan_query(QObject* parent)
{
    return make_query<WallpaperScanQuery>(parent);
}

QObject* rexon_new_wallpaper_property_set_query(QObject* parent)
{
    return make_query<WallpaperPropertySetQuery>(parent);
}

QObject* rexon_new_wallpaper_layout_set_query(QObject* parent)
{
    return make_query<WallpaperLayoutSetQuery>(parent);
}

QObject* rexon_new_wallpaper_apply_via_portal_query(QObject* parent)
{
    return make_query<WallpaperApplyViaPortalQuery>(parent);
}

// Плейлисты (waywallen 0.5.0: умные плейлисты и поколения ответов) — для
// того же стенда.
QObject* rexon_new_playlist_list_query(QObject* parent)
{
    return make_query<PlaylistListQuery>(parent);
}

QObject* rexon_new_playlist_detail_query(QObject* parent)
{
    return make_query<PlaylistDetailQuery>(parent);
}

QObject* rexon_new_playlist_mutation_query(QObject* parent)
{
    return make_query<PlaylistMutationQuery>(parent);
}

QObject* rexon_new_wallpaper_hide_query(QObject* parent)
{
    return make_query<WallpaperHideQuery>(parent);
}

QObject* rexon_new_reload_debounce(QObject* owner, int interval_ms)
{
    auto* timer = new QTimer(owner);
    timer->setSingleShot(true);
    timer->setInterval(interval_ms);
    // Прямое соединение: таймер живёт в потоке владельца, и перезагрузка
    // обязана случиться там же, где живут его свойства.
    QObject::connect(timer, &QTimer::timeout, owner, [owner]() {
        QMetaObject::invokeMethod(owner, "reload", Qt::DirectConnection);
    });
    return timer;
}

void rexon_restart_debounce(QObject* timer)
{
    if (auto* t = qobject_cast<QTimer*>(timer))
        t->start();
}

QObject* rexon_new_wallpaper_debounce(QObject* owner, int interval_ms)
{
    return rexon_new_reload_debounce(owner, interval_ms);
}

void rexon_restart_wallpaper_debounce(QObject* timer)
{
    rexon_restart_debounce(timer);
}

QObject* rexon_new_action_debounce(QObject* owner, int interval_ms)
{
    return rexon_new_reload_debounce(owner, interval_ms);
}

void rexon_restart_action_debounce(QObject* timer)
{
    rexon_restart_debounce(timer);
}

QObject* rexon_new_playlist_debounce(QObject* owner, int interval_ms)
{
    return rexon_new_reload_debounce(owner, interval_ms);
}

void rexon_restart_playlist_debounce(QObject* timer)
{
    rexon_restart_debounce(timer);
}

void rexon_delete_object(QObject* object)
{
    delete object;
}

// Версии Qt для строки запуска в журнале: рантайм (`qVersion`, библиотека,
// которая реально загружена) и сборка (`QT_VERSION_STR`, заголовки, с
// которыми собран Rexon). Расхождение — первый признак обновления Qt
// без пересборки Rexon.
extern "C" const char* rexon_qt_runtime_version()
{
    return qVersion();
}

extern "C" const char* rexon_qt_build_version()
{
    return QT_VERSION_STR;
}

extern "C" int rexon_qt_translations_dir(char* buf, int len)
{
    const QByteArray dir = QLibraryInfo::path(QLibraryInfo::TranslationsPath).toUtf8();
    if (buf != nullptr && len > 0) {
        const int n = qMin(int(dir.size()), len - 1);
        memcpy(buf, dir.constData(), size_t(n));
        buf[n] = '\0';
    }
    return int(dir.size());
}

namespace {

/// Меню правки полей QML (`TextArea`, `TextField`, `TextEdit`) — надписи
/// из `qtbase`, если у `qtdeclarative` их для этого языка нет.
///
/// Qt Quick Controls 6.9+ рисует правый щелчок по полю своим меню
/// (`QtQuick/Controls/<стиль>/impl/TextEditingContextMenu.qml`), и каждая
/// строка — `qsTr()` в своём файле: контекст `CopyAction`, `SelectAllAction`…
/// В Qt 6.12 эти контексты переведены в `qtdeclarative_*.qm` не на все
/// языки (немецкий есть; русского и китайского нет), а те же надписи
/// меню правки `QLineEdit` в `qtbase` переведены почти везде. Поэтому:
/// сначала — что скажут каталоги Qt Quick (их перевод главнее), иначе —
/// надпись `QLineEdit` без знака быстрой клавиши. Ничего не нашлось —
/// пусто, и Qt оставит английский.
class RexonQuickEditMenuTranslator final : public QTranslator {
public:
    explicit RexonQuickEditMenuTranslator(QObject* parent) : QTranslator(parent) {}

    QString translate(const char* context, const char* sourceText,
                      const char* disambiguation, int n) const override
    {
        if (context == nullptr || sourceText == nullptr || t_busy)
            return QString();
        const char* widgetSource = widgetSourceFor(context, sourceText);
        if (widgetSource == nullptr)
            return QString();
        // Вопрос остальным переводчикам — через приложение, и он вернётся
        // сюда же: флаг потока отвечает на него пустотой. Qt зовёт
        // переводчики и из QQmlThread, поэтому флаг на поток, а не общий.
        t_busy = true;
        const QString quick = QCoreApplication::translate(context, sourceText, disambiguation, n);
        const QString widget = QCoreApplication::translate("QLineEdit", widgetSource);
        t_busy = false;
        if (quick != QLatin1String(sourceText))
            return quick;
        if (widget == QLatin1String(widgetSource))
            return QString();
        return withoutMnemonic(widget);
    }

    bool isEmpty() const override { return false; }

private:
    /// Контекст меню Qt Quick ▸ та же надпись меню `QLineEdit` в `qtbase`.
    static const char* widgetSourceFor(const char* context, const char* sourceText)
    {
        static const struct { const char* context; const char* source; const char* widget; } map[] = {
            {"UndoAction", "Undo", "&Undo"},
            {"RedoAction", "Redo", "&Redo"},
            {"CutAction", "Cut", "Cu&t"},
            {"CopyAction", "Copy", "&Copy"},
            {"PasteAction", "Paste", "&Paste"},
            {"DeleteAction", "Delete", "Delete"},
            {"SelectAllAction", "Select All", "Select All"},
        };
        for (const auto& row : map) {
            if (std::strcmp(context, row.context) == 0 && std::strcmp(sourceText, row.source) == 0)
                return row.widget;
        }
        return nullptr;
    }

    /// «&Копировать» → «Копировать», «复制(&C)» → «复制», «&&» → «&».
    static QString withoutMnemonic(const QString& text)
    {
        QString out;
        out.reserve(text.size());
        for (qsizetype i = 0; i < text.size(); ++i) {
            const QChar c = text.at(i);
            if (c == QLatin1Char('(') && i + 3 < text.size() && text.at(i + 1) == QLatin1Char('&')
                && text.at(i + 2) != QLatin1Char('&') && text.at(i + 3) == QLatin1Char(')')) {
                i += 3;
                continue;
            }
            if (c == QLatin1Char('&')) {
                if (i + 1 < text.size() && text.at(i + 1) == QLatin1Char('&')) {
                    out.append(c);
                    ++i;
                }
                continue;
            }
            out.append(c);
        }
        return out.trimmed();
    }

    static thread_local bool t_busy;
};

thread_local bool RexonQuickEditMenuTranslator::t_busy = false;

} // namespace

extern "C" int rexon_install_qt_translations(const char* const* paths, int count)
{
    QCoreApplication* app = QCoreApplication::instance();
    if (app == nullptr)
        return -1;
    // Прежний набор снимаем целиком: смена языка не должна оставить
    // кнопки прежнего языка поверх нового. QPointer — если приложение уже
    // снесло переводчик вместе с собой, висячего указателя не останется.
    static QList<QPointer<QTranslator>> installed;
    for (const QPointer<QTranslator>& old : installed) {
        if (old) {
            app->removeTranslator(old);
            delete old;
        }
    }
    installed.clear();
    int loaded = 0;
    for (int i = 0; paths != nullptr && i < count; ++i) {
        if (paths[i] == nullptr || *paths[i] == '\0')
            continue;
        // Родитель — приложение: Qt держит на переводчик только слабую
        // ссылку в списке установленных.
        auto* tr = new QTranslator(app);
        if (!tr->load(QString::fromUtf8(paths[i]))) {
            // Пустой переводчик в списке — лишний вызов на каждый tr() окна.
            delete tr;
            continue;
        }
        app->installTranslator(tr);
        installed.append(tr);
        ++loaded;
    }
    // Меню правки полей QML — поверх каталогов (ставится последним, то есть
    // спрашивается первым). Без каталогов ему нечего искать.
    if (loaded > 0) {
        auto* edit = new RexonQuickEditMenuTranslator(app);
        app->installTranslator(edit);
        installed.append(edit);
    }
    return loaded;
}

namespace {

/// Переводчик строк QML поверх каталога ядра (см. заголовок).
///
/// Кэш — только для строк без числа: форма по числу зависит от `n`, и
/// хранить её по каждому `n` значило бы раздувать кэш без выигрыша. В кэш
/// ложится и «перевода нет» (пустой QString): иначе каждая непереведённая
/// подпись ходила бы в Rust при каждом пересчёте привязки.
class RexonAppTranslator final : public QTranslator {
public:
    RexonAppTranslator(QObject* parent, const char* context, RexonAppTranslate lookup)
        : QTranslator(parent), m_context(context), m_lookup(lookup)
    {
    }

    QString translate(const char* context, const char* sourceText,
                      const char* disambiguation, int n) const override
    {
        if (context == nullptr || sourceText == nullptr || m_context != context)
            return QString();
        QByteArray key;
        if (n < 0) {
            key.reserve(int(std::strlen(sourceText)) + 32);
            if (disambiguation != nullptr)
                key.append(disambiguation);
            key.append('\x04').append(sourceText);
            QMutexLocker lock(&m_mutex);
            const auto hit = m_cache.constFind(key);
            if (hit != m_cache.constEnd())
                return *hit;
        }
        QString out;
        if (!m_lookup(disambiguation, sourceText, n, &RexonAppTranslator::sink, &out))
            out = QString();
        if (n < 0) {
            QMutexLocker lock(&m_mutex);
            m_cache.insert(key, out);
        }
        return out;
    }

    // Каталог не в файле, и «пусто» по-файловому здесь не бывает: снять
    // переводчик — дело установщика (английский его не ставит вовсе).
    bool isEmpty() const override { return false; }

private:
    static void sink(void* target, const char* utf8, int len)
    {
        *static_cast<QString*>(target) = QString::fromUtf8(utf8, len);
    }

    const QByteArray m_context;
    const RexonAppTranslate m_lookup;
    mutable QMutex m_mutex;
    mutable QHash<QByteArray, QString> m_cache;
};

} // namespace

extern "C" bool rexon_install_app_translator(const char* context, RexonAppTranslate lookup)
{
    QCoreApplication* app = QCoreApplication::instance();
    if (app == nullptr)
        return false;
    // QPointer: если приложение уже снесло переводчик вместе с собой,
    // висячего указателя не останется.
    static QPointer<QTranslator> current;
    if (current) {
        app->removeTranslator(current);
        delete current;
    }
    if (context == nullptr || lookup == nullptr)
        return true;
    auto* tr = new RexonAppTranslator(app, context, lookup);
    app->installTranslator(tr);
    current = tr;
    return true;
}

extern "C" int rexon_drain_thread_pool(int ms)
{
    QThreadPool* pool = QThreadPool::globalInstance();
    if (pool == nullptr)
        return 0;
    // clear() снимает ЕЩЁ НЕ НАЧАТЫЕ задачи — ждать очередь, которую всё
    // равно никто не увидит, значит держать человека перед закрытым окном.
    pool->clear();
    if (pool->waitForDone(ms < 0 ? 0 : ms))
        return 0;
    // activeThreadCount() за границей срока может уже вернуть 0 (задача
    // кончилась между ответом и вопросом) — но раз waitForDone сказал «не
    // дождался», считаем минимум одну.
    return qMax(1, pool->activeThreadCount());
}

// Обход и по дереву QObject, и по дереву элементов: элемент сцены может
// висеть под чужим QObject-родителем (Loader, делегат), а объект без
// визуального родителя — только в QObject-дереве окна.
static void rexon_collect_players(QObject* node, QSet<QMediaPlayer*>& out)
{
    if (node == nullptr)
        return;
    for (QMediaPlayer* p : node->findChildren<QMediaPlayer*>())
        out.insert(p);
    if (auto* item = qobject_cast<QQuickItem*>(node)) {
        for (QQuickItem* child : item->childItems())
            rexon_collect_players(child, out);
    }
}

extern "C" int rexon_stop_media_players()
{
    QSet<QMediaPlayer*> players;
    for (QWindow* w : QGuiApplication::allWindows()) {
        rexon_collect_players(w, players);
        if (auto* qw = qobject_cast<QQuickWindow*>(w))
            rexon_collect_players(qw->contentItem(), players);
    }
    for (QMediaPlayer* p : players) {
        p->stop();
        // Пустой источник, а не только stop(): stop() лишь ставит движок
        // воспроизведения на паузу, а его потоки (демультиплексор,
        // декодеры) живут дальше. Пустой источник сносит движок целиком и
        // ждёт его потоки (`PlaybackEngine::~PlaybackEngine` →
        // `deleteFreeThreads` → `QThread::wait`).
        p->setSource(QUrl());
    }
    return int(players.size());
}

extern "C" bool rexon_start_ui_heartbeat(int ms, void (*tick)())
{
    static QPointer<QTimer> timer;
    QCoreApplication* app = QCoreApplication::instance();
    if (app == nullptr || tick == nullptr)
        return false;
    if (timer)
        return true;
    timer = new QTimer(app);
    timer->setInterval(ms < 10 ? 10 : ms);
    QObject::connect(timer, &QTimer::timeout, timer, [tick] { tick(); });
    timer->start();
    // Первый удар сразу: сторож судит только после первого пульса.
    tick();
    return true;
}

extern "C" int rexon_qml_ancestry(const void* variant, char* buf, int len)
{
    if (variant == nullptr || buf == nullptr || len <= 0)
        return 0;
    const QVariant& value = *static_cast<const QVariant*>(variant);
    QObject* obj = value.metaType() == QMetaType::fromType<QJSValue>()
        ? value.value<QJSValue>().toQObject()
        : value.value<QObject*>();
    QByteArray chain;
    for (int depth = 0; obj != nullptr && depth < 64; ++depth) {
        if (!chain.isEmpty())
            chain.append('/');
        chain.append(obj->metaObject()->className());
        QObject* next = obj->parent();
        if (next == nullptr) {
            if (auto* item = qobject_cast<QQuickItem*>(obj))
                next = item->parentItem();
        }
        obj = next;
    }
    const int n = qMin(int(chain.size()), len - 1);
    std::memcpy(buf, chain.constData(), size_t(n));
    buf[n] = '\0';
    return n;
}

// ── Журнал действий: фильтр ввода приложения ─────────────────────────────
//
// Механика без решений: кто взял нажатие (Qt знает это сам — захват точки
// указателя), его предки и их подписи — строками в ядро
// (`rexon_core::ui_watch::begin_input`), а что из этого действие и как его
// назвать, решает ядро. Текст полей ввода НЕ читается вовсе: там бывает
// пароль.

namespace {

QString journalClean(QString s)
{
    s.replace(QLatin1Char('\t'), QLatin1Char(' '));
    s.replace(QLatin1Char('\n'), QLatin1Char(' '));
    s.replace(QLatin1Char('\r'), QLatin1Char(' '));
    if (s.size() > 200)
        s.truncate(200);
    return s;
}

bool journalIsTextInput(const QObject* obj)
{
    return obj->inherits("QQuickTextInput") || obj->inherits("QQuickTextEdit");
}

QString journalOwnText(const QObject* obj)
{
    if (journalIsTextInput(obj))
        return {};
    for (const char* name : {"text", "displayText", "title"}) {
        const QVariant v = obj->property(name);
        if (v.metaType() == QMetaType::fromType<QString>()) {
            const QString t = v.toString().trimmed();
            if (!t.isEmpty())
                return t;
        }
    }
    return {};
}

// Первая видимая надпись (`Text`) внутри элемента — обход вширь с пределом.
QString journalInnerText(QQuickItem* root)
{
    QList<QQuickItem*> queue{root};
    for (int seen = 0; !queue.isEmpty() && seen < 300; ++seen) {
        QQuickItem* item = queue.takeFirst();
        if (journalIsTextInput(item))
            continue;
        if (item != root && item->inherits("QQuickText") && item->isVisible()) {
            const QString t = item->property("text").toString().trimmed();
            if (!t.isEmpty())
                return t;
        }
        queue.append(item->childItems());
    }
    return {};
}

QByteArray journalFlags(const QObject* obj)
{
    QByteArray f;
    if (obj->inherits("QQuickAbstractButton")) {
        f.append('b');
        if (obj->property("checkable").toBool()) {
            f.append('k');
            if (obj->property("checked").toBool())
                f.append('K');
        }
        if (obj->inherits("QQuickRadioButton") || obj->inherits("QQuickRadioDelegate"))
            f.append('r');
        if (obj->inherits("QQuickTabButton"))
            f.append('t');
    }
    if (obj->inherits("QQuickComboBox"))
        f.append('x');
    if (obj->inherits("QQuickPopupItem"))
        f.append('p');
    if (journalIsTextInput(obj))
        f.append('i');
    if (obj->inherits("QQuickPointerHandler"))
        f.append('h');
    if (obj->inherits("QQuickMouseArea"))
        f.append('m');
    if (obj->inherits("QQuickFlickable"))
        f.append('f');
    if (obj->inherits("QQuickScrollBar") || obj->inherits("QQuickSlider")
        || obj->inherits("QQuickRangeSlider") || obj->inherits("QQuickScrollIndicator"))
        f.append('s');
    return f;
}

// Звено вверх: у элемента — визуальный родитель (у всплывашки — её Popup,
// иначе цепочка ушла бы в слой окна и потеряла страницу), у прочих —
// QObject-родитель.
QObject* journalUp(QObject* obj)
{
    if (auto* item = qobject_cast<QQuickItem*>(obj)) {
        if (!obj->inherits("QQuickPopupItem") && item->parentItem() != nullptr)
            return item->parentItem();
    }
    return obj->parent();
}

QByteArray journalDescribe(QObject* first)
{
    QByteArray out;
    int depth = 0;
    for (QObject* obj = first; obj != nullptr && depth < 64; obj = journalUp(obj), ++depth) {
        QString inner;
        if (depth < 3) {
            if (auto* item = qobject_cast<QQuickItem*>(obj))
                inner = journalInnerText(item);
        }
        out.append(obj->metaObject()->className());
        out.append('\t');
        out.append(journalFlags(obj));
        out.append('\t');
        out.append(journalClean(obj->objectName()).toUtf8());
        out.append('\t');
        out.append(journalClean(journalOwnText(obj)).toUtf8());
        out.append('\t');
        out.append(journalClean(inner).toUtf8());
        out.append('\n');
    }
    return out;
}

// Запасной путь, если захвата нет: самый верхний видимый элемент под точкой.
QQuickItem* journalItemAt(QQuickWindow* window, QPointF scenePos)
{
    QQuickItem* item = window->contentItem();
    while (item != nullptr) {
        QList<QQuickItem*> kids = item->childItems();
        std::stable_sort(kids.begin(), kids.end(),
                         [](QQuickItem* a, QQuickItem* b) { return a->z() < b->z(); });
        QQuickItem* hit = nullptr;
        for (auto it = kids.crbegin(); it != kids.crend(); ++it) {
            QQuickItem* k = *it;
            if (k->isVisible() && k->opacity() > 0.0 && k->contains(k->mapFromScene(scenePos))) {
                hit = k;
                break;
            }
        }
        if (hit == nullptr)
            break;
        item = hit;
    }
    return item;
}

class InputJournalFilter : public QObject {
public:
    using Begin = unsigned long long (*)(const char*, int, int);
    using Finish = void (*)(unsigned long long);

    InputJournalFilter(Begin begin, Finish finish, QObject* parent)
        : QObject(parent), m_begin(begin), m_finish(finish) {}

protected:
    bool eventFilter(QObject* obj, QEvent* e) override
    {
        // Окну событие приходит ПЕРВЫМ (от системы); элементам его потом
        // раздаёт само окно — их копии здесь не нужны.
        auto* window = qobject_cast<QQuickWindow*>(obj);
        if (window == nullptr)
            return false;
        switch (e->type()) {
        case QEvent::MouseButtonPress: {
            auto* me = static_cast<QMouseEvent*>(e);
            if (me->button() == Qt::LeftButton) {
                m_pressWindow = window;
                m_pressPos = me->scenePosition();
            }
            break;
        }
        case QEvent::MouseButtonRelease: {
            auto* me = static_cast<QMouseEvent*>(e);
            if (me->button() != Qt::LeftButton || m_pressWindow != window)
                break;
            m_pressWindow = nullptr;
            const QPointF d = me->scenePosition() - m_pressPos;
            const int drag = QGuiApplication::styleHints()->startDragDistance();
            if (qAbs(d.x()) + qAbs(d.y()) > drag)
                break;   // протяжка/прокрутка, не щелчок
            QObject* target = nullptr;
            if (!me->points().isEmpty()) {
                const QEventPoint& pt = me->points().first();
                target = me->exclusiveGrabber(pt);
                if (target == nullptr) {
                    const auto passive = me->passiveGrabbers(pt);
                    for (const auto& p : passive) {
                        if (p) {
                            target = p.data();
                            break;
                        }
                    }
                }
            }
            if (target == nullptr)
                target = journalItemAt(window, me->scenePosition());
            report(target, 0);
            break;
        }
        case QEvent::KeyPress:
        case QEvent::KeyRelease: {
            auto* ke = static_cast<QKeyEvent*>(e);
            if (ke->isAutoRepeat())
                break;
            const bool enter = e->type() == QEvent::KeyPress
                && (ke->key() == Qt::Key_Return || ke->key() == Qt::Key_Enter);
            const bool space = e->type() == QEvent::KeyRelease && ke->key() == Qt::Key_Space;
            if (enter || space)
                report(window->activeFocusItem(), 1);
            break;
        }
        default:
            break;
        }
        return false;
    }

private:
    void report(QObject* target, int viaKey)
    {
        if (target == nullptr)
            return;
        const QByteArray desc = journalDescribe(target);
        const unsigned long long id = m_begin(desc.constData(), int(desc.size()), viaKey);
        if (id == 0)
            return;
        // Конец действия — когда окно раздало событие и вернулось в цикл.
        Finish finish = m_finish;
        QMetaObject::invokeMethod(this, [finish, id] { finish(id); }, Qt::QueuedConnection);
    }

    Begin m_begin;
    Finish m_finish;
    QPointer<QQuickWindow> m_pressWindow;
    QPointF m_pressPos;
};

}  // namespace

extern "C" bool rexon_install_input_journal(unsigned long long (*begin)(const char*, int, int),
                                            void (*finish)(unsigned long long))
{
    static QPointer<InputJournalFilter> filter;
    QCoreApplication* app = QCoreApplication::instance();
    if (app == nullptr || begin == nullptr || finish == nullptr)
        return false;
    if (filter)
        return true;
    filter = new InputJournalFilter(begin, finish, app);
    app->installEventFilter(filter);
    return true;
}

void rexon_process_events(int ms)
{
    // Крутим ВЕСЬ отведённый срок, а не «пока есть события». Голый
    // processEvents(AllEvents, ms) возвращается сразу, как только очередь
    // пуста, — а стенду поздних колбэков надо дождаться фонового потока,
    // который ещё спит и события пока не поставил.
    QDeadlineTimer deadline(ms);
    while (!deadline.hasExpired()) {
        QCoreApplication::processEvents(QEventLoop::AllEvents, 5);
        QThread::msleep(1);
    }
}

// ── Нажатия вне вкладки обоев при открытом окне поверх ────────────────────

namespace {

/// Фильтр событий окна: нажатие мыши вне `area`, которое модальный слой
/// Material съел бы, показывается слою (он закрывает свои окна поверх, как
/// по любому нажатию мимо) и затем отдаётся окну напрямую.
class OutsidePressPass final : public QObject {
public:
    OutsidePressPass(QQuickItem* area, QQuickWindow* window)
        : QObject(area), m_area(area), m_window(window)
    {
        setObjectName(QLatin1String(objectNameLiteral));
    }

    static constexpr const char* objectNameLiteral = "rexonOutsidePressPass";

    QQuickWindow* window() const { return m_window; }

    bool eventFilter(QObject* watched, QEvent* event) override
    {
        if (m_forwarding || watched != m_window || !m_area || !m_window)
            return false;
        const auto type = event->type();
        if (type != QEvent::MouseButtonPress && type != QEvent::MouseButtonDblClick
            && type != QEvent::MouseMove && type != QEvent::MouseButtonRelease)
            return false;
        auto* mouse = static_cast<QMouseEvent*>(event);
        if (type == QEvent::MouseButtonPress || type == QEvent::MouseButtonDblClick) {
            // Жест начинается нажатием: он наш, если нажали вне вкладки и
            // у слоя есть что показать (без окон поверх слой ничего не
            // блокирует, и вмешиваться незачем).
            m_passing = m_area->isVisible() && outside(mouse->position()) && overlayBusy();
        }
        if (!m_passing)
            return false;
        if (type == QEvent::MouseButtonRelease && mouse->buttons() == Qt::NoButton)
            m_passing = false;
        QObject* overlay = overlayItem();
        QPointer<QQuickWindow> window = m_window;
        m_forwarding = true;
        if (overlay)
            overlay->eventFilter(watched, event);
        if (window)
            static_cast<QObject*>(window.data())->event(event);
        m_forwarding = false;
        return true;
    }

private:
    bool outside(const QPointF& scenePoint) const
    {
        return !m_area->contains(m_area->mapFromScene(scenePoint));
    }

    QQuickItem* overlayItem() const
    {
        return m_window->contentItem()->findChild<QQuickItem*>(
            QStringLiteral("Qcm.Material.Overlay"), Qt::FindDirectChildrenOnly);
    }

    /// У слоя есть видимое содержимое (окно поверх, его затемнение).
    bool overlayBusy() const
    {
        const auto* overlay = overlayItem();
        if (!overlay)
            return false;
        const auto children = overlay->childItems();
        for (const auto* child : children)
            if (child->isVisible())
                return true;
        return false;
    }

    QPointer<QQuickItem> m_area;
    QPointer<QQuickWindow> m_window;
    bool m_passing = false;
    bool m_forwarding = false;
};

} // namespace

void rexon_pass_outside_presses(QObject* area)
{
    auto* item = qobject_cast<QQuickItem*>(area);
    if (!item || !item->window())
        return;
    auto* window = item->window();
    // Тип без Q_OBJECT: ищем по имени, а не по типу.
    auto* pass = static_cast<OutsidePressPass*>(item->findChild<QObject*>(
        QLatin1String(OutsidePressPass::objectNameLiteral), Qt::FindDirectChildrenOnly));
    if (pass && pass->window() != window) {
        delete pass;
        pass = nullptr;
    }
    if (!pass)
        pass = new OutsidePressPass(item, window);
    // Последний поставленный фильтр зовётся первым.
    window->removeEventFilter(pass);
    window->installEventFilter(pass);
}

extern "C" void rexon_set_app_link_color(bool set, unsigned char r, unsigned char g, unsigned char b)
{
    if (QGuiApplication::instance() == nullptr)
        return;
    // Пустая палитра с двумя заданными ролями: Qt 6 достраивает остальные
    // роли из системной темы, так что прочие цвета приложения не трогаются.
    QPalette palette;
    if (set) {
        const QColor color(r, g, b);
        palette.setColor(QPalette::Link, color);
        palette.setColor(QPalette::LinkVisited, color);
    }
    QGuiApplication::setPalette(palette);
}

// Свой C++ здесь вынужденный: `cxx` не умеет назвать вложенное в класс
// перечисление `QChar::Script` (генератор открывает `namespace QChar`, а это
// класс — сборка падает, проверено 06.11.2026), а без этого типа статический
// член `QFontDatabase` из моста не объявить. Решения тут нет — только
// перевод нашего номера в номер Qt.
extern "C" void rexon_add_script_fallback_font(unsigned char script, const char* family, std::size_t len)
{
    QChar::Script qtScript;
    switch (script) {
    case 0: qtScript = QChar::Script_Arabic; break;
    case 1: qtScript = QChar::Script_Hebrew; break;
    default: return;
    }
    QFontDatabase::addApplicationFallbackFontFamily(
        qtScript, QString::fromUtf8(family, static_cast<qsizetype>(len)));
}
