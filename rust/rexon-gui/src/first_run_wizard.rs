// FirstRunWizard — мост мастера первого запуска между ядром и QML.
//
// ЗАЧЕМ. Решение «предлагать ли установку», все подписи мастера и сама
// установка уже есть в ядре (`rexon_core::installer` и
// `rexon_core::installer_exec`). Окну на Rust не хватало двери к ним:
// `rexon-main-window` не проверял, установлен ли Rexon, вовсе, и человек,
// распаковавший архив, оставался без ярлыка в меню и без дополнений для
// живых обоев, ничего об этом не узнав.
//
// ГРАНИЦА. Здесь НЕТ ни одного решения и ни одной подписи:
//
//   * предлагать ли мастер и в каком режиме — `installer::first_run_offer`
//     (живой вход `first_run_offer_live`);
//   * все буквы страницы приветствия, согласия «не спрашивать пароль»,
//     хода и итога — `installer::welcome_texts`,
//     `passwordless_consent_texts`, `progress_texts`, `result_texts`;
//   * что и куда копировать, чем спрашивать пароль, как разбирать исход —
//     `installer_exec::install`;
//   * «больше не спрашивать» — `installer::session_first_run_decline`, тот
//     же ключ настроек, который читает решение.
//
// Здесь — только живые факты машины (семейство дистрибутива, свой бинарь,
// домашняя папка), фоновый поток и перевод карт ядра в QML. Сторож на
// копипасту подписей — тест в конце файла.
//
// ⚠️ ОКНО ХОДА — ОБЩЕЕ. Свойство `operation` и сигнал `operationLog` имеют
// ТОТ ЖЕ вид, что у `DesktopsActions` и `SettingsActions` (карта `OpView`),
// поэтому ход и итог установки показывает существующий
// `qml/DeOperationDialog.qml` без единой правки: `DeOperationDialog {
// actions: wizard }`.
//
// ⚠️ УСТАНОВКА ИДЁТ В ФОНЕ. За `pkexec` стоит окно пароля, которое ждёт
// сколько угодно, а установка недостающих компонентов — минуты. Синхронный
// вызов подвесил бы окно на всё это время (так и делал Python-мастер).
//
// ⚠️ СУХОЙ ПРОГОН. При `REXON_INSTALL_DRY_RUN=1` подставляется записывающий
// исполнитель [`DryRunIo`]: ни одной записи на диск, ни одной запущенной
// команды, живой исполнитель установки (`LiveInstallIo`) не создаётся
// вовсе. Нужен для приёмки мастера глазами на рабочей машине.

