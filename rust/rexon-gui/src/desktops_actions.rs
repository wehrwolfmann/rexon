// DesktopsActions — объект побочных действий вкладки «Рабочие столы» на Rust.
//
// ЗАЧЕМ. `DesktopsPage.qml` ждёт свойством `actions` объект с девятью
// методами и одним сигналом. Пока такого объекта на Rust не было, хозяин окна
// (`main_window.rs`) не мог отдать разделу «Рабочие столы» его собственную
// страницу: таблица `SECTION_PAGES` показывала в нём подраздел «Сравнение DE»,
// потому что страница столов без объекта действий отдала бы мёртвые кнопки и
// пустую строку версии. Здесь этот объект появляется, и раздел получает свою
// страницу.
//
// ГРАНИЦА ПРОВЕДЕНА ПО ОДНОМУ ПРИЗНАКУ — что действие ДЕЛАЕТ с машиной:
//
//   * читает и водит по интерфейсу → исполняет Rust:
//       - `requestVersion(deId)` — версия пакета из репозиториев дистрибутива
//         (`system_plan::repo_version_query` + `parse_repo_version`, запуск
//         через `utils::run`), в фоне, ответ сигналом `versionReady`;
//       - `openCompare()` — переход во встроенное «Сравнение DE»; страницами
//         переключает хозяин, поэтому объект только объявляет о просьбе
//         сигналом `compareRequested`.
//
//   * ставит и чинит рабочий стол → исполняет ЯДРО
//     (`rexon_core::system_de_exec`, порт `install_de`):
//       - `repairCurrent` — «Проверить мой стол»;
//       - `repair` — «Проверить / Восстановить» на карточке;
//       - `install` — «Установить» на карточке;
//       - `switchTo` — «Переключиться» (копии, выход из сессии);
//       - `remove` — «Удалить»: три защиты — кнопки нет у текущего стола,
//         `qt_desktops::remove_plan` (не текущий и не последний), и
//         `remove_de(force=false)` в ядре, которое откажет и прямому вызову.
//     Порядок как у Python (`DesktopsActions` + `confirm_de_install` +
//     `run_de_operation`), но без вложенных модальных окон: один
//     мастер-диалог на операцию (правило #16) — карта `operation`, строки
//     журнала сигналом `operationLog`, итог — сигнал `desktopsChanged`.
//     Отказ дистрибутива показывается ДО запроса пароля. Ход дела идёт в
//     рабочем потоке; отмена — только пока система не тронута.
//
//   * применяет и возвращает образы «Как в дистрибутиве» → исполняет ЯДРО
//     (`rexon_core::distro_looks`: `offer_live`, `apply_live`,
//     `revert_live`), тексты и решения окна — `qt_desktops::look_*`:
//       - `lookApply` — план в фоне, вопрос (галочка «по желанию» —
//         `setOperationOption`), ход и итог тем же окном операции;
//       - `lookRevert` — «Вернуть как было».
//     Программы из официальных репозиториев ставятся окном пароля, AUR —
//     никогда: такие пакеты называются человеку списком.
//
//   * снимает и возвращает копию панели KDE → исполняет Rust:
//       - `panelBackup` / `panelRestore`.
//     Исполнитель `PlasmaProtector` перенесён в ядро (`backup::
//     plasma_backup_run`, `plasma_restore_run`, `plasma_restart_shell`), и
//     зовётся ТЕМ ЖЕ вызовом, что у «Настроек» (`settings_actions::
//     panel_backup_now` / `panel_restore_now`) — второй копии в продукте нет.
//     Вопрос перед возвратом и тексты итога — как у Python-вкладки.
//
// ⚠️ МОЛЧАЛИВОГО «ничего не произошло» здесь быть не должно (правило #13):
// покупатель принял бы такую кнопку за поломку. Поэтому каждый неперенесённый
// метод возвращает `false` И пишет в `status` строку, которую окно показывает
// в подвале. Этот же объект стоит и за «Сравнением DE»: у страницы
// сравнения те же `install`/`switchTo`, и второго объекта действий с
// заглушкой больше нет.
//
// Приём фоновой работы повторён за `ExtensionsActions` дословно: поток,
// `qt_thread()`, доставка итога сигналом. `pacman -Si` занимает секунды, и
// синхронный запрос остановил бы вкладку ровно там, где она рисует карточку.

/// Мост cxx-qt объекта действий.
///
/// Тип объявлен `#[qml_element]`, поэтому хозяин окна заводит его сам
/// (`DesktopsActions { }`) и подставляет странице свойством `actions`.
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
        /// Чем кончилось последнее побочное действие. Окно показывает эту
        /// строку в подвале: без неё нажатие кнопки, за которой исполнителя
        /// ещё нет, выглядело бы как поломка.
        #[qproperty(QString, status, READ = status, NOTIFY = status_changed)]
        /// Окно операции над столом: вопрос, отказ, ход дела или итог.
        /// Карта с `phase` (`""` — окна нет, `blocked`, `confirm`,
        /// `password`, `running`, `done`) и всеми подписями — их даёт ядро.
        #[qproperty(QMap_QString_QVariant, operation, READ = operation, NOTIFY = operation_changed)]
        type DesktopsActions = super::DesktopsActionsRust;
    }

    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "statusChanged"]
        fn status_changed(self: Pin<&mut DesktopsActions>);

        #[qsignal]
        #[cxx_name = "operationChanged"]
        fn operation_changed(self: Pin<&mut DesktopsActions>);

        /// Строка журнала хода дела — дописывается окном, а не пересылается
        /// целиком картой: у установки стола их тысячи.
        #[qsignal]
        #[cxx_name = "operationLog"]
        fn operation_log(self: Pin<&mut DesktopsActions>, line: QString);

        /// Состав установленных столов мог измениться — страница пересчитает
        /// себя. Операции теперь идут в фоне, и синхронного «получилось ли»
        /// у методов больше нет.
        #[qsignal]
        #[cxx_name = "desktopsChanged"]
        fn desktops_changed(self: Pin<&mut DesktopsActions>);

        /// Ответ на `requestVersion`: `(deId, версия)`. Пустая версия значит
        /// «пакета в репозиториях нет либо спросить не вышло» — страница
        /// тихо прячет строку, а не пугает ошибкой.
        #[qsignal]
        #[cxx_name = "versionReady"]
        fn version_ready(self: Pin<&mut DesktopsActions>, de_id: QString, version: QString);

        /// Ответ на `requestInstalledVersion`: `(deId, готовая строка)`;
        /// пустая — версию узнать не вышло, строка скрыта.
        #[qsignal]
        #[cxx_name = "installedVersionReady"]
        fn installed_version_ready(self: Pin<&mut DesktopsActions>, de_id: QString, line: QString);

        /// Ответ на `requestPackages`: `(deId, установлен ли, строка про
        /// пакеты, предупреждение, набор не встанет из-за конфликта)`.
        /// Пустая строка — спросить не вышло, и страница остаётся при своём
        /// счёте имён. `blocked` — кнопки, ставящие набор, неактивны
        /// (`qd::PackagesView::install_blocked`).
        #[qsignal]
        #[cxx_name = "packagesReady"]
        fn packages_ready(
            self: Pin<&mut DesktopsActions>,
            de_id: QString,
            installed: bool,
            text: QString,
            notice: QString,
            blocked: bool,
        );

        /// Вторая половина ответа на `requestPackages`: пакет набора ждёт
        /// пересборки против новой библиотеки, и его можно собрать из
        /// исходников Arch — `(deId, установлен ли, подпись кнопки,
        /// пояснение)`. Пустая подпись — предлагать нечего.
        #[qsignal]
        #[cxx_name = "rebuildOfferReady"]
        fn rebuild_offer_ready(
            self: Pin<&mut DesktopsActions>,
            de_id: QString,
            installed: bool,
            button: QString,
            hint: QString,
        );

        /// Просьба показать встроенное «Сравнение DE». Сравнение — вторая
        /// страница раздела, и переключает их хозяин: сам объект действий о
        /// том, как устроено окно, не знает.
        #[qsignal]
        #[cxx_name = "compareRequested"]
        fn compare_requested(self: Pin<&mut DesktopsActions>);
    }

    extern "RustQt" {
        fn status(self: &DesktopsActions) -> QString;
        fn operation(self: &DesktopsActions) -> QMap_QString_QVariant;
    }

    extern "RustQt" {
        /// «Да» в окне вопроса — запустить операцию.
        #[qinvokable]
        #[cxx_name = "confirmOperation"]
        fn confirm_operation(self: Pin<&mut DesktopsActions>);

        /// «Нет», «Понятно», «Закрыть» — убрать окно (пока операция идёт,
        /// не действует).
        #[qinvokable]
        #[cxx_name = "dismissOperation"]
        fn dismiss_operation(self: Pin<&mut DesktopsActions>);

        /// «Отменить» — только пока система не тронута.
        #[qinvokable]
        #[cxx_name = "cancelOperation"]
        fn cancel_operation(self: Pin<&mut DesktopsActions>);

        /// Шаг пароля (SteamOS, у «deck» пароля нет): задать пароль из двух
        /// полей и продолжить операцию, что ждёт «Да».
        #[qinvokable]
        #[cxx_name = "submitPassword"]
        fn submit_password(self: Pin<&mut DesktopsActions>, first: &QString, second: &QString);
    }

    extern "RustQt" {
        /// Установка стола. Подписи — те же, что у Python-класса
        /// `DesktopsActions`, и это условие проверки: `DesktopsPage.qml` не
        /// должен измениться ни на строку.
        #[qinvokable]
        fn install(self: Pin<&mut DesktopsActions>, de_id: &QString) -> bool;

        /// Проверка и восстановление стола.
        #[qinvokable]
        fn repair(self: Pin<&mut DesktopsActions>, de_id: &QString) -> bool;

        /// Переключение на стол. Имя не `switch`: в JavaScript это ключевое
        /// слово, и вызвать такой метод из QML нельзя.
        #[qinvokable]
        #[cxx_name = "switchTo"]
        fn switch_to(self: Pin<&mut DesktopsActions>, de_id: &QString) -> bool;

        /// Удаление стола (обе защиты — в ядре, `qt_desktops::remove_plan`).
        #[qinvokable]
        fn remove(self: Pin<&mut DesktopsActions>, de_id: &QString) -> bool;

        /// «Проверить мой стол» — восстановление ТЕКУЩЕГО стола.
        #[qinvokable]
        #[cxx_name = "repairCurrent"]
        fn repair_current(self: Pin<&mut DesktopsActions>) -> bool;

        /// Снять копию настроек панели KDE.
        #[qinvokable]
        #[cxx_name = "panelBackup"]
        fn panel_backup(self: Pin<&mut DesktopsActions>) -> bool;

        /// Вернуть панель KDE из последней снятой копии.
        #[qinvokable]
        #[cxx_name = "panelRestore"]
        fn panel_restore(self: Pin<&mut DesktopsActions>) -> bool;

        /// Показать встроенное «Сравнение DE».
        #[qinvokable]
        #[cxx_name = "openCompare"]
        fn open_compare(self: Pin<&mut DesktopsActions>);

        /// Блок «Оболочки»: поставить оболочку `shellId` в композитор `deId`
        /// (вопрос с предупреждениями, затем пакеты и строка автозапуска).
        #[qinvokable]
        #[cxx_name = "shellInstall"]
        fn shell_install(self: Pin<&mut DesktopsActions>, de_id: &QString, shell_id: &QString) -> bool;

        /// Войти в композитор с оболочкой (выход из сеанса).
        #[qinvokable]
        #[cxx_name = "shellSwitch"]
        fn shell_switch(self: Pin<&mut DesktopsActions>, de_id: &QString, shell_id: &QString) -> bool;

        /// Убрать оболочку из композитора; сам стол остаётся.
        #[qinvokable]
        #[cxx_name = "shellRemove"]
        fn shell_remove(self: Pin<&mut DesktopsActions>, de_id: &QString, shell_id: &QString) -> bool;

        /// Спросить версию представительного пакета стола в репозиториях.
        /// Ответ приходит сигналом `versionReady`.
        #[qinvokable]
        #[cxx_name = "requestVersion"]
        fn request_version(self: Pin<&mut DesktopsActions>, de_id: &QString);

        /// Версия установленного стола (тот же ключевой пакет, что у
        /// «Доступно…») и, если в репозитории другая, — она. Ответ приходит
        /// сигналом `installedVersionReady`.
        #[qinvokable]
        #[cxx_name = "requestInstalledVersion"]
        fn request_installed_version(self: Pin<&mut DesktopsActions>, de_id: &QString);

        /// Сколько пакетов РЕАЛЬНО поставится (группы раскрыты, зависимости
        /// взяты) и не сломан ли набор в репозитории — пробной транзакцией
        /// без прав root. Ответ приходит сигналом `packagesReady`.
        #[qinvokable]
        #[cxx_name = "requestPackages"]
        fn request_packages(self: Pin<&mut DesktopsActions>, de_id: &QString, installed: bool);

        /// «Собрать заново из исходников Arch» под предупреждением о пакете,
        /// который ждёт пересборки: вопрос, затем сборка и установка тем же
        /// окном операции (`repo_rebuild::rebuild_live`).
        #[qinvokable]
        #[cxx_name = "rebuildFromSources"]
        fn rebuild_from_sources(self: Pin<&mut DesktopsActions>, de_id: &QString);

        /// Блок «Как в дистрибутиве»: «Применить» образ `look_id`. Сначала
        /// в фоне собирается план (пробы пакетов — секунды), потом — вопрос
        /// в окне операции, применение и итог.
        #[qinvokable]
        #[cxx_name = "lookApply"]
        fn look_apply(self: Pin<&mut DesktopsActions>, look_id: &QString);

        /// «Вернуть как было» для образа `look_id`: вопрос, ход и итог.
        #[qinvokable]
        #[cxx_name = "lookRevert"]
        fn look_revert(self: Pin<&mut DesktopsActions>, look_id: &QString);

        /// Галочка «по желанию» в окне вопроса (игровой режим SteamOS)
        /// переключена: вопрос пересобирается по другому плану.
        #[qinvokable]
        #[cxx_name = "setOperationOption"]
        fn set_operation_option(self: Pin<&mut DesktopsActions>, checked: bool);

        /// Переключатель варианта в окне вопроса (стиль SteamOS: Vapor или
        /// VGUI2) — свойство `name` образа получило значение `value`:
        /// вопрос пересобирается по другому плану.
        #[qinvokable]
        #[cxx_name = "setOperationChoice"]
        fn set_operation_choice(self: Pin<&mut DesktopsActions>, name: &QString, value: &QString);
    }

    // Объявление трейта обязано быть В МОСТЕ: cxx-qt печатает по нему алиас
    // типа потока, без которого фоновый запрос не доедет до сигнала.
    impl cxx_qt::Threading for DesktopsActions {}
}

