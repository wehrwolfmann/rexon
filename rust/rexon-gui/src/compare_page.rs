// CompareModel — the "DE comparison" tab on Rust + cxx-qt + QML.
//
// The widget tabs whose logic is already in the core are rewritten in QML
// one by one, and `qt_compare` goes FIRST — it is the smallest, and its
// data and decisions have long lived in the core (`rexon_core::qt_compare`).
// The porting scheme is tried out here: decisions and labels come from the
// core, here is the QObject wrapper, and QML has only markup; the following
// tabs go by it.
//
// What is here and what is not:
//   * there IS the QObject wrapper: the desktop list model (roles for the
//     delegate), the fixed page labels, the recommendation lines and the
//     "X versus Y" table in one call;
//   * there is NOT a single line of product decisions — which pairs to
//     offer, what to write in the legend, what colour the memory bar is and
//     what to recommend to whom is the core's business. A second copy of
//     these tables would silently drift away from the first, and the
//     customer would check the numbers on the showcase before we do.
//
// Side actions (installing a desktop, switching the session with a copy
// and a logout) are NOT part of this object. The page calls them on the
// `actions` object: in the Rust window this is `DesktopsActions`
// (src/desktops_actions.rs, the core executor `system_de_exec`, the
// progress window `qml/DeOperationDialog.qml` — one for both tabs), with
// the Python host it is the `CompareActions` class from
// rexon/qt_compare/window.py. Back comes the signal "the set of installed
// desktops changed".
//
// Warning: the model computes the "installed" state ITSELF rather than
// taking it from outside: the core knows which executables a desktop has
// (`de_installed_binaries`), and the PATH lookup is
// `system::binary_in_path`. Exactly the same answer is given by
// `DesktopEnvironment.recheck_installed()` in Python (`shutil.which`), so
// both tabs, the old and the new, show the same marks.

