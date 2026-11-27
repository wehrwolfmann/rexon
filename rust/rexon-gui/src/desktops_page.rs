// DesktopsModel — the "Desktops" tab on Rust + cxx-qt + QML.
//
// The second tab moved to QML by the "DE comparison" scheme. Data and
// decisions come from the core (`rexon_core::qt_desktops` over the desktop
// database `system::DE_META`); here there is only the QObject wrapper: the
// list model on the left with roles, and the card of the selected desktop
// as one map.
//
// What is NOT here:
//   * decisions "what to show" — the list sections, hints, the set of
//     buttons and both removal safeguards are computed by the core;
//   * side actions — installing, repairing, switching and removing go
//     through pkexec and progress windows that still live in Python
//     (`rexon/qt_desktops/window.py`, the `desktopsActions` object). The page
//     calls them by name and decides from the answer whether to recompute
//     the state.
//
// Warning: the model computes the "installed" state ITSELF
// (`compare_page::de_installed` — the desktop's executables in PATH), with
// the same answer as `DesktopEnvironment.recheck_installed()` in Python.
// One function for both QML tabs: as a second copy they would silently
// drift apart.

/// cxx-qt bridge: the list of desktops on the left plus the card of the
/// selected one on the right.
///
/// The type name is `DesktopsModel`, not `DesktopsPage`: the latter is taken
/// by the markup file itself (`qml/DesktopsPage.qml`), and a clash would
/// give "Ambiguous type detected".
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// The base Qt type. Warning: WITHOUT `#[qobject]`: for every `#[qobject]`
        /// type in `extern "C++Qt"` cxx-qt emits cast shims by the TYPE name, and a
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

        include!("cxx-qt-lib/qvector.h");
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
        // ── Fixed page labels ─────────────────────────────────────────
        #[qproperty(QString, window_title, cxx_name = "windowTitle",
                    READ = window_title, CONSTANT)]
        #[qproperty(i32, window_min_width, cxx_name = "windowMinWidth",
                    READ = window_min_width, CONSTANT)]
        #[qproperty(i32, window_min_height, cxx_name = "windowMinHeight",
                    READ = window_min_height, CONSTANT)]
        #[qproperty(QString, list_header, cxx_name = "listHeader",
                    READ = list_header, CONSTANT)]
        #[qproperty(i32, list_header_point_size, cxx_name = "listHeaderPointSize",
                    READ = list_header_point_size, CONSTANT)]
        #[qproperty(i32, list_item_point_size, cxx_name = "listItemPointSize",
                    READ = list_item_point_size, CONSTANT)]
        #[qproperty(i32, section_point_size, cxx_name = "sectionPointSize",
                    READ = section_point_size, CONSTANT)]
        #[qproperty(QString, tools_header, cxx_name = "toolsHeader",
                    READ = tools_header, CONSTANT)]
        #[qproperty(QList_QVariant, tool_buttons, cxx_name = "toolButtons",
                    READ = tool_buttons, CONSTANT)]
        #[qproperty(i32, tool_button_min_height, cxx_name = "toolButtonMinHeight",
                    READ = tool_button_min_height, CONSTANT)]
        #[qproperty(QString, tools_notice, cxx_name = "toolsNotice",
                    READ = tools_notice, CONSTANT)]
        #[qproperty(QString, placeholder_text, cxx_name = "placeholderText",
                    READ = placeholder_text, CONSTANT)]
        // The "Like in the distribution" block (the middle panel): the title, the
        // explanation and the look tiles. The tiles change after "Apply" and
        // "Restore as it was" — they are re-read by `refreshInstalled`.
        #[qproperty(QString, looks_header, cxx_name = "looksHeader",
                    READ = looks_header, CONSTANT)]
        #[qproperty(QString, looks_hint, cxx_name = "looksHint",
                    READ = looks_hint, CONSTANT)]
        #[qproperty(QList_QVariant, looks, READ = looks, NOTIFY = looks_changed)]
        #[qproperty(QString, compare_back_text, cxx_name = "compareBackText",
                    READ = compare_back_text, CONSTANT)]
        #[qproperty(i32, card_title_point_size, cxx_name = "cardTitlePointSize",
                    READ = card_title_point_size, CONSTANT)]
        #[qproperty(i32, card_status_point_size, cxx_name = "cardStatusPointSize",
                    READ = card_status_point_size, CONSTANT)]
        #[qproperty(i32, card_max_width, cxx_name = "cardMaxWidth",
                    READ = card_max_width, CONSTANT)]
        #[qproperty(i32, card_gap_left, cxx_name = "cardGapLeft",
                    READ = card_gap_left, CONSTANT)]
        #[qproperty(QMap_QString_QVariant, carousel, READ = carousel, CONSTANT)]
        #[qproperty(QMap_QString_QVariant, metrics, READ = metrics, CONSTANT)]
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        #[qproperty(QString, current_de, cxx_name = "currentDe",
                    READ = current_de, CONSTANT)]
        // ── Changing ──────────────────────────────────────────────────
        // The card of the selected desktop as one map: its content depends both on
        // the selection and on the set of installed desktops.
        #[qproperty(QMap_QString_QVariant, card, READ = card, NOTIFY = card_changed)]
        #[qproperty(QString, selected_de, cxx_name = "selectedDe",
                    READ = selected_de, NOTIFY = card_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        type DesktopsModel = super::DesktopsModelRust;
    }

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
        fn palette_changed(self: Pin<&mut DesktopsModel>);

        #[qsignal]
        #[cxx_name = "cardChanged"]
        fn card_changed(self: Pin<&mut DesktopsModel>);

        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut DesktopsModel>);

        #[qsignal]
        #[cxx_name = "looksChanged"]
        fn looks_changed(self: Pin<&mut DesktopsModel>);

        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut DesktopsModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );
    }

    unsafe extern "RustQt" {
        #[inherit]
        fn index(
            self: &DesktopsModel,
            row: i32,
            column: i32,
            parent: &QModelIndex,
        ) -> QModelIndex;
    }

    // Without #[qinvokable]: an invokable in a subclass shadows the shortened
    // clones of the base's signatures, and rowCount() without arguments from JS
    // fails.
    extern "RustQt" {
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &DesktopsModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &DesktopsModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &DesktopsModel, index: &QModelIndex, role: i32) -> QVariant;
    }

    extern "RustQt" {
        #[cxx_name = "windowTitle"]
        fn window_title(self: &DesktopsModel) -> QString;
        #[cxx_name = "windowMinWidth"]
        fn window_min_width(self: &DesktopsModel) -> i32;
        #[cxx_name = "windowMinHeight"]
        fn window_min_height(self: &DesktopsModel) -> i32;
        #[cxx_name = "listHeader"]
        fn list_header(self: &DesktopsModel) -> QString;
        #[cxx_name = "listHeaderPointSize"]
        fn list_header_point_size(self: &DesktopsModel) -> i32;
        #[cxx_name = "listItemPointSize"]
        fn list_item_point_size(self: &DesktopsModel) -> i32;
        #[cxx_name = "sectionPointSize"]
        fn section_point_size(self: &DesktopsModel) -> i32;
        #[cxx_name = "toolsHeader"]
        fn tools_header(self: &DesktopsModel) -> QString;
        #[cxx_name = "toolButtons"]
        fn tool_buttons(self: &DesktopsModel) -> QList_QVariant;
        #[cxx_name = "toolButtonMinHeight"]
        fn tool_button_min_height(self: &DesktopsModel) -> i32;
        /// Why some panel buttons are disabled (SteamOS); empty means all are
        /// enabled.
        #[cxx_name = "toolsNotice"]
        fn tools_notice(self: &DesktopsModel) -> QString;
        #[cxx_name = "placeholderText"]
        fn placeholder_text(self: &DesktopsModel) -> QString;
        #[cxx_name = "looksHeader"]
        fn looks_header(self: &DesktopsModel) -> QString;
        #[cxx_name = "looksHint"]
        fn looks_hint(self: &DesktopsModel) -> QString;
        fn looks(self: &DesktopsModel) -> QList_QVariant;
        #[cxx_name = "compareBackText"]
        fn compare_back_text(self: &DesktopsModel) -> QString;
        #[cxx_name = "cardTitlePointSize"]
        fn card_title_point_size(self: &DesktopsModel) -> i32;
        #[cxx_name = "cardStatusPointSize"]
        fn card_status_point_size(self: &DesktopsModel) -> i32;
        #[cxx_name = "cardMaxWidth"]
        fn card_max_width(self: &DesktopsModel) -> i32;
        #[cxx_name = "cardGapLeft"]
        fn card_gap_left(self: &DesktopsModel) -> i32;
        fn carousel(self: &DesktopsModel) -> QMap_QString_QVariant;
        fn metrics(self: &DesktopsModel) -> QMap_QString_QVariant;
        fn palette(self: &DesktopsModel) -> QMap_QString_QVariant;
        #[cxx_name = "currentDe"]
        fn current_de(self: &DesktopsModel) -> QString;
        fn card(self: &DesktopsModel) -> QMap_QString_QVariant;
        #[cxx_name = "selectedDe"]
        fn selected_de(self: &DesktopsModel) -> QString;
        fn count(self: &DesktopsModel) -> i32;
    }

    // ── API for QML ───────────────────────────────────────────────────
    extern "RustQt" {
        /// Select a desktop in the list. Section headers are not selectable — as in
        /// the original, where they had `NoItemFlags`.
        #[qinvokable]
        fn select(self: Pin<&mut DesktopsModel>, de_id: &QString);

        /// Re-read the set of installed desktops and redraw the card. Called after
        /// every successful operation.
        #[qinvokable]
        #[cxx_name = "refreshInstalled"]
        fn refresh_installed(self: Pin<&mut DesktopsModel>);

        /// What to do when "Delete" is pressed: both safeguards are decided by the
        /// core. A map with `kind`/`title`/`text`/`allowed`.
        #[qinvokable]
        #[cxx_name = "removePlan"]
        fn remove_plan(self: &DesktopsModel, de_id: &QString) -> QVariant;

        /// Confirmation and progress titles for an operation on a desktop. `kind`
        /// is `install`, `repair`, `switch`, `remove`.
        #[qinvokable]
        #[cxx_name = "operationTexts"]
        fn operation_texts(self: &DesktopsModel, kind: &QString, de_id: &QString) -> QVariant;

        /// Display name of a desktop.
        #[qinvokable]
        #[cxx_name = "displayName"]
        fn display_name(self: &DesktopsModel, de_id: &QString) -> QString;

        /// Version line from the repositories; an empty string means hide the line.
        #[qinvokable]
        #[cxx_name = "versionLine"]
        fn version_line(self: &DesktopsModel, version: &QString) -> QString;

        /// Full-screen viewer hint "i of n".
        #[qinvokable]
        #[cxx_name = "zoomHint"]
        fn zoom_hint(self: &DesktopsModel, index: i32, count: i32) -> QString;

        /// Carousel indicator dots.
        #[qinvokable]
        #[cxx_name = "carouselDots"]
        fn carousel_dots(self: &DesktopsModel, index: i32, count: i32) -> QString;

        /// Height of the screenshot frame by the window size and the available
        /// width.
        #[qinvokable]
        #[cxx_name = "carouselHeight"]
        fn carousel_height(self: &DesktopsModel, window_height: i32, available_width: i32) -> i32;

        /// The same height squeezed to fit the card viewport: `room` is how much
        /// height is left for the frame after everything else in the card.
        #[qinvokable]
        #[cxx_name = "carouselHeightFitted"]
        fn carousel_height_fitted(self: &DesktopsModel, full: i32, room: i32) -> i32;

        /// Width cap of the right panel by the window height: beyond it the extra
        /// width goes to the middle panel ("Like in the distribution"), as with the
        /// original's splitter.
        #[qinvokable]
        #[cxx_name = "detailPanelMaxWidth"]
        fn detail_panel_max_width(self: &DesktopsModel, window_height: i32) -> i32;

        /// Width of the middle panel ("Like in the distribution") at page width
        /// `total`: sharing the extra room between the panels — as with the
        /// original's splitter.
        #[qinvokable]
        #[cxx_name = "looksPanelWidth"]
        fn looks_panel_width(self: &DesktopsModel, total: i32, window_height: i32) -> i32;

        /// Minimum width of the middle panel from its own content: the labels must
        /// read in full at the standard window minimum. Qt measures the label
        /// widths by the font; the numbers and the arithmetic are in the core.
        #[qinvokable]
        #[cxx_name = "looksPanelMinWidth"]
        fn looks_panel_min_width(
            self: &DesktopsModel,
            header_width: i32,
            column_width: i32,
        ) -> i32;

        /// Whether "Like in the distribution" and "Tools" go side by side in a
        /// middle panel of width `inner_width` (without paddings) — the core
        /// decides.
        #[qinvokable]
        #[cxx_name = "looksToolsSideBySide"]
        fn looks_tools_side_by_side(
            self: &DesktopsModel,
            inner_width: i32,
            looks_width: i32,
            tools_width: i32,
        ) -> bool;

        /// Width of one block of the middle panel (side by side — half, in a column
        /// — the whole width).
        #[qinvokable]
        #[cxx_name = "looksToolsBlockWidth"]
        fn looks_tools_block_width(self: &DesktopsModel, inner_width: i32, side_by_side: bool)
            -> i32;

        /// Left spacer of the card wrapper at viewport width `viewport`: the card is
        /// pressed to the right edge of the panel, as in the original.
        #[qinvokable]
        #[cxx_name = "cardLeftSpacer"]
        fn card_left_spacer(self: &DesktopsModel, viewport: i32) -> i32;

        /// Role number by name (-1 if there is no such role) — for checks without a
        /// delegate.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &DesktopsModel, name: &QString) -> i32;

        /// A role's value by row number — checks without a delegate.
        #[qinvokable]
        #[cxx_name = "roleValue"]
        fn role_value(self: &DesktopsModel, row: i32, name: &QString) -> QVariant;
    }

    impl cxx_qt::Initialize for DesktopsModel {}
    // Subscribing to skin changes goes through `CxxQtThread::queue`, and the
    // thread trait must be declared IN THE BRIDGE: cxx-qt emits both
    // `qt_thread()` itself and its type alias from this line.
    impl cxx_qt::Threading for DesktopsModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QList, QMap, QMapPair_QString_QVariant, QModelIndex, QString, QVariant, QVector,
};

