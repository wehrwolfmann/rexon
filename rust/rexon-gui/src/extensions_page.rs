// ExtensionsModel — the "Extensions" tab on Rust + cxx-qt + QML.
//
// Moved to QML by the shared tab scheme: decisions and labels come from the
// core, here is the QObject wrapper, and QML has only markup.
//
// The peculiarity of this tab: before the port it was the ONLY large Qt
// tab not connected to the core at all — `rexon/qt_extensions/window.py`
// did not contain a single call to `rexon_py`, although all the extension
// mechanics had long lived in `rexon_core::{ext_manager, extension,
// constants}`. So the port is not only markup: the showcase content moved
// to `rexon_core::qt_extensions`, and the model reads the set of installed
// extensions from disk by itself, through the core.
//
// What is here and what is not:
//   * there IS the QObject wrapper: the extension list model (roles for the
//     delegate in both display modes), the fixed page labels, the filter
//     buttons, the statistics line, the detailed view in one call, the
//     status line texts;
//   * there is NOT a single line of product decisions — what to show, in
//     what order, in what colour and with what words is the core's
//     business.
//
// Side actions (installing an .rxn, removal, enabling/disabling) are NOT
// part of this object: they touch the file system, run extension scripts
// and bring up pkexec when needed — all of that is still done by
// `rexon/extensions/manager.py`. QML calls them through the `actions`
// object put in by the page host, and after each outcome asks the model to
// re-read the set (`refresh`).