/// cxx-qt bridge: the content of the comparison page. It is also the
/// desktop list model.
///
/// One QObject rather than two (page + model) on purpose: the list here is
/// fixed and belongs entirely to the page, and splitting it would require
/// connecting the objects through QML — a needless extra seam. For tabs
/// with real lists (extensions, wallpapers) the scheme stays the same: a
/// separate model held by the page as a property.
///
/// Warning: the type is called `CompareModel`, not `ComparePage`: the
/// module's .qml files become QML types by file name themselves, and
/// `ComparePage` is already taken by `qml/ComparePage.qml`. A name clash
/// gives "Ambiguous type detected" and a page that cannot find its own data
/// source. The rule for all tabs: the view is `<Name>Page.qml`, the object
/// `<Name>Model`.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// The base Qt type: cxx-qt-lib has no QAbstractListModel wrapper.
        ///
        /// Warning: WITHOUT `#[qobject]` — as in the neighbouring bridges of the
        /// crate: for every `#[qobject]` type in `extern "C++Qt"` cxx-qt emits cast
        /// shims named after the TYPE, and a second bridge with the same base gives
        /// a `duplicate symbol`. Inheritance still works: `#[base]`, `#[inherit]`
        /// and `#[cxx_override]` hang on the subclass.
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

        include!("cxx-qt-lib/qvector.h");
        /// Set of roles for dataChanged (in Qt6 QVector<int> == QList<int>).
        type QVector_i32 = cxx_qt_lib::QVector<i32>;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        // ── Fixed page labels. CONSTANT, because they do not change during the
        // tab's lifetime: they are core constants.
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
        #[qproperty(QString, install_note_text, cxx_name = "installNoteText",
                    READ = install_note_text, CONSTANT)]
        #[qproperty(QString, pairs_label, cxx_name = "pairsLabel",
                    READ = pairs_label, CONSTANT)]
        #[qproperty(QString, legend_title, cxx_name = "legendTitle",
                    READ = legend_title, CONSTANT)]
        #[qproperty(QString, legend_body, cxx_name = "legendBody",
                    READ = legend_body, CONSTANT)]
        #[qproperty(QString, table_header, cxx_name = "tableHeader",
                    READ = table_header, CONSTANT)]
        #[qproperty(QString, rec_header, cxx_name = "recHeader",
                    READ = rec_header, CONSTANT)]
        #[qproperty(i32, section_point_size, cxx_name = "sectionPointSize",
                    READ = section_point_size, CONSTANT)]
        // Quick pairs of the header: [{label, a, b}, …].
        #[qproperty(QList_QVariant, quick_pairs, cxx_name = "quickPairs",
                    READ = quick_pairs, CONSTANT)]
        // The application's standard skin palette (steam_dark, rule #17). The
        // styling is NOT rewritten in QML — it is taken from the core's ui_base.
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        // Sizes and labels of the "X versus Y" dialog.
        #[qproperty(i32, pair_min_width, cxx_name = "pairMinWidth",
                    READ = pair_min_width, CONSTANT)]
        #[qproperty(i32, pair_min_height, cxx_name = "pairMinHeight",
                    READ = pair_min_height, CONSTANT)]
        #[qproperty(i32, pair_title_point_size, cxx_name = "pairTitlePointSize",
                    READ = pair_title_point_size, CONSTANT)]
        #[qproperty(QString, pair_close_text, cxx_name = "pairCloseText",
                    READ = pair_close_text, CONSTANT)]
        // ── Changing: the recommendation lines depend on the set of installed
        // desktops, so after an installation they are different.
        #[qproperty(QList_QVariant, rec_rows, cxx_name = "recRows",
                    READ = rec_rows, NOTIFY = rec_rows_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        type CompareModel = super::CompareModelRust;
    }

    // ── Signals ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        /// The skin was changed on the fly — recompute the palette.
        ///
        /// The `palette` property is NOT `CONSTANT` precisely because of this:
        /// without a signal the QML bindings (`readonly property var pal:
        /// page.palette`) do not recompute, and an open tab stays in the previous
        /// skin's colours while the widget window is already repainted. The push
        /// comes from the shared registry (`crate::skin`), not from here.
        #[qsignal]
        #[cxx_name = "paletteChanged"]
        fn palette_changed(self: Pin<&mut CompareModel>);

        #[qsignal]
        #[cxx_name = "recRowsChanged"]
        fn rec_rows_changed(self: Pin<&mut CompareModel>);

        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut CompareModel>);

        /// dataChanged of the base class — a targeted row update.
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut CompareModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );
    }

    // index() is safe, but cxx-qt requires keeping #[inherit] in an unsafe
    // block (unsafe here is about matching the C++ signature, not about
    // danger).
    unsafe extern "RustQt" {
        #[inherit]
        fn index(
            self: &CompareModel,
            row: i32,
            column: i32,
            parent: &QModelIndex,
        ) -> QModelIndex;
    }

    // ── QAbstractListModel overrides ──────────────────────────────────────
    // Without #[qinvokable]: an invokable in a subclass shadows the shortened
    // clones of the base's signatures that moc makes for default arguments,
    // and rowCount() without arguments from JS fails with "Insufficient
    // arguments".
    extern "RustQt" {
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &CompareModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &CompareModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &CompareModel, index: &QModelIndex, role: i32) -> QVariant;
    }

    // ── Property reading ──────────────────────────────────────────────────
    extern "RustQt" {
        #[cxx_name = "windowTitle"]
        fn window_title(self: &CompareModel) -> QString;
        #[cxx_name = "windowMinWidth"]
        fn window_min_width(self: &CompareModel) -> i32;
        #[cxx_name = "windowMinHeight"]
        fn window_min_height(self: &CompareModel) -> i32;
        #[cxx_name = "headerTitle"]
        fn header_title(self: &CompareModel) -> QString;
        #[cxx_name = "headerTitlePointSize"]
        fn header_title_point_size(self: &CompareModel) -> i32;
        /// The line under the header: which desktops can be installed on SteamOS;
        /// empty means not SteamOS.
        #[cxx_name = "installNoteText"]
        fn install_note_text(self: &CompareModel) -> QString;
        #[cxx_name = "pairsLabel"]
        fn pairs_label(self: &CompareModel) -> QString;
        #[cxx_name = "legendTitle"]
        fn legend_title(self: &CompareModel) -> QString;
        #[cxx_name = "legendBody"]
        fn legend_body(self: &CompareModel) -> QString;
        #[cxx_name = "tableHeader"]
        fn table_header(self: &CompareModel) -> QString;
        #[cxx_name = "recHeader"]
        fn rec_header(self: &CompareModel) -> QString;
        #[cxx_name = "sectionPointSize"]
        fn section_point_size(self: &CompareModel) -> i32;
        #[cxx_name = "quickPairs"]
        fn quick_pairs(self: &CompareModel) -> QList_QVariant;
        fn palette(self: &CompareModel) -> QMap_QString_QVariant;
        #[cxx_name = "pairMinWidth"]
        fn pair_min_width(self: &CompareModel) -> i32;
        #[cxx_name = "pairMinHeight"]
        fn pair_min_height(self: &CompareModel) -> i32;
        #[cxx_name = "pairTitlePointSize"]
        fn pair_title_point_size(self: &CompareModel) -> i32;
        #[cxx_name = "pairCloseText"]
        fn pair_close_text(self: &CompareModel) -> QString;
        #[cxx_name = "recRows"]
        fn rec_rows(self: &CompareModel) -> QList_QVariant;
        fn count(self: &CompareModel) -> i32;
    }

    // ── API for QML ───────────────────────────────────────────────────────
    extern "RustQt" {
        /// Expand/collapse a desktop card. ONLY two roles are updated, not the
        /// whole row: the delegate redraws the header and shows the body, the rest
        /// stays in place.
        #[qinvokable]
        fn toggle(self: Pin<&mut CompareModel>, row: i32);

        /// Re-read the set of installed desktops.
        ///
        /// Called after a successful installation: without this the card would keep
        /// offering "Install" for an already installed desktop, and the
        /// recommendation line would keep a button instead of a mark. Exactly this
        /// fix was made in the widget tab (`_refresh_installed`), and preserving the
        /// bug during the port is not allowed (rule #16).
        #[qinvokable]
        #[cxx_name = "refreshInstalled"]
        fn refresh_installed(self: Pin<&mut CompareModel>);

        /// The whole "X versus Y" table: the title, an HTML table, install buttons
        /// for desktops not yet installed. An empty QVariant means at least one
        /// desktop is unknown to the core (then the dialog does not open).
        #[qinvokable]
        #[cxx_name = "pairView"]
        fn pair_view(self: &CompareModel, a_id: &QString, b_id: &QString) -> QVariant;

        /// Why the "Install" button of desktop `de_id` is disabled (SteamOS: the
        /// desktop is not installed as a systemd-sysext image); empty means it is
        /// enabled.
        #[qinvokable]
        #[cxx_name = "installBlockedText"]
        fn install_blocked_text(self: &CompareModel, de_id: &QString) -> QString;

        /// Display name of a desktop — for confirmation titles on the side of the
        /// side actions.
        #[qinvokable]
        #[cxx_name = "displayName"]
        fn display_name(self: &CompareModel, de_id: &QString) -> QString;

        /// Role number by name (-1 if there is no such role) — for checks without a
        /// delegate, as in the neighbouring models of the crate.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &CompareModel, name: &QString) -> i32;

        /// A role's value by row number — checks without a delegate.
        #[qinvokable]
        #[cxx_name = "roleValue"]
        fn role_value(self: &CompareModel, row: i32, name: &QString) -> QVariant;
    }

    // The trait declaration must be IN THE BRIDGE, otherwise an
    // `impl cxx_qt::Initialize` outside does not find the constructor: cxx-qt
    // emits it from this line.
    impl cxx_qt::Initialize for CompareModel {}
    // Subscribing to skin changes goes through `CxxQtThread::queue`, and the
    // thread trait must be declared IN THE BRIDGE: cxx-qt emits both
    // `qt_thread()` itself and its type alias from this line.
    impl cxx_qt::Threading for CompareModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QList, QMap, QMapPair_QString_QVariant, QModelIndex, QString, QVariant, QVector,
};

