// Императивная регистрация типов rexon-gui в модуле `waywallen.ui` — тем
// же способом, каким это делал C++-оригинал waywallen и каким уже работает
// модель фильтров (rexon-qt-filter/cpp/rexon_register.cpp).
//
// Сюда добавляется по строке на каждый перенесённый тип; счётчик регистраций в
// `bridge.py::register_qml_types()` на столько же убывает.
#include "rexon_register_we.h"

// Заголовки генерирует cxx-qt из мостов; путь внутри include-дерева
// повторяет путь файла в крейте.
#include "rexon-gui/src/autostart_queries.cxxqt.h"
#include "rexon-gui/src/discover_queries.cxxqt.h"
#include "rexon-gui/src/display_queries.cxxqt.h"
#include "rexon-gui/src/global_pause_queries.cxxqt.h"
#include "rexon-gui/src/playlist_item_select_storage.cxxqt.h"
#include "rexon-gui/src/playlist_queries.cxxqt.h"
#include "rexon-gui/src/plugin_queries.cxxqt.h"
#include "rexon-gui/src/plugin_translations.cxxqt.h"
#include "rexon-gui/src/remote_store_queries.cxxqt.h"
#include "rexon-gui/src/settings_queries.cxxqt.h"
#include "rexon-gui/src/source_queries.cxxqt.h"
#include "rexon-gui/src/status_queries.cxxqt.h"
#include "rexon-gui/src/tag_queries.cxxqt.h"
#include "rexon-gui/src/thumbnail_request.cxxqt.h"
#include "rexon-gui/src/we_action.cxxqt.h"
#include "rexon-gui/src/we_app.cxxqt.h"
#include "rexon-gui/src/we_daemon_dbus.cxxqt.h"
#include "rexon-gui/src/we_notify.cxxqt.h"
#include "rexon-gui/src/we_objmodel.cxxqt.h"
#include "rexon-gui/src/we_util.cxxqt.h"
#include "rexon-gui/src/user_property_model.cxxqt.h"
#include "rexon-gui/src/wallpaper_actions.cxxqt.h"
#include "rexon-gui/src/wallpaper_queries.cxxqt.h"
#include "rexon-gui/src/wallpaper_select_storage.cxxqt.h"

#include <QtQml/qqml.h>

#include <QtCore/QCoreApplication>
#include <QtCore/QJsonDocument>
#include <QtCore/QJsonObject>
#include <QtCore/QJsonParseError>
#include <QtCore/QString>
#include <QtCore/QVariant>
#include <QtQml/QJSEngine>
#include <QtQml/QQmlEngine>
#include <QtQml/qqml.h>

namespace {

/// Единственный экземпляр синглтона на весь процесс.
///
/// Родитель — приложение, как у Python-моста и по той же причине: синглтон
/// обязан пережить движок QML вкладки. Вкладка WE строит движок лениво и
/// может пересоздать его, а `Notify` к этому моменту уже держит состояние
/// демона, полученное стартовым `status_sync`.
template <typename T>
T* singleton_instance()
{
    static T* the = nullptr;
    if (the == nullptr) {
        the = new T();
        // setParent, а не аргумент конструктора: у типов с объявленным
        // конструктором cxx-qt родителя в конструкторе нет.
        the->setParent(QCoreApplication::instance());
    }
    return the;
}

/// Фабрика для `qmlRegisterSingletonType`. CppOwnership — ровно то, что
/// делает `create(QQmlEngine*, QJSEngine*)` оригинала: движок QML не должен
/// снести объект, переживающий его самого.
template <typename T>
QObject* singleton_provider(QQmlEngine*, QJSEngine*)
{
    T* instance = singleton_instance<T>();
    QJSEngine::setObjectOwnership(instance, QJSEngine::CppOwnership);
    return instance;
}

}  // namespace


extern "C" void rexon_we_set_plugin_locale(const QString& locale)
{
    if (locale.isEmpty())
        return;
    singleton_instance<PluginTranslations>()->setLocale(locale);
}


QString rexon_we_plugin_translate(const QString& message_json)
{
    if (message_json.isEmpty())
        return {};
    QJsonParseError error {};
    const auto doc = QJsonDocument::fromJson(message_json.toUtf8(), &error);
    if (error.error != QJsonParseError::NoError || !doc.isObject())
        return {};
    return singleton_instance<PluginTranslations>()->translate(doc.object().toVariantMap());
}


