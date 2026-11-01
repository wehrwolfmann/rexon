// WeActions — объект побочных действий вкладки «Wallpaper Engine» на Rust.
//
// ЗАЧЕМ. Каждая QML-страница продукта ждёт свойством `actions` объект, который
// умеет то, что меняет мир. У обоев договор самый узкий из семи вкладок —
// РОВНО ОДИН метод `retryDaemon`: поднять демона по кнопке «Повторить запуск»
// с экрана «Демон недоступен». Договор снят по самой разметке
// (`qml/WallpaperEnginePage.qml`), и держит его сторож в конце этого файла,
// чтобы «страница попросила второй метод» не выяснялось на живом окне.
//
// До сих пор этот объект жил на PySide6 (`WallpaperEngineActions` в
// `rexon/qt_wallpaper_engine/qml_window.py`), и пока он там, страница
// получала действия от хозяина-Python: без него кнопка была выключена, а
// вкладка не могла уйти с PySide6 в принципе. Приём повторён за
// `ExtensionsActions` (`src/extensions_actions.rs`) дословно: объект ничего
// не решает сам, всё решает ядро.
//
// ⚠️ ЧТО ДЕЛАЕТ `retryDaemon` И ПОЧЕМУ В ТАКОМ ПОРЯДКЕ. Питоновский оригинал
// (`WallpaperEngineQmlWindow.retry_daemon`) делал два шага: перевод модели на
// экран «подключение…» и `DaemonControl.ensure_running()`. Оба уже на Rust:
// решения автозапуска — в ядре (`we_client::ensure_running_decision`),
// исполнение — в `we_socket::ensure_running` (D-Bus, запуск отдельной сессией, поллинг
// шины). Здесь остался только их порядок, и он не косметический:
// `ensure_running` умеет отказать СИНХРОННО (бинаря демона нет вовсе) и тут
// же увести вкладку на экран отказа. Переставь шаги местами — и «подключение…»
// затёрло бы уже показанную причину отказа, а покупатель остался бы смотреть
// на вечный спиннер вместо объяснения.
//
// Экран «подключение…» ставится не прямым вызовом модели, а сигналом
// `retryStarted`, который страница разбирает блоком `Connections`. Причина —
// у объекта действий нет и не должно быть указателя на модель страницы: их
// связывает разметка, а не Rust, ровно как у `ExtensionsActions` с его
// `installFinished`/`uninstallFinished`. Заодно это работает и когда хозяин
// страницы подставил СВОЙ объект действий: старый питоновский такого сигнала
// не шлёт, `Connections` его молча не ждёт (`ignoreUnknownSignals`), а экран
// «подключение…» по-прежнему ставит он сам.
//
// ── СШИВКА СИГНАЛОВ ДЕМОНА: ТЕПЕРЬ ЗДЕСЬ ─────────────────────────────────────
//
// Метод — меньшая половина работы хозяина `qml_window.py`. Большая — швы
// между клиентом демона и моделью страницы: демон поднялся → открыть сокет
// на названный им порт; не поднялся → `showNoDaemon`; сокет открыт →
// `showConnecting` и запрос `health` со сторожем; ответ → `applyHealth`;
// молчание → `showHealthTimeout`; обрыв → `showConnecting`.
//
// Без них Rust-окно (`rexon-main-window`, Python там нет вовсе) открывало
// вкладку на «подключение…» и оставалось там навсегда: демона при открытии
// никто не звал, а «Повторить» возвращало туда же.
//
// Как сделано:
//   * события рождает транспорт (`we_socket.rs`) и раздаёт СПИСКУ подписчиков
//     (`we_socket::subscribe`) — рядом с питоновским слотом, а не вместо
//     него. Питоновская вкладка своих событий не теряет;
//   * этот объект подписывается ТОЛЬКО когда страница им реально пользуется
//     (`connectDaemon`, его зовёт страница при открытии, если хозяин не
//     подставил своих действий). У питоновского хозяина объект стоит в
//     странице без дела и демона не трогает — иначе сокет открывали бы двое;
//   * событие приходит в очередь объекта (`CxxQtThread::queue`) и
//     пересказывается странице сигналом, тем же каналом, что `retryStarted`;
//     на модель страницы указателя здесь по-прежнему нет;
//   * сторож ответа `health` — таймер шима (`we_socket::request_health`), срок
//     — ядра (`we_window::HEALTH_TIMEOUT_MS`), задержка старта — тоже ядра;
//   * признак «соединение уже было» — один, `WAS_CONNECTED` транспорта;
//   * подготовка показа обоев на KDE переехала в тот же транспорт
//     (`we_socket::prepare_display_backend`) и работает в обоих процессах.

