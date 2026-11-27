// OnboardingModel — the first-run tutorial for the Rust window.
//
// In the Python window the tutorial is drawn by `rexon/onboarding.py` (a
// widget overlay over the `QMainWindow`). The QML window has no widgets, so
// the overlay is `qml/OnboardingOverlay.qml`, and this object only relays
// the core to it: the scenario, texts, the decision "what to show", saving
// progress, the geometry of the cut-out and the card, the skin palette
// (`rexon_core::onboarding`). There is not a single decision or label of
// our own here.
//
// The state is written to the SHARED `settings.json` with the same keys as
// in the Python window: someone who took the tutorial in one entry point
// will not see it in the other.
//
// THE DOOR FOR "TAKE THE TUTORIAL AGAIN". The button lives on the
// "Settings" tab (`SettingsActions`), and the overlay on the window; they
// have no direct link. [`request_replay`] pushes the `replayRequested`
// signal to all live tutorial objects through the Qt thread queue, and the
// overlay starts over on it.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // Palette of the active skin and the overlay numbers from the core. Not
        // CONSTANT: the skin changes on the fly.
        #[qproperty(QMap_QString_QVariant, style, READ = style, NOTIFY = style_changed)]
        type OnboardingModel = super::OnboardingModelRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "styleChanged"]
        fn style_changed(self: Pin<&mut OnboardingModel>);

        /// The "Take the tutorial again" button was pressed somewhere in the window.
        #[qsignal]
        #[cxx_name = "replayRequested"]
        fn replay_requested(self: Pin<&mut OnboardingModel>);
    }

    extern "RustQt" {
        fn style(self: &OnboardingModel) -> QMap_QString_QVariant;

        /// Decision at startup: `{kind: "ask"|"show"|"nothing", step}`.
        #[qinvokable]
        fn start(self: &OnboardingModel) -> QMap_QString_QVariant;

        /// Answer to the first-run question; returns the decision about what comes
        /// next.
        #[qinvokable]
        #[cxx_name = "answerAsk"]
        fn answer_ask(self: &OnboardingModel, show: bool) -> QMap_QString_QVariant;

        /// Reset progress and turn the tutorial on; returns the decision.
        #[qinvokable]
        fn replay(self: &OnboardingModel) -> QMap_QString_QVariant;

        /// "Next" on step `step_id`: the identifier of the next step, or an empty
        /// string if the scenario is complete.
        #[qinvokable]
        fn advance(self: &OnboardingModel, step_id: &QString) -> QString;

        /// "Skip": the whole scenario is marked as completed.
        #[qinvokable]
        #[cxx_name = "skipAll"]
        fn skip_all(self: &OnboardingModel);

        /// Texts of the first-run question.
        #[qinvokable]
        fn texts(self: &OnboardingModel) -> QMap_QString_QVariant;

        /// Content of the step card; an empty map means there is no step.
        #[qinvokable]
        fn card(self: &OnboardingModel, step_id: &QString) -> QMap_QString_QVariant;

        /// Where to point the highlight: `{kind: "window"|"tab"|"tab_content", tab}`.
        #[qinvokable]
        fn aim(self: &OnboardingModel, step_id: &QString) -> QMap_QString_QVariant;

        /// Cut-out around the target: `{valid, x, y, width, height}`.
        #[qinvokable]
        fn hole(self: &OnboardingModel, x: i32, y: i32, width: i32, height: i32)
            -> QMap_QString_QVariant;

        /// Card position in the overlay: `{x, y}`.
        #[qinvokable]
        fn placement(
            self: &OnboardingModel,
            overlay_width: i32,
            overlay_height: i32,
            card_width: i32,
            card_height: i32,
            has_hole: bool,
            hole_x: i32,
            hole_y: i32,
            hole_width: i32,
            hole_height: i32,
        ) -> QMap_QString_QVariant;

        /// Card font sizes derived from the application font size:
        /// `{progress, title}`.
        #[qinvokable]
        #[cxx_name = "pointSizes"]
        fn point_sizes(self: &OnboardingModel, base: i32) -> QMap_QString_QVariant;
    }

    impl cxx_qt::Initialize for OnboardingModel {}
    impl cxx_qt::Threading for OnboardingModel {}
}

use core::pin::Pin;
use std::sync::{Mutex, OnceLock};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QVariant};

use rexon_core::onboarding as ob;
use rexon_core::settings::SettingsManager;

type VMap = QMap<QMapPair_QString_QVariant>;

fn vmap(pairs: Vec<(&str, QVariant)>) -> VMap {
    let mut out = VMap::default();
    for (key, value) in pairs {
        out.insert(QString::from(key), value);
    }
    out
}

