// Скрытая страница «Разработчик» вкладки «Wallpaper Engine» на QML —
// порт `DeveloperPageWidget` из `rexon/qt_wallpaper_engine/window.py`.
//
// Зачем понадобился этот файл. Вкладка переехала на QML шеллом (рейл и
// место страницы, `we_page.rs`), а «Разработчик» тогда не портировали
// вовсе: пункт рейла заводился с ПУСТЫМ именем QML-файла, и владелец,
// открыв его, видел пустую страницу с одним словом `dev`. Через неё, в
// частности, ставится обновление waywallen — то есть кнопка была
// недостижима.
//
// Что здесь есть и чего нет:
//   * ЕСТЬ состояние страницы: галки (они же ключи настроек), подпись
//     версии, строки статуса целей, ход операции, панель changelog,
//     доступность кнопок, отчёт и бейдж сверки паритета;
//   * НЕТ ни одной строки для человека и ни одного числа раскладки. Все
//     подписи, краски, кегли, поля и вердикты считает ядро
//     (`rexon_core::we_window`), а разбор ответов движка и сверку схемы —
//     `rexon_core::we_query` и `rexon_core::we_parity`;
//   * НЕТ и сети с файловой системой: три фоновые задачи (проверка
//     upstream, установка AppImage, установка OWE-плагина) уже переехали
//     на Rust отдельным модулем `we_update_workers` и разговаривают
//     очередью событий, которую страница вычерпывает таймером — тот же
//     приём, что у вкладки «Steam».
//
// ⚠️ Починки оригинала (правило #16 — баги при порте чиним, не
// консервируем), каждая помечена `Починка:` по месту:
//   4. ПОДПИСЬ ВЕРСИИ МОРОЗИЛА ИНТЕРФЕЙС. `_refresh_version` звал
//      `current_version()` ПРЯМО В GUI-ПОТОКЕ на каждом заходе на
//      страницу и после каждой установки, а тот уходит в `--version`
//      бинаря и в `busctl` — до десяти секунд неподвижного окна. Теперь
//      версию считает фоновая задача (`start_version_refresh`), а подпись
//      приезжает событием. Номер 4 продолжает нумерацию починок этой
//      страницы: 1-3 записаны в `rexon_core::we_window`, ещё три — в
//      `we_update_workers`;
//   6. ПОДПИСЬ ВЕРСИИ ЗАВИСЕЛА ОТ ЖИВОГО ДЕМОНА, хотя ей до него дела нет.
//      Виджетная страница грузилась общим ленивым ходом окна
//      (`_ensure_page_loaded`), а тот молча выходит, пока сокет закрыт, —
//      и на неподнявшемся движке подпись навсегда оставалась «Версия
//      waywallen: —», хотя версия читается с ДИСКА (`--version` бинаря и
//      стейт). Здесь подпись обновляется всегда; на сверку паритета этот
//      ход НЕ распространён — она действительно спрашивает демона, и её
//      авто-запуск при закрытом сокете по-прежнему молчит
//      (`reloadIfConnected`);
//   5. ГЕЙТ ОТКАЗА ОСТАВЛЯЛ КНОПКУ ВЫКЛЮЧЕННОЙ. В оригинале гейт
//      совместимости считался ДО `setEnabled(False)`, поэтому отказ
//      оставлял кнопку живой. Гейт переехал в задачу (починка 1 в
//      `we_update_workers`), и кнопка гаснет раньше ответа — значит на
//      отказе её надо вернуть, иначе повторить попытку было бы нечем.
//
// ⛔ Установку движка живьём НЕ гонять: это ~200 МиБ и системные пути.

