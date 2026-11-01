// ExtensionsModel — вкладка «Расширения» на Rust + cxx-qt + QML.
//
// Перенесена на QML по общей схеме вкладок: решения и подписи — ядро,
// здесь оболочка QObject, в QML только разметка.
//
// Особенность именно этой вкладки: до переноса она была ЕДИНСТВЕННОЙ крупной
// Qt-вкладкой, не подключённой к ядру вовсе — в `rexon/qt_extensions/window.py`
// не встречалось ни одного обращения к `rexon_py`, хотя вся механика
// расширений давно жила в `rexon_core::{ext_manager, extension, constants}`.
// Поэтому перенос — это не только разметка: содержимое витрины уехало в
// `rexon_core::qt_extensions`, а состав установленного модель читает с диска
// сама, через ядро.
//
// Что здесь есть и чего нет:
//   * ЕСТЬ оболочка QObject: модель списка расширений (роли для делегата в
//     обоих режимах показа), неизменные подписи страницы, кнопки фильтров,
//     строка статистики, подробный разбор одним вызовом, тексты строки
//     состояния;
//   * НЕТ ни одной строки продуктовых решений — что показать, в каком
//     порядке, каким цветом и какими словами, отвечает ядро.
//
// Побочные действия (установка .rxn, удаление, включение/выключение) в этот
// объект НЕ входят: они ходят в файловую систему, запускают скрипты
// расширений и при нужде поднимают pkexec — всё это пока делает
// `rexon/extensions/manager.py`. QML зовёт их через объект `actions`,
// который кладёт хозяин страницы, и после каждого исхода просит модель
// перечитать состав (`refresh`).