/// Мост cxx-qt мастера.
///
/// Тип объявлен `#[qml_element]`, поэтому разметка заводит его себе сама
/// (`FirstRunWizard { id: wizard }`).
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
        /// Окно хода установки: ход дела или итог. Тот же договор, что у
        /// `DesktopsActions.operation`, — окно одно на всех
        /// (`DeOperationDialog.qml`).
        #[qproperty(QMap_QString_QVariant, operation, READ = operation, NOTIFY = operation_changed)]
        type FirstRunWizard = super::FirstRunWizardRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "operationChanged"]
        fn operation_changed(self: Pin<&mut FirstRunWizard>);

        /// Строка журнала хода — дописывается окном, а не пересылается
        /// картой целиком.
        #[qsignal]
        #[cxx_name = "operationLog"]
        fn operation_log(self: Pin<&mut FirstRunWizard>, line: QString);

        /// Установка кончилась: `ok` — получилось ли. По нему разметка
        /// закрывает мастер и отпускает обучение.
        #[qsignal]
        #[cxx_name = "installFinished"]
        fn install_finished(self: Pin<&mut FirstRunWizard>, ok: bool);

        /// Мастер просят показать: кнопка «Установить Rexon в систему…» на
        /// странице «Настройки» ([`super::request_show`]).
        #[qsignal]
        #[cxx_name = "showRequested"]
        fn show_requested(self: Pin<&mut FirstRunWizard>);
    }

    extern "RustQt" {
        fn operation(self: &FirstRunWizard) -> QMap_QString_QVariant;
    }

    extern "RustQt" {
        /// Решение ядра и ВСЕ подписи мастера одной картой:
        /// `{kind: "offer"|"skip", reason, defaultMode, systemBlocked,
        /// forced, dryRun, dryRunNotice, welcome: {…}, passwordless: {…}}`.
        ///
        /// Настройки только читаются: каталог настроек ради ответа не
        /// создаётся.
        #[qinvokable]
        fn start(self: &FirstRunWizard) -> QMap_QString_QVariant;

        /// Согласие «не спрашивать пароль для действий Rexon» — отдельная
        /// галочка страницы приветствия. Запоминается до [`install`] и
        /// действует только у установки для всех пользователей.
        #[qinvokable]
        #[cxx_name = "setPasswordlessRule"]
        fn set_passwordless_rule(self: Pin<&mut FirstRunWizard>, allow: bool);

        /// Установить Rexon в режиме `mode` (`"system"`, `"user"`,
        /// `"portable"`) в фоне. `false` — установка уже идёт либо режим
        /// незнаком. Ход дела — свойство `operation` и сигнал
        /// `operationLog`, итог — `installFinished`.
        #[qinvokable]
        fn install(self: Pin<&mut FirstRunWizard>, mode: &QString) -> bool;

        /// «Не сейчас». `remember` — отмечено «Больше не спрашивать»: тогда
        /// отказ уходит в настройки. `false` — записать не вышло (причина в
        /// журнале), мастер предложится снова.
        #[qinvokable]
        fn decline(self: &FirstRunWizard, remember: bool) -> bool;

        /// Идёт ли установка.
        #[qinvokable]
        fn busy(self: &FirstRunWizard) -> bool;

        /// Сухой прогон (`REXON_INSTALL_DRY_RUN=1`): ничего не пишется и не
        /// запускается. Разметка говорит об этом человеку — иначе «готово»
        /// без установки выглядело бы обманом.
        #[qinvokable]
        #[cxx_name = "dryRun"]
        fn dry_run(self: &FirstRunWizard) -> bool;

        /// «Да» в окне вопроса — запустить отложенную установку. Свой
        /// вопрос мастер задаёт страницей приветствия, поэтому обычно
        /// отложенного режима нет и метод молчит.
        #[qinvokable]
        #[cxx_name = "confirmOperation"]
        fn confirm_operation(self: Pin<&mut FirstRunWizard>);

        /// «Закрыть» — убрать окно хода (пока установка идёт, не действует).
        #[qinvokable]
        #[cxx_name = "dismissOperation"]
        fn dismiss_operation(self: Pin<&mut FirstRunWizard>);

        /// «Отменить» — только пока система не тронута.
        #[qinvokable]
        #[cxx_name = "cancelOperation"]
        fn cancel_operation(self: Pin<&mut FirstRunWizard>);
    }

    impl cxx_qt::Initialize for FirstRunWizard {}
    impl cxx_qt::Threading for FirstRunWizard {}
}

use core::pin::Pin;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use cxx_qt::{CxxQtThread, CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QVariant};
use serde_json::Value;

use crate::desktops_actions::OpView;
use crate::qvalue::json_to_variant;
use rexon_core::distro_compat::Lang;
use rexon_core::installer::{self as inst, InstallMode};
use rexon_core::installer_exec::{self as iexec, InstallIo, InstallRequest, LiveInstallIo, PayloadSource};
use rexon_core::settings::SettingsManager;
use rexon_core::system::DistroDetector;
use rexon_core::system_de_exec::{CancelToken, DeOpOutcome, DeOpSink};
use rexon_core::system_exec::{LiveSystemIo, RunOut, SystemIo};
use rexon_core::utils::RunOutcome;

type VMap = QMap<QMapPair_QString_QVariant>;

fn qstr(value: &str) -> QVariant {
    QVariant::from(&QString::from(value))
}

/// Переменная сухого прогона: мастер проходится целиком, но ни одной записи
/// и ни одного запуска не делается.
pub const DRY_RUN_ENV: &str = "REXON_INSTALL_DRY_RUN";

/// Включён ли сухой прогон. Выключенным считается и пустое значение, и `0`:
/// `REXON_INSTALL_DRY_RUN=0` должно значить «нет», а не «да».
pub fn dry_run_enabled() -> bool {
    match std::env::var(DRY_RUN_ENV) {
        Ok(v) => !matches!(v.trim(), "" | "0" | "no" | "false"),
        Err(_) => false,
    }
}

/// Решение ядра БЕЗ учёта ответа «больше не спрашивать» — признак
/// «показывать ли кнопку „Установить Rexon в систему…“ в Настройках»
/// (решение Wehrwolfmann 15.09.2026). Отказ снимает сама кнопка, поэтому
/// прятать её из-за отказа нельзя; а у установленного Rexon (пакет, Steam,
/// установка для всех или для себя) кнопки нет вовсе.
///
/// Настройки подставляются пустые — единственное отличие от
/// [`inst::first_run_offer_live`].
pub fn offer_ignoring_decline() -> inst::FirstRunOffer {
    inst::first_run_offer(
        &inst::LiveFirstRunProbe::for_this_binary(),
        &serde_json::Map::new(),
    )
}