/// Мост cxx-qt: страница «Разработчик» и её запрос сверки паритета.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

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
        // ── Готовые подписи и числа страницы. CONSTANT: и то, и другое —
        // константы ядра, за время жизни вкладки они не меняются.
        #[qproperty(QMap_QString_QVariant, texts, READ = texts, CONSTANT)]
        #[qproperty(QMap_QString_QVariant, metrics, READ = metrics, CONSTANT)]
        // Палитра действующего скина (правило #17); скин меняют на лету.
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        // Прозрачность приглушённого текста — та же доля, какой красит
        // подписи виджетная версия.
        #[qproperty(f64, variant_alpha, cxx_name = "variantAlpha",
                    READ = variant_alpha, CONSTANT)]

        // ── Галки (они же ключи настроек продукта) ────────────────────
        #[qproperty(bool, auto_upstream, cxx_name = "autoUpstream",
                    READ = auto_upstream, WRITE = set_auto_upstream,
                    NOTIFY = flags_changed)]
        #[qproperty(bool, auto_download, cxx_name = "autoDownload",
                    READ = auto_download, WRITE = set_auto_download,
                    NOTIFY = flags_changed)]
        #[qproperty(bool, auto_parity, cxx_name = "autoParity",
                    READ = auto_parity, WRITE = set_auto_parity,
                    NOTIFY = flags_changed)]

        // ── Секция «Обновления upstream» ──────────────────────────────
        #[qproperty(QString, version_label, cxx_name = "versionLabel",
                    READ = version_label, NOTIFY = upstream_changed)]
        /// Строки статуса целей: записи `{key, text, color, bold}`.
        #[qproperty(QList_QVariant, targets, READ = targets,
                    NOTIFY = upstream_changed)]
        #[qproperty(QString, update_status, cxx_name = "updateStatus",
                    READ = update_status, NOTIFY = upstream_changed)]
        #[qproperty(QString, changelog, READ = changelog,
                    NOTIFY = upstream_changed)]
        #[qproperty(bool, check_enabled, cxx_name = "checkEnabled",
                    READ = check_enabled, NOTIFY = upstream_changed)]
        #[qproperty(bool, update_enabled, cxx_name = "updateEnabled",
                    READ = update_enabled, NOTIFY = upstream_changed)]
        #[qproperty(bool, owe_enabled, cxx_name = "oweEnabled",
                    READ = owe_enabled, NOTIFY = upstream_changed)]
        #[qproperty(bool, seen_enabled, cxx_name = "seenEnabled",
                    READ = seen_enabled, NOTIFY = upstream_changed)]
        #[qproperty(bool, copy_enabled, cxx_name = "copyEnabled",
                    READ = copy_enabled, NOTIFY = upstream_changed)]
        type WeDeveloperModel = super::WeDeveloperModelRust;

        // ── Запрос сверки паритета ────────────────────────────────────
        //
        // Отдельный QObject, а не метод страницы: сверка идёт по сокету
        // демона, а весь разговор с ним у вкладки ведут запросы с общей
        // базой (`impl_query_base!`). Второй способ спрашивать движок
        // здесь заводить незачем.
        #[qobject]
        #[qml_element]
        #[qproperty(i32, status, READ = status, NOTIFY = status_changed)]
        #[qproperty(QString, error, READ = error, NOTIFY = error_changed)]
        #[qproperty(bool, querying, READ = querying, NOTIFY = querying_changed)]
        #[qproperty(bool, forward_error, cxx_name = "forwardError",
                    READ = forward_error, WRITE = set_forward_error,
                    NOTIFY = forward_error_changed)]
        /// Версия ЖИВОГО демона; её называет страница (она знает
        /// синглтон шины), а не запрос.
        #[qproperty(QString, daemon_version, cxx_name = "daemonVersion",
                    READ = daemon_version, WRITE = set_daemon_version,
                    NOTIFY = daemon_version_changed)]
        #[qproperty(QString, report_text, cxx_name = "reportText",
                    READ = report_text, NOTIFY = report_changed)]
        #[qproperty(QString, badge_text, cxx_name = "badgeText",
                    READ = badge_text, NOTIFY = report_changed)]
        #[qproperty(QString, badge_color, cxx_name = "badgeColor",
                    READ = badge_color, NOTIFY = report_changed)]
        #[qproperty(bool, badge_bold, cxx_name = "badgeBold",
                    READ = badge_bold, NOTIFY = report_changed)]
        #[qproperty(bool, drift, READ = drift, NOTIFY = report_changed)]
        type WeParityQuery = super::WeParityQueryRust;
    }

    // ── Сигналы ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "paletteChanged"]
        fn palette_changed(self: Pin<&mut WeDeveloperModel>);

        /// Галки страницы (они же настройки продукта) изменились.
        #[qsignal]
        #[cxx_name = "flagsChanged"]
        fn flags_changed(self: Pin<&mut WeDeveloperModel>);

        /// Что-то в секции «Обновления upstream» изменилось. Сигнал ОДИН
        /// на всю секцию: её поля меняются пачками (пришёл ответ проверки
        /// — сменились разом статусы, changelog и четыре кнопки), и
        /// сигнал на каждое поле означал бы десяток толчков на одно
        /// событие.
        #[qsignal]
        #[cxx_name = "upstreamChanged"]
        fn upstream_changed(self: Pin<&mut WeDeveloperModel>);

        /// Архив плагина проверен и ждёт установки ЧЕРЕЗ ДЕМОНА: разметка
        /// отдаёт путь запросу `PluginInstallQuery` и заводит сторож
        /// ответа. Номер попытки — для сторожа (починка 2 ядра).
        #[qsignal]
        #[cxx_name = "oweIpcRequested"]
        fn owe_ipc_requested(self: Pin<&mut WeDeveloperModel>, zip_path: QString,
                             attempt: i32);

        /// Пометка расхождения паритета для кнопки «Разраб.» в рейле.
        #[qsignal]
        #[cxx_name = "parityDriftChanged"]
        fn parity_drift_changed(self: Pin<&mut WeDeveloperModel>, drift: bool);

        // ── База запроса сверки ───────────────────────────────────────
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut WeParityQuery>, status: i32);
        #[qsignal]
        #[cxx_name = "queryingChanged"]
        fn querying_changed(self: Pin<&mut WeParityQuery>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut WeParityQuery>);
        #[qsignal]
        #[cxx_name = "forwardErrorChanged"]
        fn forward_error_changed(self: Pin<&mut WeParityQuery>);
        #[qsignal]
        fn finished(self: Pin<&mut WeParityQuery>);
        #[qsignal]
        #[cxx_name = "errorOccurred"]
        fn error_occurred(self: Pin<&mut WeParityQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "appErrorOccurred"]
        fn app_error_occurred(self: Pin<&mut WeParityQuery>, error: QString);
        #[qsignal]
        #[cxx_name = "sendRequest"]
        fn send_request(self: Pin<&mut WeParityQuery>, token: i64, field: QString,
                        request: QByteArray);
        #[qsignal]
        #[cxx_name = "daemonVersionChanged"]
        fn daemon_version_changed(self: Pin<&mut WeParityQuery>);
        #[qsignal]
        #[cxx_name = "reportChanged"]
        fn report_changed(self: Pin<&mut WeParityQuery>);
        /// Сверка закончилась: `drift` — показывать ли пометку на «Разраб.».
        #[qsignal]
        fn checked(self: Pin<&mut WeParityQuery>, drift: bool);
    }

    // ── Чтение свойств ────────────────────────────────────────────────────
    extern "RustQt" {
        fn texts(self: &WeDeveloperModel) -> QMap_QString_QVariant;
        fn metrics(self: &WeDeveloperModel) -> QMap_QString_QVariant;
        fn palette(self: &WeDeveloperModel) -> QMap_QString_QVariant;
        #[cxx_name = "variantAlpha"]
        fn variant_alpha(self: &WeDeveloperModel) -> f64;
        #[cxx_name = "autoUpstream"]
        fn auto_upstream(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "setAutoUpstream"]
        fn set_auto_upstream(self: Pin<&mut WeDeveloperModel>, value: bool);
        #[cxx_name = "autoDownload"]
        fn auto_download(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "setAutoDownload"]
        fn set_auto_download(self: Pin<&mut WeDeveloperModel>, value: bool);
        #[cxx_name = "autoParity"]
        fn auto_parity(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "setAutoParity"]
        fn set_auto_parity(self: Pin<&mut WeDeveloperModel>, value: bool);
        #[cxx_name = "versionLabel"]
        fn version_label(self: &WeDeveloperModel) -> QString;
        fn targets(self: &WeDeveloperModel) -> QList_QVariant;
        #[cxx_name = "updateStatus"]
        fn update_status(self: &WeDeveloperModel) -> QString;
        fn changelog(self: &WeDeveloperModel) -> QString;
        #[cxx_name = "checkEnabled"]
        fn check_enabled(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "updateEnabled"]
        fn update_enabled(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "oweEnabled"]
        fn owe_enabled(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "seenEnabled"]
        fn seen_enabled(self: &WeDeveloperModel) -> bool;
        #[cxx_name = "copyEnabled"]
        fn copy_enabled(self: &WeDeveloperModel) -> bool;

        fn status(self: &WeParityQuery) -> i32;
        fn error(self: &WeParityQuery) -> QString;
        fn querying(self: &WeParityQuery) -> bool;
        #[cxx_name = "forwardError"]
        fn forward_error(self: &WeParityQuery) -> bool;
        #[cxx_name = "setForwardError"]
        fn set_forward_error(self: Pin<&mut WeParityQuery>, value: bool);
        #[cxx_name = "daemonVersion"]
        fn daemon_version(self: &WeParityQuery) -> QString;
        #[cxx_name = "setDaemonVersion"]
        fn set_daemon_version(self: Pin<&mut WeParityQuery>, value: &QString);
        #[cxx_name = "reportText"]
        fn report_text(self: &WeParityQuery) -> QString;
        #[cxx_name = "badgeText"]
        fn badge_text(self: &WeParityQuery) -> QString;
        #[cxx_name = "badgeColor"]
        fn badge_color(self: &WeParityQuery) -> QString;
        #[cxx_name = "badgeBold"]
        fn badge_bold(self: &WeParityQuery) -> bool;
        fn drift(self: &WeParityQuery) -> bool;
    }

    // ── API для QML ───────────────────────────────────────────────────────
    extern "RustQt" {
        /// Вычерпать очередь событий фоновых задач. Зовётся таймером
        /// страницы: очередь, а не колбэки, — граница потоков одна.
        #[qinvokable]
        fn pump(self: Pin<&mut WeDeveloperModel>);

        /// Страницу открыли: обновить подпись версии.
        ///
        /// `daemon_version` — свойство `Version` живого демона; спросить
        /// его умеет только Qt-слой, поэтому его называет разметка.
        #[qinvokable]
        #[cxx_name = "refreshVersion"]
        fn refresh_version(self: Pin<&mut WeDeveloperModel>, daemon_version: &QString);

        /// Кнопка «Проверить сейчас» и авто-проверка при запуске.
        #[qinvokable]
        #[cxx_name = "startCheck"]
        fn start_check(self: Pin<&mut WeDeveloperModel>, daemon_version: &QString);

        /// Текст вопроса перед установкой движка; пусто — ставить нечего.
        #[qinvokable]
        #[cxx_name = "confirmUpdateText"]
        fn confirm_update_text(self: &WeDeveloperModel) -> QString;

        /// Согласие на установку движка получено.
        #[qinvokable]
        #[cxx_name = "startEngineInstall"]
        fn start_engine_install(self: Pin<&mut WeDeveloperModel>);

        /// Текст вопроса перед установкой OWE-плагина; пусто — нечего.
        #[qinvokable]
        #[cxx_name = "confirmOweText"]
        fn confirm_owe_text(self: &WeDeveloperModel) -> QString;

        /// Согласие на установку OWE-плагина получено.
        #[qinvokable]
        #[cxx_name = "startOweInstall"]
        fn start_owe_install(self: Pin<&mut WeDeveloperModel>, daemon_version: &QString);

        /// Кнопка «OWE: отметить изученным».
        #[qinvokable]
        #[cxx_name = "markOweSeen"]
        fn mark_owe_seen(self: Pin<&mut WeDeveloperModel>);

        /// Кнопка «Копировать всё» под панелью changelog.
        #[qinvokable]
        #[cxx_name = "copyChangelog"]
        fn copy_changelog(self: Pin<&mut WeDeveloperModel>);

        /// Демон принял плагин (ответ `plugin_install`).
        #[qinvokable]
        #[cxx_name = "oweIpcInstalled"]
        fn owe_ipc_installed(self: Pin<&mut WeDeveloperModel>, plugin_id: &QString,
                             needs_restart: bool);

        /// Демон отверг установку плагина.
        #[qinvokable]
        #[cxx_name = "oweIpcRefused"]
        fn owe_ipc_refused(self: Pin<&mut WeDeveloperModel>, message: &QString);

        /// Сторож ответа демона на `plugin_install` дозвонился.
        #[qinvokable]
        #[cxx_name = "oweIpcTimeout"]
        fn owe_ipc_timeout(self: Pin<&mut WeDeveloperModel>, attempt: i32);

        /// Вкладку закрывают: погасить фоновые задачи и дождаться их.
        #[qinvokable]
        fn shutdown(self: Pin<&mut WeDeveloperModel>);

        #[qinvokable]
        fn reload(self: Pin<&mut WeParityQuery>);

        /// Авто-сверка при открытии страницы: без живого демона МОЛЧИТ.
        ///
        /// Ровно тот же порядок, что у виджетной версии: она грузила
        /// dev-страницу только при открытом сокете
        /// (`_ensure_page_loaded`), поэтому на мёртвом демоне человек
        /// видел не отказ, а обычную заглушку «нажмите „Сверить“».
        /// Ручное нажатие кнопки, наоборот, обязано объяснить, почему
        /// ничего не вышло, — им остаётся `reload`.
        #[qinvokable]
        #[cxx_name = "reloadIfConnected"]
        fn reload_if_connected(self: Pin<&mut WeParityQuery>);
        #[qinvokable]
        #[cxx_name = "deliverResponse"]
        fn deliver_response(self: Pin<&mut WeParityQuery>, token: i64, frame: &QByteArray);
    }

    impl cxx_qt::Initialize for WeDeveloperModel {}
    impl cxx_qt::Threading for WeDeveloperModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QList, QMap, QMapPair_QString_QVariant, QString, QVariant};