use core::pin::Pin;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use cxx_qt::{CxxQtThread, CxxQtType, Threading, ThreadingQueueError};
use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};

use rexon_core::de_progress as dp;
use rexon_core::distro_looks as dl;
use rexon_core::qt_compare as qc;
use rexon_core::qt_desktops as qd;
use rexon_core::system;
use rexon_core::system_de_exec as dx;
use rexon_core::shell_bundle as sb;
use rexon_core::system::DesktopShell;
use rexon_core::deepin_treeland as dt;
use rexon_core::repo_rebuild;
use rexon_core::steamos_sysext as sx;
use rexon_core::system_plan;
use rexon_core::utils;

/// Rust-состояние объекта действий.
///
/// Кэш версий живёт на сессию — ровно как у Python: второй раз за той же
/// версией пакетный менеджер не дёргается. Рядом список запросов в пути:
/// страница уже бережётся флажком `versionPending`, но он один на карточку, а
/// сюда приходят и со сменой стола, и с появлением объекта действий.
#[derive(Default)]
pub struct DesktopsActionsRust {
    status: String,
    versions: Arc<Mutex<HashMap<String, String>>>,
    in_flight: Arc<Mutex<HashSet<String>>>,
    /// Предпросмотр установки по столу: `None` внутри — спросить не вышло.
    /// Сбрасывается после каждой операции: состав системы поменялся.
    previews: Arc<Mutex<HashMap<String, Option<system_plan::InstallPreview>>>>,
    previews_in_flight: Arc<Mutex<HashSet<String>>>,
    /// Пакеты стола, которые ждут пересборки и собираются из исходников
    /// Arch (`repo_rebuild::find_candidates`). Живут и сбрасываются вместе
    /// с предпросмотром.
    rebuild_offers: Arc<Mutex<HashMap<String, Vec<repo_rebuild::Candidate>>>>,
    /// Deepin на Treeland (`deepin_treeland::card_state`): что предложить
    /// на карточке Deepin той же кнопкой под предупреждением. Живёт и
    /// сбрасывается вместе с предпросмотром.
    treeland_offers: Arc<Mutex<HashMap<String, dt::Offer>>>,
    /// Конфликт набора снимается заменой с согласия человека
    /// (`system_plan::conflict_swaps`, River 0.5 → river-classic): что
    /// заменить. Та же кнопка под предупреждением; живёт и сбрасывается
    /// вместе с предпросмотром.
    swap_offers: Arc<Mutex<HashMap<String, Vec<system_plan::PkgConflict>>>>,
    op: OpView,
    pending: Option<PendingOp>,
    cancel: Option<dx::CancelToken>,
}

/// Что показывает окно операции. Все подписи — из ядра.
///
/// Общее у всех владельцев окна `DeOperationDialog.qml` — «Рабочих столов»,
/// «Сравнения» и «Настроек» (доустановка недостающего): разбор хода и итога
/// один, чтобы окна не разъехались поведением.
#[derive(Default, Clone)]
pub(crate) struct OpView {
    pub(crate) phase: String,
    pub(crate) title: String,
    pub(crate) text: String,
    pub(crate) informative: String,
    pub(crate) status: String,
    pub(crate) bar: i32,
    pub(crate) current: String,
    pub(crate) close_text: String,
    pub(crate) close_enabled: bool,
    pub(crate) cancellable: bool,
    pub(crate) result_text: String,
    /// Необратимое действие: «Да» красным.
    pub(crate) danger: bool,
    /// Своя подпись «Да»/«Нет» вопроса (пусто — «Да»/«Нет»): у восстановления
    /// оформления это «🛠 Восстановить» и «Не сейчас», как у Python-окна.
    pub(crate) yes_text: String,
    pub(crate) no_text: String,
    /// Шаг пароля: почему пароль не принят (пусто — всё хорошо).
    pub(crate) password_error: String,
    /// Шаг пароля: `passwd` работает — поля и кнопки гаснут.
    pub(crate) password_busy: bool,
    /// Галочка «по желанию» в окне вопроса (пусто — галочки нет) и
    /// предупреждение под ней (простой HTML).
    pub(crate) option_text: String,
    pub(crate) option_hint: String,
    pub(crate) option_checked: bool,
    /// Переключатели вариантов окна вопроса (пусто — их нет).
    pub(crate) choices: Vec<qd::LookSelect>,
}

impl OpView {
    /// Окно хода только что начатой операции.
    pub(crate) fn running(title: &str) -> Self {
        OpView {
            phase: "running".to_string(),
            title: title.to_string(),
            status: dp::initial_status(),
            close_text: dp::close_text_busy(),
            ..OpView::default()
        }
    }

    /// Окно вопроса.
    pub(crate) fn confirm(title: &str, text: &str) -> Self {
        OpView {
            phase: "confirm".to_string(),
            title: title.to_string(),
            text: html(text),
            ..OpView::default()
        }
    }

    /// Шаг пароля SteamOS: у «deck» пароля нет, без него `pkexec` не пройти.
    /// «Да» — «Задать пароль», «Нет» — «Отмена».
    pub(crate) fn password(device: sx::Device) -> Self {
        OpView {
            phase: "password".to_string(),
            title: sx::password_title(),
            text: html(&sx::password_text(device)),
            yes_text: sx::password_button(),
            no_text: dx::dialog_cancel(),
            ..OpView::default()
        }
    }