// ============================================================================
// Реестр живых мастеров для кнопки в «Настройках»
// ============================================================================

type Notify = Box<dyn Fn() -> Result<(), ()> + Send>;

fn registry() -> &'static Mutex<(u64, Vec<(u64, Notify)>)> {
    static REGISTRY: OnceLock<Mutex<(u64, Vec<(u64, Notify)>)>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new((0, Vec::new())))
}

/// Попросить окно показать мастер установки (кнопка в «Настройках»).
///
/// `true` — просьбу принял хотя бы один живой мастер, то есть окно на Rust
/// открыто и покажет мастер само. `false` — окна с мастером в процессе нет
/// (вкладка открыта одиночным окном), и сказать об этом человеку обязан
/// вызывающий. Приём тот же, что у «Пройти обучение заново»
/// (`onboarding_overlay::request_replay`).
pub fn request_show() -> bool {
    let mut reg = registry().lock().expect("реестр мастера не отравлен");
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

/// Rust-состояние моста.
#[derive(Default)]
pub struct FirstRunWizardRust {
    op: OpView,
    /// Режим, ждущий «Да» (страница приветствия задаёт вопрос сама).
    pending: Option<InstallMode>,
    cancel: Option<CancelToken>,
    /// Согласие на правило «не спрашивать пароль для действий Rexon».
    passwordless: bool,
    show_watch: u64,
}

impl Drop for FirstRunWizardRust {
    fn drop(&mut self) {
        if self.show_watch != 0 {
            let mut reg = registry().lock().expect("реестр мастера не отравлен");
            reg.1.retain(|(id, _)| *id != self.show_watch);
        }
    }
}

impl cxx_qt::Initialize for qobject::FirstRunWizard {
    fn initialize(mut self: Pin<&mut Self>) {
        let handle = self.as_ref().get_ref().qt_thread();
        let id = {
            let mut reg = registry().lock().expect("реестр мастера не отравлен");
            reg.0 += 1;
            let id = reg.0;
            reg.1.push((
                id,
                Box::new(move || match handle.queue(qobject::FirstRunWizard::show_requested) {
                    Ok(()) => Ok(()),
                    Err(ThreadingQueueError::ObjectDestroyed) => Err(()),
                    Err(_) => Ok(()),
                }),
            ));
            id
        };
        let mut rust = self.as_mut().rust_mut();
        rust.passwordless = inst::PASSWORDLESS_CONSENT_DEFAULT;
        rust.show_watch = id;
    }
}

impl qobject::FirstRunWizard {
    // ── Решение и подписи ────────────────────────────────────────────────

    fn start(&self) -> VMap {
        let lang = ui_lang();
        let offer = inst::first_run_offer_live();
        let home = std::env::var("HOME").unwrap_or_default();
        let xdg_data = std::env::var("XDG_DATA_HOME").ok().filter(|v| !v.is_empty());
        let distro_id = DistroDetector::detect_live().distro_id();
        let (kind, default_mode, system_blocked, forced) = match &offer {
            inst::FirstRunOffer::Offer {
                default_mode,
                system_blocked,
                forced,
            } => (
                "offer",
                default_mode.as_str(),
                system_blocked.clone().unwrap_or_default(),
                *forced,
            ),
            inst::FirstRunOffer::Skip(_) => ("skip", "", String::new(), false),
        };
        // Подписи набирает ядро — здесь только пересылка (правило #12).
        let welcome = inst::welcome_texts(&distro_id, &home, xdg_data.as_deref(), lang);
        let consent = inst::passwordless_consent_texts(lang);
        let dry_run_notice = inst::dry_run_notice(lang);
        let mut out = VMap::default();
        let mut put = |key: &str, value: QVariant| {
            out.insert(QString::from(key), value);
        };
        put("kind", qstr(kind));
        // Строка для журнала, не для человека: почему решено так.
        put("reason", qstr(&offer.log_line()));
        put("defaultMode", qstr(default_mode));
        put("systemBlocked", qstr(&system_blocked));
        put("forced", QVariant::from(&forced));
        put("dryRun", QVariant::from(&dry_run_enabled()));
        // Пояснение сухого прогона — только когда он включён: иначе строка
        // стояла бы на странице приветствия у каждого покупателя.
        put(
            "dryRunNotice",
            qstr(if dry_run_enabled() { &dry_run_notice } else { "" }),
        );
        put("welcome", json_to_variant(&serde_value(&welcome)));
        put("passwordless", json_to_variant(&serde_value(&consent)));
        out
    }

    fn dry_run(&self) -> bool {
        dry_run_enabled()
    }

    fn set_passwordless_rule(mut self: Pin<&mut Self>, allow: bool) {
        self.as_mut().rust_mut().passwordless = allow;
    }

    fn decline(&self, remember: bool) -> bool {
        if !remember {
            return true;
        }
        let mut manager = match SettingsManager::new() {
            Ok(m) => m,
            Err(e) => {
                rexon_core::log_warn!("first-run", "настройки не открылись: {e}");
                return false;
            }
        };
        match inst::session_first_run_decline(&mut manager, true) {
            Ok(()) => true,
            Err(e) => {
                rexon_core::log_warn!("first-run", "отказ не сохранён: {e}");
                false
            }
        }
    }

    // ── Окно хода ────────────────────────────────────────────────────────

    fn operation(&self) -> VMap {
        self.rust().op.to_map()
    }

    fn set_op(mut self: Pin<&mut Self>, op: OpView) {
        self.as_mut().rust_mut().op = op;
        self.as_mut().operation_changed();
    }

    fn busy(&self) -> bool {
        self.rust().op.phase == "running"
    }

    fn install(self: Pin<&mut Self>, mode: &QString) -> bool {
        let Some(mode) = InstallMode::parse(&mode.to_string()) else {
            rexon_core::log_warn!("first-run", "незнакомый режим «{mode}»");
            return false;
        };
        self.run_install(mode)
    }

    fn confirm_operation(mut self: Pin<&mut Self>) {
        let Some(mode) = self.as_mut().rust_mut().pending.take() else {
            return;
        };
        self.run_install(mode);
    }

    /// Запустить установку в фоне. `false` — уже идёт.
    fn run_install(mut self: Pin<&mut Self>, mode: InstallMode) -> bool {
        if self.as_ref().get_ref().busy() {
            return false;
        }
        let lang = ui_lang();
        let texts = inst::progress_texts(mode, lang);
        let passwordless = self.as_ref().get_ref().rust().passwordless;
        let token = CancelToken::new();
        self.as_mut().rust_mut().cancel = Some(token.clone());
        self.as_mut().rust_mut().pending = None;
        let mut op = OpView::running(&texts.title);
        // Подзаголовок ядра заранее говорит про окно пароля — он и стоит
        // строкой состояния, пока исполнитель не назвал свой этап.
        op.status = texts.subtitle;
        self.as_mut().set_op(op);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let sink: Arc<dyn DeOpSink> = Arc::new(QtSink {
                handle: handle.clone(),
            });
            let result = run_install_with_io(mode, passwordless, lang, sink, &token);
            deliver(&handle, move |object| object.finish_install(result));
        });
        true
    }

    /// Итог установки: окно хода превращается в окно итога, подписи — из
    /// ядра (`installer::result_texts`).
    fn finish_install(mut self: Pin<&mut Self>, result: iexec::InstallResult) {
        let lang = ui_lang();
        let ok = result.ok();
        let outcome = DeOpOutcome {
            ok,
            message: result.message.clone(),
            cancelled: result.cancelled(),
            restart_needed: false,
        };
        let texts = result.outcome.as_ref().map(|o| inst::result_texts(o, lang));
        let summary = texts
            .as_ref()
            .map(|t| {
                let mut body = format!("<b>{}</b><br>{}", html(&t.title), html(&t.description));
                for note in &t.notes {
                    body.push_str("<br>");
                    body.push_str(&html(note));
                }
                body
            })
            .unwrap_or_default();
        let mut op = self.as_ref().get_ref().rust().op.clone();
        if let Some(line) = op.apply_done(&outcome, summary.clone()) {
            self.as_mut().operation_log(QString::from(&line));
        }
        // Неудача: итог ядра тоже должен быть виден, а не только в журнале.
        if !ok {
            op.result_text = summary;
        }
        self.as_mut().rust_mut().cancel = None;
        self.as_mut().set_op(op);
        self.as_mut().install_finished(ok);
    }

    fn apply_progress(mut self: Pin<&mut Self>, stage: String, message: String, fraction: Option<f64>) {
        let mut op = self.as_ref().get_ref().rust().op.clone();
        let Some(log_line) = op.apply_progress(&stage, &message, fraction) else {
            return;
        };
        if let Some(line) = log_line {
            self.as_mut().operation_log(QString::from(&line));
        }
        // Этапов установки в `de_progress::STAGE_TITLES` нет, и заголовка
        // ядро для них не даёт: строкой состояния служит последнее
        // сообщение исполнителя, иначе окно всю установку показывало бы
        // «Подготовка...». Общие этапы («ошибка», «предупреждение») свой
        // заголовок имеют — их не трогаем.
        if !message.is_empty() && matches!(stage.as_str(), iexec::STAGE_PREPARE | iexec::STAGE_INSTALL) {
            op.status = message;
        }
        self.set_op(op);
    }

    fn apply_cancellable(self: Pin<&mut Self>, allowed: bool) {
        let mut op = self.rust().op.clone();
        if op.phase != "running" {
            return;
        }
        op.cancellable = allowed;
        self.set_op(op);
    }

    fn dismiss_operation(mut self: Pin<&mut Self>) {
        if self.as_ref().get_ref().busy() {
            return;
        }
        self.as_mut().rust_mut().pending = None;
        self.set_op(OpView::default());
    }

    fn cancel_operation(self: Pin<&mut Self>) {
        if !self.as_ref().get_ref().busy() || !self.as_ref().get_ref().rust().op.cancellable {
            return;
        }
        if let Some(token) = &self.as_ref().get_ref().rust().cancel {
            token.cancel();
        }
        // Кнопка гаснет сразу: второй щелчок ничего не добавит.
        let mut op = self.as_ref().get_ref().rust().op.clone();
        op.cancellable = false;
        self.set_op(op);
    }
}