use prost::Message;
use std::sync::Arc;

use rexon_core::engine_update as eu;
use rexon_core::proto::control as pb;
use rexon_core::we_parity;
use rexon_core::we_query::{self, QuerySignal, QueryState};
use rexon_core::we_window as ww;
use rexon_core::{settings, ui_base};

use crate::impl_query_base;
use crate::query_base::decode_failed;
use crate::we_env;
use crate::we_update_workers::{self, UpdateEnv, UpdateEvent, WeUpdateTasks};

// ============================================================================
// Мелочи перевода в Qt
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

/// Строка статуса в том виде, в каком её читает разметка.
fn line_map(key: &str, line: &ww::StatusLine) -> QVariant {
    QVariant::from(&vmap(vec![
        ("key", qstr(key)),
        ("text", qstr(&line.text)),
        ("color", qstr(&line.color)),
        ("bold", QVariant::from(&line.bold)),
    ]))
}

// ============================================================================
// Состояние страницы
// ============================================================================

pub struct WeDeveloperModelRust {
    /// Номер записи в общем реестре смены скина (`crate::skin`).
    skin_watch: u64,
    auto_upstream: bool,
    auto_download: bool,
    auto_parity: bool,
    /// Фоновые задачи; `None` — рантайм не поднялся, кнопки молчат.
    tasks: Option<Arc<WeUpdateTasks>>,
    /// Подпись версии движка целиком (её собирает ядро).
    version_label: String,
    /// Строки статуса целей по порядку `we_window::STATUS_TARGETS`.
    lines: Vec<(String, ww::StatusLine)>,
    /// Последние ответы проверки — из них берутся `latest`/`title`.
    statuses: Vec<eu::TargetStatus>,
    update_status: String,
    changelog: String,
    check_enabled: bool,
    update_enabled: bool,
    owe_enabled: bool,
    seen_enabled: bool,
    copy_enabled: bool,
    /// Доступное обновление waywallen и OWE (то, что ставят кнопки).
    pending_update: Option<eu::TargetStatus>,
    pending_owe: Option<eu::TargetStatus>,
    /// Авто-установка — не больше одного раза за сессию, как в оригинале.
    auto_install_done: bool,
    auto_owe_done: bool,
    /// Версия живого демона, названная разметкой при последней операции.
    daemon_version: String,
    /// Временный архив плагина, ждущий ответа демона, и его версия.
    owe_ipc_zip: String,
    owe_ipc_version: String,
    /// Номер попытки IPC-установки (починка 2 ядра: сторож помнит СВОЮ).
    owe_ipc_attempt: u64,
}

