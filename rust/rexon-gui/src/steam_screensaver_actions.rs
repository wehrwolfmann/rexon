// SteamScreensaverActions — объект подвкладки «Заставка» вкладки «Steam»:
// заставка Rexon для экрана простоя Steam Big Picture.
//
// Решения (что собрать, куда положить, чью папку не трогать, все подписи и
// варианты) — ядро `rexon_core::steam_screensaver`. Здесь только
// исполнитель: подсчёт найденного, установка, обновление и удаление — в
// рабочем потоке (картинки уменьшаются, это секунды), разметка опрашивает
// `poll()` таймером и не стоит.
//
// ⛔ Выбор заставки в Steam (`screensaver_current_id`) Rexon не трогает ни
// здесь, ни в ядре: заставку выбирает сам человек в настройках Steam — и на
// SteamOS тоже.
//
// Числа в подписях инвокаблов не ходят (см. предупреждение в
// `steam_page.rs`): настройки приходят одной строкой JSON.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type SteamScreensaverActions = super::SteamScreensaverActionsRust;
    }

    extern "RustQt" {
        /// Варианты и пределы: JSON `{modes, logos, intervalMin,
        /// intervalMax, howTo, restartHint}`.
        #[qinvokable]
        fn choices(self: &SteamScreensaverActions) -> QString;

        /// Состояние по всем корням Steam: JSON `{steamFound, installed,
        /// foreign, text, options, updated, roots: [{label, installed,
        /// foreign, slides}]}`. Только чтение меток — мгновенно.
        #[qinvokable]
        fn status(self: &SteamScreensaverActions) -> QString;

        /// Посчитать, сколько картинок наберётся с настройками `request`
        /// (JSON настроек). Итог — в `poll()` под ключом `scan`.
        #[qinvokable]
        #[cxx_name = "startScan"]
        fn start_scan(self: &SteamScreensaverActions, request: &QString) -> QString;

        /// Установить или обновить заставку с настройками `request`.
        /// «» — пошло, иначе причина.
        #[qinvokable]
        #[cxx_name = "startInstall"]
        fn start_install(self: &SteamScreensaverActions, request: &QString) -> QString;

        /// Удалить заставку Rexon. «» — пошло, иначе причина.
        #[qinvokable]
        #[cxx_name = "startRemove"]
        fn start_remove(self: &SteamScreensaverActions) -> QString;

        /// Состояние задач: JSON `{scan: {...}, job: {...}}`.
        #[qinvokable]
        fn poll(self: &SteamScreensaverActions) -> QString;

        /// Открыть установленную заставку в браузере — посмотреть, как она
        /// выглядит. «» — открылось, иначе причина.
        #[qinvokable]
        fn preview(self: &SteamScreensaverActions) -> QString;
    }
}

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use rexon_core::i18n::tr;
use rexon_core::qt_steam as qs;
use rexon_core::steam_screensaver as ss;
use serde_json::{json, Value};

/// Подсчёт найденного.
#[derive(Default, Clone)]
struct ScanJob {
    busy: bool,
    serial: u64,
    /// (кадров с артом, скриншотов, текст).
    result: Option<(usize, usize, String)>,
}

/// Установка или удаление.
#[derive(Default, Clone)]
struct InstallJob {
    busy: bool,
    serial: u64,
    /// (получилось ли, сообщение человеку).
    result: Option<(bool, String)>,
}

#[derive(Default)]
pub struct SteamScreensaverActionsRust {
    scan: Arc<Mutex<ScanJob>>,
    job: Arc<Mutex<InstallJob>>,
}