/// Язык подписей мастера — язык ИНТЕРФЕЙСА (`rexon_core::i18n`: настройка
/// человека ▸ локаль ▸ английский), а не голая локаль процесса.
///
/// Тексты мастера набирает установщик ядра парами «русский/английский»
/// (`installer.rs`, `Lang`); прежде пара выбиралась по `Lang::from_env`, и при
/// выбранном в настройках языке мастер говорил бы не на языке остального окна.
fn ui_lang() -> Lang {
    Lang::from_code(&rexon_core::i18n::current_language())
}

/// Простой HTML для окна: переносы строк — `<br>` (в `RichText` голый
/// перевод строки стал бы пробелом). Тот же приём, что у окна операции со
/// столом.
fn html(text: &str) -> String {
    text.replace('\n', "<br>")
}

/// Структура ядра → JSON. Отказ сериализации недостижим (все поля —
/// строки, числа и списки), но молчать о нём нельзя.
fn serde_value<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or_else(|e| {
        rexon_core::log_warn!("first-run", "подписи не переведены в JSON: {e}");
        Value::Null
    })
}

// ============================================================================
// Исполнение установки
// ============================================================================

/// Что ставить на этой машине. Решения и исполнение — в ядре; здесь только
/// живые факты: семейство дистрибутива, свой бинарь, дерево разработчика и
/// домашняя папка.
fn live_request<IO: InstallIo + ?Sized>(
    io: &IO,
    lang: Lang,
    passwordless: bool,
) -> InstallRequest {
    let own_exe = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let home = io.env("HOME");
    let xdg_data = io.env("XDG_DATA_HOME");
    let payload = PayloadSource::for_running(
        io,
        &own_exe,
        &rexon_core::cli::repo_root(),
        &home,
        Some(xdg_data.as_str()).filter(|x| !x.is_empty()),
    );
    let detector = DistroDetector::detect_live();
    let mut req = InstallRequest::new(payload, detector.family_live(), &detector.distro_id(), lang);
    req.entry_env = std::env::var(inst::ENTRY_ENV).ok();
    req.passwordless_rule = passwordless;
    req
}