/// Мост cxx-qt: содержимое вкладки «Расширения». Оно же — модель списка.
///
/// ⚠️ Тип зовётся `ExtensionsModel`, а не `ExtensionsPage`: .qml-файлы модуля
/// сами становятся QML-типами по имени файла, и `ExtensionsPage` занят
/// `qml/ExtensionsPage.qml`. Правило на все вкладки: вид — `<Имя>Page.qml`,
/// объект — `<Имя>Model`.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// Базовый Qt-тип. БЕЗ `#[qobject]`: на каждый `#[qobject]`-тип в
        /// `extern "C++Qt"` cxx-qt печатает шимы приведения с именем по ТИПУ,
        /// и второй мост крейта с той же базой даёт `duplicate symbol`.
        type QAbstractListModel;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        // ── Неизменные подписи страницы ──────────────────────────────────
        #[qproperty(QString, window_title, cxx_name = "windowTitle",
                    READ = window_title, CONSTANT)]
        #[qproperty(i32, window_min_width, cxx_name = "windowMinWidth",
                    READ = window_min_width, CONSTANT)]
        #[qproperty(i32, window_min_height, cxx_name = "windowMinHeight",
                    READ = window_min_height, CONSTANT)]
        #[qproperty(QString, header_title, cxx_name = "headerTitle",
                    READ = header_title, CONSTANT)]
        #[qproperty(i32, header_title_point_size, cxx_name = "headerTitlePointSize",
                    READ = header_title_point_size, CONSTANT)]
        #[qproperty(i32, section_point_size, cxx_name = "sectionPointSize",
                    READ = section_point_size, CONSTANT)]
        #[qproperty(QString, search_placeholder, cxx_name = "searchPlaceholder",
                    READ = search_placeholder, CONSTANT)]
        #[qproperty(QString, install_button_text, cxx_name = "installButtonText",
                    READ = install_button_text, CONSTANT)]
        #[qproperty(QString, view_grid_text, cxx_name = "viewGridText",
                    READ = view_grid_text, CONSTANT)]
        #[qproperty(QString, view_grid_tooltip, cxx_name = "viewGridTooltip",
                    READ = view_grid_tooltip, CONSTANT)]
        #[qproperty(QString, view_list_text, cxx_name = "viewListText",
                    READ = view_list_text, CONSTANT)]
        #[qproperty(QString, view_list_tooltip, cxx_name = "viewListTooltip",
                    READ = view_list_tooltip, CONSTANT)]
        #[qproperty(QString, install_section_header, cxx_name = "installSectionHeader",
                    READ = install_section_header, CONSTANT)]
        #[qproperty(QString, roadmap_note, cxx_name = "roadmapNote",
                    READ = roadmap_note, CONSTANT)]
        #[qproperty(QString, uninstall_icon, cxx_name = "uninstallIcon",
                    READ = uninstall_icon, CONSTANT)]
        #[qproperty(QString, uninstall_text, cxx_name = "uninstallText",
                    READ = uninstall_text, CONSTANT)]
        #[qproperty(QString, uninstall_tooltip, cxx_name = "uninstallTooltip",
                    READ = uninstall_tooltip, CONSTANT)]
        #[qproperty(i32, card_width, cxx_name = "cardWidth",
                    READ = card_width, CONSTANT)]
        #[qproperty(i32, card_text_width, cxx_name = "cardTextWidth",
                    READ = card_text_width, CONSTANT)]
        #[qproperty(i32, preview_height, cxx_name = "previewHeight",
                    READ = preview_height, CONSTANT)]
        // Блок «⬇ Установить»: [{kind, icon, label, tooltip, enabled, text}, …].
        #[qproperty(QList_QVariant, install_sources, cxx_name = "installSources",
                    READ = install_sources, CONSTANT)]
        #[qproperty(i32, source_min_width, cxx_name = "sourceMinWidth",
                    READ = source_min_width, CONSTANT)]
        #[qproperty(i32, source_max_width, cxx_name = "sourceMaxWidth",
                    READ = source_max_width, CONSTANT)]
        #[qproperty(i32, source_min_height, cxx_name = "sourceMinHeight",
                    READ = source_min_height, CONSTANT)]
        // Справка «Создать своё».
        #[qproperty(QString, create_title, cxx_name = "createTitle",
                    READ = create_title, CONSTANT)]
        #[qproperty(QString, create_header, cxx_name = "createHeader",
                    READ = create_header, CONSTANT)]
        #[qproperty(QString, create_body, cxx_name = "createBody",
                    READ = create_body, CONSTANT)]
        #[qproperty(QString, create_close_text, cxx_name = "createCloseText",
                    READ = create_close_text, CONSTANT)]
        #[qproperty(i32, create_min_width, cxx_name = "createMinWidth",
                    READ = create_min_width, CONSTANT)]
        #[qproperty(i32, create_min_height, cxx_name = "createMinHeight",
                    READ = create_min_height, CONSTANT)]
        // Подробный разбор.
        #[qproperty(i32, detail_min_width, cxx_name = "detailMinWidth",
                    READ = detail_min_width, CONSTANT)]
        #[qproperty(i32, detail_min_height, cxx_name = "detailMinHeight",
                    READ = detail_min_height, CONSTANT)]
        #[qproperty(i32, detail_title_point_size, cxx_name = "detailTitlePointSize",
                    READ = detail_title_point_size, CONSTANT)]
        #[qproperty(i32, detail_preview_height, cxx_name = "detailPreviewHeight",
                    READ = detail_preview_height, CONSTANT)]
        #[qproperty(QString, detail_open_text, cxx_name = "detailOpenText",
                    READ = detail_open_text, CONSTANT)]
        #[qproperty(QString, detail_uninstall_text, cxx_name = "detailUninstallText",
                    READ = detail_uninstall_text, CONSTANT)]
        #[qproperty(QString, detail_close_text, cxx_name = "detailCloseText",
                    READ = detail_close_text, CONSTANT)]
        // Окно выбора файла и подтверждение — хозяину побочных действий.
        #[qproperty(QString, file_dialog_title, cxx_name = "fileDialogTitle",
                    READ = file_dialog_title, CONSTANT)]
        #[qproperty(QString, file_dialog_filter, cxx_name = "fileDialogFilter",
                    READ = file_dialog_filter, CONSTANT)]
        #[qproperty(QString, confirm_title, cxx_name = "confirmTitle",
                    READ = confirm_title, CONSTANT)]
        // Палитра стандартного скина приложения (steam_dark, правило #17).
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        // Оформление вкладки, которого нет в палитре скина: чипы, плитка,
        // заглушка превью, строка списка, блок «Установить», раскладка.
        // Меняется вместе со скином (пары «тёмная/светлая»), поэтому тот же
        // сигнал, что у палитры.
        #[qproperty(QMap_QString_QVariant, styles, READ = styles,
                    NOTIFY = palette_changed)]
        // ── Меняющееся ───────────────────────────────────────────────────
        #[qproperty(QString, stats_line, cxx_name = "statsLine",
                    READ = stats_line, NOTIFY = summary_changed)]
        #[qproperty(QString, my_header, cxx_name = "myHeader",
                    READ = my_header, NOTIFY = summary_changed)]
        #[qproperty(QString, empty_text, cxx_name = "emptyText",
                    READ = empty_text, NOTIFY = summary_changed)]
        #[qproperty(QList_QVariant, filters, READ = filters,
                    NOTIFY = summary_changed)]
        #[qproperty(QString, filter_type, cxx_name = "filterType",
                    READ = filter_type, NOTIFY = summary_changed)]
        #[qproperty(QString, search_text, cxx_name = "searchText",
                    READ = search_text, NOTIFY = summary_changed)]
        #[qproperty(QString, view_mode, cxx_name = "viewMode",
                    READ = view_mode, NOTIFY = summary_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = summary_changed)]
        #[qproperty(QString, status_text, cxx_name = "statusText",
                    READ = status_text, NOTIFY = status_changed)]
        type ExtensionsModel = super::ExtensionsModelRust;
    }

    // ── Сигналы ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        /// Всё, что меняется вместе с составом витрины: статистика, заголовок
        /// «Мои расширения», подсказка пустого состояния, кнопки фильтров,
        /// счётчик. Один сигнал, а не пять: меняются они всегда разом, и
        /// пятью сигналами разметка получила бы пять пересчётов подряд.
        /// Скин сменили на лету — палитру пересчитать.
        ///
        /// Свойство `palette` НЕ `CONSTANT` именно поэтому: без сигнала
        /// привязки QML (`readonly property var pal: page.palette`) не
        /// пересчитываются, и открытая вкладка остаётся в цветах прошлого
        /// скина, пока окно на виджетах уже перекрашено. Толчок приходит из
        /// общего реестра (`crate::skin`), а не отсюда.
        #[qsignal]
        #[cxx_name = "paletteChanged"]
        fn palette_changed(self: Pin<&mut ExtensionsModel>);

        #[qsignal]
        #[cxx_name = "summaryChanged"]
        fn summary_changed(self: Pin<&mut ExtensionsModel>);

        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut ExtensionsModel>);
    }

    // ── Наследуемые методы базового класса ────────────────────────────────
    //
    // Сброс модели целиком, а не точечное `dataChanged`: смена фильтра,
    // поиска или режима показа меняет и состав строк, и их порядок — это и
    // есть тот случай, ради которого в Qt существует reset.
    extern "RustQt" {
        /// # Safety
        /// Требует парного end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut ExtensionsModel>);
        /// # Safety
        /// Парный к begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut ExtensionsModel>);
    }

    // ── Переопределения QAbstractListModel ────────────────────────────────
    // Без #[qinvokable]: инвокабл в наследнике затеняет укороченные клоны
    // сигнатур базы, которые moc делает ради default-аргументов, и
    // rowCount() без аргументов из JS падает «Insufficient arguments».
    extern "RustQt" {
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &ExtensionsModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &ExtensionsModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &ExtensionsModel, index: &QModelIndex, role: i32) -> QVariant;
    }

    // ── Чтение свойств ────────────────────────────────────────────────────
    extern "RustQt" {
        #[cxx_name = "windowTitle"]
        fn window_title(self: &ExtensionsModel) -> QString;
        #[cxx_name = "windowMinWidth"]
        fn window_min_width(self: &ExtensionsModel) -> i32;
        #[cxx_name = "windowMinHeight"]
        fn window_min_height(self: &ExtensionsModel) -> i32;
        #[cxx_name = "headerTitle"]
        fn header_title(self: &ExtensionsModel) -> QString;
        #[cxx_name = "headerTitlePointSize"]
        fn header_title_point_size(self: &ExtensionsModel) -> i32;
        #[cxx_name = "sectionPointSize"]
        fn section_point_size(self: &ExtensionsModel) -> i32;
        #[cxx_name = "searchPlaceholder"]
        fn search_placeholder(self: &ExtensionsModel) -> QString;
        #[cxx_name = "installButtonText"]
        fn install_button_text(self: &ExtensionsModel) -> QString;
        #[cxx_name = "viewGridText"]
        fn view_grid_text(self: &ExtensionsModel) -> QString;
        #[cxx_name = "viewGridTooltip"]
        fn view_grid_tooltip(self: &ExtensionsModel) -> QString;
        #[cxx_name = "viewListText"]
        fn view_list_text(self: &ExtensionsModel) -> QString;
        #[cxx_name = "viewListTooltip"]
        fn view_list_tooltip(self: &ExtensionsModel) -> QString;
        #[cxx_name = "installSectionHeader"]
        fn install_section_header(self: &ExtensionsModel) -> QString;
        #[cxx_name = "roadmapNote"]
        fn roadmap_note(self: &ExtensionsModel) -> QString;
        #[cxx_name = "uninstallIcon"]
        fn uninstall_icon(self: &ExtensionsModel) -> QString;
        #[cxx_name = "uninstallText"]
        fn uninstall_text(self: &ExtensionsModel) -> QString;
        #[cxx_name = "uninstallTooltip"]
        fn uninstall_tooltip(self: &ExtensionsModel) -> QString;
        #[cxx_name = "cardWidth"]
        fn card_width(self: &ExtensionsModel) -> i32;
        #[cxx_name = "cardTextWidth"]
        fn card_text_width(self: &ExtensionsModel) -> i32;
        #[cxx_name = "previewHeight"]
        fn preview_height(self: &ExtensionsModel) -> i32;
        #[cxx_name = "installSources"]
        fn install_sources(self: &ExtensionsModel) -> QList_QVariant;
        #[cxx_name = "sourceMinWidth"]
        fn source_min_width(self: &ExtensionsModel) -> i32;
        #[cxx_name = "sourceMaxWidth"]
        fn source_max_width(self: &ExtensionsModel) -> i32;
        #[cxx_name = "sourceMinHeight"]
        fn source_min_height(self: &ExtensionsModel) -> i32;
        #[cxx_name = "createTitle"]
        fn create_title(self: &ExtensionsModel) -> QString;
        #[cxx_name = "createHeader"]
        fn create_header(self: &ExtensionsModel) -> QString;
        #[cxx_name = "createBody"]
        fn create_body(self: &ExtensionsModel) -> QString;
        #[cxx_name = "createCloseText"]
        fn create_close_text(self: &ExtensionsModel) -> QString;
        #[cxx_name = "createMinWidth"]
        fn create_min_width(self: &ExtensionsModel) -> i32;
        #[cxx_name = "createMinHeight"]
        fn create_min_height(self: &ExtensionsModel) -> i32;
        #[cxx_name = "detailMinWidth"]
        fn detail_min_width(self: &ExtensionsModel) -> i32;
        #[cxx_name = "detailMinHeight"]
        fn detail_min_height(self: &ExtensionsModel) -> i32;
        #[cxx_name = "detailTitlePointSize"]
        fn detail_title_point_size(self: &ExtensionsModel) -> i32;
        #[cxx_name = "detailPreviewHeight"]
        fn detail_preview_height(self: &ExtensionsModel) -> i32;
        #[cxx_name = "detailOpenText"]
        fn detail_open_text(self: &ExtensionsModel) -> QString;
        #[cxx_name = "detailUninstallText"]
        fn detail_uninstall_text(self: &ExtensionsModel) -> QString;
        #[cxx_name = "detailCloseText"]
        fn detail_close_text(self: &ExtensionsModel) -> QString;
        #[cxx_name = "fileDialogTitle"]
        fn file_dialog_title(self: &ExtensionsModel) -> QString;
        #[cxx_name = "fileDialogFilter"]
        fn file_dialog_filter(self: &ExtensionsModel) -> QString;
        #[cxx_name = "confirmTitle"]
        fn confirm_title(self: &ExtensionsModel) -> QString;
        fn palette(self: &ExtensionsModel) -> QMap_QString_QVariant;
        fn styles(self: &ExtensionsModel) -> QMap_QString_QVariant;
        #[cxx_name = "statsLine"]
        fn stats_line(self: &ExtensionsModel) -> QString;
        #[cxx_name = "myHeader"]
        fn my_header(self: &ExtensionsModel) -> QString;
        #[cxx_name = "emptyText"]
        fn empty_text(self: &ExtensionsModel) -> QString;
        fn filters(self: &ExtensionsModel) -> QList_QVariant;
        #[cxx_name = "filterType"]
        fn filter_type(self: &ExtensionsModel) -> QString;
        #[cxx_name = "searchText"]
        fn search_text(self: &ExtensionsModel) -> QString;
        #[cxx_name = "viewMode"]
        fn view_mode(self: &ExtensionsModel) -> QString;
        fn count(self: &ExtensionsModel) -> i32;
        #[cxx_name = "statusText"]
        fn status_text(self: &ExtensionsModel) -> QString;
    }

    // ── API для QML ───────────────────────────────────────────────────────
    //
    // ⚠️ Ни одного инвокабла с числом в подписи: cxx-qt пишет в метаобъект
    // тип как `::std::int32_t`, метатипа с таким именем нет, и вызов через
    // `QMetaObject.invokeMethod` молча возвращает False. Всё, что нужно
    // звать снаружи (проверки, хозяин страницы), принимает QString.
    extern "RustQt" {
        /// Перечитать состав установленных расширений с диска.
        #[qinvokable]
        fn refresh(self: Pin<&mut ExtensionsModel>);

        /// Выбрать фильтр по типу (`all` либо идентификатор типа).
        #[qinvokable]
        #[cxx_name = "setFilter"]
        fn set_filter(self: Pin<&mut ExtensionsModel>, filter_type: &QString);

        /// Задать строку поиска.
        #[qinvokable]
        #[cxx_name = "setSearch"]
        fn set_search(self: Pin<&mut ExtensionsModel>, text: &QString);

        /// Переключить режим показа (`grid` либо `list`).
        #[qinvokable]
        #[cxx_name = "setViewMode"]
        fn set_view_mode(self: Pin<&mut ExtensionsModel>, mode: &QString);

        /// Подробный разбор расширения целиком. Пустой QVariant — такого
        /// расширения на витрине нет (окно тогда не открывается).
        #[qinvokable]
        #[cxx_name = "detailFor"]
        fn detail_for(self: &ExtensionsModel, ext_id: &QString) -> QVariant;

        /// Текст подтверждения удаления с витрины.
        #[qinvokable]
        #[cxx_name = "confirmUninstall"]
        fn confirm_uninstall(self: &ExtensionsModel, ext_id: &QString) -> QString;

        /// Текст подтверждения удаления из подробного разбора (с каталогом).
        #[qinvokable]
        #[cxx_name = "confirmUninstallDetailed"]
        fn confirm_uninstall_detailed(self: &ExtensionsModel, ext_id: &QString) -> QString;

        /// Название расширения по идентификатору (для строки состояния).
        #[qinvokable]
        #[cxx_name = "displayName"]
        fn display_name(self: &ExtensionsModel, ext_id: &QString) -> QString;

        /// Принимаем ли такой файл перетаскиванием.
        #[qinvokable]
        #[cxx_name = "acceptsDrop"]
        fn accepts_drop(self: &ExtensionsModel, path: &QString) -> bool;

        // ── Строка состояния: текст собирает ядро, разметка только зовёт ──
        #[qinvokable]
        #[cxx_name = "noteInstalling"]
        fn note_installing(self: Pin<&mut ExtensionsModel>, path: &QString);
        /// Готовая строка в строку состояния (итог «Применить»).
        #[qinvokable]
        #[cxx_name = "noteText"]
        fn note_text(self: Pin<&mut ExtensionsModel>, text: &QString);
        #[qinvokable]
        #[cxx_name = "noteInstalled"]
        fn note_installed(self: Pin<&mut ExtensionsModel>, name: &QString);
        #[qinvokable]
        #[cxx_name = "noteInstallError"]
        fn note_install_error(self: Pin<&mut ExtensionsModel>, message: &QString);
        #[qinvokable]
        #[cxx_name = "noteInstallBusy"]
        fn note_install_busy(self: Pin<&mut ExtensionsModel>);
        #[qinvokable]
        #[cxx_name = "noteUninstalling"]
        fn note_uninstalling(self: Pin<&mut ExtensionsModel>, name: &QString);
        #[qinvokable]
        #[cxx_name = "noteUninstalled"]
        fn note_uninstalled(self: Pin<&mut ExtensionsModel>, ext_id: &QString);
        #[qinvokable]
        #[cxx_name = "noteUninstallError"]
        fn note_uninstall_error(self: Pin<&mut ExtensionsModel>, message: &QString);
        #[qinvokable]
        #[cxx_name = "noteUninstallBusy"]
        fn note_uninstall_busy(self: Pin<&mut ExtensionsModel>);
        #[qinvokable]
        #[cxx_name = "noteEnabled"]
        fn note_enabled(self: Pin<&mut ExtensionsModel>, ext_id: &QString);
        #[qinvokable]
        #[cxx_name = "noteDisabled"]
        fn note_disabled(self: Pin<&mut ExtensionsModel>, ext_id: &QString);
        #[qinvokable]
        #[cxx_name = "noteToggleError"]
        fn note_toggle_error(self: Pin<&mut ExtensionsModel>, message: &QString);

        /// Номер роли по имени (-1, если роли нет) — для проверок без
        /// делегата, как в соседних моделях крейта.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &ExtensionsModel, name: &QString) -> i32;

        /// Значение роли строки по её идентификатору — проверки без делегата.
        /// Номер строки не годится: инвокабл с числом снаружи не зовётся.
        #[qinvokable]
        #[cxx_name = "roleValueOf"]
        fn role_value_of(self: &ExtensionsModel, ext_id: &QString, name: &QString) -> QVariant;
    }

    // Объявление трейта обязано быть В МОСТЕ, иначе `impl cxx_qt::Initialize`
    // снаружи не находит конструктора: cxx-qt печатает его по этой строке.
    impl cxx_qt::Initialize for ExtensionsModel {}
    // Подписка на смену скина идёт через `CxxQtThread::queue`, и трейт
    // потока обязан быть объявлен В МОСТЕ: cxx-qt печатает по этой
    // строке и сам `qt_thread()`, и его алиас типа.
    impl cxx_qt::Threading for ExtensionsModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QList, QMap, QMapPair_QString_QVariant, QModelIndex, QString, QVariant,
};