/// Мост cxx-qt объекта действий.
///
/// Тип объявлен `#[qml_element]`, поэтому страница заводит его себе сама
/// (`WeActions { }`) и полна без хозяина; хозяин окна может подставить свой.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

    }

    // Шим пропуска нажатий. QObject объявлять не надо — cxx-qt знает его сам.
    unsafe extern "C++" {
        include!("rexon_gui_factory.h");
        /// # Safety
        /// `area` обязан быть живым QObject.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_pass_outside_presses(area: *mut QObject);
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        // Плагин обоев KDE: сообщение человеку, предложение поставить версию
        // Rexon и признак «ставим». Решение — ядро
        // (`waywallen_kde_plugin::plan_plugin`), доставка — транспорт.
        #[qproperty(QString, kde_plugin_notice, cxx_name = "kdePluginNotice")]
        #[qproperty(bool, kde_plugin_can_install, cxx_name = "kdePluginCanInstall")]
        #[qproperty(bool, kde_plugin_installing, cxx_name = "kdePluginInstalling")]
        // Идёт запуск движка: сторож запуска транспорта ждёт шину
        // (`we_client::StartWatch`). Пока он идёт, общий сторож экрана
        // ожидания стоит — срок у запуска свой.
        #[qproperty(bool, engine_starting, cxx_name = "engineStarting")]
        // Пояснение под «Запускается…», когда запуск долгий, но процесс жив
        // (`we_window::text_connecting_slow`); пусто — пояснять нечего.
        #[qproperty(QString, engine_start_note, cxx_name = "engineStartNote")]
        #[qproperty(QString, kde_plugin_install_label, cxx_name = "kdePluginInstallLabel",
                    READ = kde_plugin_install_label, CONSTANT)]
        type WeActions = super::WeActionsRust;
    }

    unsafe extern "RustQt" {
        /// Взялись за повторный запуск демона.
        ///
        /// Страница по нему переводит модель на экран «подключение…» — сам
        /// объект действий модели не знает и знать не должен.
        #[qsignal]
        #[cxx_name = "retryStarted"]
        fn retry_started(self: Pin<&mut WeActions>);

        /// Ждём движок: сокет открылся и идёт проверка готовности, либо
        /// сокет оборвался и транспорт переподключается сам.
        #[qsignal]
        fn connecting(self: Pin<&mut WeActions>);

        /// Демон не поднялся. `reason` — сырая причина, подпись для человека
        /// из неё складывает ядро (`showNoDaemon` модели).
        #[qsignal]
        #[cxx_name = "daemonFailed"]
        fn daemon_failed(self: Pin<&mut WeActions>, reason: QString);

        /// Ответ `health`: поле `state`. Вердикт выносит ядро (`applyHealth`).
        #[qsignal]
        #[cxx_name = "healthReceived"]
        fn health_received(self: Pin<&mut WeActions>, state: QString);

        /// Движок подключился и молчит на проверку готовности.
        #[qsignal]
        #[cxx_name = "healthTimedOut"]
        fn health_timed_out(self: Pin<&mut WeActions>);
    }

    extern "RustQt" {
        /// Кнопка «Повторить запуск» с экрана «Демон недоступен».
        #[qinvokable]
        #[cxx_name = "retryDaemon"]
        fn retry_daemon(self: Pin<&mut WeActions>);

        /// Открытие вкладки: подписаться на события демона и поднять его.
        #[qinvokable]
        #[cxx_name = "connectDaemon"]
        fn connect_daemon(self: Pin<&mut WeActions>);

        /// Кнопка «Поставить версию Rexon»: согласие заменить плагин обоев
        /// KDE (чужой неизвестной версии либо не подходящий к демону).
        #[qinvokable]
        #[cxx_name = "installKdePlugin"]
        fn install_kde_plugin(self: Pin<&mut WeActions>);

        fn kde_plugin_install_label(self: &WeActions) -> QString;

        /// Нажатия мыши вне `area` (вкладки обоев) доходят до окна и при
        /// открытом окне поверх — иначе разделы Rexon над вкладкой
        /// переставали нажиматься (шим `rexon_pass_outside_presses`).
        ///
        /// # Safety
        /// `area` — живой элемент сцены.
        #[qinvokable]
        #[cxx_name = "passOutsidePresses"]
        unsafe fn pass_outside_presses(self: &WeActions, area: *mut QObject);
    }

    // Событие транспорта ставится в очередь объекта — трейт потока обязан
    // быть объявлен В МОСТЕ.
    impl cxx_qt::Threading for WeActions {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;

use rexon_core::we_window as ww;

use crate::we_socket::{self, DaemonEvent};

/// Rust-состояние объекта действий: ровно номер подписки.
///
/// Весь порядок автозапуска демона — поллинг шины, счётчик попыток, признак
/// «соединение уже было» — живёт в `we_socket.rs` одним экземпляром на
/// поток. Второе место, где то же самое хранится, разъехалось бы с первым
/// молча (правило #12).
#[derive(Default)]
pub struct WeActionsRust {
    /// Номер в списке подписчиков транспорта; 0 — не подписан.
    subscription: u64,
    /// Номер подписки на состояние плагина обоев KDE; 0 — не подписан.
    kde_subscription: u64,
    kde_plugin_notice: QString,
    kde_plugin_can_install: bool,
    kde_plugin_installing: bool,
    engine_starting: bool,
    engine_start_note: QString,
}

/// Отписаться вместе с объектом: иначе транспорт звал бы в очередь
/// уничтоженной страницы (безвредно, но копилось бы на каждое открытие).
impl Drop for WeActionsRust {
    fn drop(&mut self) {
        we_socket::unsubscribe(self.subscription);
        we_socket::unsubscribe_kde_plugin(self.kde_subscription);
    }
}

impl qobject::WeActions {
    /// # Safety
    /// `area` — живой элемент сцены (его передаёт разметка).
    unsafe fn pass_outside_presses(&self, area: *mut qobject::QObject) {
        if !area.is_null() {
            // SAFETY: указатель не нулевой и пришёл от движка QML.
            unsafe { qobject::rexon_pass_outside_presses(area) };
        }
    }

    fn retry_daemon(mut self: Pin<&mut Self>) {
        self.as_mut().subscribe_once();
        // Сначала экран ожидания, потом попытка: `ensure_running` умеет
        // отказать синхронно, и его экран отказа обязан остаться последним.
        // Разбор порядка — в шапке файла.
        self.as_mut().retry_started();
        we_socket::ensure_running();
    }

    fn connect_daemon(mut self: Pin<&mut Self>) {
        self.as_mut().subscribe_once();
        we_socket::ensure_running_later(ww::STARTUP_DELAY_MS);
    }

    /// Подписаться на события транспорта один раз за жизнь объекта.
    fn subscribe_once(mut self: Pin<&mut Self>) {
        if self.rust().subscription != 0 {
            return;
        }
        let thread = self.qt_thread();
        let id = we_socket::subscribe(move |event| {
            let event = event.clone();
            // Объект мог умереть — это `ObjectDestroyed`, а не повод падать.
            let _ = thread.queue(move |obj| obj.on_daemon_event(event));
        });
        self.as_mut().rust_mut().subscription = id;

        // Состояние плагина KDE приходит из фонового потока подготовки —
        // в очередь объекта, как и события демона.
        let thread = self.qt_thread();
        let kde = we_socket::subscribe_kde_plugin(move |state| {
            let state = state.clone();
            let _ = thread.queue(move |obj| obj.on_kde_plugin_state(state));
        });
        self.as_mut().rust_mut().kde_subscription = kde;
    }

    fn install_kde_plugin(self: Pin<&mut Self>) {
        we_socket::install_kde_plugin_with_consent();
    }

    fn kde_plugin_install_label(&self) -> QString {
        QString::from(&rexon_core::waywallen_kde_plugin::install_rexon_version_label())
    }

    /// Пересказать странице состояние плагина обоев KDE.
    fn on_kde_plugin_state(mut self: Pin<&mut Self>, state: we_socket::KdePluginState) {
        let text = match (&state.failure, &state.notice) {
            (Some(reason), _) => rexon_core::waywallen_kde_plugin::install_failed_text(reason),
            (None, Some(notice)) => notice.text(),
            (None, None) => String::new(),
        };
        let can_install = !state.installing
            && state.notice.as_ref().is_some_and(|n| n.offers_install());
        self.as_mut().set_kde_plugin_notice(QString::from(&text));
        self.as_mut().set_kde_plugin_can_install(can_install);
        self.as_mut().set_kde_plugin_installing(state.installing);
    }

    /// Пересказать событие транспорта странице.
    fn on_daemon_event(mut self: Pin<&mut Self>, event: DaemonEvent) {
        // Запуск движка кончается ответом «готов» или отказом.
        if matches!(event, DaemonEvent::Ready(_) | DaemonEvent::Failed(_)) {
            self.as_mut().set_engine_starting(false);
            self.as_mut().set_engine_start_note(QString::default());
        }
        match event {
            DaemonEvent::Starting { slow } => {
                self.as_mut().set_engine_starting(true);
                let note = if slow { ww::text_connecting_slow() } else { String::new() };
                self.as_mut().set_engine_start_note(QString::from(&note));
                self.as_mut().connecting();
            }
            DaemonEvent::Ready(status) => {
                if we_socket::is_open() {
                    // Сокет уже открыт (страницу открыли второй раз в том же
                    // процессе): сигнала `connected` не будет, спрашиваем сразу.
                    self.as_mut().connecting();
                    we_socket::request_health(ww::HEALTH_TIMEOUT_MS);
                } else {
                    we_socket::open(i32::try_from(status.ws_port).unwrap_or(0));
                }
            }
            DaemonEvent::Failed(reason) => {
                self.as_mut().daemon_failed(QString::from(&reason));
            }
            DaemonEvent::Connected => {
                self.as_mut().connecting();
                we_socket::request_health(ww::HEALTH_TIMEOUT_MS);
            }
            DaemonEvent::Health(state) => {
                self.as_mut().health_received(QString::from(&state));
            }
            DaemonEvent::HealthTimeout => self.as_mut().health_timed_out(),
            // Сообщения человеку показывает App тостом (`errorOccurred`).
            DaemonEvent::Notice(_) => {}
            // До первого соединения экран ожидания и так на месте.
            DaemonEvent::Disconnected { was_connected } => {
                if was_connected {
                    self.as_mut().connecting();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// Разметка вкладки — единственный источник договора.
    const PAGE_QML: &str = include_str!("../qml/WallpaperEnginePage.qml");

    /// Имена, которые страница спрашивает у объекта действий.
    fn asked_methods(qml: &str) -> Vec<String> {
        const CALL: &str = "root.actions.";
        let mut found: Vec<String> = qml
            .match_indices(CALL)
            .map(|(at, _)| {
                qml[at + CALL.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect::<String>()
            })
            .filter(|name| !name.is_empty())
            .collect();
        found.sort();
        found.dedup();
        found
    }

    /// Сторож договора: страница обязана спрашивать РОВНО то, что мост умеет.
    ///
    /// Стоит он не для красоты. Расхождение здесь невидимо целиком: и сборка,
    /// и `cargo test` зелёные — QML разбирает вызов только в работе, и
    /// незнакомый метод оборачивается строкой в журнале да мёртвой кнопкой у
    /// покупателя. Появился у страницы второй запрос — либо он дописывается
    /// сюда, либо этот сторож краснеет.
    #[test]
    fn page_asks_only_what_the_bridge_provides() {
        assert_eq!(
            asked_methods(PAGE_QML),
            vec!["retryDaemon".to_string()],
            "договор страницы «Wallpaper Engine» с объектом действий разошёлся \
             с мостом src/we_actions.rs: допишите недостающий #[qinvokable] \
             (и уберите лишний), иначе вызов молча умрёт в работе"
        );
    }

    /// Обратная половина того же договора: сигнал моста обязан кто-то слушать.
    ///
    /// Без обработчика `retryStarted` кнопка «Повторить запуск» сработала бы,
    /// но экран «подключение…» не появился бы вовсе — вкладка выглядела бы
    /// заевшей на отказе, хотя демон в это время уже поднимается.
    #[test]
    fn page_listens_to_the_retry_signal() {
        assert!(
            PAGE_QML.contains("function onRetryStarted()"),
            "страница «Wallpaper Engine» перестала слушать сигнал retryStarted \
             объекта действий: экран «подключение…» после нажатия кнопки \
             «Повторить запуск» не появится"
        );
    }

    /// Каждый сигнал демона обязан доходить до модели страницы.
    ///
    /// Потеряй страница любой из обработчиков — и вкладка застрянет на одном
    /// экране: без `onHealthReceived` навсегда на «подключение…» (ровно тот
    /// дефект, ради которого сшивка и переехала сюда), без `onDaemonFailed`
    /// — на нём же вместо объяснения.
    #[test]
    fn page_listens_to_every_daemon_signal() {
        for handler in [
            "function onConnecting()",
            "function onDaemonFailed(reason)",
            "function onHealthReceived(state)",
            "function onHealthTimedOut()",
        ] {
            assert!(
                PAGE_QML.contains(handler),
                "страница «Wallpaper Engine» не слушает `{handler}` объекта действий"
            );
        }
    }

    /// Общий сторож экрана ожидания обязан стоять, пока идёт запуск движка,
    /// а страница — показывать пояснение долгого запуска. Иначе первые же
    /// 25 с долгого (но честного) первого запуска AppImage кончались бы
    /// отказом — дефект 30.10.2026 (r1895), только другим таймером.
    #[test]
    fn page_pauses_screen_watchdog_while_engine_starts() {
        assert!(
            PAGE_QML.contains("!ownActions.engineStarting"),
            "сторож экрана ожидания не смотрит на engineStarting"
        );
        assert!(
            PAGE_QML.contains("ownActions.engineStartNote"),
            "страница не показывает пояснение долгого запуска"
        );
    }

    /// Страница сама зовёт демона при открытии — но только СВОИМ объектом.
    ///
    /// Чужой объект действий (питоновский хозяин) ведёт демона сам, и второй
    /// ведущий открыл бы второй сокет. Вызов через `root.actions` сюда не
    /// годится ещё и потому, что у чужого объекта такого метода нет.
    #[test]
    fn page_connects_daemon_only_with_own_actions() {
        assert!(
            PAGE_QML.contains("if (root.actions === ownActions)")
                && PAGE_QML.contains("ownActions.connectDaemon()"),
            "страница «Wallpaper Engine» не зовёт демона при открытии: вкладка \
             останется на «подключение…» навсегда"
        );
    }
}