impl Default for WeDeveloperModelRust {
    fn default() -> Self {
        let settings = settings::SettingsManager::new().ok();
        let flag = |key: &str, fallback: bool| -> bool {
            settings
                .as_ref()
                .and_then(|s| s.get(key))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(fallback)
        };
        Self {
            skin_watch: 0,
            auto_upstream: flag(ww::KEY_AUTO_UPSTREAM, ww::DEFAULT_AUTO_UPSTREAM),
            auto_download: flag(ww::KEY_AUTO_DOWNLOAD, ww::DEFAULT_AUTO_DOWNLOAD),
            auto_parity: flag(ww::KEY_AUTO_PARITY, ww::DEFAULT_AUTO_PARITY),
            tasks: WeUpdateTasks::new(UpdateEnv::from_process()).map(Arc::new),
            version_label: ww::version_label_initial().to_string(),
            lines: ww::STATUS_TARGETS
                .iter()
                .map(|(key, name)| {
                    ((*key).to_string(), ww::target_status_initial_parts(name))
                })
                .collect(),
            statuses: Vec::new(),
            update_status: String::new(),
            changelog: String::new(),
            // Проверять можно сразу, ставить — только после проверки: до
            // неё неизвестно ни что ставить, ни откуда. Ровно те же
            // начальные значения, что у виджетной версии.
            check_enabled: true,
            update_enabled: false,
            owe_enabled: false,
            seen_enabled: false,
            copy_enabled: false,
            pending_update: None,
            pending_owe: None,
            auto_install_done: false,
            auto_owe_done: false,
            daemon_version: String::new(),
            owe_ipc_zip: String::new(),
            owe_ipc_version: String::new(),
            owe_ipc_attempt: 0,
        }
    }
}

impl Drop for WeDeveloperModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
        // Задачи гасим и здесь, а не только по `shutdown()` из разметки:
        // вкладку могут закрыть, не спросив страницу.
        if let Some(tasks) = self.tasks.take() {
            tasks.shutdown();
        }
    }
}

impl qobject::WeDeveloperModel {
    // ── Подписи, числа и оформление ───────────────────────────────────────

    /// Все подписи страницы одной картой: разметка читает их по имени.
    ///
    /// Картой, а не два десятка отдельных свойств: подписи — константы
    /// ядра, у каждой был бы свой геттер и своя строка в мосте, а читаются
    /// они все одинаково.
    fn texts(&self) -> QMap<QMapPair_QString_QVariant> {
        vmap(vec![
            ("title", qstr(&ww::dev_title())),
            ("subtitle", qstr(&ww::dev_subtitle())),
            ("sectionUpstream", qstr(&ww::section_upstream())),
            ("sectionParity", qstr(&ww::section_parity())),
            ("cbAutoUpstream", qstr(&ww::cb_auto_upstream())),
            ("cbAutoDownload", qstr(&ww::cb_auto_download())),
            ("cbAutoParity", qstr(&ww::cb_auto_parity())),
            ("btnCheck", qstr(&ww::btn_check())),
            ("btnUpdate", qstr(&ww::btn_update())),
            ("btnOwe", qstr(&ww::btn_owe())),
            ("btnSeen", qstr(&ww::btn_seen())),
            ("btnCopy", qstr(&ww::btn_copy())),
            ("btnParity", qstr(&ww::btn_parity())),
            ("labelChangelog", qstr(&ww::label_changelog())),
            ("placeholderChangelog", qstr(&ww::placeholder_changelog())),
            ("placeholderParity", qstr(&ww::placeholder_parity())),
            ("confirmUpdateTitle", qstr(&ww::confirm_update_title())),
            ("confirmOweTitle", qstr(&ww::confirm_owe_title())),
            ("dialogYes", qstr(&ww::dialog_yes())),
            ("dialogNo", qstr(&ww::dialog_no())),
        ])
    }

    /// Все числа раскладки одной картой — тем же доводом, что и подписи.
    fn metrics(&self) -> QMap<QMapPair_QString_QVariant> {
        let num = |v: i32| QVariant::from(&v);
        vmap(vec![
            ("titleFontSize", num(ww::DEV_TITLE_FONT_SIZE)),
            ("titleFontWeight", num(ww::DEV_TITLE_FONT_WEIGHT)),
            ("sectionFontSize", num(ww::DEV_SECTION_FONT_SIZE)),
            ("sectionFontWeight", num(ww::DEV_SECTION_FONT_WEIGHT)),
            ("monoFontSize", num(ww::DEV_MONO_FONT_SIZE)),
            ("mutedFontSize", num(ww::DEV_MUTED_FONT_SIZE)),
            ("statusFontSize", num(ww::DEV_STATUS_FONT_SIZE)),
            ("changelogLabelWeight", num(ww::DEV_CHANGELOG_LABEL_WEIGHT)),
            ("pageMargin", num(ww::DEV_PAGE_MARGIN)),
            ("columnSpacing", num(ww::DEV_COLUMN_SPACING)),
            ("panePadding", num(ww::DEV_PANE_PADDING)),
            ("paneSpacing", num(ww::DEV_PANE_SPACING)),
            ("rowSpacing", num(ww::DEV_ROW_SPACING)),
            ("changelogMinHeight", num(ww::DEV_CHANGELOG_MIN_HEIGHT)),
            ("changelogMaxHeight", num(ww::DEV_CHANGELOG_MAX_HEIGHT)),
            ("startupDelayMs", num(ww::STARTUP_DELAY_MS)),
            ("oweIpcTimeoutMs", num(ww::OWE_IPC_TIMEOUT_MS)),
        ])
    }