/// cxx-qt bridge: the content of the "Extensions" tab. It is also the list
/// model.
///
/// Warning: the type is called `ExtensionsModel`, not `ExtensionsPage`: the
/// module's .qml files become QML types by file name themselves, and
/// `ExtensionsPage` is taken by `qml/ExtensionsPage.qml`. The rule for all
/// tabs: the view is `<Name>Page.qml`, the object `<Name>Model`.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// The base Qt type. WITHOUT `#[qobject]`: for every `#[qobject]` type in
        /// `extern "C++Qt"` cxx-qt emits cast shims named after the TYPE, and a
        /// second bridge of the crate with the same base gives a `duplicate symbol`.
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
        // ── Fixed page labels ────────────────────────────────────────────
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
        // The "⬇ Install" block: [{kind, icon, label, tooltip, enabled, text}, …].
        #[qproperty(QList_QVariant, install_sources, cxx_name = "installSources",
                    READ = install_sources, CONSTANT)]
        #[qproperty(i32, source_min_width, cxx_name = "sourceMinWidth",
                    READ = source_min_width, CONSTANT)]
        #[qproperty(i32, source_max_width, cxx_name = "sourceMaxWidth",
                    READ = source_max_width, CONSTANT)]
        #[qproperty(i32, source_min_height, cxx_name = "sourceMinHeight",
                    READ = source_min_height, CONSTANT)]
        // The "Create your own" help.
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
        // Detailed view.
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
        // The file picker window and the confirmation — for the side-actions host.
        #[qproperty(QString, file_dialog_title, cxx_name = "fileDialogTitle",
                    READ = file_dialog_title, CONSTANT)]
        #[qproperty(QString, file_dialog_filter, cxx_name = "fileDialogFilter",
                    READ = file_dialog_filter, CONSTANT)]
        #[qproperty(QString, confirm_title, cxx_name = "confirmTitle",
                    READ = confirm_title, CONSTANT)]
        // The application's standard skin palette (steam_dark, rule #17).
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        // Tab styling that is not in the skin palette: chips, the tile, the
        // preview placeholder, the list row, the "Install" block, the layout. It
        // changes together with the skin ("dark/light" pairs), hence the same
        // signal as for the palette.
        #[qproperty(QMap_QString_QVariant, styles, READ = styles,
                    NOTIFY = palette_changed)]
        // ── Changing ─────────────────────────────────────────────────────
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

    // ── Signals ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        /// Everything that changes together with the showcase contents: the
        /// statistics, the "My extensions" title, the empty state hint, the filter
        /// buttons, the counter. One signal, not five: they always change together,
        /// and with five signals the markup would get five recomputations in a row.
        /// The skin was changed on the fly — recompute the palette.
        ///
        /// The `palette` property is NOT `CONSTANT` precisely because of this:
        /// without a signal the QML bindings (`readonly property var pal:
        /// page.palette`) do not recompute, and an open tab stays in the previous
        /// skin's colours while the widget window is already repainted. The push
        /// comes from the shared registry (`crate::skin`), not from here.
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

    // ── Inherited methods of the base class ───────────────────────────────
    //
    // Resetting the whole model rather than a targeted `dataChanged`: a change
    // of the filter, the search or the display mode changes both the set of
    // rows and their order — exactly the case reset exists for in Qt.
    extern "RustQt" {
        /// # Safety
        /// Requires a paired end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut ExtensionsModel>);
        /// # Safety
        /// Paired with begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut ExtensionsModel>);
    }

    // ── QAbstractListModel overrides ──────────────────────────────────────
    // Without #[qinvokable]: an invokable in a subclass shadows the shortened
    // clones of the base's signatures that moc makes for default arguments,
    // and rowCount() without arguments from JS fails with "Insufficient
    // arguments".
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

    // ── Property reading ──────────────────────────────────────────────────
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

    // ── API for QML ───────────────────────────────────────────────────────
    //
    // Warning: not a single invokable with a number in its signature: cxx-qt
    // writes the type into the metaobject as `::std::int32_t`, there is no
    // metatype with that name, and a call through `QMetaObject.invokeMethod`
    // silently returns False. Everything that must be called from outside
    // (checks, the page host) takes a QString.
    extern "RustQt" {
        /// Re-read the set of installed extensions from disk.
        #[qinvokable]
        fn refresh(self: Pin<&mut ExtensionsModel>);

        /// Choose a filter by type (`all` or a type identifier).
        #[qinvokable]
        #[cxx_name = "setFilter"]
        fn set_filter(self: Pin<&mut ExtensionsModel>, filter_type: &QString);

        /// Set the search string.
        #[qinvokable]
        #[cxx_name = "setSearch"]
        fn set_search(self: Pin<&mut ExtensionsModel>, text: &QString);

        /// Switch the display mode (`grid` or `list`).
        #[qinvokable]
        #[cxx_name = "setViewMode"]
        fn set_view_mode(self: Pin<&mut ExtensionsModel>, mode: &QString);

        /// The detailed view of an extension in full. An empty QVariant means there
        /// is no such extension on the showcase (then the window does not open).
        #[qinvokable]
        #[cxx_name = "detailFor"]
        fn detail_for(self: &ExtensionsModel, ext_id: &QString) -> QVariant;

        /// Text of the removal confirmation from the showcase.
        #[qinvokable]
        #[cxx_name = "confirmUninstall"]
        fn confirm_uninstall(self: &ExtensionsModel, ext_id: &QString) -> QString;

        /// Text of the removal confirmation from the detailed view (with the
        /// directory).
        #[qinvokable]
        #[cxx_name = "confirmUninstallDetailed"]
        fn confirm_uninstall_detailed(self: &ExtensionsModel, ext_id: &QString) -> QString;

        /// Extension name by identifier (for the status line).
        #[qinvokable]
        #[cxx_name = "displayName"]
        fn display_name(self: &ExtensionsModel, ext_id: &QString) -> QString;

        /// Whether such a file is accepted by drag and drop.
        #[qinvokable]
        #[cxx_name = "acceptsDrop"]
        fn accepts_drop(self: &ExtensionsModel, path: &QString) -> bool;

        // ── Status line: the core composes the text, the markup only calls ──
        #[qinvokable]
        #[cxx_name = "noteInstalling"]
        fn note_installing(self: Pin<&mut ExtensionsModel>, path: &QString);
        /// A ready line for the status line (the result of "Apply").
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

        /// Role number by name (-1 if there is no such role) — for checks without a
        /// delegate, as in the neighbouring models of the crate.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &ExtensionsModel, name: &QString) -> i32;

        /// A row's role value by its identifier — checks without a delegate. A row
        /// number will not do: an invokable with a number is not callable from
        /// outside.
        #[qinvokable]
        #[cxx_name = "roleValueOf"]
        fn role_value_of(self: &ExtensionsModel, ext_id: &QString, name: &QString) -> QVariant;
    }

    // The trait declaration must be IN THE BRIDGE, otherwise an
    // `impl cxx_qt::Initialize` outside does not find the constructor: cxx-qt
    // emits it from this line.
    impl cxx_qt::Initialize for ExtensionsModel {}
    // Subscribing to skin changes goes through `CxxQtThread::queue`, and the
    // thread trait must be declared IN THE BRIDGE: cxx-qt emits both
    // `qt_thread()` itself and its type alias from this line.
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
// Model roles
// ============================================================================

/// Roles start at Qt::UserRole + 1 (Qt::UserRole == 256).
const ROLE_BASE: i32 = 256 + 1;

/// Role names in number order — the single source of truth for roleNames,
/// data() and roleNumber().
///
/// The roles cover BOTH display modes at once: the grid delegate takes
/// `typeLine` and `name`, the list delegate `titleHtml` and `subtitle`. We do
/// not create two models for one showcase: their set and order are shared,
/// only the markup differs.
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

/// The group title role of "list" mode — the only one that is not about the
/// extension but about its place in the list, so it goes last and
/// separately.
const ROLE_GROUP_HEADER: &str = "groupHeader";
const ROLE_GROUP_HEADER_NUM: i32 = ROLE_BASE + ROLE_NAMES.len() as i32;