use rexon_core::qt_extensions as qe;
use rexon_core::ui_base;

// ============================================================================
// Роли модели
// ============================================================================

/// Роли начинаются с Qt::UserRole + 1 (Qt::UserRole == 256).
const ROLE_BASE: i32 = 256 + 1;

/// Имена ролей в порядке номеров — единственный источник истины и для
/// roleNames, и для data(), и для roleNumber().
///
/// Роли покрывают ОБА режима показа сразу: делегат сетки берёт `typeLine` и
/// `name`, делегат списка — `titleHtml` и `subtitle`. Двух моделей на одну
/// витрину не заводим: состав и порядок у них общие, разной остаётся только
/// разметка.
const ROLE_NAMES: [&str; 16] = [
    "extId",
    "extType",
    "name",
    "nameTooltip",
    "typeLine",
    "authorLine",
    "imageUrl",
    "placeholder",
    "hasToggle",
    "stateText",
    "stateColor",
    "toggleText",
    "toggleIcon",
    "toggleTooltip",
    "titleHtml",
    "subtitle",
];

/// Роль заголовка группы режима «список» — она одна не про расширение, а про
/// его место в списке, поэтому идёт последней и отдельно.
const ROLE_GROUP_HEADER: &str = "groupHeader";
const ROLE_GROUP_HEADER_NUM: i32 = ROLE_BASE + ROLE_NAMES.len() as i32;