    /// Палитра ДЕЙСТВУЮЩЕГО скина (правило #17) — тем же способом, что у
    /// шелла вкладки: своих цветов страница не заводит.
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        for (key, value) in &ui_base::active_qml_palette() {
            out.insert(QString::from(key), qstr(value));
        }
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        out
    }

    fn variant_alpha(&self) -> f64 {
        ww::VARIANT_ALPHA as f64
    }

    // ── Галки ─────────────────────────────────────────────────────────────

    fn auto_upstream(&self) -> bool {
        self.rust().auto_upstream
    }

    fn auto_download(&self) -> bool {
        self.rust().auto_download
    }

    fn auto_parity(&self) -> bool {
        self.rust().auto_parity
    }

    fn set_auto_upstream(mut self: Pin<&mut Self>, value: bool) {
        if self.rust().auto_upstream == value {
            return;
        }
        self.as_mut().rust_mut().auto_upstream = value;
        self.as_mut().store_flag(ww::KEY_AUTO_UPSTREAM, value);
        self.as_mut().flags_changed();
    }

    fn set_auto_download(mut self: Pin<&mut Self>, value: bool) {
        if self.rust().auto_download == value {
            return;
        }
        self.as_mut().rust_mut().auto_download = value;
        self.as_mut().store_flag(ww::KEY_AUTO_DOWNLOAD, value);
        self.as_mut().flags_changed();
    }

    fn set_auto_parity(mut self: Pin<&mut Self>, value: bool) {
        if self.rust().auto_parity == value {
            return;
        }
        self.as_mut().rust_mut().auto_parity = value;
        self.as_mut().store_flag(ww::KEY_AUTO_PARITY, value);
        self.as_mut().flags_changed();
    }

    /// Записать галку в настройки продукта. Неудача записи молчит: она не
    /// повод отменять сам переключатель, а без настроек страница работает.
    ///
    /// ⚠️ Менеджер настроек берётся ЗАНОВО на каждую запись, а не держится
    /// полем страницы. Сохранение отдаёт на диск ВСЮ карту настроек целиком,
    /// поэтому долгоживущий снимок возвращал файл к тому виду, какой он имел
    /// в момент открытия страницы: скин, громкость обоев и обучение,
    /// изменённые с тех пор в «Настройках», молча откатывались одним щелчком
    /// галки здесь, а ключи, заведённые за это время, пропадали совсем.
    /// Так же поступают и все остальные писатели этого файла
    /// (`settings_actions::save_setting`, `tray`, `appearance_actions`).
    fn store_flag(self: Pin<&mut Self>, key: &str, value: bool) {
        let _ = settings::SettingsManager::new()
            .and_then(|mut m| m.set(key, serde_json::Value::Bool(value)));
    }

    // ── Секция «Обновления upstream» ──────────────────────────────────────

    fn version_label(&self) -> QString {
        QString::from(&self.rust().version_label)
    }

    fn targets(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (key, line) in &self.rust().lines {
            out.append(line_map(key, line));
        }
        out
    }

    fn update_status(&self) -> QString {
        QString::from(&self.rust().update_status)
    }

    fn changelog(&self) -> QString {
        QString::from(&self.rust().changelog)
    }

    fn check_enabled(&self) -> bool {
        self.rust().check_enabled
    }

    fn update_enabled(&self) -> bool {
        self.rust().update_enabled
    }

    fn owe_enabled(&self) -> bool {
        self.rust().owe_enabled
    }

    fn seen_enabled(&self) -> bool {
        self.rust().seen_enabled
    }

    fn copy_enabled(&self) -> bool {
        self.rust().copy_enabled
    }

    /// Поставить строку статуса цели. Нет такой цели — тишина: состав
    /// целей называет ядро, и лишний ключ здесь не ошибка страницы.
    fn set_line(mut self: Pin<&mut Self>, key: &str, line: ww::StatusLine) {
        let mut me = self.as_mut().rust_mut();
        if let Some(slot) = me.lines.iter_mut().find(|(k, _)| k == key) {
            slot.1 = line;
        }
    }

    // ── Вход из разметки ──────────────────────────────────────────────────

    fn refresh_version(mut self: Pin<&mut Self>, daemon_version: &QString) {
        let version = daemon_version.to_string();
        self.as_mut().rust_mut().daemon_version = version.clone();
        // ⚠️ Починка 4: версию считает ФОНОВАЯ задача. У оригинала этот
        // вызов уходил в `--version` бинаря и `busctl` прямо в GUI-потоке.
        if let Some(tasks) = self.rust().tasks.clone() {
            tasks.start_version_refresh(version);
        }
    }

    fn start_check(mut self: Pin<&mut Self>, daemon_version: &QString) {
        let version = daemon_version.to_string();
        self.as_mut().rust_mut().daemon_version = version.clone();
        let Some(tasks) = self.rust().tasks.clone() else {
            return;
        };
        // Ранний выход оригинала: проверка уже идёт.
        if !tasks.start_check(version) {
            return;
        }
        {
            let mut me = self.as_mut().rust_mut();
            me.check_enabled = false;
            me.update_status = ww::text_check_running().to_string();
        }
        self.as_mut().upstream_changed();
    }

    fn confirm_update_text(&self) -> QString {
        let Some(st) = self.rust().pending_update.as_ref() else {
            return QString::default();
        };
        QString::from(&ww::confirm_update_text(&st.latest, &st.asset_url))
    }

    fn start_engine_install(mut self: Pin<&mut Self>) {
        let Some(st) = self.rust().pending_update.clone() else {
            return;
        };
        let Some(tasks) = self.rust().tasks.clone() else {
            return;
        };
        if !tasks.start_engine_install(st.asset_url, st.latest) {
            return;
        }
        self.as_mut().rust_mut().update_enabled = false;
        self.as_mut().upstream_changed();
    }

    fn confirm_owe_text(&self) -> QString {
        let Some(st) = self.rust().pending_owe.as_ref() else {
            return QString::default();
        };
        QString::from(&ww::confirm_owe_text(&st.latest, &st.asset_url))
    }

    fn start_owe_install(mut self: Pin<&mut Self>, daemon_version: &QString) {
        let version = daemon_version.to_string();
        self.as_mut().rust_mut().daemon_version = version.clone();
        let Some(st) = self.rust().pending_owe.clone() else {
            return;
        };
        let Some(tasks) = self.rust().tasks.clone() else {
            return;
        };
        // Демон подключён → hot-install его же IPC; иначе распаковка в
        // каталог плагинов внутри задачи. Развилку решает ядро.
        let fs_install = ww::plugin_fs_install(crate::discover_queries::connected());
        if !tasks.start_owe_install(st.asset_url, st.latest, fs_install, version) {
            return;
        }
        self.as_mut().rust_mut().owe_enabled = false;
        self.as_mut().upstream_changed();
    }

    fn mark_owe_seen(mut self: Pin<&mut Self>) {
        let Some(st) = self
            .rust()
            .statuses
            .iter()
            .find(|s| s.key == "owe")
            .cloned()
        else {
            return;
        };
        if st.latest.is_empty() {
            return;
        }
        let Some(tasks) = self.rust().tasks.clone() else {
            return;
        };
        tasks.mark_seen("owe", &st.latest);
        let line = ww::owe_seen_parts(&st.title, &st.latest);
        self.as_mut().set_line("owe", line);
        self.as_mut().rust_mut().seen_enabled = false;
        self.as_mut().upstream_changed();
    }

    fn copy_changelog(mut self: Pin<&mut Self>) {
        let text = self.rust().changelog.clone();
        if text.is_empty() {
            return;
        }
        // ⚠️ Функция шима зовётся ЧЕРЕЗ мост синглтона `Action`, а не
        // объявляется здесь второй раз: cxx печатает переходник по имени
        // функции, и два объявления одной и той же дают дубль символа на
        // линковке (`duplicate symbol: cxxbridge1$…$rexon_we_copy_to_clipboard`).
        crate::we_action::qobject::rexon_we_copy_to_clipboard(&QString::from(&text));
        self.as_mut().rust_mut().update_status = ww::text_changelog_copied().to_string();
        self.as_mut().upstream_changed();
    }

    fn shutdown(mut self: Pin<&mut Self>) {
        if let Some(tasks) = self.as_mut().rust_mut().tasks.take() {
            tasks.shutdown();
        }
    }

    // ── Ответ демона на установку плагина ─────────────────────────────────

    fn owe_ipc_installed(
        mut self: Pin<&mut Self>,
        plugin_id: &QString,
        needs_restart: bool,
    ) {
        self.as_mut().drop_ipc_zip();
        let version = self.rust().owe_ipc_version.clone();
        if let Some(tasks) = self.rust().tasks.clone() {
            tasks.mark_installed_tag("owe", &version);
        }
        let text = ww::owe_ipc_success_text(&plugin_id.to_string(), needs_restart);
        self.finish_owe_install(true, &text);
    }

    fn owe_ipc_refused(mut self: Pin<&mut Self>, message: &QString) {
        self.as_mut().drop_ipc_zip();
        let text = message.to_string();
        let text = if text.is_empty() {
            ww::text_owe_ipc_refused().to_string()
        } else {
            text
        };
        self.finish_owe_install(false, &text);
    }

    /// Сторож ответа демона. Срабатывает, только если он от ТЕКУЩЕЙ
    /// попытки и ответа всё ещё нет — починка 2 ядра.
    fn owe_ipc_timeout(mut self: Pin<&mut Self>, attempt: i32) {
        let fires = ww::owe_ipc_timeout_fires(
            attempt.max(0) as u64,
            self.rust().owe_ipc_attempt,
            !self.rust().owe_ipc_zip.is_empty(),
        );
        if !fires {
            return;
        }
        self.as_mut().drop_ipc_zip();
        self.finish_owe_install(false, &ww::text_owe_ipc_timeout());
    }

    /// Убрать временный архив, который ждал ответа демона.
    fn drop_ipc_zip(mut self: Pin<&mut Self>) {
        let zip = std::mem::take(&mut self.as_mut().rust_mut().owe_ipc_zip);
        we_update_workers::drop_tmp(&zip);
    }

    // ── Очередь событий фоновых задач ─────────────────────────────────────

    fn pump(mut self: Pin<&mut Self>) {
        loop {
            let Some(tasks) = self.rust().tasks.clone() else {
                return;
            };
            let Some(event) = tasks.poll_event() else {
                return;
            };
            self.as_mut().handle_event(event);
        }
    }
}