fn qjson(v: &Value) -> QString {
    QString::from(&v.to_string())
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

fn roots() -> Vec<PathBuf> {
    ss::steam_roots(&home())
}

/// Настройки из разметки поверх тех, с которыми заставка уже собрана.
fn options_of(request: &QString, base: &ss::Options) -> ss::Options {
    let v: Value = serde_json::from_str(&request.to_string()).unwrap_or(Value::Null);
    ss::Options::from_ui(&v, base)
}

/// Настройки установленной заставки (первой найденной), иначе по умолчанию.
fn installed_options() -> ss::Options {
    roots()
        .iter()
        .map(|r| ss::status(r))
        .find(|s| s.installed)
        .map(|s| s.options)
        .unwrap_or_default()
}

fn status_json() -> Value {
    let statuses: Vec<ss::Status> = roots().iter().map(|r| ss::status(r)).collect();
    let installed = statuses.iter().find(|s| s.installed);
    json!({
        "steamFound": !statuses.is_empty(),
        "installed": installed.is_some(),
        "foreign": statuses.iter().any(|s| s.foreign),
        "text": ss::status_text(&statuses),
        "options": installed.map(|s| s.options.clone()).unwrap_or_default().to_ui(),
        "updated": installed.map_or(0, |s| s.updated),
        "roots": statuses.iter().map(|s| json!({
            "label": ss::root_label(std::path::Path::new(&s.root)),
            "installed": s.installed,
            "foreign": s.foreign,
            "slides": s.slides,
        })).collect::<Vec<_>>(),
    })
}

/// Тег языка страницы — язык интерфейса Rexon.
fn page_locale() -> String {
    ss::page_locale(&rexon_core::i18n::current_language())
}

impl qobject::SteamScreensaverActions {
    fn choices(&self) -> QString {
        qjson(&json!({
            "modes": ss::mode_choices(),
            "logos": ss::logo_choices(),
            "intervalMin": ss::INTERVAL_MIN,
            "intervalMax": ss::INTERVAL_MAX,
            "howTo": ss::how_to_select(),
            "restartHint": ss::restart_hint(),
        }))
    }

    fn status(&self) -> QString {
        qjson(&status_json())
    }

    fn start_scan(&self, request: &QString) -> QString {
        let opts = options_of(request, &installed_options());
        let slot = self.rust().scan.clone();
        let serial = {
            let mut job = slot.lock().unwrap();
            job.busy = true;
            job.serial += 1;
            job.result = None;
            job.serial
        };
        std::thread::spawn(move || {
            let lang = qs::steam_language();
            // Картинки берутся из первого корня, где есть аккаунт (обычно он
            // один); так же их соберёт и установка.
            let (mut art, mut shots) = (0, 0);
            for root in roots() {
                let (g, s) = ss::collect(&root, lang);
                let (a, b) = ss::planned_counts(&g, &s, &opts);
                if a + b > art + shots {
                    (art, shots) = (a, b);
                }
            }
            let text = ss::found_text(art, shots);
            let mut job = slot.lock().unwrap();
            // Человек успел поменять настройки — старый подсчёт не нужен.
            if job.serial == serial {
                job.busy = false;
                job.result = Some((art, shots, text));
            }
        });
        QString::default()
    }

    fn start_install(&self, request: &QString) -> QString {
        let targets = roots();
        if targets.is_empty() {
            return QString::from(&ss::status_text(&[]));
        }
        let opts = options_of(request, &installed_options());
        self.run_job(move || {
            let lang = qs::steam_language();
            let locale = page_locale();
            let mut reports = Vec::new();
            let mut errors = Vec::new();
            for root in &targets {
                match ss::install(root, &opts, lang, &locale) {
                    Ok(r) => reports.push(r),
                    Err(e) => errors.push(e),
                }
            }
            (!reports.is_empty(), ss::installed_message(&reports, &errors))
        })
    }

    fn start_remove(&self) -> QString {
        let targets = roots();
        self.run_job(move || {
            let mut removed = 0;
            let mut errors = Vec::new();
            for root in &targets {
                match ss::remove(root) {
                    Ok(true) => removed += 1,
                    Ok(false) => {}
                    Err(e) => errors.push(e),
                }
            }
            (errors.is_empty(), ss::removed_message(removed, &errors))
        })
    }

    /// Запустить установку или удаление в рабочем потоке; второй раз, пока
    /// первая не кончилась, — отказ словами.
    fn run_job(&self, work: impl FnOnce() -> (bool, String) + Send + 'static) -> QString {
        let slot = self.rust().job.clone();
        {
            let mut job = slot.lock().unwrap();
            if job.busy {
                return QString::from(&tr("Already working — please wait."));
            }
            job.busy = true;
            job.serial += 1;
            job.result = None;
        }
        std::thread::spawn(move || {
            let res = work();
            let mut job = slot.lock().unwrap();
            job.busy = false;
            job.result = Some(res);
        });
        QString::default()
    }

    fn poll(&self) -> QString {
        let s = self.rust().scan.lock().unwrap().clone();
        let j = self.rust().job.lock().unwrap().clone();
        let scan = match &s.result {
            None => json!({"busy": s.busy, "serial": s.serial, "done": false}),
            Some((art, shots, text)) => json!({
                "busy": false, "serial": s.serial, "done": true,
                "art": art, "shots": shots, "text": text,
            }),
        };
        let job = match &j.result {
            None => json!({"busy": j.busy, "serial": j.serial, "done": false}),
            Some((ok, message)) => json!({
                "busy": false, "serial": j.serial, "done": true, "ok": ok, "message": message,
                // Свежее состояние сразу — разметке не нужен второй вызов.
                "status": status_json(),
            }),
        };
        qjson(&json!({"scan": scan, "job": job}))
    }

    fn preview(&self) -> QString {
        let Some(root) = roots().into_iter().find(|r| ss::status(r).installed) else {
            return QString::from(&tr("Install the screensaver first."));
        };
        let Some(dir) = ss::installed_dir(&root) else {
            return QString::from(&tr("Install the screensaver first."));
        };
        let page = dir.join("index.html");
        let url = crate::steam_showcase_actions::file_url(&page.to_string_lossy());
        if crate::appearance_actions::qobject::open_url_external(&QString::from(&url)) {
            QString::default()
        } else {
            QString::from(&tr("Could not open the browser."))
        }
    }
}