use rexon_core::qt_compare as qc;
use rexon_core::system::{binary_in_path, de_installed_binaries, DE_META};
use rexon_core::ui_base;

// ============================================================================
// Plain layer: list rows (no Qt here)
// ============================================================================

/// One list row: the desktop identifier plus its observed state.
///
/// Everything else (the header, specs, description, button) is computed by
/// the core on the fly — these are cheap string operations over a constant
/// table, and keeping a copy of them in the state would mean creating a
/// second source of truth.
#[derive(Debug, Clone, Default)]
pub struct DeRow {
    pub de_id: String,
    pub installed: bool,
    pub expanded: bool,
}

/// Whether the desktop is installed: it has a non-empty list of executables
/// and all of them are in PATH.
///
/// The same answer as `DesktopEnvironment.recheck_installed()` in Python.
pub fn de_installed(de_id: &str) -> bool {
    let binaries = de_installed_binaries(de_id);
    !binaries.is_empty() && binaries.iter().all(|b| binary_in_path(b))
}

/// The list of desktops in the order of the core database — the same order
/// in which `DesktopEnvironment.load_all()` gives them to the widget tab.
pub fn load_rows() -> Vec<DeRow> {
    DE_META
        .iter()
        .map(|m| DeRow {
            de_id: m.id.to_string(),
            installed: de_installed(m.id),
            expanded: false,
        })
        .collect()
}