    /// Карта для разметки: состояние и подписи кнопок.
    pub(crate) fn to_map(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut m = QMap::<QMapPair_QString_QVariant>::default();
        let s = |v: &str| QVariant::from(&QString::from(v));
        m.insert(QString::from("phase"), s(&self.phase));
        m.insert(QString::from("title"), s(&self.title));
        m.insert(QString::from("text"), s(&self.text));
        m.insert(QString::from("informative"), s(&self.informative));
        m.insert(QString::from("status"), s(&self.status));
        m.insert(QString::from("bar"), QVariant::from(&self.bar));
        m.insert(QString::from("barMax"), QVariant::from(&dp::BAR_MAX));
        m.insert(QString::from("current"), s(&self.current));
        m.insert(QString::from("closeText"), s(&self.close_text));
        m.insert(QString::from("closeEnabled"), QVariant::from(&self.close_enabled));
        m.insert(QString::from("cancellable"), QVariant::from(&self.cancellable));
        m.insert(QString::from("resultText"), s(&self.result_text));
        m.insert(QString::from("danger"), QVariant::from(&self.danger));
        let or = |own: &str, fallback: String| {
            if own.is_empty() {
                fallback
            } else {
                own.to_string()
            }
        };
        m.insert(QString::from("yesText"), s(&or(&self.yes_text, dx::dialog_yes())));
        m.insert(QString::from("noText"), s(&or(&self.no_text, dx::dialog_no())));
        m.insert(QString::from("okText"), s(&dp::confirm_blocked_button()));
        m.insert(QString::from("cancelText"), s(&dx::dialog_cancel()));
        if self.phase == "password" {
            m.insert(QString::from("passwordNewText"), s(&sx::password_field_new()));
            m.insert(QString::from("passwordRepeatText"), s(&sx::password_field_repeat()));
        }
        m.insert(QString::from("passwordError"), s(&self.password_error));
        m.insert(QString::from("passwordBusy"), QVariant::from(&self.password_busy));
        m.insert(QString::from("optionText"), s(&self.option_text));
        m.insert(QString::from("optionHint"), s(&self.option_hint));
        m.insert(QString::from("optionChecked"), QVariant::from(&self.option_checked));
        let mut choices = QList::<QVariant>::default();
        for c in &self.choices {
            let mut options = QList::<QVariant>::default();
            for o in &c.options {
                let mut om = QMap::<QMapPair_QString_QVariant>::default();
                om.insert(QString::from("value"), s(&o.value));
                om.insert(QString::from("label"), s(&o.label));
                options.append(QVariant::from(&om));
            }
            let mut cm = QMap::<QMapPair_QString_QVariant>::default();
            cm.insert(QString::from("name"), s(&c.name));
            cm.insert(QString::from("label"), s(&c.label));
            cm.insert(QString::from("current"), s(&c.current));
            cm.insert(QString::from("options"), QVariant::from(&options));
            choices.append(QVariant::from(&cm));
        }
        m.insert(QString::from("choices"), QVariant::from(&choices));
        m
    }

    /// Сообщение хода дела. `false` — окно уже не в ходе, менять нечего;
    /// иначе строка для журнала окна, если она есть.
    pub(crate) fn apply_progress(
        &mut self,
        stage: &str,
        message: &str,
        fraction: Option<f64>,
    ) -> Option<Option<String>> {
        if self.phase != "running" {
            return None;
        }
        let update = dp::on_progress(stage, message, fraction);
        if let Some(bar) = update.bar {
            self.bar = bar;
        }
        if let Some(status) = update.status {
            self.status = status.to_string();
        }
        if let Some(current) = update.current {
            self.current = current;
        }
        Some(update.log_line)
    }

    /// Итог операции. `result` — приписка под журналом при удаче. Отдаёт
    /// строку для журнала окна, если она есть.
    pub(crate) fn apply_done(&mut self, outcome: &dx::DeOpOutcome, result: String) -> Option<String> {
        let done = dp::on_done(outcome.ok, &outcome.message);
        self.phase = "done".to_string();
        if let Some(bar) = done.bar {
            self.bar = bar;
        }
        self.status = if outcome.cancelled {
            dx::status_cancelled()
        } else {
            done.status
        };
        if let Some(current) = done.current {
            self.current = current;
        }
        self.close_text = done.close_text;
        self.close_enabled = done.close_enabled;
        self.cancellable = false;
        if outcome.ok {
            self.result_text = result;
        }
        done.log_line
    }
}

/// Операция, ждущая «Да».
#[derive(Clone)]
enum PendingOp {
    /// Установка стола — она же проверка и восстановление.
    Install {
        de_id: String,
        /// Установленные пакеты, которые человек согласился заменить
        /// (пусто — обычная установка).
        replace: Vec<String>,
        operation_title: String,
        done_title: String,
        done_text: String,
        /// Итог, если исполнитель скажет «нужна перезагрузка» (пусто — тот же).
        done_text_restart: String,
    },
    /// Переключение на другой стол с выходом из сессии.
    Switch {
        target: String,
        operation_title: String,
    },
    /// Удаление стола (защиты уже пройдены, ядро проверит ещё раз).
    Remove {
        de_id: String,
        operation_title: String,
    },
    /// Сборка пакетов, ждущих пересборки, из исходников Arch.
    Rebuild {
        packages: Vec<String>,
        operation_title: String,
    },
    /// Deepin на Treeland: перевести установленный обычный Deepin (X11) на
    /// Treeland (`update == false`) или обновить свою сборку deepin-session.
    Treeland {
        update: bool,
        operation_title: String,
    },
    /// Связка «композитор + оболочка»: установка, вход, снятие.
    Shell {
        job: ShellJob,
        wm: String,
        shell: DesktopShell,
        operation_title: String,
        /// Приписка под журналом при удаче (простой HTML).
        result: String,
    },
    /// «Применить» образ «Как в дистрибутиве»: предложение ядра и выбор
    /// галочки «по желанию»; стол сеанса снят при нажатии.
    Look {
        offer: Box<dl::Offer>,
        /// Выбранные варианты (пусто — по умолчанию).
        values: rexon_core::rxn_manifest::PropValues,
        opt_in: bool,
        de: String,
        operation_title: String,
    },
    /// «Вернуть как было» образа.
    LookRevert {
        id: String,
        de: String,
        operation_title: String,
    },
}

/// Что сделать с оболочкой.
#[derive(Clone, Copy)]
enum ShellJob {
    Install,
    Switch,
    Remove,
}

/// Что исполняет рабочий поток.
enum Job {
    Install(String, Vec<String>),
    Switch(String),
    Remove(String),
    Rebuild(Vec<String>),
    Treeland { update: bool },
    Shell(ShellJob, String, DesktopShell),
    /// Образ, план (`None` — применять нечего), что вернуть первым, стол.
    LookApply {
        look: Box<dl::Look>,
        plan: Option<Box<dl::LookPlan>>,
        replaces: Vec<(String, String)>,
        de: String,
    },
    LookRevert {
        id: String,
        de: String,
    },
}

/// Простой HTML ядра для окна: переносы строк — `<br>` (в `RichText` голый
/// перевод строки — пробел).
fn html(text: &str) -> String {
    text.replace('\n', "<br>")
}

impl qobject::DesktopsActions {
    fn status(&self) -> QString {
        QString::from(&self.rust().status)
    }

    fn set_status(mut self: Pin<&mut Self>, text: String) {
        if self.rust().status == text {
            return;
        }
        self.as_mut().rust_mut().status = text;
        self.as_mut().status_changed();
    }

    // ── Читающее и навигационное: исполняет Rust ─────────────────────────

    fn open_compare(mut self: Pin<&mut Self>) {
        self.as_mut().compare_requested();
    }