// ============================================================================
// Внутреннее: приём событий и пересчёты
// ============================================================================

impl qobject::WeDeveloperModel {
    fn handle_event(mut self: Pin<&mut Self>, event: UpdateEvent) {
        match event {
            UpdateEvent::EngineVersion(version) => {
                self.as_mut().rust_mut().version_label = ww::version_label(&version);
                self.as_mut().upstream_changed();
            }
            UpdateEvent::Progress(message) => {
                self.as_mut().rust_mut().update_status = message;
                self.as_mut().upstream_changed();
            }
            UpdateEvent::CheckDone(statuses) => self.as_mut().on_check_done(statuses),
            UpdateEvent::EngineDone { ok, message } => {
                self.as_mut().on_engine_done(ok, &message);
            }
            UpdateEvent::OweDone {
                ok,
                message,
                zip_path,
            } => self.as_mut().on_owe_done(ok, &message, &zip_path),
            UpdateEvent::OweGateRefused(text) => {
                {
                    let mut me = self.as_mut().rust_mut();
                    me.update_status = text;
                    // ⚠️ Починка 5: кнопку возвращаем. Гейт теперь считает
                    // задача, и кнопка успела погаснуть до его ответа —
                    // оставить её мёртвой значило бы отнять повтор.
                    me.owe_enabled = me.pending_owe.is_some();
                }
                self.as_mut().upstream_changed();
            }
        }
    }