// ============================================================================
// Model roles
// ============================================================================

/// Roles start at Qt::UserRole + 1 (Qt::UserRole == 256).
const ROLE_BASE: i32 = 256 + 1;

/// Role names in number order — the single source of truth for roleNames,
/// data() and roleNumber().
const ROLE_NAMES: [&str; 16] = [
    "deId",
    "displayName",
    "icon",
    "installed",
    "expanded",
    "headerText",
    "ramLabel",
    "ramMax",
    "ramValue",
    "ramColor",
    "starsLabels",
    "specs",
    "description",
    "actionKind",
    "actionText",
    "actionAccent",
];

const ROLE_INSTALLED: usize = 3;
const ROLE_EXPANDED: usize = 4;
const ROLE_HEADER_TEXT: usize = 5;
const ROLE_ACTION_KIND: usize = 13;
const ROLE_ACTION_TEXT: usize = 14;
const ROLE_ACTION_ACCENT: usize = 15;

/// Roles that change when a card is expanded.
fn toggle_roles() -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    roles.append(ROLE_BASE + ROLE_EXPANDED as i32);
    roles.append(ROLE_BASE + ROLE_HEADER_TEXT as i32);
    roles
}

/// Roles that change when a desktop is installed.
///
/// An empty vector ("all roles changed") would be more expensive here and
/// would make the delegate recompute even the specs that did not change.
fn installed_roles() -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    roles.append(ROLE_BASE + ROLE_INSTALLED as i32);
    roles.append(ROLE_BASE + ROLE_HEADER_TEXT as i32);
    roles.append(ROLE_BASE + ROLE_ACTION_KIND as i32);
    roles.append(ROLE_BASE + ROLE_ACTION_TEXT as i32);
    roles.append(ROLE_BASE + ROLE_ACTION_ACCENT as i32);
    roles
}

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

/// Rust state of the page.
#[derive(Default)]
pub struct CompareModelRust {
    /// Entry number in the shared skin change registry (`crate::skin`).
    /// 0 means the page is not subscribed yet.
    skin_watch: u64,
    rows: Vec<DeRow>,
    /// The state is built on first access, not in the constructor: a declared
    /// constructor in cxx-qt cancels the default one with a parent, and PATH at
    /// object creation time is the same anyway.
    loaded: bool,
}

/// Remove the skin change subscription. Without this the registry would
/// keep the closed tab's entry until the next broadcast — that one would
/// filter it out by `ObjectDestroyed`, but there is no reason to accumulate
/// garbage.
impl Drop for CompareModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