    fn request_version(mut self: Pin<&mut Self>, de_id: &QString) {
        let de_id = de_id.to_string();
        if de_id.is_empty() {
            return;
        }

        // Уже спрашивали — ответ отдаём немедленно, тем же сигналом. Пустая
        // строка в кэше тоже ответ: «пакета нет», и спрашивать снова незачем.
        let cached = self
            .as_ref()
            .get_ref()
            .rust()
            .versions
            .lock()
            .ok()
            .and_then(|map| map.get(&de_id).cloned());
        if let Some(version) = cached {
            self.as_mut()
                .version_ready(QString::from(&de_id), QString::from(&version));
            return;
        }

        {
            let Ok(mut flight) = self.as_ref().get_ref().rust().in_flight.lock() else {
                return;
            };
            if !flight.insert(de_id.clone()) {
                return;
            }
        }

        let versions = self.as_ref().get_ref().rust().versions.clone();
        let in_flight = self.as_ref().get_ref().rust().in_flight.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let version = repo_version(&de_id).unwrap_or_default();
            if let Ok(mut map) = versions.lock() {
                map.insert(de_id.clone(), version.clone());
            }
            if let Ok(mut flight) = in_flight.lock() {
                flight.remove(&de_id);
            }
            let queued = handle.queue(move |mut object| {
                object
                    .as_mut()
                    .version_ready(QString::from(&de_id), QString::from(&version));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    // Молчать нельзя: без ответа страница осталась бы с
                    // поднятым `versionPending` и не спросила бы больше
                    // никогда.
                    rexon_core::log_warn!("desktops", "версия стола не доставлена странице: {e:?}");
                }
            }
        });
    }

    fn request_installed_version(mut self: Pin<&mut Self>, de_id: &QString) {
        let de_id = de_id.to_string();
        if de_id.is_empty() {
            return;
        }
        let versions = self.as_ref().get_ref().rust().versions.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let cached = versions.lock().ok().and_then(|map| map.get(&de_id).cloned());
            let available = match cached {
                Some(v) => v,
                None => {
                    let v = repo_version(&de_id).unwrap_or_default();
                    if let Ok(mut map) = versions.lock() {
                        map.insert(de_id.clone(), v.clone());
                    }
                    v
                }
            };
            let installed = installed_version(&de_id).unwrap_or_default();
            let newer = repo_is_newer(&installed, &available);
            let line = qd::installed_version_line(&installed, &available, newer);
            let queued = handle.queue(move |mut object| {
                object
                    .as_mut()
                    .installed_version_ready(QString::from(&de_id), QString::from(&line));
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("desktops", "версия установленного стола не доставлена странице: {e:?}");
                }
            }
        });
    }

    fn request_packages(mut self: Pin<&mut Self>, de_id: &QString, installed: bool) {
        let de_id = de_id.to_string();
        if de_id.is_empty() {
            return;
        }
        let cached = self
            .as_ref()
            .get_ref()
            .rust()
            .previews
            .lock()
            .ok()
            .and_then(|map| map.get(&de_id).cloned());
        if let Some(preview) = cached {
            let view = packages_view_of(&de_id, installed, preview.as_ref());
            self.as_mut().packages_ready(
                QString::from(&de_id),
                installed,
                QString::from(&view.text),
                QString::from(&view.notice),
                view.install_blocked,
            );
            let offered = self
                .as_ref()
                .get_ref()
                .rust()
                .rebuild_offers
                .lock()
                .ok()
                .is_some_and(|map| map.get(&de_id).is_some_and(|c| !c.is_empty()));
            let treeland = self
                .as_ref()
                .get_ref()
                .rust()
                .treeland_offers
                .lock()
                .ok()
                .and_then(|map| map.get(&de_id).cloned());
            let swaps = self
                .as_ref()
                .get_ref()
                .rust()
                .swap_offers
                .lock()
                .ok()
                .and_then(|map| map.get(&de_id).cloned())
                .unwrap_or_default();
            let (button, hint) = offer_texts(treeland.as_ref(), &swaps, offered);
            self.as_mut().rebuild_offer_ready(
                QString::from(&de_id),
                installed,
                QString::from(&button),
                QString::from(&hint),
            );
            return;
        }
        {
            let Ok(mut flight) = self.as_ref().get_ref().rust().previews_in_flight.lock() else {
                return;
            };
            if !flight.insert(de_id.clone()) {
                return;
            }
        }
        let previews = self.as_ref().get_ref().rust().previews.clone();
        let offers = self.as_ref().get_ref().rust().rebuild_offers.clone();
        let treeland_offers = self.as_ref().get_ref().rust().treeland_offers.clone();
        let swap_offers = self.as_ref().get_ref().rust().swap_offers.clone();
        let in_flight = self.as_ref().get_ref().rust().previews_in_flight.clone();
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let family = system::DistroDetector::detect_live().family_live();
            let packages = system::get_packages(&de_id, family);
            // Проба обычного набора. У Deepin её не заменить пробой варианта
            // целиком: `card_state` берёт её запасным путём (с запретом
            // deepin-kwin), если проба Treeland не вышла, а вне Arch и на
            // неизменяемой системе она единственная.
            let preview = system_plan::install_preview_live(family, &packages);
            // Deepin ставится только на Treeland: стоит обычный (X11) —
            // предложить перевести его на Treeland; своя сборка уже стоит —
            // показывать пробу её набора и предложить обновление.
            let treeland = dt::card_state_live(&de_id, family, preview.as_ref());
            let preview = treeland.preview.map(Some).unwrap_or(preview);
            // Пакеты из AUR в наборе стола: один запрос RPC на весь список
            // (признаки риска для строки-предупреждения карточки).
            if !installed {
                aur_trust_warm(&system::get_aur_packages(&de_id, family));
            }
            // Что из набора не ставится, потому что ждёт пересборки против
            // новой библиотеки, — то можно собрать из исходников Arch. При
            // конфликте с установленным пересборка не поможет — не
            // предлагаем (`qd::rebuild_can_help`).
            let broken: Vec<String> = preview
                .as_ref()
                .filter(|p| qd::rebuild_can_help(p))
                .map(|p| p.blocked.iter().chain(&p.skipped).cloned().collect())
                .unwrap_or_default();
            let candidates = repo_rebuild::find_candidates_live(&broken);
            // Конфликт с другой веткой той же программы (River 0.5) —
            // кнопка замены с вопросом.
            let swaps = preview.as_ref().map(system_plan::conflict_swaps).unwrap_or_default();
            let (button, hint) = offer_texts(treeland.offer.as_ref(), &swaps, !candidates.is_empty());
            if let Ok(mut map) = swap_offers.lock() {
                if swaps.is_empty() {
                    map.remove(&de_id);
                } else {
                    map.insert(de_id.clone(), swaps);
                }
            }
            if let Ok(mut map) = offers.lock() {
                map.insert(de_id.clone(), candidates);
            }
            if let Ok(mut map) = treeland_offers.lock() {
                match treeland.offer {
                    Some(offer) => map.insert(de_id.clone(), offer),
                    None => map.remove(&de_id),
                };
            }
            if let Ok(mut map) = previews.lock() {
                map.insert(de_id.clone(), preview.clone());
            }
            if let Ok(mut flight) = in_flight.lock() {
                flight.remove(&de_id);
            }
            let view = packages_view_of(&de_id, installed, preview.as_ref());
            let queued = handle.queue(move |mut object| {
                object.as_mut().packages_ready(
                    QString::from(&de_id),
                    installed,
                    QString::from(&view.text),
                    QString::from(&view.notice),
                    view.install_blocked,
                );
                object.as_mut().rebuild_offer_ready(
                    QString::from(&de_id),
                    installed,
                    QString::from(&button),
                    QString::from(&hint),
                );
            });
            if let Err(e) = queued {
                if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
                    rexon_core::log_warn!("desktops", "состав пакетов стола не доставлен странице: {e:?}");
                }
            }
        });
    }

    // ── Установка, смена, удаление: исполнителя у ядра ещё нет ───────────

    fn install(self: Pin<&mut Self>, de_id: &QString) -> bool {
        // Порт `DesktopsActions.install`: общий вопрос с разбором
        // совместимости (`confirm_de_install`), затем `install_de` в ядре.
        let de_id = de_id.to_string();
        let Some(name) = known_name(&de_id) else {
            self.report_unknown(&de_id);
            return false;
        };
        self.ask_install(&de_id, qd::install_texts(&name), Vec::new());
        false
    }

    fn repair(mut self: Pin<&mut Self>, de_id: &QString) -> bool {
        // Порт `DesktopsActions.repair`: повторная штатная установка —
        // пакетный менеджер с `--needed` сам пропускает стоящее. Ход и итог
        // приходят позже сигналами, поэтому ответ метода всегда `false`.
        let de_id = de_id.to_string();
        let Some(name) = known_name(&de_id) else {
            self.report_unknown(&de_id);
            return false;
        };
        let current = current_de().unwrap_or_default();
        if self.as_mut().refused_on_steamos("repair", &de_id, &current, &qd::repair_texts(&name).operation_title) {
            return false;
        }
        self.ask_install(&de_id, qd::repair_texts(&name), Vec::new());
        false
    }

    /// SteamOS: кнопка, которую карточка показывает неактивной
    /// (`qd::steamos_button_block`), и при прямом вызове не идёт дальше —
    /// окно с той же причиной. `true` — отказано.
    fn refused_on_steamos(mut self: Pin<&mut Self>, kind: &str, de_id: &str, current: &str, operation_title: &str) -> bool {
        let steamos = rexon_core::distro_compat::running_on_steamos();
        let Some(why) = qd::steamos_button_block(kind, de_id, de_id == current, steamos) else {
            return false;
        };
        self.as_mut().set_status(dx::operation_footer(operation_title, &why));
        self.show_notice(operation_title, &why);
        true
    }

    fn switch_to(mut self: Pin<&mut Self>, de_id: &QString) -> bool {
        // Порт `DesktopsActions.switchTo`: вопрос, затем `switch_de` в ядре
        // (копии, приложения по умолчанию, выход из сессии). На текущий стол
        // ядро не переключит — кнопки на нём и так нет.
        let de_id = de_id.to_string();
        let Some(name) = known_name(&de_id) else {
            self.report_unknown(&de_id);
            return false;
        };
        if self.busy() {
            self.set_status(dx::busy_message());
            return false;
        }
        // Тексты переключения общие с «Сравнением DE» — там они дословно те
        // же, и вторая копия разъехалась бы молча.
        let texts = qc::switch_texts(&name);
        self.as_mut().rust_mut().pending = Some(PendingOp::Switch {
            target: de_id,
            operation_title: texts.operation_title.clone(),
        });
        self.as_mut()
            .set_status(dx::awaiting_answer(&texts.operation_title));
        self.set_op(OpView {
            phase: "confirm".to_string(),
            title: texts.confirm_title.to_string(),
            text: html(&texts.confirm_text),
            ..OpView::default()
        });
        false
    }

    fn remove(mut self: Pin<&mut Self>, de_id: &QString) -> bool {
        // Порт `DesktopsActions.remove`. Защиты три, и все решает ядро:
        //   1. кнопки «Удалить» у текущего стола нет (`card_buttons`);
        //   2. `qt_desktops::remove_plan` — не текущий и не последний
        //      установленный, иначе окно с объяснением без кнопки удаления;
        //   3. `remove_de(force=false)` в исполнителе — прямой вызов тоже
        //      откажет.
        let de_id = de_id.to_string();
        let Some(name) = known_name(&de_id) else {
            self.report_unknown(&de_id);
            return false;
        };
        if self.busy() {
            self.set_status(dx::busy_message());
            return false;
        }
        let current = current_de().unwrap_or_default();
        if self.as_mut().refused_on_steamos("remove", &de_id, &current, &qd::remove_operation_title(&name)) {
            return false;
        }
        let others = system::DE_META
            .iter()
            .filter(|m| m.id != de_id && crate::compare_page::de_installed(m.id))
            .count();
        let plan = qd::remove_plan(&de_id, &name, &current, others);
        let operation_title = qd::remove_operation_title(&name);
        if !plan.allowed() {
            self.as_mut().rust_mut().pending = None;
            self.as_mut()
                .set_status(dx::operation_footer(&operation_title, &plan.title));
            self.set_op(OpView {
                phase: "blocked".to_string(),
                title: plan.title.to_string(),
                text: html(&plan.text),
                ..OpView::default()
            });
            return false;
        }
        let text = qd::steamos_remove_text(&de_id, sx::live_release().is_some()).unwrap_or(plan.text);
        self.as_mut().rust_mut().pending = Some(PendingOp::Remove {
            de_id,
            operation_title: operation_title.clone(),
        });
        self.as_mut().set_status(dx::awaiting_answer(&operation_title));
        self.set_op(OpView {
            phase: "confirm".to_string(),
            title: plan.title.to_string(),
            text: html(&text),
            danger: true,
            ..OpView::default()
        });
        false
    }

    fn repair_current(mut self: Pin<&mut Self>) -> bool {
        // Порт `DesktopsActions.repairCurrent`: восстановление ТЕКУЩЕГО
        // стола. Не опознан — окно с объяснением, а не молчание.
        let current = current_de().filter(|de| known_name(de).is_some());
        let Some(current) = current else {
            let (title, text) = (qd::repair_current_unknown_title(), qd::repair_current_unknown_text());
            self.as_mut().set_status(format!("{title} — {text}"));
            self.show_notice(&title, &text);
            return false;
        };
        let texts = qd::repair_texts(qd::display_name(&current));
        if self.as_mut().refused_on_steamos("repair_current", &current, &current, &texts.operation_title) {
            return false;
        }
        self.ask_install(&current, texts, Vec::new());
        false
    }

    // ── Окно операции ────────────────────────────────────────────────────

    fn operation(&self) -> QMap<QMapPair_QString_QVariant> {
        self.rust().op.to_map()
    }

    fn set_op(mut self: Pin<&mut Self>, op: OpView) {
        self.as_mut().rust_mut().op = op;
        self.as_mut().operation_changed();
    }

    fn busy(&self) -> bool {
        self.rust().op.phase == "running"
    }

    /// Окно-сообщение с одной кнопкой «Понятно».
    fn show_notice(self: Pin<&mut Self>, title: &str, text: &str) {
        self.set_op(OpView {
            phase: "blocked".to_string(),
            title: title.to_string(),
            text: html(text),
            ..OpView::default()
        });
    }

    /// Порт `confirm_de_install`: отказ дистрибутива — окном без кнопки
    /// «установить» ДО запроса пароля; предупреждение — в том же вопросе.
    /// `replace` — согласие заменить установленные пакеты (кнопка замены);
    /// тогда «Да» красное: установленное будет снято.
    fn ask_install(mut self: Pin<&mut Self>, de_id: &str, texts: qd::OpTexts, replace: Vec<String>) {
        if self.busy() {
            self.set_status(dx::busy_message());
            return;
        }
        // SteamOS: стол-образ — свой вопрос (сборка из пакетов Valve,
        // пересборка после обновлений), а не пакетный.
        let version = sx::live_release().map(|r| r.version_id);
        let texts = qd::steamos_op_texts(texts, de_id, version.as_deref());
        let verdict = dx::install_verdict_live(de_id);
        match dp::confirm_install_plan(Some(&verdict), &texts.confirm_html) {
            dp::ConfirmPlan::Blocked {
                window_title,
                text,
                informative,
                ..
            } => {
                self.as_mut().rust_mut().pending = None;
                self.as_mut()
                    .set_status(dx::operation_footer(&texts.operation_title, &window_title));
                self.set_op(OpView {
                    phase: "blocked".to_string(),
                    title: window_title,
                    text: html(&text),
                    informative: html(&informative),
                    ..OpView::default()
                });
            }
            dp::ConfirmPlan::Ask { body } => {
                let danger = !replace.is_empty();
                self.as_mut().rust_mut().pending = Some(PendingOp::Install {
                    de_id: de_id.to_string(),
                    replace,
                    operation_title: texts.operation_title.clone(),
                    done_title: texts.done_title.to_string(),
                    done_text: texts.done_text.clone(),
                    done_text_restart: texts.done_text_restart.clone(),
                });
                self.as_mut()
                    .set_status(dx::awaiting_answer(&texts.operation_title));
                self.set_op(OpView {
                    phase: "confirm".to_string(),
                    title: texts.confirm_title.to_string(),
                    text: html(&body),
                    danger,
                    ..OpView::default()
                });
            }
        }
    }

    fn confirm_operation(mut self: Pin<&mut Self>) {
        if self.busy() {
            return;
        }
        let Some(pending) = self.as_mut().rust_mut().pending.take() else {
            return;
        };
        // SteamOS: у «deck» без пароля `pkexec` не пройти — сначала шаг
        // пароля в этом же окне, операция ждёт его в `pending`.
        match steamos_admin_gate(&pending) {
            None | Some(sx::AdminGate::Ready) => {}
            Some(sx::AdminGate::AskPassword) => {
                let title = pending_title(&pending);
                self.as_mut().rust_mut().pending = Some(pending);
                self.as_mut().set_status(dx::awaiting_answer(&title));
                self.set_op(OpView::password(sx::live_device()));
                return;
            }
            Some(sx::AdminGate::Refused(why)) => {
                let title = pending_title(&pending);
                self.as_mut().set_status(dx::operation_footer(&title, &why));
                self.show_notice(&title, &why);
                return;
            }
        }
        self.start_operation(pending);
    }

    /// Запустить операцию, на которую ответили «Да» (и, если надо, задали
    /// пароль): окно хода и рабочий поток.
    fn start_operation(mut self: Pin<&mut Self>, pending: PendingOp) {
        // Итог «перезагрузите» вместо обычного — когда исполнитель по плану
        // установки решил, что без перезагрузки сделанное не заработает
        // (`DeOpOutcome::restart_needed`).
        let mut restart_result: Option<String> = None;
        let (operation_title, result, job) = match pending {
            PendingOp::Install {
                de_id,
                replace,
                operation_title,
                done_title,
                done_text,
                done_text_restart,
            } => {
                if !done_text_restart.is_empty() {
                    restart_result = Some(format!("<b>{done_title}</b><br>{}", html(&done_text_restart)));
                }
                // Пустой итог (стол-образ SteamOS): его пишет исполнитель.
                let result = if done_text.is_empty() {
                    String::new()
                } else {
                    format!("<b>{done_title}</b><br>{}", html(&done_text))
                };
                (operation_title, result, Job::Install(de_id, replace))
            }
            // Успех смены — это выход из сессии; итоговой приписки нет.
            PendingOp::Switch {
                target,
                operation_title,
            } => (operation_title, String::new(), Job::Switch(target)),
            PendingOp::Remove {
                de_id,
                operation_title,
            } => (operation_title, String::new(), Job::Remove(de_id)),
            // Итог (что собрано и что Arch пришлёт свою пересборку) пишет
            // в журнал окно по `DeOpOutcome`; приписка под журналом не нужна.
            PendingOp::Rebuild {
                packages,
                operation_title,
            } => (operation_title, String::new(), Job::Rebuild(packages)),
            PendingOp::Treeland {
                update,
                operation_title,
            } => {
                let result = if update { String::new() } else { html(&dt::install_result_text()) };
                (operation_title, result, Job::Treeland { update })
            }
            PendingOp::Shell {
                job,
                wm,
                shell,
                operation_title,
                result,
            } => (operation_title, result, Job::Shell(job, wm, shell)),
            // Итог (что применено, что пропущено, что поставить из AUR)
            // пишет ядро — приписку собирает рабочий поток.
            PendingOp::Look {
                offer,
                values,
                opt_in,
                de,
                operation_title,
            } => {
                let plan = qd::look_chosen_plan(&offer, &values, opt_in).cloned().map(Box::new);
                let offer = *offer;
                let job = Job::LookApply {
                    look: Box::new(offer.look),
                    plan,
                    replaces: offer.replaces,
                    de,
                };
                (operation_title, String::new(), job)
            }
            PendingOp::LookRevert {
                id,
                de,
                operation_title,
            } => (operation_title, String::new(), Job::LookRevert { id, de }),
        };
        let token = dx::CancelToken::new();
        self.as_mut().rust_mut().cancel = Some(token.clone());
        self.as_mut().set_op(OpView::running(&operation_title));
        let handle = self.as_mut().qt_thread();
        let current = current_de().unwrap_or_else(|| "unknown".to_string());
        std::thread::spawn(move || {
            let sink: Arc<dyn dx::DeOpSink> = Arc::new(QtSink {
                handle: handle.clone(),
            });
            // Итог образа «Как в дистрибутиве» — словами ядра под журналом.
            let mut job_result: Option<String> = None;
            let outcome = match job {
                Job::LookApply {
                    look,
                    plan,
                    replaces,
                    de,
                } => {
                    let report = |stage: &str, message: &str| sink.progress(stage, message, None);
                    let applied = match plan {
                        Some(plan) => dl::apply_live(&look, &plan, &replaces, &de, &report),
                        None => Err(qd::look_nothing_to_apply()),
                    };
                    look_outcome(&*sink, applied.map(|a| a.message), &mut job_result)
                }
                Job::LookRevert { id, de } => {
                    let reverted = dl::revert_live(&id, &de).map(|r| r.message);
                    look_outcome(&*sink, reverted, &mut job_result)
                }
                Job::Install(de_id, replace) if replace.is_empty() => dx::install_de_live(sink, token, &de_id, &current),
                Job::Install(de_id, replace) => dx::install_de_replacing_live(sink, token, &de_id, &current, &replace),
                Job::Switch(target) => dx::switch_de_live(sink, token, &target, &current),
                Job::Remove(de_id) => dx::remove_de_live(sink, token, &de_id, &current),
                Job::Rebuild(packages) => repo_rebuild::rebuild_live(sink, token, &packages),
                Job::Treeland { update: false } => dt::install_live(sink, token, &current),
                Job::Treeland { update: true } => dt::update_live(sink, token),
                Job::Shell(ShellJob::Install, wm, shell) => dx::shell_install_live(sink, token, &wm, shell),
                Job::Shell(ShellJob::Switch, wm, shell) => {
                    dx::shell_switch_live(sink, token, &wm, shell, &current)
                }
                Job::Shell(ShellJob::Remove, wm, shell) => {
                    dx::shell_remove_live(sink, token, &wm, shell, &current)
                }
            };
            let result = match restart_result {
                Some(text) if outcome.restart_needed => text,
                _ => job_result.unwrap_or(result),
            };
            deliver(&handle, move |object| {
                object.finish_operation(outcome, operation_title, result);
            });
        });
    }

    fn finish_operation(mut self: Pin<&mut Self>, outcome: dx::DeOpOutcome, title: String, result: String) {
        let mut op = self.rust().op.clone();
        if let Some(line) = op.apply_done(&outcome, result) {
            self.as_mut().operation_log(QString::from(&line));
        }
        self.as_mut().rust_mut().cancel = None;
        let footer = dx::operation_footer(&title, &op.status);
        self.as_mut().set_op(op);
        self.as_mut().set_status(footer);
        // Состав системы мог поменяться и при неудаче (часть поставилась):
        // прежний предпросмотр больше не правда.
        if let Ok(mut map) = self.as_ref().get_ref().rust().previews.lock() {
            map.clear();
        }
        if let Ok(mut map) = self.as_ref().get_ref().rust().rebuild_offers.lock() {
            map.clear();
        }
        if let Ok(mut map) = self.as_ref().get_ref().rust().treeland_offers.lock() {
            map.clear();
        }
        if let Ok(mut map) = self.as_ref().get_ref().rust().swap_offers.lock() {
            map.clear();
        }
        if outcome.ok {
            self.desktops_changed();
        }
    }

    fn apply_progress(mut self: Pin<&mut Self>, stage: String, message: String, fraction: Option<f64>) {
        let mut op = self.rust().op.clone();
        let Some(log_line) = op.apply_progress(&stage, &message, fraction) else {
            return;
        };
        if let Some(line) = log_line {
            self.as_mut().operation_log(QString::from(&line));
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
        if self.busy() || self.rust().op.password_busy {
            return;
        }
        // Окно вопроса или отказа закрыли — строка подвала «ответьте в
        // открывшемся окне» больше не правда, её надо снять. Итог
        // завершённой операции остаётся: он по-прежнему верен.
        let phase = self.rust().op.phase.clone();
        if phase == "confirm" || phase == "blocked" || phase == "password" {
            self.as_mut().set_status(String::new());
        }
        self.as_mut().rust_mut().pending = None;
        self.set_op(OpView::default());
    }

    fn cancel_operation(mut self: Pin<&mut Self>) {
        if !self.busy() || !self.rust().op.cancellable {
            return;
        }
        if let Some(token) = &self.rust().cancel {
            token.cancel();
        }
        // Кнопка гаснет сразу: второй щелчок ничего не добавит, а исполнитель
        // остановится на ближайшей безопасной точке.
        let mut op = self.rust().op.clone();
        op.cancellable = false;
        self.as_mut().set_op(op);
    }

    fn submit_password(mut self: Pin<&mut Self>, first: &QString, second: &QString) {
        let mut op = self.rust().op.clone();
        if op.phase != "password" || op.password_busy || self.rust().pending.is_none() {
            return;
        }
        let (first, second) = (first.to_string(), second.to_string());
        if let Err(why) = sx::check_new_password(&first, &second) {
            op.password_error = why;
            self.set_op(op);
            return;
        }
        // `passwd` с задержками PAM — секунды; окно ждёт с погашенными
        // полями, закрыть его в это время нельзя.
        op.password_error = String::new();
        op.password_busy = true;
        self.as_mut().set_op(op);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let result = sx::set_password_live(&first);
            drop(first);
            deliver(&handle, move |object| object.password_done(result));
        });
    }

    /// `passwd` ответил: задан — операция идёт дальше; нет — его слова под
    /// полями, можно попробовать другой пароль.
    fn password_done(mut self: Pin<&mut Self>, result: Result<(), String>) {
        let mut op = self.rust().op.clone();
        if op.phase != "password" {
            return;
        }
        op.password_busy = false;
        if let Err(why) = result {
            rexon_core::log_warn!("steamos", "passwd не задал пароль: {why}");
            op.password_error = why;
            self.set_op(op);
            return;
        }
        let Some(pending) = self.as_mut().rust_mut().pending.take() else {
            self.set_op(OpView::default());
            return;
        };
        self.as_mut().start_operation(pending);
        self.as_mut().operation_log(QString::from(&sx::password_set_text()));
    }

    // ── Панель KDE: исполняет ядро, тем же вызовом, что и «Настройки» ────
    //
    // Паритет с Python-вкладкой (`DesktopsActions.panelBackup/panelRestore`
    // в `rexon/qt_desktops/window.py`): копия снимается без вопроса, возврат
    // — только после «Да» на тот же вопрос ядра; итог — тексты ядра
    // `panel_*_result`. Оба метода возвращают `false`: состав установленных
    // столов от панели не меняется, пересчитывать его странице незачем.

    fn panel_backup(self: Pin<&mut Self>) -> bool {
        let data_home = utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref().filter(|v| !v.is_empty()), &std::env::var("HOME").unwrap_or_default());
        let (ok, message) = match current_de().as_deref() {
            // MATE и XFCE держат ОДНУ копию: новая заменяет прежнюю, а у
            // MATE это ещё и копия «Вернуть прежнюю панель» — без спроса не
            // затираем.
            Some(de @ ("mate" | "xfce")) => {
                let exists = if de == "mate" {
                    std::path::Path::new(&rexon_core::mate_panel::PanelCtx::from_env().backup_path()).is_file()
                } else {
                    std::path::Path::new(&rexon_core::backup::xfce_panel_copy_path(&data_home)).is_file()
                };
                if exists {
                    let (title, text) = qd::panel_backup_replace_confirm(de == "mate");
                    if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                        return false;
                    }
                }
                if de == "mate" {
                    rexon_core::mate_panel::save_snapshot(&rexon_core::animations::SystemIo, &rexon_core::mate_panel::PanelCtx::from_env())
                } else {
                    rexon_core::backup::xfce_panel_save_live(&data_home)
                }
            }
            // Прочие столы: одна копия на стол (`backup::panel_copy_*`).
            Some(de) if de != "kde" && rexon_core::backup::live_panel_copy_spec(de).is_some() => {
                if rexon_core::backup::panel_copy_exists_live(de) {
                    let (title, text) = qd::panel_backup_replace_confirm(false);
                    if !crate::extensions_actions::ask_confirm(&QString::from(&title), &QString::from(&text)) {
                        return false;
                    }
                }
                rexon_core::backup::panel_copy_save_live(de)
            }
            _ => crate::settings_actions::panel_backup_now("manual")
                // Отказ ДИСКА (каталог не создался, мета-файл не записался) — у
                // Python это было исключение, и оно шло в тот же итог «не
                // удалось» со своим текстом.
                .unwrap_or_else(|detail| (false, detail)),
        };
        self.set_status(panel_status(&qd::panel_backup_result(ok, &message)));
        false
    }

    fn panel_restore(self: Pin<&mut Self>) -> bool {
        let agreed = crate::extensions_actions::ask_confirm(
            &QString::from(&qd::panel_restore_confirm_title()),
            &QString::from(&qd::panel_restore_confirm_text()),
        );
        if !agreed {
            // Человек передумал — сказать нечего (как у Python).
            return false;
        }
        let (ok, message) = match current_de().as_deref() {
            // Та же копия, что у «Вернуть прежнюю панель» раздела «Раскладка
            // панели» (`mate_panel::restore`).
            Some("mate") => {
                let text = rexon_core::mate_panel::restore(&rexon_core::animations::SystemIo, &rexon_core::mate_panel::PanelCtx::from_env());
                (text.starts_with('✅'), text)
            }
            Some("xfce") => {
                let data_home = utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref().filter(|v| !v.is_empty()), &std::env::var("HOME").unwrap_or_default());
                rexon_core::backup::xfce_panel_restore_live(&data_home)
            }
            Some(de) if de != "kde" && rexon_core::backup::live_panel_copy_spec(de).is_some() => {
                rexon_core::backup::panel_copy_restore_live(de)
            }
            _ => crate::settings_actions::panel_restore_now(None),
        };
        self.set_status(panel_status(&qd::panel_restore_result(ok, &message)));
        false
    }

    // ── Блок «Оболочки» ──────────────────────────────────────────────────
    //
    // Вопрос, отказ и ход — тем же окном операции, что у столов. Отказы
    // (SteamOS, нет в репозиториях, нет самого стола) — окном без «Да» ДО
    // пароля; пароль спрашивает исполнитель ядра, только если пакеты нужно
    // ставить или снимать.

    /// Разобрать пару «композитор, оболочка» и показать отказ, если её нет.
    fn shell_target(mut self: Pin<&mut Self>, de_id: &QString, shell_id: &QString) -> Option<(String, DesktopShell)> {
        let wm = de_id.to_string();
        let shell = sb::shell_by_id(&shell_id.to_string());
        match shell {
            Some(shell) if sb::SHELL_WMS.contains(&wm.as_str()) => {
                if self.busy() {
                    self.set_status(dx::busy_message());
                    return None;
                }
                Some((wm, shell))
            }
            _ => {
                self.as_mut().report_unknown(&wm);
                None
            }
        }
    }

    // ── Пакет, ждущий пересборки: собрать из исходников Arch ─────────────

    fn rebuild_from_sources(mut self: Pin<&mut Self>, de_id: &QString) {
        let de_id = de_id.to_string();
        if self.busy() {
            self.set_status(dx::busy_message());
            return;
        }
        // Та же кнопка на карточке Deepin — перевести обычный Deepin на
        // Treeland (`Offer::Install`) или обновить свою сборку.
        let treeland = self
            .as_ref()
            .get_ref()
            .rust()
            .treeland_offers
            .lock()
            .ok()
            .and_then(|map| map.get(&de_id).cloned());
        if let Some(offer) = treeland {
            let (update, operation_title, text) = match &offer {
                dt::Offer::Install => (false, dt::install_title(), dt::install_confirm_text()),
                dt::Offer::Update { local, repo } => (true, dt::update_title(), dt::update_confirm_text(local, repo)),
            };
            self.as_mut().rust_mut().pending = Some(PendingOp::Treeland {
                update,
                operation_title: operation_title.clone(),
            });
            self.as_mut().set_status(dx::awaiting_answer(&operation_title));
            self.set_op(OpView::confirm(&operation_title, &text));
            return;
        }
        // Та же кнопка — замена другой ветки той же программы (River 0.5):
        // обычный вопрос установки с текстом замены и согласием на неё.
        let swaps = self
            .as_ref()
            .get_ref()
            .rust()
            .swap_offers
            .lock()
            .ok()
            .and_then(|map| map.get(&de_id).cloned())
            .unwrap_or_default();
        if !swaps.is_empty() {
            let Some(name) = known_name(&de_id) else {
                self.report_unknown(&de_id);
                return;
            };
            let mut texts = if crate::compare_page::de_installed(&de_id) {
                qd::repair_texts(&name)
            } else {
                qd::install_texts(&name)
            };
            texts.confirm_title = system_plan::conflict_swap_title(&swaps);
            texts.confirm_html = system_plan::conflict_swap_confirm_text(&swaps, &name);
            let replace: Vec<String> = swaps.iter().map(|c| c.installed.clone()).collect();
            self.ask_install(&de_id, texts, replace);
            return;
        }
        let candidates = self
            .as_ref()
            .get_ref()
            .rust()
            .rebuild_offers
            .lock()
            .ok()
            .and_then(|map| map.get(&de_id).cloned())
            .unwrap_or_default();
        if candidates.is_empty() {
            // Кнопка пережила предложение: состав системы сменился после
            // операции, и собирать больше нечего.
            self.set_status(repo_rebuild::offer_gone());
            return;
        }
        let packages: Vec<String> = candidates.iter().map(|c| c.package.clone()).collect();
        let operation_title = repo_rebuild::operation_title(&packages);
        self.as_mut().rust_mut().pending = Some(PendingOp::Rebuild {
            packages,
            operation_title: operation_title.clone(),
        });
        self.as_mut().set_status(dx::awaiting_answer(&operation_title));
        self.set_op(OpView::confirm(&operation_title, &repo_rebuild::confirm_text(&candidates)));
    }

    fn ask_shell(mut self: Pin<&mut Self>, pending: PendingOp, title: &str, text: &str, danger: bool) {
        let operation_title = match &pending {
            PendingOp::Shell { operation_title, .. } => operation_title.clone(),
            _ => String::new(),
        };
        self.as_mut().rust_mut().pending = Some(pending);
        self.as_mut().set_status(dx::awaiting_answer(&operation_title));
        self.set_op(OpView {
            phase: "confirm".to_string(),
            title: title.to_string(),
            text: html(text),
            danger,
            ..OpView::default()
        });
    }

    fn shell_install(mut self: Pin<&mut Self>, de_id: &QString, shell_id: &QString) -> bool {
        let Some((wm, shell)) = self.as_mut().shell_target(de_id, shell_id) else {
            return false;
        };
        let wm_name = qd::display_name(&wm).to_string();
        let family = system::DistroDetector::detect_live().family_live();
        let refusal = if let Some(why) = rexon_core::distro_compat::SteamOsBlock::Desktops.live() {
            Some(why)
        } else if let Err(u) = sb::packages(shell, &wm, family) {
            Some(sb::unavailable_text(u, shell, &wm_name))
        } else if !crate::compare_page::de_installed(&wm) {
            Some(sb::install_desktop_first(&wm_name))
        } else {
            None
        };
        let texts_title = qd::shell_install_texts(shell, &wm, &[], &[]).operation_title;
        if let Some(why) = refusal {
            self.as_mut().set_status(dx::operation_footer(&texts_title, &why));
            self.show_notice(&texts_title, &why);
            return false;
        }
        // DMS смотрит в ~/.config/hypr мимо XDG_CONFIG_HOME — и мы туда же.
        let hypr = format!("{}/.config/hypr", std::env::var("HOME").unwrap_or_default());
        let hypr_lua = std::path::Path::new(&format!("{hypr}/hyprland.lua")).is_file();
        let hypr_strays = std::path::Path::new(&format!("{hypr}/hyprland.conf")).is_file()
            || std::fs::read_dir(format!("{hypr}/dms"))
                .map(|d| d.flatten().any(|e| e.path().extension().is_some_and(|x| x == "conf")))
                .unwrap_or(false);
        let warnings = sb::install_warnings(shell, &wm, hypr_lua, hypr_strays);
        // Что реально поставится — то и называем (пусто — пароль не нужен).
        let candidates = sb::packages(shell, &wm, family).unwrap_or_default();
        self.ask_shell_after_query(candidates, false, move |packages| {
            let t = qd::shell_install_texts(shell, &wm, packages, &warnings);
            let pending = PendingOp::Shell {
                job: ShellJob::Install,
                wm,
                shell,
                operation_title: t.operation_title.clone(),
                result: format!("<b>{}</b><br>{}", t.done_title, html(&t.done_text)),
            };
            (pending, t.confirm_title, t.confirm_html, false)
        });
        false
    }

    /// Задать вопрос про связку, когда станет ясно, какие пакеты из
    /// `candidates` стоят: оставляются стоящие (`installed`) или нестоящие.
    ///
    /// `pacman -Q` по каждому пакету занимает секунды — в потоке интерфейса
    /// окно на это время замирало. Проверка идёт в фоне, вопрос приходит в
    /// поток Qt готовым.
    fn ask_shell_after_query(
        mut self: Pin<&mut Self>,
        candidates: Vec<&'static str>,
        installed: bool,
        question: impl FnOnce(&[&str]) -> (PendingOp, String, String, bool) + Send + 'static,
    ) {
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let sys = rexon_core::system_exec::live();
            let packages: Vec<&str> = candidates.into_iter().filter(|p| sys.is_pkg_installed(p) == installed).collect();
            let (pending, title, text, danger) = question(&packages);
            deliver(&handle, move |object| object.ask_shell(pending, &title, &text, danger));
        });
    }

    fn shell_switch(mut self: Pin<&mut Self>, de_id: &QString, shell_id: &QString) -> bool {
        let Some((wm, shell)) = self.as_mut().shell_target(de_id, shell_id) else {
            return false;
        };
        let t = qd::shell_switch_texts(shell, &wm);
        let pending = PendingOp::Shell {
            job: ShellJob::Switch,
            wm,
            shell,
            operation_title: t.operation_title.clone(),
            result: String::new(),
        };
        self.ask_shell(pending, &t.confirm_title, &t.confirm_html, false);
        false
    }

    fn shell_remove(mut self: Pin<&mut Self>, de_id: &QString, shell_id: &QString) -> bool {
        let Some((wm, shell)) = self.as_mut().shell_target(de_id, shell_id) else {
            return false;
        };
        let family = system::DistroDetector::detect_live().family_live();
        let home = std::env::var("HOME").unwrap_or_default();
        let cfg = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty());
        let read = |p: &str| std::fs::read_to_string(p).ok();
        let still: Vec<&str> = sb::SHELL_WMS
            .iter()
            .copied()
            .filter(|o| *o != wm && sb::block_installed(o, shell, &home, cfg.as_deref(), &read))
            .collect();
        let candidates = sb::packages_to_remove(shell, &wm, family, &still);
        self.ask_shell_after_query(candidates, true, move |packages| {
            let t = qd::shell_remove_texts(shell, &wm, packages);
            let pending = PendingOp::Shell {
                job: ShellJob::Remove,
                wm,
                shell,
                operation_title: t.operation_title.clone(),
                result: String::new(),
            };
            (pending, t.confirm_title, t.confirm_html, true)
        });
        false
    }

    // ── Блок «Как в дистрибутиве» ────────────────────────────────────────
    //
    // Тем же окном операции, что у столов: план в фоне → вопрос (галочка
    // «по желанию») → (пароль SteamOS) → ход в рабочем потоке → итог.
    // Тексты и решения — ядро (`qt_desktops::look_*`, `distro_looks`).

    fn look_apply(mut self: Pin<&mut Self>, look_id: &QString) {
        if self.busy() {
            self.set_status(dx::busy_message());
            return;
        }
        let id = look_id.to_string();
        let Some(de) = current_de() else {
            let (title, text) = qd::look_unknown_desktop();
            self.show_notice(&title, &text);
            return;
        };
        let title = qd::look_apply_title(&qd::look_name(&id));
        let mut op = OpView::running(&title);
        op.status = qd::look_checking();
        self.as_mut().set_status(qd::look_checking());
        self.as_mut().set_op(op);
        let handle = self.as_mut().qt_thread();
        std::thread::spawn(move || {
            let offer = dl::offer_live(&id, &de);
            deliver(&handle, move |object| object.look_offered(offer, de, title));
        });
    }

    /// План собран: вопрос или отказ.
    fn look_offered(mut self: Pin<&mut Self>, offer: Result<dl::Offer, String>, de: String, title: String) {
        match offer {
            Ok(offer) => self.show_look_confirm(offer, Default::default(), false, de, title),
            Err(why) => {
                self.as_mut().set_status(dx::operation_footer(&title, &why));
                self.show_notice(&title, &why);
            }
        }
    }

    fn show_look_confirm(
        mut self: Pin<&mut Self>,
        offer: dl::Offer,
        values: rexon_core::rxn_manifest::PropValues,
        opt_in: bool,
        de: String,
        title: String,
    ) {
        match qd::look_confirm(&offer, &values, opt_in) {
            qd::LookConfirm::Blocked { title: window_title, text } => {
                self.as_mut().rust_mut().pending = None;
                self.as_mut().set_status(dx::operation_footer(&title, &window_title));
                self.set_op(OpView {
                    phase: "blocked".to_string(),
                    title: window_title,
                    text,
                    ..OpView::default()
                });
            }
            qd::LookConfirm::Ask {
                title: window_title,
                text,
                yes_text,
                no_text,
                choices,
                option,
            } => {
                let (option_text, option_hint) = option.map(|o| (o.text, o.hint)).unwrap_or_default();
                self.as_mut().rust_mut().pending = Some(PendingOp::Look {
                    offer: Box::new(offer),
                    values,
                    opt_in,
                    de,
                    operation_title: title.clone(),
                });
                self.as_mut().set_status(dx::awaiting_answer(&title));
                self.set_op(OpView {
                    phase: "confirm".to_string(),
                    title: window_title,
                    text,
                    yes_text,
                    no_text,
                    option_checked: opt_in && !option_text.is_empty(),
                    option_text,
                    option_hint,
                    choices,
                    ..OpView::default()
                });
            }
        }
    }

    fn set_operation_option(mut self: Pin<&mut Self>, checked: bool) {
        if self.rust().op.phase != "confirm" {
            return;
        }
        match self.as_mut().rust_mut().pending.take() {
            Some(PendingOp::Look {
                offer,
                values,
                de,
                operation_title,
                ..
            }) => self.show_look_confirm(*offer, values, checked, de, operation_title),
            other => self.as_mut().rust_mut().pending = other,
        }
    }

    fn set_operation_choice(mut self: Pin<&mut Self>, name: &QString, value: &QString) {
        if self.rust().op.phase != "confirm" {
            return;
        }
        match self.as_mut().rust_mut().pending.take() {
            Some(PendingOp::Look {
                offer,
                mut values,
                opt_in,
                de,
                operation_title,
            }) => {
                values.insert(name.to_string(), serde_json::Value::String(value.to_string()));
                self.show_look_confirm(*offer, values, opt_in, de, operation_title)
            }
            other => self.as_mut().rust_mut().pending = other,
        }
    }

    fn look_revert(mut self: Pin<&mut Self>, look_id: &QString) {
        if self.busy() {
            self.set_status(dx::busy_message());
            return;
        }
        let id = look_id.to_string();
        let Some(de) = current_de() else {
            let (title, text) = qd::look_unknown_desktop();
            self.show_notice(&title, &text);
            return;
        };
        let name = qd::look_name(&id);
        let (title, text, yes_text) = qd::look_revert_confirm(&name);
        let operation_title = qd::look_revert_title(&name);
        self.as_mut().rust_mut().pending = Some(PendingOp::LookRevert {
            id,
            de,
            operation_title: operation_title.clone(),
        });
        self.as_mut().set_status(dx::awaiting_answer(&operation_title));
        self.set_op(OpView {
            phase: "confirm".to_string(),
            title,
            text: html(&text),
            yes_text,
            no_text: dx::dialog_cancel(),
            ..OpView::default()
        });
    }

    fn report_unknown(self: Pin<&mut Self>, de_id: &str) {
        self.set_status(rexon_core::i18n::trf("Rexon doesn't know the desktop “%1”.", &[&de_id]));
    }
}