use rexon_core::qt_compare as qc;
use rexon_core::distro_compat::SteamOsBlock;
use rexon_core::qt_desktops as qd;
use rexon_core::screenshots;
use rexon_core::system::{self, DistroDetector, PkgFamily};
use rexon_core::ui_base;

use crate::compare_page::de_installed;
use rexon_core::shell_bundle::{self as sb, ShellState};
use rexon_core::system::DesktopShell;

/// State of a shell on THIS machine for the card of compositor `wm`: the
/// package in `PATH`, our block in the config, the mode mark. Read-only.
fn live_shell_state(wm: &str, shell: DesktopShell, wm_installed: bool, current: &str) -> ShellState {
    let home = std::env::var("HOME").unwrap_or_default();
    let cfg = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty());
    let state = std::env::var("XDG_STATE_HOME").ok().filter(|v| !v.is_empty());
    let read = |p: &str| std::fs::read_to_string(p).ok();
    ShellState {
        wm_installed,
        binary_present: rexon_core::utils::which(sb::binary(shell), std::env::var("PATH").ok().as_deref())
            .is_some(),
        block_present: sb::block_installed(wm, shell, &home, cfg.as_deref(), &read),
        marker_on: std::path::Path::new(&sb::marker_path(wm, shell, &home, state.as_deref())).is_file(),
        is_current_wm: current == wm,
    }
}