impl CompareModelRust {
    fn ensure_loaded(&mut self) {
        if !self.loaded {
            self.rows = load_rows();
            self.loaded = true;
        }
    }

    /// List of identifiers of installed desktops — the input of the
    /// recommendations.
    fn installed_ids(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|r| r.installed)
            .map(|r| r.de_id.clone())
            .collect()
    }
}

impl qobject::CompareModel {
    // ── Fixed labels ──────────────────────────────────────────────────────
    fn window_title(&self) -> QString {
        QString::from(&qc::window_title())
    }

    fn window_min_width(&self) -> i32 {
        qc::WINDOW_MIN_SIZE.0 as i32
    }

    fn window_min_height(&self) -> i32 {
        qc::WINDOW_MIN_SIZE.1 as i32
    }

    fn header_title(&self) -> QString {
        QString::from(&qc::header_title())
    }

    fn header_title_point_size(&self) -> i32 {
        qc::HEADER_TITLE_POINT_SIZE
    }

    /// SteamOS (Steam Deck and Steam Machine): which desktops are installed as
    /// systemd-sysext images — in one phrase under the header (the same summary
    /// as in the refusal window). Not SteamOS — empty.
    fn install_note_text(&self) -> QString {
        if !rexon_core::distro_compat::running_on_steamos() {
            return QString::default();
        }
        QString::from(&rexon_core::i18n::tr(rexon_core::distro_compat::STEAMOS_SUMMARY_DESKTOPS))
    }

    /// SteamOS: verified desktops can be installed — empty; for the others the
    /// button is visible but disabled, and the reason is the same as on the
    /// "Desktops" tab card (`distro_compat::steamos_desktop_card_block`, one for
    /// the product).
    fn install_blocked_text(&self, de_id: &QString) -> QString {
        let steamos = rexon_core::distro_compat::running_on_steamos();
        QString::from(
            &rexon_core::distro_compat::steamos_desktop_card_block(&de_id.to_string(), steamos)
                .unwrap_or_default(),
        )
    }

    fn pairs_label(&self) -> QString {
        QString::from(&qc::pairs_label())
    }

    fn legend_title(&self) -> QString {
        QString::from(&qc::legend_title())
    }

    fn legend_body(&self) -> QString {
        QString::from(&qc::legend_body())
    }

    fn table_header(&self) -> QString {
        QString::from(&qc::table_header())
    }

    fn rec_header(&self) -> QString {
        QString::from(&qc::rec_header())
    }

    fn section_point_size(&self) -> i32 {
        qc::SECTION_POINT_SIZE
    }

    fn quick_pairs(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (label, a, b) in qc::QUICK_PAIRS {
            out.append(QVariant::from(&vmap(vec![
                ("label", qstr(label)),
                ("a", qstr(a)),
                ("b", qstr(b)),
            ])));
        }
        out
    }