    /// Порт `_on_upstream_results`.
    fn on_check_done(mut self: Pin<&mut Self>, statuses: Vec<eu::TargetStatus>) {
        self.as_mut().rust_mut().check_enabled = true;
        if statuses.is_empty() {
            // Сеть/рейт-лимит — тихий статус, без крешей.
            self.as_mut().rust_mut().update_status = ww::text_check_failed().to_string();
            self.as_mut().upstream_changed();
            return;
        }
        let mut pending_update = None;
        let mut pending_owe = None;
        let mut seen_enabled = self.rust().seen_enabled;
        for st in &statuses {
            let line = ww::target_status_parts(st.ok, &st.title, &st.note, st.update_available);
            self.as_mut().set_line(&st.key, line);
            if !st.ok {
                continue;
            }
            if st.key == "waywallen" && st.update_available && !st.asset_url.is_empty() {
                pending_update = Some(st.clone());
            }
            if st.key == "owe" {
                seen_enabled = st.update_available;
                if st.update_available && !st.asset_url.is_empty() {
                    pending_owe = Some(st.clone());
                }
            }
        }
        let with_changelog: Vec<eu::TargetStatus> = statuses
            .iter()
            .filter(|s| !s.changelog.is_empty())
            .cloned()
            .collect();
        let rendered = eu::render_changelog_text(&with_changelog);
        {
            let mut me = self.as_mut().rust_mut();
            me.statuses = statuses;
            me.update_enabled = pending_update.is_some();
            me.owe_enabled = pending_owe.is_some();
            me.pending_update = pending_update;
            me.pending_owe = pending_owe;
            me.seen_enabled = seen_enabled;
            me.changelog = ww::changelog_panel_text(&rendered);
            me.copy_enabled = !rendered.is_empty();
            me.update_status = ww::text_check_done().to_string();
        }
        self.as_mut().upstream_changed();

        // Авто-установка: только по галке (дефолт ВЫКЛ) и один раз за
        // сессию. Вердикт — у ядра.
        let auto = self.rust().auto_download;
        let want_engine = ww::should_auto_install(
            self.rust().pending_update.is_some(),
            auto,
            self.rust().auto_install_done,
        );
        if want_engine {
            self.as_mut().rust_mut().auto_install_done = true;
            self.as_mut().start_engine_install();
        }
        let want_owe = ww::should_auto_install(
            self.rust().pending_owe.is_some(),
            auto,
            self.rust().auto_owe_done,
        );
        if want_owe {
            self.as_mut().rust_mut().auto_owe_done = true;
            let version = QString::from(&self.rust().daemon_version);
            self.as_mut().start_owe_install(&version);
        }
    }

    /// Порт `_on_install_done`.
    fn on_engine_done(mut self: Pin<&mut Self>, ok: bool, message: &str) {
        {
            let mut me = self.as_mut().rust_mut();
            me.update_status = ww::install_done_text(ok, message);
            // Удалась установка — ставить больше нечего.
            me.update_enabled = !ok;
        }
        if ok {
            let line = ww::waywallen_installed_parts();
            self.as_mut().set_line("waywallen", line);
        }
        self.as_mut().upstream_changed();
        // Подпись версии перечитываем ФОНОМ (починка 4).
        let version = QString::from(&self.rust().daemon_version);
        self.as_mut().refresh_version(&version);
    }

    /// Порт `_on_owe_worker_done`.
    fn on_owe_done(mut self: Pin<&mut Self>, ok: bool, message: &str, zip_path: &str) {
        if !ok {
            {
                let mut me = self.as_mut().rust_mut();
                me.update_status = ww::owe_done_text(false, message);
                me.owe_enabled = true;
            }
            self.as_mut().upstream_changed();
            return;
        }
        if zip_path.is_empty() {
            // ФС-установку задача уже выполнила.
            self.finish_owe_install(true, message);
            return;
        }
        if !crate::discover_queries::connected() {
            // Демон умер, пока качали — ФС-фолбэк (быстро: без сети).
            let tag = self
                .rust()
                .pending_owe
                .as_ref()
                .map(|s| s.latest.clone())
                .unwrap_or_default();
            let (ok2, text) = match self.rust().tasks.clone() {
                Some(tasks) => tasks.install_plugin_zip_now(zip_path, &tag),
                None => (false, ww::text_owe_ipc_refused().to_string()),
            };
            we_update_workers::drop_tmp(zip_path);
            self.finish_owe_install(ok2, &text);
            return;
        }
        // Hot-install через демона: архив распакует он сам.
        let attempt = {
            let mut me = self.as_mut().rust_mut();
            me.update_status = ww::text_owe_ipc_running().to_string();
            me.owe_ipc_zip = zip_path.to_string();
            me.owe_ipc_version = me
                .pending_owe
                .as_ref()
                .map(|s| s.latest.clone())
                .unwrap_or_default();
            me.owe_ipc_attempt += 1;
            me.owe_ipc_attempt
        };
        self.as_mut().upstream_changed();
        // Сокет трогает разметка через `PluginInstallQuery`, она же заводит
        // сторож ответа: свой второй способ говорить с демоном здесь не
        // нужен.
        let zip = QString::from(zip_path);
        self.as_mut()
            .owe_ipc_requested(zip, attempt.min(i32::MAX as u64) as i32);
    }

    /// Порт `_finish_owe_install`.
    fn finish_owe_install(mut self: Pin<&mut Self>, ok: bool, message: &str) {
        {
            let mut me = self.as_mut().rust_mut();
            me.update_status = ww::owe_done_text(ok, message);
            me.owe_enabled = !ok;
        }
        if ok {
            let tag_version = eu::version_tag_to_state(&self.rust().owe_ipc_version);
            let fs_version = self
                .rust()
                .tasks
                .as_ref()
                .map(|t| we_update_workers::owe_installed_version(t.env()))
                .unwrap_or_default();
            let version = if fs_version.is_empty() {
                tag_version
            } else {
                fs_version
            };
            let line = ww::owe_installed_parts(&version);
            self.as_mut().set_line("owe", line);
            self.as_mut().rust_mut().seen_enabled = false;
        }
        self.as_mut().upstream_changed();
    }
}

impl cxx_qt::Initialize for qobject::WeDeveloperModel {
    fn initialize(mut self: Pin<&mut Self>) {
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::WeDeveloperModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
    }
}

// ============================================================================
// WeParityQuery — сверка схемы renderer-настроек с реестром UI
// ============================================================================

#[derive(Default)]
pub struct WeParityQueryRust {
    base: QueryState,
    data: we_query::ParitySchemaData,
    pending: std::collections::BTreeSet<i64>,
    /// Версия ЖИВОГО демона — её называет страница.
    daemon_version: String,
    /// Текст панели отчёта.
    report_text: String,
    /// Бейдж рядом с кнопкой «Сверить с движком».
    badge: Option<ww::StatusLine>,
    /// Показывать ли пометку на кнопке «Разраб.».
    drift: bool,
}