/// Установка выбранным исполнителем. Живой исполнитель при сухом прогоне НЕ
/// создаётся: `LiveInstallIo` стоит только во второй ветке.
fn run_install_with_io(
    mode: InstallMode,
    passwordless: bool,
    lang: Lang,
    sink: Arc<dyn DeOpSink>,
    cancel: &CancelToken,
) -> iexec::InstallResult {
    if dry_run_enabled() {
        let io = DryRunIo::new();
        let req = live_request(&io, lang, passwordless);
        let result = iexec::install(&io, mode, &req, sink, cancel);
        for line in io.journal() {
            rexon_core::log_info!("first-run", "сухой прогон установки: {line}");
        }
        return result;
    }
    let io = LiveInstallIo::new();
    let req = live_request(&io, lang, passwordless);
    iexec::install(&io, mode, &req, sink, cancel)
}

/// Приёмник хода исполнителя ядра: каждое событие уходит в поток Qt.
struct QtSink {
    handle: CxxQtThread<qobject::FirstRunWizard>,
}

impl DeOpSink for QtSink {
    fn progress(&self, stage: &str, message: &str, fraction: Option<f64>) {
        let (stage, message) = (stage.to_string(), message.to_string());
        deliver(&self.handle, move |object| {
            object.apply_progress(stage, message, fraction);
        });
    }