fn qs(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

fn qi(value: i32) -> QVariant {
    QVariant::from(&value)
}

/// The core's decision in a form convenient for the overlay.
fn start_map(start: &ob::Start) -> VMap {
    let (kind, step) = match start {
        ob::Start::Ask => ("ask", ""),
        ob::Start::Show(id) => ("show", *id),
        ob::Start::Nothing => ("nothing", ""),
    };
    vmap(vec![("kind", qs(kind)), ("step", qs(step))])
}

/// Unsaved progress does not abort the tutorial, but it must not be kept
/// quiet: otherwise the user will see the tutorial again and nobody will
/// know why.
fn report(saved: std::io::Result<()>) {
    if let Err(e) = saved {
        rexon_core::log_warn!("onboarding", "прогресс не сохранён в настройки: {e}");
    }
}

/// The live settings manager; `None` means the settings directory is not
/// available.
fn settings() -> Option<SettingsManager> {
    match SettingsManager::new() {
        Ok(m) => Some(m),
        Err(e) => {
            rexon_core::log_warn!("onboarding", "настройки не открылись: {e}");
            None
        }
    }
}

// ============================================================================
// Registry of live objects for "take it again"
// ============================================================================

type Notify = Box<dyn Fn() -> Result<(), ()> + Send>;

fn registry() -> &'static Mutex<(u64, Vec<(u64, Notify)>)> {
    static REGISTRY: OnceLock<Mutex<(u64, Vec<(u64, Notify)>)>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new((0, Vec::new())))
}

/// Ask the window to run the tutorial again (the button in "Settings").
///
/// `true` means at least one live tutorial object accepted the request,
/// i.e. the Rust window is open and will start the tutorial by itself.
/// `false` means there is no window with an overlay in the process (the tab
/// is open in the Python host or as a standalone window), and the caller is
/// obliged to tell the user so.
pub fn request_replay() -> bool {
    let mut reg = registry().lock().expect("реестр обучения не отравлен");
    let mut told = 0usize;
    reg.1.retain(|(_, notify)| match notify() {
        Ok(()) => {
            told += 1;
            true
        }
        Err(()) => false,
    });
    told > 0
}

// ============================================================================
// Object
// ============================================================================

#[derive(Default)]
pub struct OnboardingModelRust {
    skin_watch: u64,
    replay_watch: u64,
}

impl Drop for OnboardingModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
        if self.replay_watch != 0 {
            let mut reg = registry().lock().expect("реестр обучения не отравлен");
            reg.1.retain(|(id, _)| *id != self.replay_watch);
        }
    }
}

impl cxx_qt::Initialize for qobject::OnboardingModel {
    fn initialize(mut self: Pin<&mut Self>) {
        let skin = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::OnboardingModel::style_changed,
        );
        let handle = self.as_ref().get_ref().qt_thread();
        let replay = {
            let mut reg = registry().lock().expect("реестр обучения не отравлен");
            reg.0 += 1;
            let id = reg.0;
            reg.1.push((
                id,
                Box::new(move || {
                    match handle.queue(qobject::OnboardingModel::replay_requested) {
                        Ok(()) => Ok(()),
                        Err(cxx_qt::ThreadingQueueError::ObjectDestroyed) => Err(()),
                        Err(_) => Ok(()),
                    }
                }),
            ));
            id
        };
        let mut rust = self.as_mut().rust_mut();
        rust.skin_watch = skin;
        rust.replay_watch = replay;
    }
}

impl qobject::OnboardingModel {
    fn style(&self) -> VMap {
        let pal = ob::overlay_palette();
        let (r, g, b, a) = ob::DIM_COLOR;
        let (ml, mt, mr, mb) = ob::CARD_MARGINS;
        vmap(vec![
            // QML reads `#AARRGGBB`.
            ("dim", qs(&format!("#{a:02x}{r:02x}{g:02x}{b:02x}"))),
            ("halo", qs(&pal.halo_hex)),
            ("haloHover", qs(&pal.halo_hover)),
            ("haloWidth", qi(ob::HALO_WIDTH)),
            ("cornerRadius", qi(ob::CORNER_RADIUS)),
            ("cardBackground", qs(&pal.card_background)),
            ("cardText", qs(&pal.card_text)),
            ("cardMuted", qs(&pal.card_muted)),
            ("onAccent", qs(pal.on_accent)),
            ("cardMaxWidth", qi(ob::CARD_MAX_WIDTH)),
            ("cardGap", qi(ob::CARD_GAP)),
            ("cardRadius", qi(ob::CARD_RADIUS)),
            ("marginLeft", qi(ml)),
            ("marginTop", qi(mt)),
            ("marginRight", qi(mr)),
            ("marginBottom", qi(mb)),
            ("spacing", qi(ob::CARD_SPACING)),
            ("buttonSpacing", qi(ob::CARD_BUTTON_SPACING)),
            ("bodyWeight", qi(ob::BODY_FONT_WEIGHT)),
            ("nextWeight", qi(ob::NEXT_FONT_WEIGHT)),
            ("nextRadius", qi(ob::NEXT_RADIUS)),
            ("nextPadV", qi(ob::NEXT_PADDING.0)),
            ("nextPadH", qi(ob::NEXT_PADDING.1)),
            ("skipPadV", qi(ob::SKIP_PADDING.0)),
            ("skipPadH", qi(ob::SKIP_PADDING.1)),
        ])
    }