/// The card's "Shells" block as a list of maps for QML; empty means there is
/// no block.
fn shell_rows_qml(wm: &str, family: PkgFamily, wm_installed: bool, current: &str) -> QList<QVariant> {
    let block = SteamOsBlock::Desktops.live();
    let rows = qd::shell_rows(
        wm,
        family,
        &|shell| live_shell_state(wm, shell, wm_installed, current),
        block.as_deref(),
    );
    let mut out = QList::<QVariant>::default();
    for row in rows {
        let mut buttons = QList::<QVariant>::default();
        for b in &row.buttons {
            buttons.append(QVariant::from(&vmap(vec![
                ("kind", qstr(b.kind)),
                ("text", qstr(&b.text)),
                ("tooltip", qstr(&b.tooltip)),
                ("style", qstr(b.style)),
                ("enabled", QVariant::from(&b.enabled)),
            ])));
        }
        out.append(QVariant::from(&vmap(vec![
            ("shell", qstr(row.shell)),
            ("name", qstr(row.name)),
            ("description", qstr(&row.description)),
            ("statusText", qstr(&row.status_text)),
            ("running", QVariant::from(&row.running)),
            ("hint", qstr(&row.hint)),
            ("buttons", QVariant::from(&buttons)),
        ])));
    }
    out
}

// ============================================================================
// Live environment: package family, current desktop, screenshots
// ============================================================================

/// Package family of this distribution. Probed once per object: os-release
/// does not change while the tab is running.
fn detect_family() -> PkgFamily {
    DistroDetector::detect_live().family_live()
}

/// Identifier of the desktop running NOW; an empty string means not
/// recognized. The same answer is given by `SystemInfo.current_de` in
/// Python.
fn detect_current() -> String {
    let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = std::env::var("DESKTOP_SESSION").unwrap_or_default();
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    system::detect_current_de(&xdg, &session, wayland).unwrap_or_default()
}

/// What to show in a desktop's carousel: screenshots or a single
/// placeholder.
///
/// The screenshots are found by the core (`carousel_paths_on_disk`): first
/// those shipped in the package (River), then the downloaded ones — with
/// the same check its download uses to decide what is missing.
fn carousel_images(dir: &str, de_id: &str) -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let bundled = screenshots::bundled_screenshot_dirs(&rexon_core::cli::repo_root(), &home);
    let paths = screenshots::carousel_paths_on_disk(dir, &bundled, de_id);
    if !paths.is_empty() {
        return paths;
    }
    // The placeholder and its label ("loading" / "will appear when there is
    // internet" / "not yet") are chosen by the core according to its download.
    screenshots::placeholder_on_disk(dir, de_id).into_iter().collect()
}

// ============================================================================
// Model roles
// ============================================================================

const ROLE_BASE: i32 = 256 + 1;

const ROLE_NAMES: [&str; 8] = [
    "kind",
    "deId",
    "text",
    "tooltip",
    "installed",
    "available",
    "selectable",
    "isCurrent",
];

const ROLE_TOOLTIP: usize = 3;
const ROLE_INSTALLED: usize = 4;

/// Roles that change when a desktop is installed or removed.
fn installed_roles() -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    roles.append(ROLE_BASE + ROLE_INSTALLED as i32);
    roles.append(ROLE_BASE + ROLE_TOOLTIP as i32);
    roles
}

// ============================================================================
// Translation odds and ends in Qt
// ============================================================================

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

fn qint(value: i32) -> QVariant {
    QVariant::from(&value)
}

/// A core QSS colour in the notation QML understands.
fn qcss(css: &str) -> QVariant {
    qstr(&ui_base::css_color_to_qml(css))
}

/// A share of the neutral grey (`ui_base::neutral_rgba`) in QML notation.
fn qneutral(alpha: f64) -> QVariant {
    qcss(&ui_base::neutral_rgba(alpha))
}

