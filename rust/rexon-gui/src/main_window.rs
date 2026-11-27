// MainWindowModel — the host of the Rexon main window on Rust + cxx-qt +
// QML.
//
// WHY. Until now the dependency pointed the wrong way: the program was
// started by Python (`rexon.py` → `rexon/app.py` → `rexon/qt_main/app.py`),
// it created the `QApplication`, it built the `QMainWindow` with a
// `QTabWidget`, and Rust connected to it as a library (`librexon_gui.so`
// through `ctypes.CDLL`) and handed QML pages into the widgets through a
// `QQuickWidget`. The window belonged to the widgets, and QML lay INSIDE
// them. As long as the owner of the process is Python, leaving it is
// impossible in principle: you can only remove what you own.
//
// Here the same window is turned upside down: the host is Rust, the whole
// window is an `ApplicationWindow` in QML (`qml/MainWindow.qml`), and there
// are no widgets in the process at all.
//
// Warning: THIS IS A SKELETON, NOT A REPLACEMENT FOR THE PRODUCT. The live
// Rexon still starts as before and does not use this code: the
// `rexon-main-window` binary is a separate developer entry point, exactly
// like the `rexon-gui-selftest` bench next to it. We grow it alongside,
// behind a separate entry point, as we already did seven times with tabs.
//
// WHAT IS HERE:
//   * `MainWindowModel` — the title, the minimum size, the set of sections
//     and the skin palette. Not a single decision of its own: the order and
//     labels of the sections come from `rexon_core::qt_main`, the palette
//     from `ui_base::active_qml_palette`, as for all ported tabs.
//
// "DE comparison" no longer has its own side-actions object here: the page
// takes the same `DesktopsActions` as "Desktops" (src/desktops_actions.rs)
// — one executor (`rexon_core::system_de_exec`) and one progress window
// (`qml/DeOperationDialog.qml`) for both tabs.

/// cxx-qt bridge: the main window host.
///
/// Warning: the type is called `MainWindowModel`, not `MainWindow`: the
/// module's .qml files become QML types by file name themselves, and
/// `MainWindow` is taken by `qml/MainWindow.qml`. The rule for all tabs and
/// the window: the view is `<Name>.qml`, the object `<Name>Model`.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // ── Window. CONSTANT: does not change during the window's lifetime.
        #[qproperty(QString, window_title, cxx_name = "windowTitle",
                    READ = window_title, CONSTANT)]
        #[qproperty(i32, window_min_width, cxx_name = "windowMinWidth",
                    READ = window_min_width, CONSTANT)]
        #[qproperty(i32, window_min_height, cxx_name = "windowMinHeight",
                    READ = window_min_height, CONSTANT)]
        // ── Sections: [{id, label, page}, …] in window order. Order and labels
        // come from the core; here there is only laying them out as maps for QML.
        #[qproperty(QList_QVariant, sections, READ = sections, CONSTANT)]
        // ── Standard skin palette (steam_dark, rule #17). Not CONSTANT for the
        // same reason as in the tabs: the skin changes on the fly, and without a
        // signal the window would keep the previous skin's colours.
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        // ── Tab plate: colours and layout of the "chip" in the section strip. The
        // markup defines no numbers of its own — they are in the core
        // (`qt_main::TAB_*`), shared with the widget window. CONSTANT: the plate's
        // neutral grey does not depend on the skin.
        #[qproperty(QMap_QString_QVariant, tab_style, cxx_name = "tabStyle",
                    READ = tab_style, CONSTANT)]
        type MainWindowModel = super::MainWindowModelRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "paletteChanged"]
        fn palette_changed(self: Pin<&mut MainWindowModel>);
    }

    extern "RustQt" {
        #[cxx_name = "windowTitle"]
        fn window_title(self: &MainWindowModel) -> QString;
        #[cxx_name = "windowMinWidth"]
        fn window_min_width(self: &MainWindowModel) -> i32;
        #[cxx_name = "windowMinHeight"]
        fn window_min_height(self: &MainWindowModel) -> i32;
        fn sections(self: &MainWindowModel) -> QList_QVariant;
        fn palette(self: &MainWindowModel) -> QMap_QString_QVariant;
        #[cxx_name = "tabStyle"]
        fn tab_style(self: &MainWindowModel) -> QMap_QString_QVariant;
    }

    extern "RustQt" {
        /// Text in place of a section that has no Rust host yet.
        #[qinvokable]
        #[cxx_name = "notPortedNotice"]
        fn not_ported_notice(self: &MainWindowModel, section_id: &QString) -> QString;

        /// Tour mode (`--tour`): the answer to the previous command (JSON, empty —
        /// the first call) → the next command (JSON). The core decides
        /// (`rexon_core::main_window_tour`).
        #[qinvokable]
        #[cxx_name = "tourNext"]
        fn tour_next(self: &MainWindowModel, report: &QString) -> QString;

        /// Tour mode: whether a page method may be called. The executor asks before
        /// EVERY call.
        #[qinvokable]
        #[cxx_name = "tourCallAllowed"]
        fn tour_call_allowed(self: &MainWindowModel, method: &QString) -> bool;

        /// Tour mode: wait measurement (JSON) → `wait`/`ready`/`timeout`.
        #[qinvokable]
        #[cxx_name = "tourWaitVerdict"]
        fn tour_wait_verdict(self: &MainWindowModel, sample: &QString) -> QString;

        /// Tour mode: capture the whole window into the PNG `path`.
        #[qinvokable]
        #[cxx_name = "tourGrab"]
        fn tour_grab(self: &MainWindowModel, path: &QString) -> bool;
    }

    // Trait declarations must be IN THE BRIDGE: cxx-qt emits both the
    // constructor and the thread type alias from them.
    impl cxx_qt::Initialize for MainWindowModel {}
    impl cxx_qt::Threading for MainWindowModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};