// ============================================================================
// Доставка хода дела из рабочего потока
// ============================================================================

/// Приёмник хода исполнителя ядра: каждое событие уходит в поток Qt.
struct QtSink {
    handle: CxxQtThread<qobject::DesktopsActions>,
}

impl dx::DeOpSink for QtSink {
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
    handle: &CxxQtThread<qobject::DesktopsActions>,
    work: impl FnOnce(Pin<&mut qobject::DesktopsActions>) + Send + 'static,
) {
    if let Err(e) = handle.queue(work) {
        if !matches!(e, ThreadingQueueError::ObjectDestroyed) {
            rexon_core::log_warn!("desktops", "ход операции со столом не доставлен окну: {e:?}");
        }
    }
}

/// Строка подвала из итога ядра по панели KDE. У Python это было окно с
/// заголовком и текстом; в подвале — одна строка, поэтому перенос строки
/// внутри текста становится пробелом.
fn panel_status(texts: &qd::PanelTexts) -> String {
    format!("{} — {}", texts.title, texts.text.replace('\n', " "))
}

/// Отображаемое имя стола; `None` — ядро такого не знает.
/// SteamOS и стол-образ (установка, проверка, удаление): что сделать до
/// помощника под паролем. `None` — не SteamOS или операция не образа.
fn steamos_admin_gate(pending: &PendingOp) -> Option<sx::AdminGate> {
    // Образ ставит программы окном пароля — на SteamOS у «deck» без пароля
    // его не пройти. Ничего не ставит — пароль не нужен.
    if let PendingOp::Look { offer, values, opt_in, .. } = pending {
        let installs = qd::look_chosen_plan(offer, values, *opt_in).is_some_and(|p| !p.install.is_empty());
        if !installs || sx::live_release().is_none() {
            return None;
        }
        return Some(sx::admin_gate(sx::live_password_state(), sx::live_device()));
    }
    let de_id = match pending {
        PendingOp::Install { de_id, .. } | PendingOp::Remove { de_id, .. } => de_id,
        _ => return None,
    };
    if !matches!(sx::support(de_id), sx::Support::Supported(_)) || sx::live_release().is_none() {
        return None;
    }
    Some(sx::admin_gate(sx::live_password_state(), sx::live_device()))
}