/// Tone of a "special" button — a map that `RexonButton.tone` understands:
/// background, hover background, text colour, rounding, weight and
/// (optionally) a border.
fn tone(
    background: QVariant,
    hover: QVariant,
    color: &str,
    radius: i32,
    bold: bool,
    border: Option<QVariant>,
) -> QVariant {
    let mut pairs = vec![
        ("background", background),
        ("hover", hover),
        ("color", qstr(color)),
        ("radius", qint(radius)),
        ("bold", QVariant::from(&bold)),
    ];
    if let Some(border) = border {
        pairs.push(("border", border));
    }
    QVariant::from(&vmap(pairs))
}

/// Tones of the shared desktop operation window (`DeOperationDialog.qml`) —
/// into the page palette by the core table `qt_desktops::OPERATION_DIALOG_TONES`.
///
/// BOTH pages with this window call it: "Desktops" and "DE comparison". A
/// page that did not provide these keys got "Yes" as an ordinary skin button
/// instead of a blue or red one (that is how it was in "Compare").
pub(crate) fn insert_operation_tones(out: &mut QMap<QMapPair_QString_QVariant>) {
    for (key, t) in qd::OPERATION_DIALOG_TONES {
        out.insert(
            QString::from(key),
            tone(qcss(t.background), qcss(t.hover), t.color, t.radius, t.bold, None),
        );
    }
}

/// Rust state of the tab.
#[derive(Default)]
pub struct DesktopsModelRust {
    /// Entry number in the shared skin change registry (`crate::skin`).
    /// 0 means the page is not subscribed yet.
    skin_watch: u64,
    rows: Vec<qd::ListRow>,
    selected: String,
    current_de: String,
    family: Option<PkgFamily>,
    screenshot_dir: String,
    loaded: bool,
}

/// Remove the skin change subscription. Without this the registry would
/// keep the closed tab's entry until the next broadcast — that one would
/// filter it out by `ObjectDestroyed`, but there is no reason to accumulate
/// garbage.
impl Drop for DesktopsModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

impl DesktopsModelRust {
    fn family(&self) -> PkgFamily {
        self.family.unwrap_or(PkgFamily::Unknown)
    }

    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.family = Some(detect_family());
        self.current_de = detect_current();
        let home = std::env::var("HOME").unwrap_or_default();
        // Taking `$XDG_CONFIG_HOME` into account: Python takes the screenshots by
        // an XDG-aware path, and the carousel must look in the same place.
        let xdg = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty());
        self.screenshot_dir = screenshots::screenshot_dir_xdg(xdg.as_deref(), &home);
        self.rows = qd::list_rows(self.family(), &de_installed);
        // The CURRENT desktop is shown first: the user opens the tab and sees their
        // own desktop, not the first in the list (in the original that was
        // Cinnamon — simply because it is first). If the current one is not in the
        // list — the first selectable item.
        let current = self.current_de.clone();
        self.selected = self
            .rows
            .iter()
            .find(|r| r.kind == "de" && r.de_id == current)
            .or_else(|| self.rows.iter().find(|r| r.kind == "de"))
            .map(|r| r.de_id.clone())
            .unwrap_or_default();
        self.loaded = true;
    }

    fn row_of(&self, de_id: &str) -> Option<&qd::ListRow> {
        self.rows.iter().find(|r| r.kind == "de" && r.de_id == de_id)
    }

    /// How many OTHER desktops are installed (the desktop itself is not
    /// counted) — the input of the second removal safeguard.
    fn other_installed(&self, de_id: &str) -> usize {
        self.rows
            .iter()
            .filter(|r| r.kind == "de" && r.installed && r.de_id != de_id)
            .count()
    }
}

impl qobject::DesktopsModel {
    // ── Fixed labels ──────────────────────────────────────────────────
    fn window_title(&self) -> QString {
        QString::from(&qd::window_title())
    }

    fn window_min_width(&self) -> i32 {
        qd::WINDOW_MIN_SIZE.0 as i32
    }

    fn window_min_height(&self) -> i32 {
        qd::WINDOW_MIN_SIZE.1 as i32
    }

    fn list_header(&self) -> QString {
        QString::from(&qd::list_header())
    }

    fn list_header_point_size(&self) -> i32 {
        qd::LIST_HEADER_POINT_SIZE
    }

    fn list_item_point_size(&self) -> i32 {
        qd::LIST_ITEM_POINT_SIZE
    }

    fn section_point_size(&self) -> i32 {
        qd::SECTION_POINT_SIZE
    }

    fn tools_header(&self) -> QString {
        QString::from(&qd::tools_header())
    }

    fn tool_buttons(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        // SteamOS: "Check my desktop" means installing missing packages, so its
        // button is visible but disabled; the reason is a line under the column
        // (`toolsNotice`), since a disabled button's tooltip does not pop up.
        let steamos = rexon_core::distro_compat::running_on_steamos();
        let de = self.rust().current_de.clone();
        let have = |tool: &str| rexon_core::utils::which(tool, std::env::var("PATH").ok().as_deref()).is_some();
        let panel_backup =
            qd::panel_backup_supported(&de, &have) || rexon_core::backup::live_panel_copy_spec(&de).is_some();
        for button in qd::tool_buttons(&de, panel_backup) {
            let enabled = qd::steamos_button_block(button.kind, &de, true, steamos).is_none();
            out.append(QVariant::from(&vmap(vec![
                ("kind", qstr(button.kind)),
                ("text", qstr(&button.text)),
                ("tooltip", qstr(&button.tooltip)),
                ("enabled", QVariant::from(&enabled)),
            ])));
        }
        out
    }

    fn tools_notice(&self) -> QString {
        // Rexon recognized the session's desktop but does not support it (UKUI) —
        // say so plainly rather than stay silent above a list where this desktop
        // is absent.
        let notices: Vec<String> = [
            SteamOsBlock::Desktops.live(),
            system::unsupported_desktop_notice(&self.rust().current_de),
        ]
        .into_iter()
        .flatten()
        .collect();
        QString::from(&notices.join("\n\n"))
    }

    fn tool_button_min_height(&self) -> i32 {
        qd::TOOL_BUTTON_MIN_HEIGHT
    }

    fn placeholder_text(&self) -> QString {
        QString::from(&qd::placeholder_text())
    }

    fn looks_header(&self) -> QString {
        QString::from(&qd::looks_header())
    }

    fn looks_hint(&self) -> QString {
        QString::from(&qd::looks_hint())
    }

