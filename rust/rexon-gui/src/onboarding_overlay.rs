// OnboardingModel — обучение при первом запуске для окна на Rust.
//
// У Python-окна обучение рисует `rexon/onboarding.py` (виджетная накладка
// поверх `QMainWindow`). У окна на QML виджетов нет, поэтому накладка —
// `qml/OnboardingOverlay.qml`, а этот объект только переводит ей ядро:
// сценарий, тексты, решение «что показывать», сохранение прогресса,
// геометрию выреза и карточки, палитру скина (`rexon_core::onboarding`).
// Ни одного собственного решения и ни одной подписи здесь нет.
//
// Состояние пишется в ОБЩИЙ `settings.json` теми же ключами, что у
// Python-окна: прошедший обучение в одном входе не увидит его во втором.
//
// ⭐ ДВЕРЬ ДЛЯ «ПРОЙТИ ОБУЧЕНИЕ ЗАНОВО». Кнопка живёт на вкладке «Настройки»
// (`SettingsActions`), а накладка — на окне; прямой ссылки у них нет.
// [`request_replay`] толкает сигнал `replayRequested` всем живым объектам
// обучения через очередь потока Qt, и накладка по нему начинает заново.

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
        // Палитра действующего скина и числа накладки из ядра. Не CONSTANT:
        // скин меняется на лету.
        #[qproperty(QMap_QString_QVariant, style, READ = style, NOTIFY = style_changed)]
        type OnboardingModel = super::OnboardingModelRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "styleChanged"]
        fn style_changed(self: Pin<&mut OnboardingModel>);

        /// Кнопка «Пройти обучение заново» нажата где-то в окне.
        #[qsignal]
        #[cxx_name = "replayRequested"]
        fn replay_requested(self: Pin<&mut OnboardingModel>);
    }

    extern "RustQt" {
        fn style(self: &OnboardingModel) -> QMap_QString_QVariant;

        /// Решение при запуске: `{kind: "ask"|"show"|"nothing", step}`.
        #[qinvokable]
        fn start(self: &OnboardingModel) -> QMap_QString_QVariant;

        /// Ответ на вопрос первого запуска; отдаёт решение, что дальше.
        #[qinvokable]
        #[cxx_name = "answerAsk"]
        fn answer_ask(self: &OnboardingModel, show: bool) -> QMap_QString_QVariant;

        /// Сбросить прогресс и включить обучение; отдаёт решение.
        #[qinvokable]
        fn replay(self: &OnboardingModel) -> QMap_QString_QVariant;

        /// «Далее» на шаге `step_id`: идентификатор следующего шага либо
        /// пустая строка, если сценарий пройден.
        #[qinvokable]
        fn advance(self: &OnboardingModel, step_id: &QString) -> QString;

        /// «Пропустить»: весь сценарий помечается пройденным.
        #[qinvokable]
        #[cxx_name = "skipAll"]
        fn skip_all(self: &OnboardingModel);

        /// Тексты вопроса первого запуска.
        #[qinvokable]
        fn texts(self: &OnboardingModel) -> QMap_QString_QVariant;

        /// Содержимое карточки шага; пустая карта — шага нет.
        #[qinvokable]
        fn card(self: &OnboardingModel, step_id: &QString) -> QMap_QString_QVariant;

        /// Куда наводить подсветку: `{kind: "window"|"tab"|"tab_content", tab}`.
        #[qinvokable]
        fn aim(self: &OnboardingModel, step_id: &QString) -> QMap_QString_QVariant;

        /// Вырез вокруг цели: `{valid, x, y, width, height}`.
        #[qinvokable]
        fn hole(self: &OnboardingModel, x: i32, y: i32, width: i32, height: i32)
            -> QMap_QString_QVariant;

        /// Место карточки в накладке: `{x, y}`.
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

        /// Кегли карточки от кегля приложения: `{progress, title}`.
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

/// Решение ядра в виде, удобном накладке.
fn start_map(start: &ob::Start) -> VMap {
    let (kind, step) = match start {
        ob::Start::Ask => ("ask", ""),
        ob::Start::Show(id) => ("show", *id),
        ob::Start::Nothing => ("nothing", ""),
    };
    vmap(vec![("kind", qs(kind)), ("step", qs(step))])
}

/// Несохранившийся прогресс не обрывает обучение, но молчать о нём нельзя:
/// иначе человек увидит его снова и никто не узнает почему.
fn report(saved: std::io::Result<()>) {
    if let Err(e) = saved {
        rexon_core::log_warn!("onboarding", "прогресс не сохранён в настройки: {e}");
    }
}

/// Живой менеджер настроек; `None` — каталог настроек недоступен.
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
// Реестр живых объектов для «пройти заново»
// ============================================================================

type Notify = Box<dyn Fn() -> Result<(), ()> + Send>;

fn registry() -> &'static Mutex<(u64, Vec<(u64, Notify)>)> {
    static REGISTRY: OnceLock<Mutex<(u64, Vec<(u64, Notify)>)>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new((0, Vec::new())))
}

/// Попросить окно пройти обучение заново (кнопка в «Настройках»).
///
/// `true` — просьбу принял хотя бы один живой объект обучения, то есть
/// окно на Rust открыто и начнёт обучение само. `false` — окна с накладкой
/// в процессе нет (вкладка открыта у Python-хозяина либо одиночным окном), и
/// сказать человеку об этом обязан вызывающий.
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
// Объект
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
            // QML читает `#AARRGGBB`.
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