    /// Palette of the ACTIVE skin (the saved `ui_skin`, falling back to the
    /// standard one by rule #17). The QML tab's styling is taken from here
    /// rather than written anew: there is one skin for the whole product, and a
    /// second copy of it in .qml would set the tab apart from the rest of Rexon.
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        let colors = ui_base::active_qml_palette();
        // Whether the skin is dark — by the same criterion as the whole product,
        // not "steam_dark in the name": the customer chooses the skin, and "Light"
        // must count as light.
        let dark = colors
            .get("bg_primary")
            .and_then(|c| ui_base::parse_hex(c))
            .map(|(r, g, b)| ui_base::is_dark_rgb(r, g, b))
            .unwrap_or(true);
        for (key, value) in &colors {
            out.insert(QString::from(key), qstr(value));
        }
        // Colours the tab itself sets through the core: the "installed" mark in the
        // recommendations and the memory bar thresholds live in qt_compare.
        out.insert(
            QString::from("rec_badge"),
            qstr(qc::REC_BADGE_COLOR),
        );
        // The amber "why disabled" line — the same colour as "unavailable" on the
        // "Desktops" tab.
        out.insert(
            QString::from("status_unavailable"),
            qstr(rexon_core::qt_desktops::STATUS_UNAVAILABLE_COLOR),
        );
        out.insert(QString::from("spec_good"), qstr(qc::GOOD_COLOR));
        out.insert(QString::from("spec_bad"), qstr(qc::BAD_COLOR));
        // Groove of the memory bar. The page skin is dark (rule #17), but we ask
        // the core by theme rather than taking the dark variant by hand: otherwise
        // a light skin would get the dark groove.
        out.insert(QString::from("ram_track"), qstr(qc::ram_track_color(dark)));
        // Weight of the secondary text (the pair label, the legend body, the
        // desktop description, the recommendation subline). In the widget tab this
        // is `font-weight: 600` from `secondary_text_qss`; without it QML set the
        // same lines in the regular weight (measurement 03.09.2026: the KDE
        // description is 426 pixels wide in the widgets, 417 in QML).
        out.insert(
            QString::from("text_secondary_weight"),
            QVariant::from(&ui_base::SECONDARY_TEXT_WEIGHT),
        );
        // Tone of the blue install button — a map that RexonButton understands
        // (`tone`). The colours are the same as `INSTALL_BUTTON_QSS` of the widget
        // tab; previously QML took `highlighted` — the skin accent with a different
        // opacity, that is, its own colour instead of the core's.
        out.insert(
            QString::from("install_tone"),
            QVariant::from(&vmap(vec![
                ("background", qstr(qc::INSTALL_BUTTON_COLOR)),
                ("hover", qstr(qc::INSTALL_BUTTON_HOVER_COLOR)),
                ("radius", QVariant::from(&qc::INSTALL_BUTTON_RADIUS)),
                ("bold", QVariant::from(&true)),
            ])),
        );
        // "Yes"/"Abort" of the shared operation window (`DeOperationDialog.qml`) —
        // the "Desktops" tones from the same core table. Without them "Yes" came
        // out as an ordinary skin button rather than blue or red depending on the
        // operation.
        crate::desktops_page::insert_operation_tones(&mut out);
        // Background dimming under a modal window. The page defines no shade of
        // its own: dimming is one for the product (`ui_base::MODAL_DIM_RGBA`), and
        // the tutorial dims the window with the same colour. The next QML tabs
        // should repeat this line as is.
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        out
    }

    fn pair_min_width(&self) -> i32 {
        qc::PAIR_MIN_SIZE.0 as i32
    }

    fn pair_min_height(&self) -> i32 {
        qc::PAIR_MIN_SIZE.1 as i32
    }

    fn pair_title_point_size(&self) -> i32 {
        qc::PAIR_TITLE_POINT_SIZE
    }

    fn pair_close_text(&self) -> QString {
        QString::from(&qc::pair_close_text())
    }

    // ── Recommendations ───────────────────────────────────────────────────
    fn rec_rows(&self) -> QList<QVariant> {
        // A reading getter cannot load the state on demand (it has &self), but the
        // list of desktops does not depend on PATH in composition — only in its
        // marks, and those are computed by `installed_ids` of the already loaded
        // state. QML does not get here before the first access to the model: the
        // property is read by the same page as the delegates.
        let installed = self.rust().installed_ids();
        let mut out = QList::<QVariant>::default();
        for row in qc::rec_rows(&installed) {
            let color = if row.mark == "badge" {
                qc::REC_BADGE_COLOR
            } else {
                ""
            };
            out.append(QVariant::from(&vmap(vec![
                ("labelHtml", qstr(&row.label_html)),
                ("sub", qstr(&row.sub)),
                ("deId", qstr(row.de_id)),
                ("mark", qstr(row.mark)),
                ("text", qstr(&row.text)),
                ("color", qstr(color)),
            ])));
        }
        out
    }

    fn count(&self) -> i32 {
        self.rust().rows.len() as i32
    }

    // ── QAbstractListModel ────────────────────────────────────────────────
    fn row_count(&self, parent: &QModelIndex) -> i32 {
        // In a flat list only the root has children.
        if parent.is_valid() {
            0
        } else {
            self.rust().rows.len() as i32
        }
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut names = qobject::QHash_i32_QByteArray::default();
        for (i, name) in ROLE_NAMES.iter().enumerate() {
            names.insert(ROLE_BASE + i as i32, QByteArray::from(*name));
        }
        names
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let row = index.row();
        if !index.is_valid() || row < 0 || row as usize >= self.rust().rows.len() {
            return QVariant::default();
        }
        let entry = &self.rust().rows[row as usize];
        let Ok(slot) = usize::try_from(role - ROLE_BASE) else {
            return QVariant::default();
        };
        self.role_of(entry, ROLE_NAMES.get(slot).copied())
    }

    /// Value of one role of a row. Separate from `data` so that `roleValue`
    /// answers the same way — otherwise the checks would look somewhere other
    /// than where the delegate looks.
    fn role_of(&self, entry: &DeRow, name: Option<&str>) -> QVariant {
        // The core hands over the card body as one structure: ram_label, the bar,
        // the stars, specs, the description and the button are computed together.
        // `dark` = true: the tab runs under the standard steam_dark skin, and the
        // only thing in the body that depends on the theme is the QSS backing of
        // the bar, which QML does not have at all (the fill colour does not depend
        // on the theme).
        let body = qc::card_body(&entry.de_id, entry.installed, true);
        match name {
            Some("deId") => qstr(&entry.de_id),
            Some("displayName") => qstr(self.display_name_of(&entry.de_id)),
            Some("icon") => qstr(qc::de_icon(&entry.de_id)),
            Some("installed") => QVariant::from(&entry.installed),
            Some("expanded") => QVariant::from(&entry.expanded),
            Some("headerText") => {
                let text = qc::card_header_tail(&entry.de_id, entry.installed)
                    .unwrap_or_else(|| self.display_name_of(&entry.de_id).to_string());
                qstr(&text)
            }
            Some("ramLabel") => match &body {
                Some(b) => qstr(&b.ram_label),
                None => QVariant::default(),
            },
            Some("ramMax") => QVariant::from(&(qc::RAM_SCALE_MAX as i32)),
            Some("ramValue") => match &body {
                Some(b) => QVariant::from(&(b.ram_bar.value as i32)),
                None => QVariant::from(&0_i32),
            },
            Some("ramColor") => match &body {
                Some(b) => qstr(b.ram_bar.color),
                None => QVariant::default(),
            },
            Some("starsLabels") => {
                let mut out = QList::<QVariant>::default();
                if let Some(b) = &body {
                    for label in &b.stars_labels {
                        out.append(qstr(label));
                    }
                }
                QVariant::from(&out)
            }
            Some("specs") => {
                let mut out = QList::<QVariant>::default();
                for spec in qc::specs(&entry.de_id) {
                    out.append(QVariant::from(&vmap(vec![
                        ("label", qstr(&spec.label)),
                        ("valueHtml", qstr(&spec.value_html)),
                        ("color", qstr(spec.color)),
                    ])));
                }
                QVariant::from(&out)
            }
            Some("description") => match &body {
                Some(b) => qstr(&b.description),
                None => qstr(""),
            },
            Some("actionKind") => qstr(qc::card_action(&entry.de_id, entry.installed).kind),
            Some("actionText") => qstr(&qc::card_action(&entry.de_id, entry.installed).text),
            // The highlighted (blue) button — only for installation: in the core it is
            // marked with a non-empty QSS, here it is a flag for QML.
            Some("actionAccent") => {
                let action = qc::card_action(&entry.de_id, entry.installed);
                QVariant::from(&!action.qss.is_empty())
            }
            _ => QVariant::default(),
        }
    }

    /// Display name by identifier; the identifier itself if the desktop is not
    /// in the database (in the interface that is better than an empty string).
    fn display_name_of<'a>(&self, de_id: &'a str) -> &'a str {
        DE_META
            .iter()
            .find(|m| m.id == de_id)
            .map(|m| m.display_name)
            .unwrap_or(de_id)
    }

    // ── API for QML ───────────────────────────────────────────────────────
    fn toggle(mut self: Pin<&mut Self>, row: i32) {
        let Ok(idx) = usize::try_from(row) else {
            return;
        };
        if idx >= self.rust().rows.len() {
            return;
        }
        {
            let mut state = self.as_mut().rust_mut();
            state.rows[idx].expanded = !state.rows[idx].expanded;
        }
        let index = self.index(row, 0, &QModelIndex::default());
        let roles = toggle_roles();
        self.as_mut().data_changed(&index, &index, &roles);
    }

    fn refresh_installed(mut self: Pin<&mut Self>) {
        let rows = {
            let mut state = self.as_mut().rust_mut();
            state.ensure_loaded();
            for row in state.rows.iter_mut() {
                row.installed = de_installed(&row.de_id);
            }
            state.rows.len()
        };
        if rows > 0 {
            let first = self.index(0, 0, &QModelIndex::default());
            let last = self.index(rows as i32 - 1, 0, &QModelIndex::default());
            let roles = installed_roles();
            self.as_mut().data_changed(&first, &last, &roles);
        }
        // The recommendation lines are computed from the set of installed
        // desktops — after an installation their right part is different (button
        // → mark).
        self.as_mut().rec_rows_changed();
        self.as_mut().count_changed();
    }

    fn pair_view(&self, a_id: &QString, b_id: &QString) -> QVariant {
        let a_id = a_id.to_string();
        let b_id = b_id.to_string();
        let rows = &self.rust().rows;
        let installed = |id: &str| rows.iter().any(|r| r.de_id == id && r.installed);
        let Some(view) = qc::pair_view(&a_id, &b_id, installed(&a_id), installed(&b_id)) else {
            // The pair is unknown to the core — the widget tab simply did not open the
            // dialog in this case; an empty QVariant means the same to QML.
            return QVariant::default();
        };
        let mut buttons = QList::<QVariant>::default();
        for button in &view.buttons {
            buttons.append(QVariant::from(&vmap(vec![
                ("deId", qstr(&button.de_id)),
                ("text", qstr(&button.text)),
                ("accent", QVariant::from(&!button.qss.is_empty())),
            ])));
        }
        QVariant::from(&vmap(vec![
            ("windowTitle", qstr(&view.window_title)),
            ("heading", qstr(&view.heading)),
            ("html", qstr(&view.html)),
            ("buttons", QVariant::from(&buttons)),
            ("closeText", qstr(&view.close_text)),
            ("minWidth", QVariant::from(&(view.min_size.0 as i32))),
            ("minHeight", QVariant::from(&(view.min_size.1 as i32))),
            ("titlePointSize", QVariant::from(&view.title_point_size)),
        ]))
    }

    fn display_name(&self, de_id: &QString) -> QString {
        QString::from(self.display_name_of(&de_id.to_string()))
    }

    fn role_number(&self, name: &QString) -> i32 {
        let name = name.to_string();
        ROLE_NAMES
            .iter()
            .position(|n| *n == name)
            .map(|i| ROLE_BASE + i as i32)
            .unwrap_or(-1)
    }

    fn role_value(&self, row: i32, name: &QString) -> QVariant {
        let Ok(idx) = usize::try_from(row) else {
            return QVariant::default();
        };
        let rows = &self.rust().rows;
        let Some(entry) = rows.get(idx) else {
            return QVariant::default();
        };
        let name = name.to_string();
        self.role_of(entry, Some(name.as_str()))
    }
}

// ============================================================================
// State initialization
// ============================================================================

impl cxx_qt::Initialize for qobject::CompareModel {
    /// The list of desktops is built right when the object is created:
    /// `rowCount` is called from QML before any of our code, and an empty model
    /// on the first frame would mean an empty tab until the first
    /// `refreshInstalled`.
    fn initialize(mut self: Pin<&mut Self>) {
        // Subscribing to skin changes: one line per page, the broadcast is shared
        // (`crate::skin`). Removed in `Drop` of the same state.
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::CompareModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
        self.as_mut().rust_mut().ensure_loaded();
    }
}