/// Заголовок операции, что ждёт «Да», — для подвала и окна отказа.
fn pending_title(pending: &PendingOp) -> String {
    match pending {
        PendingOp::Install { operation_title, .. }
        | PendingOp::Switch { operation_title, .. }
        | PendingOp::Remove { operation_title, .. }
        | PendingOp::Rebuild { operation_title, .. }
        | PendingOp::Treeland { operation_title, .. }
        | PendingOp::Shell { operation_title, .. }
        | PendingOp::Look { operation_title, .. }
        | PendingOp::LookRevert { operation_title, .. } => operation_title.clone(),
    }
}

/// Итог применения или возврата образа: удача — приписка под журналом
/// словами ядра; неудача — причина строкой ошибки в журнале (итог окна при
/// неудаче своей строки не пишет).
fn look_outcome(sink: &dyn dx::DeOpSink, result: Result<String, String>, job_result: &mut Option<String>) -> dx::DeOpOutcome {
    match result {
        Ok(message) => {
            *job_result = Some(qd::look_result_html(&message));
            dx::DeOpOutcome {
                ok: true,
                message: dp::MSG_OK.to_string(),
                cancelled: false,
                restart_needed: false,
            }
        }
        Err(why) => {
            sink.progress(dp::STAGE_ERROR, &why, None);
            dx::DeOpOutcome {
                ok: false,
                message: why,
                cancelled: false,
                restart_needed: false,
            }
        }
    }
}