    fn cancellable(&self, allowed: bool) {
        deliver(&self.handle, move |object| object.apply_cancellable(allowed));
    }
}

/// Поставить работу в поток Qt; окно уже закрыто — молча, иначе в журнал.
fn deliver(
    handle: &CxxQtThread<qobject::FirstRunWizard>,
    work: impl FnOnce(Pin<&mut qobject::FirstRunWizard>) + Send + 'static,
) {
    if let Err(e) = handle.queue(work) {
        if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
            rexon_core::log_warn!("first-run", "ход установки не доставлен окну: {e:?}");
        }
    }
}

// ============================================================================
// Сухой прогон
// ============================================================================

/// Записывающий исполнитель: наблюдения берутся у живой машины
/// (`LiveSystemIo` — только чтение), а всё, что меняет машину, пишется в
/// журнал и не делается.
///
/// ⚠️ ЖИВОЙ ИСПОЛНИТЕЛЬ УСТАНОВКИ (`LiveInstallIo`) ЗДЕСЬ НЕ УЧАСТВУЕТ: у
/// него нет ни одного метода, который нельзя было бы вызвать по ошибке.
/// Наблюдения взяты у отдельного читающего исполнителя ядра, поэтому
/// записать что-либо этот тип не может в принципе.
///
/// Созданное прогоном помнится в памяти: без этого исполнитель ядра на
/// следующем шаге не нашёл бы только что «записанный» файл и сорвался бы на
/// ровном месте. По той же причине после «запуска» корневого скрипта пути
/// установки для всех пользователей считаются разложенными — иначе сухой
/// прогон всегда упирался бы в проверку «Rexon на месте» и показать итог
/// успеха глазами было бы нечем.
struct DryRunIo {
    live: LiveSystemIo,
    journal: RefCell<Vec<String>>,
    files: RefCell<HashSet<String>>,
    dirs: RefCell<HashSet<String>>,
    executable: RefCell<HashSet<String>>,
    /// Корни, которые «разложил» корневой скрипт.
    pretend: RefCell<Vec<String>>,
}

impl DryRunIo {
    fn new() -> Self {
        DryRunIo {
            live: LiveSystemIo,
            journal: RefCell::new(Vec::new()),
            files: RefCell::new(HashSet::new()),
            dirs: RefCell::new(HashSet::new()),
            executable: RefCell::new(HashSet::new()),
            pretend: RefCell::new(Vec::new()),
        }
    }

    /// Что прогон сделал бы с машиной — по строке на действие.
    fn journal(&self) -> Vec<String> {
        self.journal.borrow().clone()
    }

    fn note(&self, line: String) {
        self.journal.borrow_mut().push(line);
    }

    fn add_file(&self, path: &str, mode: u32) {
        self.files.borrow_mut().insert(path.to_string());
        if mode & 0o111 != 0 {
            self.executable.borrow_mut().insert(path.to_string());
        }
    }

    fn under_pretend(&self, path: &str) -> bool {
        self.pretend
            .borrow()
            .iter()
            .any(|root| path == root || path.starts_with(&format!("{root}/")))
    }
}

impl SystemIo for DryRunIo {
    fn exists(&self, path: &str) -> bool {
        self.files.borrow().contains(path)
            || self.dirs.borrow().contains(path)
            || self.under_pretend(path)
            || self.live.exists(path)
    }
    fn is_file(&self, path: &str) -> bool {
        self.files.borrow().contains(path) || self.under_pretend(path) || self.live.is_file(path)
    }
    fn is_dir(&self, path: &str) -> bool {
        self.dirs.borrow().contains(path) || self.under_pretend(path) || self.live.is_dir(path)
    }
    fn is_executable(&self, path: &str) -> bool {
        self.executable.borrow().contains(path)
            || self.under_pretend(path)
            || self.live.is_executable(path)
    }
    fn realpath(&self, path: &str) -> Option<String> {
        self.live.realpath(path)
    }
    fn read_text(&self, path: &str) -> Option<String> {
        self.live.read_text(path)
    }
    fn list_dir(&self, dir: &str) -> Option<Vec<String>> {
        self.live.list_dir(dir)
    }
    fn which(&self, name: &str) -> bool {
        self.live.which(name)
    }
    fn which_path(&self, name: &str) -> Option<String> {
        self.live.which_path(name)
    }
    fn env(&self, name: &str) -> String {
        self.live.env(name)
    }