use rexon_core::qt_main;
use rexon_core::ui_base;

// ============================================================================
// Sections: which of them the Rust frame already shows for real
// ============================================================================

/// Section → the QML page the frame puts into it.
///
/// Empty means the section has no page; the frame will show an explanation
/// from the core (`qt_main::not_ported_notice`) rather than an empty
/// rectangle.
///
/// Warning: the table lives HERE, not in the core, on purpose: the core does
/// not and must not know that .qml files exist. It does, however, set the
/// order and the set of sections entirely — here there is only the
/// readiness mark.
///
/// The page identifier does NOT look up markup: the markup behind it is held
/// by the `pageComponents` map in `qml/MainWindow.qml`, and the
/// `every_page_is_in_the_qml_map` guard does not let these two lists drift
/// apart.
///
/// "desktops" has its own page, not the "compare" sub-page, and this is a
/// correction to the facts, not to the plan: a side-actions object appeared
/// for "Desktops" (`src/desktops_actions.rs`), and the page stopped handing
/// out dead buttons. "DE comparison" did not go anywhere — since 08.07.2026
/// it is the page INSIDE "Desktops", and the window shows it there, as the
/// section's second page (the `pageComponents` map in qml/MainWindow.qml).
/// So the section no longer has a sub-page: a "DE comparison" label on the
/// tab itself would mean that the comparison is the whole section.
const SECTION_PAGES: &[(&str, &str)] = &[
    ("desktops", "desktops"),
    ("appearance", "appearance"),
    ("extensions", "extensions"),
    ("wallpaper_engine", "wallpaper_engine"),
    ("steam", "steam"),
    ("settings", "settings"),
];

fn page_of(section_id: &str) -> &'static str {
    SECTION_PAGES
        .iter()
        .find(|(id, _)| *id == section_id)
        .map(|(_, page)| *page)
        .unwrap_or("")
}

// ============================================================================
// Translation odds and ends in Qt
// ============================================================================