extern "C" void rexon_do_register_we_types()
{
    // Идемпотентно: Python зовёт регистрацию один раз, но повторный вызов
    // (второе окно, перезапуск вкладки) не должен ронять Qt.
    static bool registered = false;
    if (registered)
        return;
    registered = true;

    // Партия 1: редактор свойств обои в панели деталей. Имя типа обязано
    // совпадать посимвольно — вендорный QML не трогаем.
    qmlRegisterType<UserPropertyListModel>("waywallen.ui", 1, 0,
                                           "UserPropertyListModel");

    // Партии 2-3: страница Discover целиком — четыре запроса и модель
    // выдачи, которой владеет запрос поиска. Сама RemoteListModel в QML не
    // создаётся ни разу (её отдаёт свойство `searchQuery.model`), поэтому
    // регистрировать её отдельным типом незачем — как и у вендора.
    qmlRegisterType<RemoteAvailabilityQuery>("waywallen.ui", 1, 0,
                                             "RemoteAvailabilityQuery");
    qmlRegisterType<RemoteSearchQuery>("waywallen.ui", 1, 0,
                                       "RemoteSearchQuery");
    qmlRegisterType<RemoteDetailsQuery>("waywallen.ui", 1, 0,
                                        "RemoteDetailsQuery");
    qmlRegisterType<RemoteDownloadQuery>("waywallen.ui", 1, 0,
                                         "RemoteDownloadQuery");

    // Партия 4: страница «Обои». Модель библиотеки отдельным типом не
    // регистрируется — ею владеет запрос и отдаёт свойством `data`, как у
    // вендора. Хранилище выбора берёт её тем же свойством, поэтому оба
    // типа обязаны приехать ОДНОЙ партией: питоновская модель Rust-хранилищу
    // не подойдёт (qobject_cast её не примет), и наоборот.
    qmlRegisterType<WallpaperListQuery>("waywallen.ui", 1, 0,
                                        "WallpaperListQuery");
    qmlRegisterType<WallpaperSelectStorage>("waywallen.ui", 1, 0,
                                            "WallpaperSelectStorage");

    // Вторая половина партии 4: запросы-действия страницы. Друг от друга
    // они не зависят, но регистрируются вместе — половинчатая страница
    // означала бы, что часть кнопок ходит одним путём, а часть другим.
    qmlRegisterType<WallpaperApplyQuery>("waywallen.ui", 1, 0,
                                         "WallpaperApplyQuery");
    qmlRegisterType<WallpaperGetQuery>("waywallen.ui", 1, 0, "WallpaperGetQuery");
    qmlRegisterType<WallpaperRemoveQuery>("waywallen.ui", 1, 0,
                                          "WallpaperRemoveQuery");
    qmlRegisterType<WallpaperScanQuery>("waywallen.ui", 1, 0, "WallpaperScanQuery");
    qmlRegisterType<WallpaperPropertySetQuery>("waywallen.ui", 1, 0,
                                               "WallpaperPropertySetQuery");
    qmlRegisterType<WallpaperLayoutSetQuery>("waywallen.ui", 1, 0,
                                             "WallpaperLayoutSetQuery");
    qmlRegisterType<WallpaperApplyViaPortalQuery>("waywallen.ui", 1, 0,
                                                  "WallpaperApplyViaPortalQuery");
    // waywallen 0.4.4: скрыть обои (страница «Обои» и «Скрытые обои»).
    qmlRegisterType<WallpaperHideQuery>("waywallen.ui", 1, 0, "WallpaperHideQuery");

    // Партия 6: всё, чем живут страницы управления, кроме дисплеев и
    // настроек. Порядок здесь повторяет порядок страниц вкладки, а не
    // алфавит: так видно, что ни одна страница не осталась наполовину.
    //
    // Менеджер плагинов — PluginManagePage и PluginSettingsPage.
    qmlRegisterType<PluginListQuery>("waywallen.ui", 1, 0, "PluginListQuery");
    qmlRegisterType<PluginInstallQuery>("waywallen.ui", 1, 0,
                                        "PluginInstallQuery");
    qmlRegisterType<PluginInspectQuery>("waywallen.ui", 1, 0,
                                        "PluginInspectQuery");
    qmlRegisterType<PluginDeleteQuery>("waywallen.ui", 1, 0, "PluginDeleteQuery");
    qmlRegisterType<PluginUpdateCheckQuery>("waywallen.ui", 1, 0,
                                            "PluginUpdateCheckQuery");
    qmlRegisterType<PluginUpdateInstallQuery>("waywallen.ui", 1, 0,
                                              "PluginUpdateInstallQuery");
    qmlRegisterType<PluginActionQuery>("waywallen.ui", 1, 0, "PluginActionQuery");

    // Плейлисты — листы выбора на странице «Обои» и страница правки.
    qmlRegisterType<PlaylistListQuery>("waywallen.ui", 1, 0, "PlaylistListQuery");
    qmlRegisterType<PlaylistMutationQuery>("waywallen.ui", 1, 0,
                                           "PlaylistMutationQuery");
    // Вендор 0.3.7: подробности одного плейлиста (PlaylistEditPage) и
    // выбор его состава. Хранилище объявлено ПРЯМО в WallpaperPage.qml —
    // без него страница «Обои» не создаётся вовсе.
    qmlRegisterType<PlaylistDetailQuery>("waywallen.ui", 1, 0,
                                         "PlaylistDetailQuery");
    qmlRegisterType<PlaylistItemSelectStorage>("waywallen.ui", 1, 0,
                                               "PlaylistItemSelectStorage");

    // Источники и библиотеки — SourceManagePage и AddLibraryPage.
    qmlRegisterType<SourceListQuery>("waywallen.ui", 1, 0, "SourceListQuery");
    qmlRegisterType<LibraryAddQuery>("waywallen.ui", 1, 0, "LibraryAddQuery");
    // waywallen 0.5.0: переименование библиотеки.
    qmlRegisterType<LibraryRenameQuery>("waywallen.ui", 1, 0, "LibraryRenameQuery");
    qmlRegisterType<LibraryRemoveQuery>("waywallen.ui", 1, 0, "LibraryRemoveQuery");
    qmlRegisterType<LibraryAutoDetectQuery>("waywallen.ui", 1, 0,
                                            "LibraryAutoDetectQuery");

    // Теги и возрастные рейтинги — фильтры «Обои» и Discover.
    qmlRegisterType<TagListQuery>("waywallen.ui", 1, 0, "TagListQuery");
    qmlRegisterType<ContentRatingListQuery>("waywallen.ui", 1, 0,
                                            "ContentRatingListQuery");

    // Миниатюры карточек.
    qmlRegisterType<ThumbnailRequest>("waywallen.ui", 1, 0, "ThumbnailRequest");

    // Автозапуск (v0.2.5): у нас роутится локально — systemd/XDG.
    qmlRegisterType<AutostartGetQuery>("waywallen.ui", 1, 0, "AutostartGetQuery");
    qmlRegisterType<AutostartSetQuery>("waywallen.ui", 1, 0, "AutostartSetQuery");

    // Типы v0.2.6 — RemoteInfoPage, RemoteManagePage и панель деталей.
    qmlRegisterType<RemoteStoreItem>("waywallen.ui", 1, 0, "RemoteStoreItem");
    qmlRegisterType<QrLoginCancelQuery>("waywallen.ui", 1, 0, "QrLoginCancelQuery");
    qmlRegisterType<RemoteSettingsPatchQuery>("waywallen.ui", 1, 0,
                                              "RemoteSettingsPatchQuery");
    qmlRegisterType<RemoteSubscriptionQuery>("waywallen.ui", 1, 0,
                                             "RemoteSubscriptionQuery");
    qmlRegisterType<WallpaperUnsubscribeQuery>("waywallen.ui", 1, 0,
                                               "WallpaperUnsubscribeQuery");

    // Партия 7: то, что вендор добавил в v0.3.5 и чего у нас не было ни
    // на C++, ни на Python. Без этих шести имён вендорный QML не
    // КОМПИЛИРУЕТСЯ: «is not a type» роняет страницу целиком, а не одну
    // кнопку, — так вкладка теряла «Дисплеи» и главное окно.
    //
    // Холсты — DisplaysPage.
    qmlRegisterType<CanvasLayoutSetQuery>("waywallen.ui", 1, 0,
                                          "CanvasLayoutSetQuery");
    qmlRegisterType<CanvasMutationQuery>("waywallen.ui", 1, 0,
                                         "CanvasMutationQuery");
    // waywallen 0.5.0: DisplaysPage объявляет свойства ТИПОВ дисплея и
    // холста (`selected as W.Display`). Создаёт их не QML, а менеджеры
    // App, поэтому регистрация без создания — только имя типа в модуле.
    // Без неё «W.Display is not a type» роняет страницу «Мониторы» целиком.
    qmlRegisterUncreatableType<Display>("waywallen.ui", 1, 0, "Display",
                                        "Display is provided by App.displayManager");
    qmlRegisterUncreatableType<Canvas>("waywallen.ui", 1, 0, "Canvas",
                                       "Canvas is provided by App.displayManager");

    // Партия 8: остаток Python-моста. Страница «Статус» целиком,
    // раскладка и псевдоним дисплея, чтение настроек — после этой
    // партии типов из Python в модуле `waywallen.ui` не остаётся.
    qmlRegisterType<HealthQuery>("waywallen.ui", 1, 0, "HealthQuery");
    // waywallen 0.3.8: журнал демона одним запросом — диалог со
    // страницы «Статус».
    qmlRegisterType<DaemonLogQuery>("waywallen.ui", 1, 0, "DaemonLogQuery");
    qmlRegisterType<RendererListQuery>("waywallen.ui", 1, 0, "RendererListQuery");
    qmlRegisterType<RendererPluginListQuery>("waywallen.ui", 1, 0,
                                             "RendererPluginListQuery");
    qmlRegisterType<RendererKillQuery>("waywallen.ui", 1, 0, "RendererKillQuery");
    qmlRegisterType<DisplayLayoutSetQuery>("waywallen.ui", 1, 0,
                                           "DisplayLayoutSetQuery");
    qmlRegisterType<DisplayRenameQuery>("waywallen.ui", 1, 0, "DisplayRenameQuery");
    // waywallen 0.4.2: ручная пауза одного дисплея — DisplaysPage.
    qmlRegisterType<DisplayPauseSetQuery>("waywallen.ui", 1, 0,
                                          "DisplayPauseSetQuery");
    qmlRegisterType<SettingsGetQuery>("waywallen.ui", 1, 0, "SettingsGetQuery");
    qmlRegisterType<SettingsSetQuery>("waywallen.ui", 1, 0, "SettingsSetQuery");

    // Глобальные пауза/звук/стоп — StatusPage, плюс Ctrl+P в Window.qml.
    qmlRegisterType<GlobalPauseToggleQuery>("waywallen.ui", 1, 0,
                                            "GlobalPauseToggleQuery");
    qmlRegisterType<GlobalPauseSetQuery>("waywallen.ui", 1, 0,
                                         "GlobalPauseSetQuery");
    qmlRegisterType<GlobalMuteSetQuery>("waywallen.ui", 1, 0,
                                        "GlobalMuteSetQuery");
    qmlRegisterType<GlobalStopSetQuery>("waywallen.ui", 1, 0,
                                        "GlobalStopSetQuery");

    // Партия 5: пять синглтонов. Регистрация ИМЕННО singleton-овая — QML
    // обращается к ним по имени типа без создания экземпляра
    // (`W.Notify.daemonPhase`), и обычный qmlRegisterType здесь дал бы
    // «Cannot access property of non-object».
    qmlRegisterSingletonType<Util>("waywallen.ui", 1, 0, "Util",
                                   &singleton_provider<Util>);
    qmlRegisterSingletonType<Action>("waywallen.ui", 1, 0, "Action",
                                     &singleton_provider<Action>);
    qmlRegisterSingletonType<Notify>("waywallen.ui", 1, 0, "Notify",
                                     &singleton_provider<Notify>);
    qmlRegisterSingletonType<App>("waywallen.ui", 1, 0, "App",
                                  &singleton_provider<App>);
    qmlRegisterSingletonType<DaemonDBusClient>("waywallen.ui", 1, 0,
                                               "DaemonDBusClient",
                                               &singleton_provider<DaemonDBusClient>);

    // Подписи плагинов (waywallen 0.3.7). Вендорный `I18n.qml` состоит
    // ровно из обращения к этому синглтону, а его зовут семь страниц —
    // без регистрации не поднимется ни одна из них.
    qmlRegisterSingletonType<PluginTranslations>("waywallen.ui", 1, 0,
                                                 "PluginTranslations",
                                                 &singleton_provider<PluginTranslations>);

    // Проверка обновлений waywallen (апстрим 0.4.4): заглушка без сети —
    // движок обновляется вместе с пакетом Rexon (src/we_util.rs).
    qmlRegisterSingletonType<UpdateChecker>("waywallen.ui", 1, 0,
                                            "UpdateChecker",
                                            &singleton_provider<UpdateChecker>);

    // Notify и App создаются СРАЗУ, не дожидаясь первого обращения из QML.
    // Демон шлёт стартовый `status_sync` и снимки дисплеев/рендереров ровно
    // один раз — на подключение WebSocket, а движок вкладки строится лениво.
    // Синглтон, родившийся позже, потерял бы и переход в Ready, и весь
    // первый снимок. Оригинал делает то же самое и по той же причине
    // (`App::init`: «Eagerly construct the daemon-event mirror»).
    singleton_instance<Notify>();
    singleton_instance<App>();
}