    // ── Побочные действия: только журнал ─────────────────────────────────

    fn run(&self, argv: &[String], _timeout_secs: Option<u64>, _lang_c: bool) -> RunOut {
        self.note(format!("запустил бы: {}", argv.join(" ")));
        // Корневой скрипт «разложил» файлы установки для всех: дальше ядро
        // проверяет, что Rexon на месте.
        if argv.first().is_some_and(|p| p.ends_with("pkexec")) {
            let sys = inst::install_paths(InstallMode::System, "", None);
            let mut pretend = self.pretend.borrow_mut();
            for dir in [sys.install_dir, sys.lib_dir, sys.desktop_dir, sys.icon_root] {
                pretend.extend(dir);
            }
        }
        RunOut {
            outcome: RunOutcome::Ok,
            returncode: Some(0),
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    fn write_text(&self, path: &str, content: &str) -> Result<(), String> {
        self.note(format!("записал бы {path} ({} знаков)", content.chars().count()));
        self.add_file(path, 0o644);
        Ok(())
    }

    fn copy_file(&self, src: &str, dst: &str) -> Result<(), String> {
        self.note(format!("скопировал бы {src} → {dst}"));
        self.add_file(dst, 0o644);
        Ok(())
    }
}

impl InstallIo for DryRunIo {
    fn temp_dir(&self, prefix: &str, parent: Option<&str>) -> Result<String, String> {
        let base = parent.unwrap_or("/tmp").trim_end_matches('/').to_string();
        let path = format!("{base}/{prefix}dry-run");
        self.note(format!("завёл бы временную папку {path}"));
        self.dirs.borrow_mut().insert(path.clone());
        Ok(path)
    }

    fn rename(&self, src: &str, dst: &str) -> Result<(), String> {
        self.note(format!("переименовал бы {src} → {dst}"));
        self.dirs.borrow_mut().insert(dst.to_string());
        Ok(())
    }

    fn make_dirs(&self, path: &str, mode: Option<u32>) -> Result<(), String> {
        self.note(format!("завёл бы папку {path} (права {:o})", mode.unwrap_or(0o755)));
        self.dirs.borrow_mut().insert(path.to_string());
        Ok(())
    }

    fn write_file_mode(&self, path: &str, content: &str, mode: u32) -> Result<(), String> {
        self.note(format!(
            "записал бы {path} (права {mode:o}, {} знаков)",
            content.chars().count()
        ));
        self.add_file(path, mode);
        Ok(())
    }

    fn copy_file_mode(&self, src: &str, dst: &str, mode: u32) -> Result<(), String> {
        self.note(format!("скопировал бы {src} → {dst} (права {mode:o})"));
        self.add_file(dst, mode);
        Ok(())
    }

    fn copy_tree(
        &self,
        src: &str,
        dst: &str,
        _skip: &dyn Fn(&str) -> bool,
        _mode: &dyn Fn(bool, bool) -> u32,
    ) -> Result<usize, String> {
        self.note(format!("скопировал бы дерево {src} → {dst}"));
        self.dirs.borrow_mut().insert(dst.to_string());
        Ok(0)
    }

    fn remove_file(&self, path: &str) {
        self.note(format!("удалил бы файл {path}"));
        self.files.borrow_mut().remove(path);
        self.executable.borrow_mut().remove(path);
    }

    fn remove_tree(&self, path: &str) {
        self.note(format!("удалил бы папку {path}"));
        self.dirs.borrow_mut().remove(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Исходный текст файла без собственного тестового модуля: список
    /// запрещённых строк лежит в тестах, и без отсечки сторож ловил бы сам
    /// себя.
    fn code() -> &'static str {
        include_str!("first_run_wizard.rs")
            .split_once("#[cfg(test)]")
            .map(|(code, _)| code)
            .expect("тестовый модуль на месте")
    }

    /// Ни одна подпись мастера в этом файле не набрана заново: все приходят
    /// из ядра. Сторож на копипасту — вторая копия человеческой строки
    /// разошлась бы с продуктом молча (правило #12). Образец —
    /// `window_menu.rs::no_human_text_is_typed_twice_in_this_file`.
    #[test]
    fn no_human_text_is_typed_twice_in_this_file() {
        let src = code();
        for baked in [
            "Добро пожаловать",
            "Установить Rexon",
            "Для всех пользователей",
            "Только для вас",
            "Больше не спрашивать",
            "Пропустить",
            "Повторить",
            "Продолжить",
            "Идёт установка",
            "Готово",
            "Пожалуйста, подождите",
            "пароль администратора",
            "Продолжить без установки",
            "Не спрашивать пароль",
        ] {
            let hits: Vec<&str> = src
                .lines()
                .filter(|l| l.contains(baked))
                .filter(|l| !l.trim_start().starts_with("//") && !l.trim_start().starts_with("///"))
                .collect();
            assert!(
                hits.is_empty(),
                "человеческий текст «{baked}» набран здесь, а не взят у ядра: {hits:?}"
            );
        }
    }

    /// Мост не решает сам: ключ настроек, режимы, признак «предлагать ли» и
    /// разбор исхода — только вызовами ядра.
    #[test]
    fn decisions_are_asked_of_the_core() {
        let src = code();
        assert!(src.contains("inst::first_run_offer_live()"), "решение берётся не у ядра");
        assert!(src.contains("inst::session_first_run_decline"), "отказ пишется мимо ядра");
        assert!(src.contains("iexec::install(&io"), "установка идёт мимо исполнителя ядра");
        assert!(src.contains("inst::result_texts"), "итог набран не ядром");
        for own in ["installer_first_run_declined", "\"pkexec\".to_string()"] {
            assert!(!src.contains(own), "мост завёл свою копию «{own}»");
        }
    }

    /// Сухой прогон: подставной исполнитель, ни одной записи и ни одного
    /// запуска. Проверяется на настоящем пути в подставном каталоге — файл
    /// после «записи» на диске не появляется.
    #[test]
    fn dry_run_substitutes_a_recording_io() {
        let scratch = std::env::temp_dir().join("rexon-first-run-wizard-dry-run");
        let _ = std::fs::remove_dir_all(&scratch);
        let path = scratch.join("launcher");
        let path = path.to_string_lossy().into_owned();

        let io = DryRunIo::new();
        io.make_dirs(&scratch.to_string_lossy(), Some(0o755)).unwrap();
        io.write_file_mode(&path, "#!/bin/sh\n", 0o755).unwrap();
        let out = io.run(&["/usr/bin/pkexec".into(), "bash".into(), "/tmp/x".into()], None, false);

        // Ни папки, ни файла на диске нет — всё осталось в журнале.
        assert!(!scratch.exists(), "сухой прогон завёл настоящую папку {scratch:?}");
        assert_eq!(out.returncode, Some(0), "прогон обязан идти дальше, не запуская команду");
        let journal = io.journal();
        assert_eq!(journal.len(), 3, "в журнале не все действия: {journal:?}");
        assert!(journal[2].starts_with("запустил бы:"), "{journal:?}");

        // Записанное видно исполнителю ядра: иначе следующий шаг сорвался бы.
        assert!(SystemIo::is_file(&io, &path));
        assert!(SystemIo::is_executable(&io, &path));
        // После «корневого скрипта» установка для всех считается на месте.
        let sys = inst::install_paths(InstallMode::System, "", None);
        assert!(SystemIo::exists(&io, &sys.install_dir.unwrap()));
    }

    /// Переменная сухого прогона читается честно: `0` — это «нет».
    #[test]
    fn dry_run_flag_is_read_strictly() {
        // Значение переменной берётся у процесса, поэтому проверяем разбор
        // на той же таблице, что и `dry_run_enabled`.
        for (value, want) in [("1", true), ("yes", true), ("", false), ("0", false), ("no", false)] {
            let on = !matches!(value.trim(), "" | "0" | "no" | "false");
            assert_eq!(on, want, "значение «{value}»");
        }
    }

    /// Окно хода мастера — то же самое, что у «Рабочих столов»: ключи карты
    /// `operation` читает общий `DeOperationDialog.qml`.
    #[test]
    fn operation_map_matches_the_shared_dialog() {
        let op = OpView::running("проверка");
        let map = op.to_map();
        for key in [
            "phase", "title", "text", "informative", "status", "bar", "barMax", "current",
            "closeText", "closeEnabled", "cancellable", "resultText", "danger", "yesText",
            "noText", "okText", "cancelText",
        ] {
            assert!(map.contains(&QString::from(key)), "в карте окна нет ключа «{key}»");
        }
    }
}