fn qstr(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

fn vmap(pairs: Vec<(&str, QVariant)>) -> QMap<QMapPair_QString_QVariant> {
    let mut out = QMap::<QMapPair_QString_QVariant>::default();
    for (key, value) in pairs {
        out.insert(QString::from(key), value);
    }
    out
}

// ============================================================================
// Window host
// ============================================================================

/// Rust state of the window.
#[derive(Default)]
pub struct MainWindowModelRust {
    /// Entry number in the shared skin change registry (`crate::skin`).
    /// 0 means the window is not subscribed yet.
    skin_watch: u64,
}

impl Drop for MainWindowModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

impl qobject::MainWindowModel {
    fn window_title(&self) -> QString {
        // The same title as the Python window's: the desktop launcher moves to this
        // window, and a developer note in it would be jargon in front of the
        // customer.
        QString::from(&qt_main::window_title())
    }

    fn window_min_width(&self) -> i32 {
        qt_main::WINDOW_MIN_SIZE.0
    }

    fn window_min_height(&self) -> i32 {
        qt_main::WINDOW_MIN_SIZE.1
    }

    /// Tab bar contents: [{id, label, page, subsection}, …] in window order.
    fn sections(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for id in qt_main::SECTIONS {
            let page = page_of(id);
            out.append(QVariant::from(&vmap(vec![
                ("id", qstr(id)),
                ("label", qstr(&qt_main::section_tab_label(id))),
                // Which page to put into the section; empty means a placeholder.
                ("page", qstr(page)),
                // Sub-page label: "DE comparison" lives INSIDE "Desktops", and the window
                // must show that rather than pass the sub-page off as a section.
                (
                    "subsection",
                    qstr(&if page.is_empty() || page == *id {
                        // There is no sub-page: the section shows ITS OWN page (or a placeholder
                        // for now) and needs no second name. Without this check "Appearance" would
                        // be labelled with the sub-page "appearance" — an identifier instead of a
                        // human name.
                        String::new()
                    } else {
                        qt_main::subsection_label(page)
                    }),
                ),
            ])));
        }
        out
    }

    /// Palette of the ACTIVE skin — the same source as for the ported tabs. The
    /// window defines no colours of its own: a second palette would silently
    /// set the frame apart from the product.
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        // The operation window over the main window (restoring the look at
        // start-up): the dimming and the "Yes" colours come from the same helpers
        // as for the "Desktops", "Compare" and "Settings" pages.
        out.insert(QString::from("modal_dim"), qstr(&ui_base::modal_dim_argb_hex()));
        crate::desktops_page::insert_operation_tones(&mut out);
        out
    }

    /// Tab plate: colours and layout shared with the widget window.
    ///
    /// WHY IT EXISTS. Inactive tabs were drawn transparent, and the section
    /// strip came out as one solid block: the boundary between neighbours was
    /// not visible at all (Wehrwolfmann's remark 18.09.2026). In the widget
    /// window every tab has its own plate with a gap — here the same numbers
    /// are taken from the core rather than picked by eye a second time.
    ///
    /// The colours go through `css_color_to_qml`: QML reads the `rgba(...)`
    /// notation as an invalid colour and draws it BLACK.
    fn tab_style(&self) -> QMap<QMapPair_QString_QVariant> {
        let col = |css: &str| qstr(&ui_base::css_color_to_qml(css));
        let num = |v: i32| QVariant::from(&v);
        vmap(vec![
            ("idleFill", col(&qt_main::tab_idle_fill())),
            ("hoverFill", col(&qt_main::tab_hover_fill())),
            ("radius", num(qt_main::TAB_RADIUS)),
            ("marginH", num(qt_main::TAB_MARGIN_H)),
            ("marginV", num(qt_main::TAB_MARGIN_V)),
        ])
    }

    fn not_ported_notice(&self, section_id: &QString) -> QString {
        QString::from(qt_main::not_ported_notice(&section_id.to_string()))
    }

    fn tour_next(&self, report: &QString) -> QString {
        QString::from(&tour_step(&report.to_string()))
    }

    fn tour_call_allowed(&self, method: &QString) -> bool {
        tour::call_allowed(&method.to_string())
    }

    fn tour_wait_verdict(&self, sample: &QString) -> QString {
        let sample: tour::WaitSample = serde_json::from_str(&sample.to_string()).unwrap_or_default();
        QString::from(tour::wait_verdict(&sample).as_str())
    }

    fn tour_grab(&self, path: &QString) -> bool {
        crate::tray::grab_main_window(&path.to_string())
    }
}

// ============================================================================
// Tour mode: the state lives here, the decisions in the core
// ============================================================================

use rexon_core::main_window_tour as tour;

/// The tour of this process. Under a lock rather than in the window object:
/// the shared timeout writes the report from its own thread (`tour_abort`).
static TOUR: std::sync::Mutex<Option<tour::Tour>> = std::sync::Mutex::new(None);

/// Start a tour (called by the binary before the QML is loaded).
pub fn tour_install(cfg: tour::TourConfig) {
    if let Ok(mut t) = TOUR.lock() {
        *t = Some(tour::Tour::new(cfg));
    }
}