impl_query_base!(WeParityQuery);

impl qobject::WeParityQuery {
    pub fn daemon_version(&self) -> QString {
        QString::from(&self.daemon_version)
    }

    pub fn set_daemon_version(mut self: Pin<&mut Self>, value: &QString) {
        let next = value.to_string();
        if self.daemon_version == next {
            return;
        }
        self.as_mut().rust_mut().daemon_version = next;
        self.daemon_version_changed();
    }

    pub fn report_text(&self) -> QString {
        QString::from(&self.report_text)
    }

    pub fn badge_text(&self) -> QString {
        QString::from(self.badge.as_ref().map_or("", |b| b.text.as_str()))
    }

    pub fn badge_color(&self) -> QString {
        QString::from(self.badge.as_ref().map_or("", |b| b.color.as_str()))
    }

    pub fn badge_bold(&self) -> bool {
        self.badge.as_ref().is_some_and(|b| b.bold)
    }

    pub fn drift(&self) -> bool {
        self.drift
    }

    /// Кнопка «Сверить с движком» и авто-сверка при открытии страницы.
    ///
    /// Демона нет — своя короткая строка ядра, а не общая ошибка базы
    /// «нет соединения»: у виджетной версии проверка стояла ДО отправки, и
    /// человек читал именно её.
    pub fn reload(mut self: Pin<&mut Self>) {
        if !crate::discover_queries::connected() {
            self.as_mut().rust_mut().report_text = ww::text_parity_no_daemon().to_string();
            self.as_mut().report_changed();
            return;
        }
        let payload = pb::RendererPluginListRequest {}.encode_to_vec();
        if let Some(token) = self.as_mut().send_frame("renderer_plugin_list", payload) {
            self.rust_mut().pending.insert(token);
        }
    }

    /// Авто-сверка при открытии страницы (см. мост).
    pub fn reload_if_connected(self: Pin<&mut Self>) {
        if !crate::discover_queries::connected() {
            return;
        }
        self.reload();
    }

    pub fn deliver_response(mut self: Pin<&mut Self>, token: i64, frame: &QByteArray) {
        if !self.as_mut().rust_mut().pending.remove(&token) {
            return;
        }
        let parsed = {
            let mut rust = self.as_mut().rust_mut();
            let rust = &mut *rust;
            we_query::apply_parity_schema(&mut rust.base, &mut rust.data, frame.as_slice())
        };
        let Ok(signals) = parsed else {
            let signals = decode_failed(&mut self.as_mut().rust_mut().base);
            self.dispatch(signals);
            return;
        };
        self.as_mut().dispatch(signals);
        self.as_mut().rebuild_report();
    }

    /// Пересобрать панель, бейдж и пометку по последнему ответу.
    fn rebuild_report(mut self: Pin<&mut Self>) {
        // Движок ответил ошибкой — его текст, а при пустом текст ядра.
        if self.base.status == we_query::STATUS_ERROR {
            let text = if self.base.error.is_empty() {
                ww::text_parity_request_failed().to_string()
            } else {
                self.base.error.clone()
            };
            self.as_mut().rust_mut().report_text = text;
            self.as_mut().report_changed();
            return;
        }
        let Some(report) = self.rust().data.report.clone() else {
            return;
        };
        let daemon_version = self.rust().daemon_version.clone();
        // Тег нашего снапшота протокола: при расхождении с версией живого
        // демона бейдж честно скажет, что схему сверяли по УСТАРЕВШЕМУ
        // протоколу.
        // ⚠️ Тег ищется в каталоге ВКЛАДКИ, а не пакета `rexon/`: путь
        // между ними знает ядро. Прямой `pkg_dir` не находил файла, и
        // отчёт писал «(тег не записан)» (снимок стенда 06.09).
        let tag = we_parity::read_proto_upstream_tag_from_pkg(&we_env::pkg_dir());
        let proto_match = we_parity::daemon_matches_snapshot(&tag, &daemon_version);
        let text = ww::parity_report_header(&daemon_version, &tag)
            + &we_parity::render_report_text(&report);
        let badge = ww::parity_badge_parts(
            proto_match,
            &daemon_version,
            &tag,
            report.has_drift(),
            report.issues.len(),
            report.warning_count(),
            report.unchecked.iter().map(|(_, keys)| keys.len()).sum(),
        );
        let drift = ww::parity_drift_flag(report.has_drift(), proto_match);
        {
            let mut me = self.as_mut().rust_mut();
            me.report_text = text;
            me.badge = Some(badge);
            me.drift = drift;
        }
        self.as_mut().report_changed();
        self.as_mut().checked(drift);
    }

    pub fn emit_property(mut self: Pin<&mut Self>, name: &str) {
        match name {
            // Панель, бейдж и пометку пересобирает `rebuild_report` —
            // ему нужен ещё и тег снапшота, которого у ядра нет.
            "report" => self.as_mut().report_changed(),
            other => debug_assert!(false, "нет сигнала для свойства {other}"),
        }
    }

    pub fn dispatch(self: Pin<&mut Self>, signals: Vec<QuerySignal>) {
        self.dispatch_base(signals);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Строки статуса заводятся ровно по списку целей ядра и в его порядке:
    /// разметка рисует их подряд, а не по именам.
    #[test]
    fn initial_lines_follow_core_targets() {
        let me = WeDeveloperModelRust::default();
        assert_eq!(me.lines.len(), ww::STATUS_TARGETS.len());
        for ((key, line), (want_key, want_name)) in me.lines.iter().zip(ww::STATUS_TARGETS) {
            assert_eq!(key, want_key);
            assert_eq!(line.text, ww::target_status_initial_parts(want_name).text);
            assert!(line.color.is_empty(), "у начальной строки краски нет");
        }
    }

    /// Кнопки установки заперты до первой проверки: ставить нечего и
    /// неоткуда — то же начальное состояние, что у виджетной версии.
    #[test]
    fn install_buttons_locked_before_first_check() {
        let me = WeDeveloperModelRust::default();
        assert!(me.check_enabled);
        assert!(!me.update_enabled);
        assert!(!me.owe_enabled);
        assert!(!me.seen_enabled);
        assert!(!me.copy_enabled);
        assert_eq!(me.version_label, ww::version_label_initial());
    }
}