fn known_name(de_id: &str) -> Option<String> {
    system::DE_META
        .iter()
        .find(|meta| meta.id == de_id)
        .map(|meta| meta.display_name.to_string())
}

/// Текущий рабочий стол по окружению сессии; `None` — не опознан.
///
/// Тот же ответ даёт `SystemInfo.current_de` на Python и `DesktopsModel`
/// рядом: третьей копии детекции здесь нет, зовётся ядро.
fn current_de() -> Option<String> {
    let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = std::env::var("DESKTOP_SESSION").unwrap_or_default();
    let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
    system::detect_current_de(&xdg, &session, wayland).filter(|de| !de.is_empty())
}

/// Блок «пакеты» карточки по ответу пробной транзакции; без ответа — счёт
/// имён ядра, как у страницы до ответа.
/// Подписи предложения пересборки: пустые — предлагать нечего.
/// Подпись кнопки и пояснение: перевести обычный Deepin на Treeland или
/// обновить свою сборку deepin-session.
fn treeland_offer_texts(offer: &dt::Offer) -> (String, String) {
    match offer {
        dt::Offer::Install => (dt::install_button(), dt::install_hint()),
        dt::Offer::Update { local, repo } => (dt::update_button(), dt::update_hint(local, repo)),
    }
}

/// Подписи кнопки под предупреждением карточки: вариант Deepin на
/// Treeland, иначе замена конфликтующего, иначе пересборка.
fn offer_texts(
    treeland: Option<&dt::Offer>,
    swaps: &[system_plan::PkgConflict],
    rebuild_offered: bool,
) -> (String, String) {
    match treeland {
        Some(offer) => treeland_offer_texts(offer),
        None if !swaps.is_empty() => (
            system_plan::conflict_swap_button(swaps),
            system_plan::conflict_swap_hint(swaps),
        ),
        None => rebuild_offer_texts(rebuild_offered),
    }
}