fn journal_of(cfg: &tour::TourConfig) -> String {
    std::fs::read(cfg.out_dir.join(tour::JOURNAL_FILE))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

fn write_report(cfg: &tour::TourConfig, text: &str) {
    let path = cfg.out_dir.join(tour::REPORT_FILE);
    match std::fs::write(&path, text) {
        Ok(()) => println!("tour: report {}", path.display()),
        Err(e) => println!("tour: report {} not written: {e}", path.display()),
    }
}

/// One tour step: take the answer, give out a command. At the end — a
/// report to disk.
fn tour_step(report: &str) -> String {
    let Ok(mut guard) = TOUR.lock() else {
        return tour::Command::Finish { exit_code: tour::EXIT_BAD_ARGS, report: String::new() }.to_json();
    };
    let Some(t) = guard.as_mut() else {
        return tour::Command::Finish { exit_code: tour::EXIT_BAD_ARGS, report: String::new() }.to_json();
    };
    let journal = journal_of(&t.cfg);
    let report = (!report.trim().is_empty()).then(|| {
        let mut r = tour::Report::from_json(report);
        r.log_offset = journal.len() as u64;
        r
    });
    let cmd = t.next(report, &journal);
    match &cmd {
        tour::Command::Finish { exit_code, report } => {
            write_report(&t.cfg, report);
            println!("tour finished: {} screenshots, code {exit_code}", t.screens().len());
        }
        other => println!("tour: {}", other.to_json()),
    }
    cmd.to_json()
}

/// Shared timeout: a report of what was done and the exit code. Called from
/// the watchdog thread; if the tour is already over — `None`.
pub fn tour_abort(reason: &str) -> Option<i32> {
    let guard = TOUR.lock().ok()?;
    let t = guard.as_ref()?;
    if t.is_finished() {
        return None;
    }
    let (text, code) = t.report(&journal_of(&t.cfg), Some(reason));
    write_report(&t.cfg, &text);
    Some(code)
}

impl cxx_qt::Initialize for qobject::MainWindowModel {
    fn initialize(mut self: Pin<&mut Self>) {
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::MainWindowModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every core section has a row in the readiness table, and vice versa.
    ///
    /// Without this guard a new section in the core would silently get an empty
    /// page, and a removed one would leave an orphaned row here (rule #12).
    #[test]
    fn section_pages_cover_core_sections() {
        let core: Vec<&str> = qt_main::SECTIONS.to_vec();
        let here: Vec<&str> = SECTION_PAGES.iter().map(|(id, _)| *id).collect();
        assert_eq!(
            core, here,
            "the section readiness table diverged from the core \
             (rexon_core::qt_main::SECTIONS)"
        );
    }

    /// Every non-empty page in the table must be listed in the window's
    /// `pageComponents` map.
    ///
    /// A divergence of these two lists would mean a section that Rust declared
    /// ready but QML cannot open: the user would see the explanation "not
    /// ported yet" instead of the page — that is, a lie about the cause, and a
    /// silent one (rule #12).
    #[test]
    fn every_page_is_in_the_qml_map() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("qml/MainWindow.qml");
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let map = src
            .split_once("readonly property var pageComponents: ({")
            .and_then(|(_, rest)| rest.split_once("})"))
            .map(|(body, _)| body)
            .expect("the pageComponents map was not found in qml/MainWindow.qml");
        for (id, page) in SECTION_PAGES {
            if page.is_empty() {
                continue;
            }
            assert!(
                map.contains(&format!("\"{page}\":")),
                "page \"{page}\" of section \"{id}\" is missing from the window's \
                 pageComponents map (qml/MainWindow.qml)"
            );
        }
    }

    /// A section whose page is named like itself has no sub-page: a sub-page
    /// label is "DE comparison" inside "Desktops", not a page identifier passed
    /// off as a name.
    #[test]
    fn only_a_real_subsection_gets_a_subsection_label() {
        for (id, page) in SECTION_PAGES {
            if page.is_empty() || page == id {
                continue;
            }
            assert_ne!(
                qt_main::subsection_label(page),
                *page,
                "subsection \"{page}\" (section \"{id}\") has no human \
                 label in rexon_core::qt_main::subsection_label"
            );
        }
    }

}