    /// Look tiles: what to show and which buttons — the core
    /// (`qt_desktops::look_tiles`); whether it is applied — by the shared
    /// journal.
    fn looks(&self) -> QList<QVariant> {
        let rexon = rexon_core::distro_looks::rexon_dir_live();
        let lang = rexon_core::i18n::current_language();
        let applied = |id: &str| rexon_core::distro_looks::is_applied(id, &rexon);
        let mut out = QList::<QVariant>::default();
        for tile in qd::look_tiles(&lang, &applied) {
            let mut buttons = QList::<QVariant>::default();
            for b in &tile.buttons {
                buttons.append(QVariant::from(&vmap(vec![
                    ("kind", qstr(b.kind)),
                    ("text", qstr(&b.text)),
                    ("tooltip", qstr(&b.tooltip)),
                    ("style", qstr(b.style)),
                ])));
            }
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(&tile.id)),
                ("name", qstr(&tile.name)),
                ("description", qstr(&tile.description)),
                ("applied", QVariant::from(&tile.applied)),
                ("badge", qstr(&tile.badge)),
                ("buttons", QVariant::from(&buttons)),
            ])));
        }
        out
    }

    fn compare_back_text(&self) -> QString {
        QString::from(&qd::compare_back_text())
    }

    fn card_title_point_size(&self) -> i32 {
        qd::CARD_TITLE_POINT_SIZE
    }

    fn card_status_point_size(&self) -> i32 {
        qd::CARD_STATUS_POINT_SIZE
    }

    fn card_max_width(&self) -> i32 {
        qd::CARD_MAX_WIDTH
    }

    fn card_gap_left(&self) -> i32 {
        qd::CARD_GAP_LEFT
    }

    /// Labels and numbers of the screenshot carousel as one map.
    fn carousel(&self) -> QMap<QMapPair_QString_QVariant> {
        vmap(vec![
            ("emptyText", qstr(&qd::no_screenshots_text())),
            ("zoomTooltip", qstr(&qd::carousel_zoom_tooltip())),
            ("prevTooltip", qstr(&qd::carousel_prev_tooltip())),
            ("nextTooltip", qstr(&qd::carousel_next_tooltip())),
            ("zoomButtonTooltip", qstr(&qd::carousel_zoom_button_tooltip())),
            ("zoomPrevTooltip", qstr(&qd::zoom_prev_tooltip())),
            ("zoomNextTooltip", qstr(&qd::zoom_next_tooltip())),
            ("zoomCloseTooltip", qstr(&qd::zoom_close_tooltip())),
            ("zoomTitle", qstr(&qd::zoom_title())),
            ("minHeight", QVariant::from(&qd::CAROUSEL_MIN_HEIGHT)),
            ("maxHeight", QVariant::from(&qd::CAROUSEL_MAX_HEIGHT)),
        ])
    }

    /// Markup sizes as one map: panel paddings, font sizes, button sizes. All
    /// numbers come from the tab core, where they are written out from the
    /// original's style sheet; the page defines none of its own.
    fn metrics(&self) -> QMap<QMapPair_QString_QVariant> {
        let (list_l, list_t, list_r, list_b) = qd::LIST_MARGINS;
        let (tools_l, tools_t, tools_r, tools_b) = qd::TOOLS_MARGINS;
        vmap(vec![
            ("listMarginLeft", qint(list_l)),
            ("listMarginTop", qint(list_t)),
            ("listMarginRight", qint(list_r)),
            ("listMarginBottom", qint(list_b)),
            ("listSpacing", qint(qd::LIST_SPACING)),
            ("listMinWidth", qint(qd::LIST_MIN_WIDTH)),
            ("listMaxWidth", qint(qd::LIST_MAX_WIDTH)),
            ("listInitialWidth", qint(qd::LIST_INITIAL_WIDTH)),
            ("listItemPaddingV", qint(qd::LIST_ITEM_PADDING_V)),
            ("listItemPaddingH", qint(qd::LIST_ITEM_PADDING_H)),
            ("listItemRadius", qint(qd::LIST_ITEM_RADIUS)),
            ("toolsMarginLeft", qint(tools_l)),
            ("toolsMarginTop", qint(tools_t)),
            ("toolsMarginRight", qint(tools_r)),
            ("toolsMarginBottom", qint(tools_b)),
            ("toolsSpacing", qint(qd::TOOLS_SPACING)),
            ("toolsMinWidth", qint(qd::TOOLS_MIN_WIDTH)),
            ("splitHandleWidth", qint(qd::SPLIT_HANDLE_WIDTH)),
            ("toolsInitialWidth", qint(qd::TOOLS_INITIAL_WIDTH)),
            ("toolButtonMaxWidth", qint(qd::TOOL_BUTTON_MAX_WIDTH)),
            ("looksToolsGap", qint(qd::LOOKS_TOOLS_GAP)),
            ("cardMarginTop", qint(qd::CARD_MARGIN_TOP)),
            ("cardMarginRight", qint(qd::CARD_MARGIN_RIGHT)),
            ("cardMarginBottom", qint(qd::CARD_MARGIN_BOTTOM)),
            ("cardSpacing", qint(qd::CARD_SPACING)),
            ("headerRowSpacing", qint(qd::HEADER_ROW_SPACING)),
            ("scrollbarReserve", qint(qd::DETAIL_SCROLLBAR_RESERVE)),
            ("placeholderPointSize", qint(qd::PLACEHOLDER_POINT_SIZE)),
            ("descPointSize", qint(qd::CARD_DESC_POINT_SIZE)),
            ("statLabelPointSize", qint(qd::STAT_LABEL_POINT_SIZE)),
            ("statSpacing", qint(qd::STAT_SPACING)),
            ("packagesPointSize", qint(qd::PACKAGES_POINT_SIZE)),
            ("versionPointSize", qint(qd::VERSION_POINT_SIZE)),
            ("buttonRowTopMargin", qint(qd::BUTTON_ROW_TOP_MARGIN)),
            ("buttonRowSpacing", qint(qd::BUTTON_ROW_SPACING)),
            ("cardButtonPaddingV", qint(qd::CARD_BUTTON_PADDING_V)),
            ("cardButtonPaddingH", qint(qd::CARD_BUTTON_PADDING_H)),
            ("carouselFrameRadius", qint(qd::CAROUSEL_FRAME_RADIUS)),
            ("carouselFramePaddingH", qint(qd::CAROUSEL_FRAME_PADDING_H)),
            ("carouselDotsSpacing", qint(qd::CAROUSEL_DOTS_SPACING)),
            ("noScreenshotsPointSize", qint(qd::NO_SCREENSHOTS_POINT_SIZE)),
            ("overlayMargin", qint(qd::OVERLAY_MARGIN)),
            ("arrowSize", qint(qd::ARROW_SIZE)),
            ("arrowPointSize", qint(qd::ARROW_POINT_SIZE)),
            ("zoomButtonSize", qint(qd::ZOOM_BUTTON_SIZE)),
            ("zoomButtonPointSize", qint(qd::ZOOM_BUTTON_POINT_SIZE)),
            ("zoomMargin", qint(qd::ZOOM_MARGIN)),
            ("zoomArrowSize", qint(qd::ZOOM_ARROW_SIZE)),
            ("zoomArrowPointSize", qint(qd::ZOOM_ARROW_POINT_SIZE)),
            ("zoomClosePointSize", qint(qd::ZOOM_CLOSE_POINT_SIZE)),
            ("zoomHintPadding", qint(qd::ZOOM_HINT_PADDING)),
            ("shellsPanelWidth", qint(qd::SHELLS_PANEL_WIDTH)),
            ("shellsPanelPadding", qint(qd::SHELLS_PANEL_PADDING)),
            ("shellsRowSpacing", qint(qd::SHELLS_ROW_SPACING)),
            ("shellsHeaderPointSize", qint(qd::SHELLS_HEADER_POINT_SIZE)),
            ("shellsSideBySideMinWidth", qint(qd::SHELLS_SIDE_BY_SIDE_MIN_WIDTH)),
        ])
    }

    /// Palette of the ACTIVE skin (the saved `ui_skin`, falling back to the
    /// standard one by rule #17) plus the colours the tab itself sets through
    /// the core. The page defines no colours of its own.
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let colors = ui_base::active_qml_palette();
        let dark = ui_base::active_skin_is_dark();
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &colors {
            out.insert(QString::from(key), qstr(value));
        }
        let text_primary = colors
            .get("text_primary")
            .map(String::as_str)
            .unwrap_or(ui_base::DARK_PRIMARY);
        out.insert(
            QString::from("status_installed"),
            qstr(qd::STATUS_INSTALLED_COLOR),
        );
        out.insert(
            QString::from("status_unavailable"),
            qstr(qd::STATUS_UNAVAILABLE_COLOR),
        );
        out.insert(
            QString::from("current_badge"),
            qstr(qd::CURRENT_BADGE_COLOR),
        );
        // Weight of the secondary text (description, specs, packages, version,
        // carousel dots): in the widgets this is `font-weight: 600` from
        // `secondary_text_qss`; without it QML set the same lines in the regular
        // weight (measurement 03.09.2026).
        out.insert(
            QString::from("text_secondary_weight"),
            qint(ui_base::SECONDARY_TEXT_WEIGHT),
        );
        // List section header: in the widgets it is the PlaceholderText role of
        // the skin palette, not secondary text with opacity.
        out.insert(
            QString::from("placeholder_text"),
            qstr(&ui_base::active_placeholder_text_color()),
        );
        // List: the background of the selected item and of the item under the
        // cursor. Previously the page took the skin accent with opacity 0.4 — its
        // own colour instead of the original's blue `rgba(59,130,246,0.4)`.
        out.insert(QString::from("list_selected"), qcss(qd::LIST_SELECTED_QSS));
        out.insert(
            QString::from("list_selected_text"),
            qstr(qd::LIST_SELECTED_TEXT_COLOR),
        );
        out.insert(QString::from("list_hover"), qcss(qd::list_hover_qss(dark)));
        // The toolbar, card separators and the carousel frame are shares of the
        // neutral grey, as in the original's QSS.
        // Splitter handle: without it QML took a light grey one from the Basic
        // style.
        out.insert(
            QString::from("split_handle"),
            qcss(qd::split_handle_qss(dark)),
        );
        out.insert(QString::from("tools_bg"), qneutral(qd::TOOLS_PANEL_BG_ALPHA));
        out.insert(
            QString::from("tools_border"),
            qneutral(qd::TOOLS_PANEL_BORDER_ALPHA),
        );
        out.insert(QString::from("separator"), qneutral(qd::SEPARATOR_ALPHA));
        out.insert(
            QString::from("carousel_frame"),
            qneutral(qd::CAROUSEL_FRAME_ALPHA),
        );
        // Card action buttons: blue, neutral with a border, and red. The red one
        // is a tone of its own, not the shared `RexonDangerButton`: in the original
        // its background is 0.75 versus 0.2 in "Extensions", rounding 6 and a bold
        // label, and the shared element cannot do that. Blue and red are the same
        // `primary_tone`/`destructive_tone` the operation window takes.
        insert_operation_tones(&mut out);
        out.insert(
            QString::from("neutral_tone"),
            tone(
                qneutral(qd::NEUTRAL_BUTTON_BG_ALPHA),
                qneutral(qd::NEUTRAL_BUTTON_HOVER_ALPHA),
                text_primary,
                qd::CARD_BUTTON_RADIUS,
                // The secondary button ("Check / Repair") is in the regular weight: a bold
                // long label competed with the main button and looked heavy
                // (Wehrwolfmann's remark 10.10.2026).
                false,
                Some(qneutral(qd::NEUTRAL_BUTTON_BORDER_ALPHA)),
            ),
        );
        // Arrows and the magnifier over the screenshot, the full-screen viewer
        // buttons — round and dark, as in the original, not skin buttons.
        out.insert(
            QString::from("arrow_tone"),
            tone(
                qcss(qd::OVERLAY_BUTTON_QSS),
                qcss(qd::ARROW_HOVER_QSS),
                qd::OVERLAY_TEXT_COLOR,
                qd::round_radius(qd::ARROW_SIZE),
                true,
                None,
            ),
        );
        out.insert(
            QString::from("zoom_button_tone"),
            tone(
                qcss(qd::OVERLAY_BUTTON_QSS),
                qcss(qd::ZOOM_BUTTON_HOVER_QSS),
                qd::OVERLAY_TEXT_COLOR,
                qd::round_radius(qd::ZOOM_BUTTON_SIZE),
                false,
                None,
            ),
        );
        out.insert(
            QString::from("zoom_arrow_tone"),
            tone(
                qcss(qd::OVERLAY_BUTTON_QSS),
                qcss(qd::ZOOM_BUTTON_HOVER_QSS),
                qd::OVERLAY_TEXT_COLOR,
                qd::round_radius(qd::ZOOM_ARROW_SIZE),
                true,
                None,
            ),
        );
        out.insert(QString::from("zoom_bg"), qcss(qd::ZOOM_BG_QSS));
        out.insert(QString::from("zoom_hint"), qcss(qd::ZOOM_HINT_QSS));
        // Background dimming under a modal window — one for the product.
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        out
    }

    fn current_de(&self) -> QString {
        QString::from(&self.rust().current_de)
    }

    fn selected_de(&self) -> QString {
        QString::from(&self.rust().selected)
    }

    fn count(&self) -> i32 {
        self.rust().rows.len() as i32
    }

    // ── Card of the selected desktop ──────────────────────────────────
    fn card(&self) -> QMap<QMapPair_QString_QVariant> {
        let state = self.rust();
        let Some(row) = state.row_of(&state.selected) else {
            // Nothing is selected — the page shows an invitation to choose.
            return vmap(vec![("deId", qstr(""))]);
        };
        let de_id = row.de_id.as_str();
        let is_current = de_id == state.current_de;
        let status = qd::status_view(row.installed, row.available);

        let mut stats = QList::<QVariant>::default();
        for cell in qd::stats(de_id) {
            stats.append(QVariant::from(&vmap(vec![
                ("icon", qstr(&cell.icon)),
                ("value", qstr(&cell.value)),
                ("label", qstr(&cell.label)),
            ])));
        }

        // SteamOS (Steam Deck and Steam Machine): verified desktops are installed
        // and removed as systemd-sysext images — the buttons are live; for the
        // others the buttons are visible but disabled, and the reason stands as a
        // line next to them — we do not hide a button silently (rule #13). The
        // core decides (`steamos_button_block`).
        let steamos = rexon_core::distro_compat::running_on_steamos();
        let mut buttons = QList::<QVariant>::default();
        let mut reasons: Vec<String> = Vec::new();
        for button in qd::card_buttons(row.installed, row.available, is_current) {
            let block = qd::steamos_button_block(button.kind, de_id, is_current, steamos);
            let enabled = block.is_none();
            if let Some(why) = block.filter(|w| !reasons.contains(w)) {
                reasons.push(why);
            }
            buttons.append(QVariant::from(&vmap(vec![
                ("kind", qstr(button.kind)),
                ("text", qstr(&button.text)),
                ("tooltip", qstr(&button.tooltip)),
                ("style", qstr(button.style)),
                ("enabled", QVariant::from(&enabled)),
                // It installs the desktop's set: disabled while it conflicts with
                // something installed (the `packagesReady` answer, the reason is above the
                // buttons).
                ("installsSet", QVariant::from(&qd::installs_desktop_set(button.kind))),
            ])));
        }
        let blocked_notice = reasons.join("\n");

        let main_pkgs = system::get_packages(de_id, state.family()).len();
        let aur_pkgs = system::get_aur_packages(de_id, state.family()).len();
        // An honest count is given by a trial transaction (Arch, in the background
        // — the actions object, `requestPackages`); until its answer — "counting",
        // not a known-wrong number of names. Where there is no probe — the core's
        // count of names. On SteamOS a `pacman -Sp` probe over the packages of the
        // system itself says nothing about what the systemd-sysext image will
        // build, and its "conflict" would switch off a live "Install" button — so
        // it is not done there.
        let probe = row.available && state.family() == system::PkgFamily::Arch && !steamos;
        let packages_text = if probe {
            qd::packages_counting()
        } else {
            qd::packages_view(row.installed, None, main_pkgs, aur_pkgs).text
        };

        let mut images = QList::<QVariant>::default();
        for path in carousel_images(&state.screenshot_dir, de_id) {
            // QML loads a picture by address, not by path.
            images.append(qstr(&format!("file://{path}")));
        }

        vmap(vec![
            ("deId", qstr(de_id)),
            ("displayName", qstr(qd::display_name(de_id))),
            ("icon", qstr(qc::de_icon(de_id))),
            ("title", qstr(&qd::item_text(de_id))),
            ("installed", QVariant::from(&row.installed)),
            ("available", QVariant::from(&row.available)),
            ("isCurrent", QVariant::from(&is_current)),
            ("statusText", qstr(&status.text)),
            ("statusColor", qstr(status.color)),
            ("statusBold", QVariant::from(&status.bold)),
            // The "Partially installed" colour (header replacement from the package
            // probe answer).
            ("statusPartialColor", qstr(qd::STATUS_UNAVAILABLE_COLOR)),
            ("description", qstr(&qd::description(de_id))),
            ("stats", QVariant::from(&stats)),
            ("packagesText", qstr(&packages_text)),
            ("packagesProbe", QVariant::from(&probe)),
            ("buttons", QVariant::from(&buttons)),
            ("blockedNotice", qstr(&blocked_notice)),
            ("notice", qstr(&if row.available { String::new() } else { qd::unavailable_notice() })),
            ("currentBadgeText", qstr(&qd::current_badge_text())),
            ("currentBadgeTooltip", qstr(&qd::current_badge_tooltip())),
            ("versionChecking", qstr(&qd::version_checking())),
            ("images", QVariant::from(&images)),
            // The "Shells" block on the right: only for Hyprland, niri and sway.
            ("shellsHeader", qstr(&qd::shells_header())),
            ("shellsTooltip", qstr(&qd::shells_header_tooltip())),
            (
                "shells",
                QVariant::from(&shell_rows_qml(de_id, state.family(), row.installed, &state.current_de)),
            ),
        ])
    }

    // ── QAbstractListModel ────────────────────────────────────────────
    fn row_count(&self, parent: &QModelIndex) -> i32 {
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
    /// answers the same way: otherwise the checks would look somewhere other
    /// than where the delegate looks.
    fn role_of(&self, entry: &qd::ListRow, name: Option<&str>) -> QVariant {
        match name {
            Some("kind") => qstr(entry.kind),
            Some("deId") => qstr(&entry.de_id),
            Some("text") => qstr(&entry.text),
            Some("tooltip") => qstr(&entry.tooltip),
            Some("installed") => QVariant::from(&entry.installed),
            Some("available") => QVariant::from(&entry.available),
            // A section header is visible but not selectable — in the original it was
            // `NoItemFlags`.
            Some("selectable") => QVariant::from(&(entry.kind == "de")),
            Some("isCurrent") => {
                QVariant::from(&(entry.kind == "de" && entry.de_id == self.rust().current_de))
            }
            _ => QVariant::default(),
        }
    }

    // ── API for QML ───────────────────────────────────────────────────
    fn select(mut self: Pin<&mut Self>, de_id: &QString) {
        let de_id = de_id.to_string();
        {
            let mut state = self.as_mut().rust_mut();
            state.ensure_loaded();
            if state.selected == de_id || state.row_of(&de_id).is_none() {
                return;
            }
            state.selected = de_id;
        }
        self.as_mut().card_changed();
    }

    fn refresh_installed(mut self: Pin<&mut Self>) {
        let rows = {
            let mut state = self.as_mut().rust_mut();
            state.ensure_loaded();
            for row in state.rows.iter_mut() {
                if row.kind != "de" {
                    continue;
                }
                row.installed = de_installed(&row.de_id);
                row.tooltip = qd::item_tooltip(&row.de_id, row.installed, row.available);
            }
            state.rows.len()
        };
        if rows > 0 {
            let first = self.index(0, 0, &QModelIndex::default());
            let last = self.index(rows as i32 - 1, 0, &QModelIndex::default());
            let roles = installed_roles();
            self.as_mut().data_changed(&first, &last, &roles);
        }
        self.as_mut().card_changed();
        self.as_mut().count_changed();
        // A look was applied or restored — re-read the tiles.
        self.as_mut().looks_changed();
    }

    fn remove_plan(&self, de_id: &QString) -> QVariant {
        let de_id = de_id.to_string();
        let state = self.rust();
        if state.row_of(&de_id).is_none() {
            return QVariant::default();
        }
        let plan = qd::remove_plan(
            &de_id,
            qd::display_name(&de_id),
            &state.current_de,
            state.other_installed(&de_id),
        );
        QVariant::from(&vmap(vec![
            ("kind", qstr(plan.kind)),
            ("title", qstr(&plan.title)),
            ("text", qstr(&plan.text)),
            ("allowed", QVariant::from(&plan.allowed())),
            (
                "operationTitle",
                qstr(&qd::remove_operation_title(qd::display_name(&de_id))),
            ),
        ]))
    }

    fn operation_texts(&self, kind: &QString, de_id: &QString) -> QVariant {
        let de_id = de_id.to_string();
        let name = qd::display_name(&de_id);
        match kind.to_string().as_str() {
            "install" => {
                let t = qd::install_texts(name);
                QVariant::from(&vmap(vec![
                    ("confirmTitle", qstr(&t.confirm_title)),
                    ("confirmHtml", qstr(&t.confirm_html)),
                    ("operationTitle", qstr(&t.operation_title)),
                    ("doneTitle", qstr(&t.done_title)),
                    ("doneText", qstr(&t.done_text)),
                ]))
            }
            "repair" => {
                let t = qd::repair_texts(name);
                QVariant::from(&vmap(vec![
                    ("confirmTitle", qstr(&t.confirm_title)),
                    ("confirmHtml", qstr(&t.confirm_html)),
                    ("operationTitle", qstr(&t.operation_title)),
                    ("doneTitle", qstr(&t.done_title)),
                    ("doneText", qstr(&t.done_text)),
                ]))
            }
            // The switching texts are shared with "DE comparison": they are word for
            // word the same there, and a second copy would silently drift apart.
            "switch" => {
                let t = qc::switch_texts(name);
                QVariant::from(&vmap(vec![
                    ("confirmTitle", qstr(&t.confirm_title)),
                    ("confirmText", qstr(&t.confirm_text)),
                    ("operationTitle", qstr(&t.operation_title)),
                ]))
            }
            "remove" => self.remove_plan(&QString::from(&de_id)),
            _ => QVariant::default(),
        }
    }

    fn display_name(&self, de_id: &QString) -> QString {
        QString::from(qd::display_name(&de_id.to_string()))
    }

    fn version_line(&self, version: &QString) -> QString {
        QString::from(&qd::version_line(&version.to_string()))
    }

    fn zoom_hint(&self, index: i32, count: i32) -> QString {
        QString::from(&qd::zoom_hint(index.max(0) as usize, count.max(0) as usize))
    }

    fn carousel_dots(&self, index: i32, count: i32) -> QString {
        QString::from(&qd::carousel_dots(
            index.max(0) as usize,
            count.max(0) as usize,
        ))
    }

    fn carousel_height(&self, window_height: i32, available_width: i32) -> i32 {
        qd::carousel_height(window_height, available_width)
    }

    fn carousel_height_fitted(&self, full: i32, room: i32) -> i32 {
        qd::carousel_height_fitted(full, room)
    }

    fn detail_panel_max_width(&self, window_height: i32) -> i32 {
        qd::detail_panel_max_width(window_height)
    }

    fn looks_panel_width(&self, total: i32, window_height: i32) -> i32 {
        qd::split_widths(total, window_height).1
    }

    fn looks_panel_min_width(&self, header_width: i32, column_width: i32) -> i32 {
        qd::looks_panel_min_width(header_width, column_width)
    }

    fn looks_tools_side_by_side(&self, inner_width: i32, looks_width: i32, tools_width: i32) -> bool {
        qd::looks_tools_side_by_side(inner_width, looks_width, tools_width)
    }

    fn looks_tools_block_width(&self, inner_width: i32, side_by_side: bool) -> i32 {
        qd::looks_tools_block_width(inner_width, side_by_side)
    }

    fn card_left_spacer(&self, viewport: i32) -> i32 {
        qd::card_left_spacer(viewport)
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

impl cxx_qt::Initialize for qobject::DesktopsModel {
    /// The list of desktops is built at once: `rowCount` is called from QML
    /// before any of our code, and an empty model on the first frame would mean
    /// an empty tab.
    fn initialize(mut self: Pin<&mut Self>) {
        // Subscribing to skin changes: one line per page, the broadcast is shared
        // (`crate::skin`). Removed in `Drop` of the same state.
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::DesktopsModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
        self.as_mut().rust_mut().ensure_loaded();
        self.as_mut().start_screenshot_download();
    }
}

impl qobject::DesktopsModel {
    /// Download the official desktop screenshots that are missing from the
    /// cache.
    ///
    /// Without this, on a FRESH machine the cards would stay with placeholders
    /// forever: the page reads screenshots only from disk, and they were
    /// downloaded by the Python host, which the Rust window does not call. The
    /// download is run by the core (`auto_download_all_live`) in its own thread
    /// — the window does not wait; the directory and the file names are the
    /// same as in Python, so the two windows share the cache.
    ///
    /// The card is re-read ONCE — when the batch of the shown desktop has
    /// finished: the screenshots arrived or the placeholder changed "loading"
    /// to an honest label. A card change resets the carousel slide and asks for
    /// the package version again, so we do not trigger it on every file. With a
    /// warm cache there are no tasks at all.
    ///
    /// The thread, "one download per process" and the placeholder state belong
    /// to the core (`start_auto_download_live`); here there is only redrawing.
    fn start_screenshot_download(mut self: Pin<&mut Self>) {
        let dir = self.rust().screenshot_dir.clone();
        let handle = self.as_mut().qt_thread();
        screenshots::start_auto_download_live(&dir, move |outcome| {
            if !outcome.de_finished {
                return;
            }
            let de = outcome.de.clone();
            // The page may have closed — then there is nobody to redraw.
            let _ = handle.queue(move |mut object| {
                if object.rust().selected == de {
                    object.as_mut().card_changed();
                }
            });
        });
    }
}