fn rebuild_offer_texts(offered: bool) -> (String, String) {
    if offered {
        (repo_rebuild::offer_button(), repo_rebuild::offer_hint())
    } else {
        (String::new(), String::new())
    }
}

fn packages_view_of(
    de_id: &str,
    installed: bool,
    preview: Option<&system_plan::InstallPreview>,
) -> qd::PackagesView {
    let family = system::DistroDetector::detect_live().family_live();
    let main = system::get_packages(de_id, family).len();
    let aur_names = system::get_aur_packages(de_id, family);
    let mut view = qd::packages_view(installed, preview, main, aur_names.len());
    // Признаки риска у пакетов из AUR (то же, что значок на плитке AUR в
    // «Оформлении»): только данные, уже полученные `aur_trust_warm`, — без
    // сети, этот путь зовётся и из потока Qt. Установку не запрещает.
    if !installed && !aur_names.is_empty() {
        let now = rexon_core::aur_trust::now_secs();
        let views = rexon_core::aur_trust::rpc_views(None, &aur_names, now);
        let path = rexon_core::aur_trust::live_cache_path();
        let risk = rexon_core::aur_trust::desktop_notice_for(&aur_names, &views, &path, now);
        if !risk.is_empty() {
            if !view.notice.is_empty() {
                view.notice.push('\n');
            }
            view.notice.push_str(&risk);
        }
    }
    view
}

/// Спросить AUR про пакеты набора стола (один запрос RPC, ответ держится в
/// памяти). Без сети — молча ничего.
fn aur_trust_warm(names: &[String]) {
    if names.is_empty() {
        return;
    }
    let mut net = rexon_core::net::UreqNet::new();
    rexon_core::aur_trust::rpc_views(Some(&mut net), names, rexon_core::aur_trust::now_secs());
}

// ============================================================================
// Версия пакета из репозиториев дистрибутива
// ============================================================================

/// Версия представительного пакета стола в репозиториях ТЕКУЩЕГО дистрибутива
/// либо `None`.
///
/// Порт `rexon.system.query_repo_version`: решения принимает ядро (выбор
/// команды и разбор вывода), здесь только `which` и запуск. Тихо, без паники:
/// «спросить не вышло» для этой строки — то же, что «пакета нет», её просто
/// не покажут.
fn repo_version(de_id: &str) -> Option<String> {
    let package = system::get_check_binary(de_id)?;
    let family = system::DistroDetector::detect_live().family_live();
    let query = system_plan::repo_version_query(family, package)?;
    // Бинарь проверяется ДО запуска — как в оригинале: на дистрибутиве без
    // этого менеджера запуск дал бы «не нашлось» вместо честного «не знаю».
    utils::which(&query.binary, None)?;

    let report = utils::run(
        &utils::RunSpec {
            argv: query.cmd.argv.clone(),
            output: utils::RunOutput::Capture,
            timeout_secs: query.cmd.timeout_secs,
            lang_c: query.cmd.lang_c,
            ..Default::default()
        },
        None,
    );
    let rc = report.returncode?;
    let stdout = utils::decode_text(&report.stdout);
    if stdout.is_empty() {
        return None;
    }
    if query.check_returncode && rc != 0 {
        return None;
    }
    system_plan::parse_repo_version(family, &stdout)
}

/// Установленная версия ключевого пакета стола (без эпохи и номера сборки,
/// как и версия из репозитория) либо `None`.
fn installed_version(de_id: &str) -> Option<String> {
    let package = system::get_check_binary(de_id)?;
    let family = system::DistroDetector::detect_live().family_live();
    let query = system_plan::installed_version_query(family, package)?;
    utils::which(&query.binary, None)?;
    let report = utils::run(
        &utils::RunSpec {
            argv: query.cmd.argv.clone(),
            output: utils::RunOutput::Capture,
            timeout_secs: query.cmd.timeout_secs,
            lang_c: query.cmd.lang_c,
            ..Default::default()
        },
        None,
    );
    let ok = report.returncode == Some(0);
    let raw = system_plan::parse_installed_version(family, ok, &utils::decode_text(&report.stdout))?;
    let clean = system_plan::clean_pkg_version(&raw);
    (!clean.is_empty()).then_some(clean)
}

/// Новее ли версия в репозитории установленной: `vercmp` из pacman (Arch).
/// Где его нет — `None`, и строка не утверждает, что вышло обновление.
fn repo_is_newer(installed: &str, available: &str) -> Option<bool> {
    if installed.is_empty() || available.is_empty() || installed == available {
        return Some(false);
    }
    utils::which("vercmp", None)?;
    let report = utils::run(
        &utils::RunSpec {
            argv: vec!["vercmp".into(), installed.to_string(), available.to_string()],
            output: utils::RunOutput::Capture,
            timeout_secs: Some(5),
            ..Default::default()
        },
        None,
    );
    let out = utils::decode_text(&report.stdout);
    let n: i32 = out.trim().parse().ok()?;
    Some(n < 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Образцы ответов панели берутся отсюда же, откуда их берёт продукт.
    use rexon_core::backup;

    /// Договор со страницей: у каждого стола базы есть представительный
    /// пакет, за которым можно сходить в репозитории. Забытый стол молча
    /// оставил бы карточку без строки «Доступно в твоём дистрибутиве».
    #[test]
    fn every_desktop_has_a_package_to_ask_about() {
        for meta in system::DE_META {
            assert!(
                system::get_check_binary(meta.id).is_some(),
                "у стола «{}» нет пакета для запроса версии",
                meta.id
            );
        }
    }

    /// Неизвестный стол не роняет объект и не выдаёт себя за известный.
    #[test]
    fn unknown_desktop_is_reported_not_swallowed() {
        assert!(known_name("no-such-de").is_none());
        assert!(known_name("kde").is_some());
        assert!(repo_version("no-such-de").is_none());
    }

    /// Итог по панели KDE — тексты ядра (те же, что в окне Python-вкладки),
    /// собранные в ОДНУ строку подвала: заголовок, тире, текст без переноса.
    ///
    /// ⚠️ВХОДНЫЕ ОБРАЗЦЫ БЕРУТСЯ ИЗ ЯДРА, А НЕ НАБИРАЮТСЯ ЗДЕСЬ РУКАМИ.
    /// Сюда приходит ровно то, что вернул `backup::plasma_backup_run` /
    /// `plasma_restore_run`, и переписанная от руки копия молча устаревает:
    /// строка проходит подвал насквозь, тест остаётся зелёным и описывает
    /// продукт, которого уже нет (так и случилось с прежними «Бэкап создан
    /// (2 файлов)» и «Нет бэкапов Plasma» — продукт давно говорит иначе).
    #[test]
    fn panel_result_is_one_footer_line_of_core_texts() {
        // Язык закреплён исходный: сверка не зависит от локали машины, а
        // русский текст сторожит сверка ядра (`qt_desktops_parity`).
        rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
        let created = backup::msg_backup_created(2);
        let line = panel_status(&qd::panel_backup_result(true, &created));
        assert_eq!(
            line,
            format!("Panel Saved — A copy of your panel settings has been saved. {created}")
        );

        // Возврат прошёл: у итога ядра есть СВОЙ перенос строки (хвост о
        // перезапуске панели), и подвал обязан расправить его в пробел.
        let restored = format!(
            "{}{}",
            backup::msg_restored_files(3, "20260730_010203"),
            backup::msg_plasmashell_restarted()
        );
        assert!(restored.contains('\n'), "{restored}");
        let line = panel_status(&qd::panel_restore_result(true, &restored));
        assert_eq!(
            line,
            format!(
                "Panel Restored — The panel has been restored from the copy. {}",
                restored.replace('\n', " ")
            )
        );
        assert!(!line.contains('\n'), "{line}");

        // Возврата не вышло: причина ядра доезжает до подвала целиком.
        let why = backup::msg_no_plasma_backups();
        let line = panel_status(&qd::panel_restore_result(false, &why));
        assert!(line.starts_with("Couldn't Restore — "), "{line}");
        assert!(line.ends_with(&why), "{line}");
        assert!(!line.contains('\n'), "{line}");
    }
}