// ============================================================================
// Translation odds and ends in Qt
// ============================================================================

/// Build a QVariantMap from pairs. QML reads such a map with a dot
/// (`row.label`).
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

/// A path on disk → an address for `Image.source`. An empty path stays
/// empty: the markup decides by it whether to draw a placeholder icon.
fn file_url(path: &str) -> String {
    if path.is_empty() {
        String::new()
    } else {
        format!("file://{path}")
    }
}

// ============================================================================
// Rust state of the page
// ============================================================================

/// Tab state: what was read from disk and what of it is shown.
#[derive(Default)]
pub struct ExtensionsModelRust {
    /// Entry number in the shared skin change registry (`crate::skin`).
    /// 0 means the page is not subscribed yet.
    skin_watch: u64,
    /// Everything installed, in core order.
    all: Vec<qe::ExtView>,
    /// Numbers of the shown rows in `all`, already taking the filter, the
    /// search and the display mode into account (in "list" mode the order is by
    /// group).
    visible: Vec<usize>,
    /// Group title for each shown row ("" — not the first in the group, or grid
    /// mode).
    group_headers: Vec<String>,
    filter_type: String,
    search: String,
    view_mode: String,
    status: String,
    /// The set is built on first access, not in the constructor: a declared
    /// constructor in cxx-qt cancels the default one with a parent.
    loaded: bool,
}

/// Remove the skin change subscription. Without this the registry would
/// keep the closed tab's entry until the next broadcast — that one would
/// filter it out by `ObjectDestroyed`, but there is no reason to accumulate
/// garbage.
impl Drop for ExtensionsModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

impl ExtensionsModelRust {
    /// Rebuild what is shown by the current filter, search and mode.
    fn recompute(&mut self) {
        let filter = if self.filter_type.is_empty() {
            qe::FILTER_ALL_ID.to_string()
        } else {
            self.filter_type.clone()
        };
        // The filter may have stayed from a set that no longer exists.
        let items = qe::filter_items(&qe::installed_types(&self.all));
        self.filter_type = qe::normalize_filter(&filter, &items);

        let picked = qe::apply_filters(&self.all, &self.filter_type, &self.search);
        // From references back to numbers: the model stores numbers so as not to
        // keep a second copy of the extensions.
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

    /// Extension by the number of a shown row.
    fn row(&self, row: i32) -> Option<&qe::ExtView> {
        let index = *self.visible.get(usize::try_from(row).ok()?)?;
        self.all.get(index)
    }

    /// Extension by identifier — among EVERYTHING installed, not only what is
    /// shown: the detailed view is also opened from a row that the search will
    /// hide a moment later.
    fn by_id(&self, ext_id: &str) -> Option<&qe::ExtView> {
        self.all.iter().find(|e| e.ext_id == ext_id)
    }
}

impl qobject::ExtensionsModel {
    // ── Fixed labels ──────────────────────────────────────────────────────
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

    /// Palette of the ACTIVE skin (the saved `ui_skin`, falling back to the
    /// standard one by rule #17) plus the colours the tab itself sets through
    /// the core. The QML tab's styling is taken from here rather than written
    /// anew: there is one skin for the whole product.
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
        // Weight of secondary labels (600 — the widgets' `secondary_text_qss`):
        // one number for both wrappers, the key is the same as in "Compare".
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
        // Background dimming under a modal window is a product-wide decision
        // (`ui_base::MODAL_DIM_RGBA`); the tutorial dims the window with the same
        // colour.
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        out
    }

    /// Tab styling that is not in the skin palette.
    ///
    /// Warning: EVERY colour goes through [`ui_base::css_color_to_qml`]: the
    /// core writes them the way the widgets' QSS understands them
    /// (`rgba(r,g,b,a)`), while QML silently reads such notation as an invalid
    /// colour and draws it BLACK.
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

        // Modal windows of the tab: the detailed view and the "Create your own"
        // help.
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

    // ── Changing ──────────────────────────────────────────────────────────
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

    // ── API for QML ───────────────────────────────────────────────────────
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
        // The mode is switched only to a KNOWN one: pressing the already selected
        // button must not clear the selection (as in the original, where the
        // button got `checked` back).
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

    // ── Status line ───────────────────────────────────────────────────────
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

    // ── Checks without a delegate ─────────────────────────────────────────
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
            // The group title is a property of the PLACE, so we look for the row among
            // the shown ones, not for the extension among the installed ones.
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

/// An extension role's value by its name. One table for `data()`,
/// `roleValueOf` and the checks — the roles must not have two copies.
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
        // Subscribing to skin changes: one line per page, the broadcast is shared
        // (`crate::skin`). Removed in `Drop` of the same state.
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::ExtensionsModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
        self.as_mut().rust_mut().ensure_loaded();
    }
}