// ============================================================================
// Мелочи перевода в Qt
// ============================================================================

/// Собрать QVariantMap из пар. QML читает такую карту точкой (`row.label`).
fn vmap(pairs: Vec<(&str, QVariant)>) -> QMap<QMapPair_QString_QVariant> {
    let mut out = QMap::<QMapPair_QString_QVariant>::default();
    for (key, value) in pairs {
        out.insert(QString::from(key), value);
    }
    out
}

fn qstr(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

/// Путь на диске → адрес для `Image.source`. Пустой путь остаётся пустым:
/// разметка по нему решает, рисовать ли значок-заглушку.
fn file_url(path: &str) -> String {
    if path.is_empty() {
        String::new()
    } else {
        format!("file://{path}")
    }
}

// ============================================================================
// Rust-состояние страницы
// ============================================================================

/// Состояние вкладки: что прочитано с диска и что из этого показано.
#[derive(Default)]
pub struct ExtensionsModelRust {
    /// Номер записи в общем реестре смены скина (`crate::skin`).
    /// 0 — страница ещё не подписана.
    skin_watch: u64,
    /// Всё установленное, в порядке ядра.
    all: Vec<qe::ExtView>,
    /// Номера показанных строк в `all`, уже с учётом фильтра, поиска и
    /// режима показа (в режиме «список» порядок групповой).
    visible: Vec<usize>,
    /// Заголовок группы для каждой показанной строки («» — не первая в
    /// группе либо режим сетки).
    group_headers: Vec<String>,
    filter_type: String,
    search: String,
    view_mode: String,
    status: String,
    /// Состав собирается при первом обращении, а не в конструкторе:
    /// объявленный конструктор в cxx-qt отменяет дефолтный с родителем.
    loaded: bool,
}

/// Снять подписку на смену скина. Без этого реестр держал бы запись
/// закрытой вкладки до первой рассылки — та её отсеет по
/// `ObjectDestroyed`, но копить мусор незачем.
impl Drop for ExtensionsModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

impl ExtensionsModelRust {
    /// Пересобрать показанное по текущим фильтру, поиску и режиму.
    fn recompute(&mut self) {
        let filter = if self.filter_type.is_empty() {
            qe::FILTER_ALL_ID.to_string()
        } else {
            self.filter_type.clone()
        };
        // Фильтр мог остаться от состава, которого больше нет.
        let items = qe::filter_items(&qe::installed_types(&self.all));
        self.filter_type = qe::normalize_filter(&filter, &items);

        let picked = qe::apply_filters(&self.all, &self.filter_type, &self.search);
        // От ссылок обратно к номерам: модель хранит номера, чтобы не
        // держать вторую копию расширений.
        let mut order: Vec<usize> = picked
            .iter()
            .map(|p| {
                self.all
                    .iter()
                    .position(|e| std::ptr::eq(e, *p))
                    .expect("отобранное расширение обязано быть в составе")
            })
            .collect();
        let mut headers = vec![String::new(); order.len()];

        if self.view_mode == qe::VIEW_LIST {
            let groups = qe::list_groups(&picked);
            let mut grouped: Vec<usize> = Vec::with_capacity(order.len());
            let mut grouped_headers: Vec<String> = Vec::with_capacity(order.len());
            for grp in &groups {
                for (n, row) in grp.rows.iter().enumerate() {
                    grouped.push(order[*row]);
                    grouped_headers.push(if n == 0 {
                        grp.header_html.clone()
                    } else {
                        String::new()
                    });
                }
            }
            order = grouped;
            headers = grouped_headers;
        }

        self.visible = order;
        self.group_headers = headers;
    }

    fn ensure_loaded(&mut self) {
        if !self.loaded {
            self.view_mode = qe::VIEW_GRID.to_string();
            self.filter_type = qe::FILTER_ALL_ID.to_string();
            self.all = qe::installed();
            self.recompute();
            self.loaded = true;
        }
    }

    /// Расширение по номеру показанной строки.
    fn row(&self, row: i32) -> Option<&qe::ExtView> {
        let index = *self.visible.get(usize::try_from(row).ok()?)?;
        self.all.get(index)
    }

    /// Расширение по идентификатору — среди ВСЕГО установленного, а не
    /// только показанного: подробный разбор открывают и по строке, которую
    /// поиск через мгновение спрячет.
    fn by_id(&self, ext_id: &str) -> Option<&qe::ExtView> {
        self.all.iter().find(|e| e.ext_id == ext_id)
    }
}

impl qobject::ExtensionsModel {
    // ── Неизменные подписи ────────────────────────────────────────────────
    fn window_title(&self) -> QString {
        QString::from(&qe::window_title())
    }

    fn window_min_width(&self) -> i32 {
        qe::WINDOW_MIN_SIZE.0 as i32
    }

    fn window_min_height(&self) -> i32 {
        qe::WINDOW_MIN_SIZE.1 as i32
    }

    fn header_title(&self) -> QString {
        QString::from(&qe::header_title())
    }

    fn header_title_point_size(&self) -> i32 {
        qe::HEADER_TITLE_POINT_SIZE
    }

    fn section_point_size(&self) -> i32 {
        qe::SECTION_POINT_SIZE
    }

    fn search_placeholder(&self) -> QString {
        QString::from(&qe::search_placeholder())
    }

    fn install_button_text(&self) -> QString {
        QString::from(&qe::install_button_text())
    }

    fn view_grid_text(&self) -> QString {
        QString::from(qe::VIEW_GRID_TEXT)
    }

    fn view_grid_tooltip(&self) -> QString {
        QString::from(&qe::view_grid_tooltip())
    }

    fn view_list_text(&self) -> QString {
        QString::from(qe::VIEW_LIST_TEXT)
    }

    fn view_list_tooltip(&self) -> QString {
        QString::from(&qe::view_list_tooltip())
    }

    fn install_section_header(&self) -> QString {
        QString::from(&qe::install_section_header())
    }

    fn roadmap_note(&self) -> QString {
        QString::from(&qe::roadmap_note())
    }

    fn uninstall_icon(&self) -> QString {
        QString::from(qe::UNINSTALL_ICON)
    }

    fn uninstall_text(&self) -> QString {
        QString::from(&qe::uninstall_text())
    }

    fn uninstall_tooltip(&self) -> QString {
        QString::from(&qe::uninstall_tooltip())
    }

    fn card_width(&self) -> i32 {
        qe::CARD_W
    }

    fn card_text_width(&self) -> i32 {
        qe::CARD_TEXT_W
    }

    fn preview_height(&self) -> i32 {
        qe::PREVIEW_H
    }

    fn install_sources(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for source in qe::install_sources() {
            out.append(QVariant::from(&vmap(vec![
                ("kind", qstr(&source.kind)),
                ("icon", qstr(&source.icon)),
                ("label", qstr(&source.label)),
                ("tooltip", qstr(&source.tooltip)),
                ("text", qstr(&source.text())),
                ("enabled", QVariant::from(&source.enabled)),
            ])));
        }
        out
    }

    fn source_min_width(&self) -> i32 {
        qe::INSTALL_SOURCE_MIN_WIDTH
    }

    fn source_max_width(&self) -> i32 {
        qe::INSTALL_SOURCE_MAX_WIDTH
    }

    fn source_min_height(&self) -> i32 {
        qe::INSTALL_SOURCE_MIN_HEIGHT
    }

    fn create_title(&self) -> QString {
        QString::from(&qe::create_title())
    }

    fn create_header(&self) -> QString {
        QString::from(&qe::create_header())
    }

    fn create_body(&self) -> QString {
        QString::from(&qe::create_body_html())
    }

    fn create_close_text(&self) -> QString {
        QString::from(&qe::create_close_text())
    }

    fn create_min_width(&self) -> i32 {
        qe::CREATE_MIN_SIZE.0 as i32
    }

    fn create_min_height(&self) -> i32 {
        qe::CREATE_MIN_SIZE.1 as i32
    }

    fn detail_min_width(&self) -> i32 {
        qe::DETAIL_MIN_SIZE.0 as i32
    }

    fn detail_min_height(&self) -> i32 {
        qe::DETAIL_MIN_SIZE.1 as i32
    }

    fn detail_title_point_size(&self) -> i32 {
        qe::DETAIL_TITLE_POINT_SIZE
    }

    fn detail_preview_height(&self) -> i32 {
        qe::DETAIL_PREVIEW_SIZE.1 as i32
    }

    fn detail_open_text(&self) -> QString {
        QString::from(&qe::detail_open_folder_text())
    }

    fn detail_uninstall_text(&self) -> QString {
        QString::from(&qe::detail_uninstall_text())
    }

    fn detail_close_text(&self) -> QString {
        QString::from(&qe::detail_close_text())
    }

    fn file_dialog_title(&self) -> QString {
        QString::from(&qe::file_dialog_title())
    }

    fn file_dialog_filter(&self) -> QString {
        QString::from(&qe::file_dialog_filter())
    }

    fn confirm_title(&self) -> QString {
        QString::from(&qe::confirm_title())
    }

    /// Палитра ДЕЙСТВУЮЩЕГО скина (сохранённый `ui_skin`, откат к
    /// стандартному по правилу #17) плюс цвета, которые задаёт сама вкладка
    /// через ядро. Оформление QML-вкладки берётся отсюда, а не пишется
    /// заново: скин один на весь продукт.
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        out.insert(
            QString::from("state_enabled"),
            qstr(qe::STATE_ENABLED_COLOR),
        );
        out.insert(
            QString::from("state_disabled"),
            qstr(qe::STATE_DISABLED_COLOR),
        );
        out.insert(QString::from("accent_info"), qstr(qe::ACCENT_INFO_COLOR));
        // Начертание вторичных подписей (600 — `secondary_text_qss`
        // виджетов): число одно на обе обёртки, ключ — тот же, что у
        // «Сравнения».
        out.insert(
            QString::from("text_secondary_weight"),
            QVariant::from(&ui_base::SECONDARY_TEXT_WEIGHT),
        );
        out.insert(QString::from("danger"), qstr(qe::DANGER_COLOR));
        out.insert(
            QString::from("danger_fill_alpha"),
            QVariant::from(&qe::DANGER_FILL_ALPHA),
        );
        out.insert(
            QString::from("danger_fill_hover_alpha"),
            QVariant::from(&qe::DANGER_FILL_HOVER_ALPHA),
        );
        // Затемнение фона под модальным окном — общее решение продукта
        // (`ui_base::MODAL_DIM_RGBA`), тем же цветом гасит окно обучение.
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        out
    }

    /// Оформление вкладки, которого нет в палитре скина.
    ///
    /// ⚠️ КАЖДАЯ краска проходит через [`ui_base::css_color_to_qml`]: у ядра
    /// они набраны так, как их понимает QSS виджетов (`rgba(r,g,b,a)`), а QML
    /// такую запись молча читает как недопустимый цвет и рисует ЧЁРНЫМ.
    fn styles(&self) -> QMap<QMapPair_QString_QVariant> {
        let dark = ui_base::active_skin_is_dark();
        let col = |css: &str| qstr(&ui_base::css_color_to_qml(css));
        let num = |v: i32| QVariant::from(&v);

        let page = vmap(vec![
            ("margin", num(qe::PAGE_MARGIN)),
            ("spacing", num(qe::PAGE_SPACING)),
            ("gridSpacing", num(qe::GRID_SPACING)),
            ("filterSpacing", num(qe::FILTER_SPACING)),
            ("emptyPadding", num(qe::EMPTY_PADDING)),
        ]);

        let view_button = vmap(vec![
            ("pointSize", num(qe::VIEW_BUTTON_POINT_SIZE)),
            ("minWidth", num(qe::VIEW_BUTTON_MIN_WIDTH)),
        ]);

        let (chip_border, chip_on_bg, chip_on_border, chip_hover) = qe::chip_colors();
        let chip = vmap(vec![
            ("border", col(&chip_border)),
            ("checkedBackground", col(&chip_on_bg)),
            ("checkedBorder", col(&chip_on_border)),
            ("hoverBackground", col(&chip_hover)),
            ("radius", num(qe::CHIP_RADIUS)),
            ("paddingH", num(qe::CHIP_PADDING_H)),
            ("paddingV", num(qe::CHIP_PADDING_V)),
        ]);

        let (card_bg, card_border, card_hover) = qe::card_colors(dark);
        let card = vmap(vec![
            ("background", col(&card_bg)),
            ("border", col(&card_border)),
            ("hover", col(&card_hover)),
            ("radius", num(qe::CARD_RADIUS)),
            ("margin", num(qe::CARD_MARGIN)),
            ("spacing", num(qe::CARD_SPACING)),
            ("buttonSpacing", num(qe::CARD_BUTTON_SPACING)),
        ]);

        let (ph_bg, ph_fg) = qe::placeholder_colors(dark);
        let placeholder = vmap(vec![
            ("background", col(&ph_bg)),
            ("color", col(&ph_fg)),
            ("pointSize", num(qe::PLACEHOLDER_POINT_SIZE)),
        ]);

        let (row_bg, row_hover) = qe::list_row_colors(dark);
        let row = vmap(vec![
            ("background", col(&row_bg)),
            ("hover", col(&row_hover)),
            ("radius", num(qe::LIST_ROW_RADIUS)),
            ("marginH", num(qe::LIST_ROW_MARGIN_H)),
            ("marginV", num(qe::LIST_ROW_MARGIN_V)),
            ("padding", num(qe::LIST_ROW_PADDING)),
            ("spacing", num(qe::LIST_ROW_SPACING)),
            ("iconWidth", num(qe::LIST_ICON_WIDTH)),
            ("iconPointSize", num(qe::LIST_ICON_POINT_SIZE)),
            ("buttonWidth", num(qe::LIST_BUTTON_WIDTH)),
        ]);

        let group = vmap(vec![
            ("paddingTop", num(qe::GROUP_HEADER_PADDING_TOP)),
            ("paddingBottom", num(qe::GROUP_HEADER_PADDING_BOTTOM)),
        ]);

        let (src_bg, src_hover, src_border) = qe::install_source_colors(dark);
        let source = vmap(vec![
            ("background", col(&src_bg)),
            ("hover", col(&src_hover)),
            ("border", col(&src_border)),
            ("radius", num(qe::INSTALL_SOURCE_RADIUS)),
            ("padding", num(qe::INSTALL_SOURCE_PADDING)),
            ("spacing", num(qe::INSTALL_SOURCE_SPACING)),
        ]);

        // Модальные окна вкладки: разбор и справка «Создать своё».
        let dialog = vmap(vec![
            ("margin", num(qe::DIALOG_MARGIN)),
            ("detailSpacing", num(qe::DETAIL_SPACING)),
            ("createSpacing", num(qe::CREATE_SPACING)),
            ("techPaddingV", num(qe::DETAIL_TECH_PADDING_V)),
            ("bodyBorderWidth", num(qe::BODY_BORDER_WIDTH)),
            ("bodyRadius", num(qe::BODY_RADIUS)),
            ("bodyPadding", num(qe::BODY_PADDING)),
            ("bodyMinHeight", num(qe::BODY_MIN_HEIGHT)),
            (
                "dangerFillAlpha",
                QVariant::from(&qe::DETAIL_DANGER_FILL_ALPHA),
            ),
        ]);

        vmap(vec![
            ("page", QVariant::from(&page)),
            ("dialog", QVariant::from(&dialog)),
            ("viewButton", QVariant::from(&view_button)),
            ("chip", QVariant::from(&chip)),
            ("card", QVariant::from(&card)),
            ("placeholder", QVariant::from(&placeholder)),
            ("row", QVariant::from(&row)),
            ("group", QVariant::from(&group)),
            ("source", QVariant::from(&source)),
        ])
    }

    // ── Меняющееся ────────────────────────────────────────────────────────
    fn stats_line(&self) -> QString {
        QString::from(&qe::stats_line(&qe::stats(&self.rust().all)))
    }

    fn my_header(&self) -> QString {
        QString::from(
            &qe::my_header(self.rust().visible.len(), self.rust().all.len())
                .unwrap_or_default(),
        )
    }

    fn empty_text(&self) -> QString {
        QString::from(
            &qe::empty_text(self.rust().visible.len(), self.rust().all.len()).unwrap_or_default(),
        )
    }

    fn filters(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for item in qe::filter_items(&qe::installed_types(&self.rust().all)) {
            let checked = item.id == self.rust().filter_type;
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(&item.id)),
                ("label", qstr(&item.label)),
                ("checked", QVariant::from(&checked)),
            ])));
        }
        out
    }

    fn filter_type(&self) -> QString {
        QString::from(&self.rust().filter_type)
    }

    fn search_text(&self) -> QString {
        QString::from(&self.rust().search)
    }

    fn view_mode(&self) -> QString {
        QString::from(&self.rust().view_mode)
    }

    fn count(&self) -> i32 {
        self.rust().visible.len() as i32
    }

    fn status_text(&self) -> QString {
        QString::from(&self.rust().status)
    }

    // ── QAbstractListModel ────────────────────────────────────────────────
    fn row_count(&self, _parent: &QModelIndex) -> i32 {
        self.rust().visible.len() as i32
    }

    fn role_names(&self) -> cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray> {
        let mut roles = cxx_qt_lib::QHash::<cxx_qt_lib::QHashPair_i32_QByteArray>::default();
        for (offset, name) in ROLE_NAMES.iter().enumerate() {
            roles.insert(ROLE_BASE + offset as i32, QByteArray::from(*name));
        }
        roles.insert(ROLE_GROUP_HEADER_NUM, QByteArray::from(ROLE_GROUP_HEADER));
        roles
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let row = index.row();
        if role == ROLE_GROUP_HEADER_NUM {
            return match self.rust().group_headers.get(row as usize) {
                Some(header) => qstr(header),
                None => QVariant::default(),
            };
        }
        let Some(ext) = self.rust().row(row) else {
            return QVariant::default();
        };
        let Some(name) = ROLE_NAMES.get((role - ROLE_BASE) as usize) else {
            return QVariant::default();
        };
        role_value(ext, name)
    }

    // ── API для QML ───────────────────────────────────────────────────────
    fn refresh(mut self: Pin<&mut Self>) {
        unsafe { self.as_mut().begin_reset_model() };
        {
            let mut state = self.as_mut().rust_mut();
            state.all = qe::installed();
            state.loaded = true;
            state.recompute();
        }
        unsafe { self.as_mut().end_reset_model() };
        self.as_mut().summary_changed();
    }

    fn set_filter(mut self: Pin<&mut Self>, filter_type: &QString) {
        let wanted = filter_type.to_string();
        if self.rust().filter_type == wanted {
            return;
        }
        unsafe { self.as_mut().begin_reset_model() };
        {
            let mut state = self.as_mut().rust_mut();
            state.filter_type = wanted;
            state.recompute();
        }
        unsafe { self.as_mut().end_reset_model() };
        self.as_mut().summary_changed();
    }

    fn set_search(mut self: Pin<&mut Self>, text: &QString) {
        let wanted = text.to_string();
        if self.rust().search == wanted {
            return;
        }
        unsafe { self.as_mut().begin_reset_model() };
        {
            let mut state = self.as_mut().rust_mut();
            state.search = wanted;
            state.recompute();
        }
        unsafe { self.as_mut().end_reset_model() };
        self.as_mut().summary_changed();
    }

    fn set_view_mode(mut self: Pin<&mut Self>, mode: &QString) {
        let wanted = mode.to_string();
        // Режим переключается только на ИЗВЕСТНЫЙ: нажатие на уже
        // выбранную кнопку не должно снимать выбор (как в оригинале, где
        // кнопке возвращали `checked`).
        if wanted != qe::VIEW_GRID && wanted != qe::VIEW_LIST {
            return;
        }
        if self.rust().view_mode == wanted {
            return;
        }
        unsafe { self.as_mut().begin_reset_model() };
        {
            let mut state = self.as_mut().rust_mut();
            state.view_mode = wanted;
            state.recompute();
        }
        unsafe { self.as_mut().end_reset_model() };
        self.as_mut().summary_changed();
    }

    fn detail_for(&self, ext_id: &QString) -> QVariant {
        let Some(ext) = self.rust().by_id(&ext_id.to_string()) else {
            return QVariant::default();
        };
        let readme = qe::read_readme(ext);
        let home = std::env::var("HOME").unwrap_or_default();
        let d = qe::detail(ext, readme.as_deref(), &home);
        QVariant::from(&vmap(vec![
            ("extId", qstr(&d.ext_id)),
            ("windowTitle", qstr(&d.window_title)),
            ("headingHtml", qstr(&d.heading_html)),
            ("metaLine", qstr(&d.meta_line)),
            ("previewUrl", qstr(&file_url(&d.preview))),
            ("bodyHtml", qstr(&d.body_html)),
            ("tagsLine", qstr(&d.tags_line)),
            ("techHtml", qstr(&d.tech_html)),
            ("pathHtml", qstr(&d.path_html)),
            ("dir", qstr(&d.dir)),
            ("dirUrl", qstr(&file_url(&d.dir))),
        ]))
    }

    fn confirm_uninstall(&self, ext_id: &QString) -> QString {
        let id = ext_id.to_string();
        let name = self
            .rust()
            .by_id(&id)
            .map(|e| e.name.clone())
            .unwrap_or(id);
        QString::from(&qe::confirm_uninstall(&name))
    }

    fn confirm_uninstall_detailed(&self, ext_id: &QString) -> QString {
        let id = ext_id.to_string();
        match self.rust().by_id(&id) {
            Some(ext) => QString::from(&qe::confirm_uninstall_detailed(&ext.name, &ext.dir)),
            None => QString::from(&qe::confirm_uninstall(&id)),
        }
    }

    fn display_name(&self, ext_id: &QString) -> QString {
        let id = ext_id.to_string();
        QString::from(
            &self
                .rust()
                .by_id(&id)
                .map(|e| e.name.clone())
                .unwrap_or(id),
        )
    }

    fn accepts_drop(&self, path: &QString) -> bool {
        qe::is_supported_drop(&path.to_string())
    }

    // ── Строка состояния ──────────────────────────────────────────────────
    fn set_status(mut self: Pin<&mut Self>, text: String) {
        self.as_mut().rust_mut().status = text;
        self.as_mut().status_changed();
    }

    fn note_installing(self: Pin<&mut Self>, path: &QString) {
        let raw = path.to_string();
        let name = raw.rsplit('/').next().unwrap_or(&raw).to_string();
        self.set_status(qe::status_installing(&name));
    }

    fn note_text(self: Pin<&mut Self>, text: &QString) {
        self.set_status(text.to_string());
    }

    fn note_installed(self: Pin<&mut Self>, name: &QString) {
        self.set_status(qe::status_installed(&name.to_string()));
    }

    fn note_install_error(self: Pin<&mut Self>, message: &QString) {
        self.set_status(qe::status_install_error(&message.to_string()));
    }

    fn note_install_busy(self: Pin<&mut Self>) {
        self.set_status(qe::status_install_busy());
    }

    fn note_uninstalling(self: Pin<&mut Self>, name: &QString) {
        self.set_status(qe::status_uninstalling(&name.to_string()));
    }

    fn note_uninstalled(self: Pin<&mut Self>, ext_id: &QString) {
        self.set_status(qe::status_uninstalled(&ext_id.to_string()));
    }

    fn note_uninstall_error(self: Pin<&mut Self>, message: &QString) {
        self.set_status(qe::status_uninstall_error(&message.to_string()));
    }

    fn note_uninstall_busy(self: Pin<&mut Self>) {
        self.set_status(qe::status_uninstall_busy());
    }

    fn note_enabled(self: Pin<&mut Self>, ext_id: &QString) {
        self.set_status(qe::status_toggled(true, &ext_id.to_string()));
    }

    fn note_disabled(self: Pin<&mut Self>, ext_id: &QString) {
        self.set_status(qe::status_toggled(false, &ext_id.to_string()));
    }

    fn note_toggle_error(self: Pin<&mut Self>, message: &QString) {
        self.set_status(qe::status_toggle_error(&message.to_string()));
    }

    // ── Проверки без делегата ─────────────────────────────────────────────
    fn role_number(&self, name: &QString) -> i32 {
        let wanted = name.to_string();
        if wanted == ROLE_GROUP_HEADER {
            return ROLE_GROUP_HEADER_NUM;
        }
        ROLE_NAMES
            .iter()
            .position(|n| *n == wanted)
            .map(|pos| ROLE_BASE + pos as i32)
            .unwrap_or(-1)
    }

    fn role_value_of(&self, ext_id: &QString, name: &QString) -> QVariant {
        let id = ext_id.to_string();
        let key = name.to_string();
        if key == ROLE_GROUP_HEADER {
            // Заголовок группы — свойство МЕСТА, поэтому ищем строку среди
            // показанных, а не расширение среди установленных.
            for (row, index) in self.rust().visible.iter().enumerate() {
                if self.rust().all[*index].ext_id == id {
                    return qstr(&self.rust().group_headers[row]);
                }
            }
            return QVariant::default();
        }
        match self.rust().by_id(&id) {
            Some(ext) => role_value(ext, &key),
            None => QVariant::default(),
        }
    }
}