    fn start(&self) -> VMap {
        match settings() {
            Some(m) => start_map(&ob::session_start(&m)),
            None => start_map(&ob::Start::Nothing),
        }
    }

    fn answer_ask(&self, show: bool) -> VMap {
        let Some(mut m) = settings() else {
            return start_map(&ob::Start::Nothing);
        };
        let (start, saved) = ob::session_answer(&mut m, show);
        report(saved);
        start_map(&start)
    }

    fn replay(&self) -> VMap {
        let Some(mut m) = settings() else {
            return start_map(&ob::Start::Nothing);
        };
        let (start, saved) = ob::session_replay(&mut m);
        report(saved);
        start_map(&start)
    }

    fn advance(&self, step_id: &QString) -> QString {
        let Some(mut m) = settings() else {
            return QString::default();
        };
        let (next, saved) = ob::session_advance(&mut m, &step_id.to_string());
        report(saved);
        QString::from(next.map(|s| s.id).unwrap_or(""))
    }

    fn skip_all(&self) {
        if let Some(mut m) = settings() {
            report(ob::session_skip(&mut m));
        }
    }

    fn texts(&self) -> VMap {
        vmap(vec![
            ("askTitle", qs(&ob::ask_title())),
            ("askText", qs(&ob::ask_text())),
            ("askYes", qs(&ob::ask_yes())),
            ("askNo", qs(&ob::ask_no())),
        ])
    }

    fn card(&self, step_id: &QString) -> VMap {
        let Some(c) = ob::card_content(&step_id.to_string()) else {
            return VMap::default();
        };
        vmap(vec![
            ("progress", qs(&c.progress)),
            ("title", qs(&c.title)),
            ("text", qs(&c.text)),
            ("nextText", qs(&c.next_text)),
            ("skipText", qs(&c.skip_text)),
            ("skipVisible", QVariant::from(&c.skip_visible)),
        ])
    }

    fn aim(&self, step_id: &QString) -> VMap {
        let (kind, tab) = match ob::aim_target(&step_id.to_string()) {
            ob::AimTarget::WholeWindow => ("window", ""),
            ob::AimTarget::TabHeader(id) => ("tab", id),
            ob::AimTarget::TabContent(id) => ("tab_content", id),
        };
        vmap(vec![("kind", qs(kind)), ("tab", qs(tab))])
    }

    fn hole(&self, x: i32, y: i32, width: i32, height: i32) -> VMap {
        match ob::spotlight_hole(Some((x, y, width, height))) {
            Some((hx, hy, hw, hh)) => vmap(vec![
                ("valid", QVariant::from(&true)),
                ("x", qi(hx)),
                ("y", qi(hy)),
                ("width", qi(hw)),
                ("height", qi(hh)),
            ]),
            None => vmap(vec![
                ("valid", QVariant::from(&false)),
                ("x", qi(0)),
                ("y", qi(0)),
                ("width", qi(0)),
                ("height", qi(0)),
            ]),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn placement(
        &self,
        overlay_width: i32,
        overlay_height: i32,
        card_width: i32,
        card_height: i32,
        has_hole: bool,
        hole_x: i32,
        hole_y: i32,
        hole_width: i32,
        hole_height: i32,
    ) -> VMap {
        let hole = has_hole.then_some((hole_x, hole_y, hole_width, hole_height));
        let p = ob::card_placement(
            (overlay_width, overlay_height),
            (card_width, card_height),
            hole,
        );
        vmap(vec![("x", qi(p.x)), ("y", qi(p.y))])
    }

    fn point_sizes(&self, base: i32) -> VMap {
        let (progress, title) = ob::card_point_sizes(base);
        vmap(vec![("progress", qi(progress)), ("title", qi(title))])
    }
}