/// Значение роли расширения по её имени. Одна таблица на `data()`,
/// `roleValueOf` и проверки — двух копий у ролей быть не должно.
fn role_value(ext: &qe::ExtView, name: &str) -> QVariant {
    let card = qe::card(ext);
    let row = qe::list_row(ext);
    match name {
        "extId" => qstr(&ext.ext_id),
        "extType" => qstr(&ext.ext_type),
        "name" => qstr(&card.name),
        "nameTooltip" => qstr(&card.name_tooltip),
        "typeLine" => qstr(&card.type_line),
        "authorLine" => qstr(&card.author_line),
        "imageUrl" => qstr(&file_url(&card.image)),
        "placeholder" => qstr(&card.placeholder),
        "hasToggle" => QVariant::from(&card.has_toggle),
        "stateText" => qstr(&card.state_text),
        "stateColor" => qstr(&card.state_color),
        "toggleText" => qstr(&card.toggle_text),
        "toggleIcon" => qstr(&row.toggle_icon),
        "toggleTooltip" => qstr(&row.toggle_tooltip),
        "titleHtml" => qstr(&row.title_html),
        "subtitle" => qstr(&row.subtitle),
        _ => QVariant::default(),
    }
}

impl cxx_qt::Initialize for qobject::ExtensionsModel {
    fn initialize(mut self: Pin<&mut Self>) {
        // Подписка на смену скина: одна строка на страницу, рассылка — общая
        // (`crate::skin`). Снимается в `Drop` того же состояния.
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::ExtensionsModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
        self.as_mut().rust_mut().ensure_loaded();
    }
}
