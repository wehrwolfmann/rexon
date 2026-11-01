// DesktopsModel — вкладка «Рабочие столы» на Rust + cxx-qt + QML.
//
// Вторая вкладка, перенесённая на QML по схеме «Сравнения DE». Данные и решения — ядро
// (`rexon_core::qt_desktops` поверх базы столов `system::DE_META`), здесь
// только оболочка QObject: модель списка слева с ролями и карточка
// выбранного стола одной картой.
//
// Чего здесь НЕТ:
//   * решений «что показать» — секции списка, подсказки, состав кнопок и
//     обе защиты удаления считает ядро;
//   * побочных действий — установка, восстановление, переключение и
//     удаление ходят через pkexec и окна хода дела, пока живущие на Python
//     (`rexon/qt_desktops/window.py`, объект `desktopsActions`). Страница
//     зовёт их по именам и по ответу решает, пересчитывать ли состояние.
//
// ⚠️ Состояние «установлен» модель считает САМА (`compare_page::de_installed`
// — исполняемые файлы стола в PATH), тем же ответом, что даёт
// `DesktopEnvironment.recheck_installed()` на Python. Одна функция на обе
// QML-вкладки: второй копией они разъехались бы молча.

/// Мост cxx-qt: список столов слева плюс карточка выбранного справа.
///
/// Имя типа — `DesktopsModel`, а не `DesktopsPage`: последнее занято самим
/// файлом разметки (`qml/DesktopsPage.qml`), и совпадение дало бы
/// «Ambiguous type detected».
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// Базовый Qt-тип. ⚠️ БЕЗ `#[qobject]`: на каждый `#[qobject]`-тип в
        /// `extern "C++Qt"` cxx-qt печатает шимы приведения по имени ТИПА, и
        /// второй мост крейта с той же базой даёт `duplicate symbol`.
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
        // ── Неизменные подписи страницы ───────────────────────────────
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
        // Блок «Как в дистрибутиве» (средняя панель): заголовок, пояснение и
        // плитки образов. Плитки меняются после «Применить» и «Вернуть как
        // было» — их перечитывает `refreshInstalled`.
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
        // ── Меняющееся ────────────────────────────────────────────────
        // Карточка выбранного стола одной картой: её содержимое зависит и
        // от выбора, и от состава установленных столов.
        #[qproperty(QMap_QString_QVariant, card, READ = card, NOTIFY = card_changed)]
        #[qproperty(QString, selected_de, cxx_name = "selectedDe",
                    READ = selected_de, NOTIFY = card_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        type DesktopsModel = super::DesktopsModelRust;
    }

    unsafe extern "RustQt" {
        /// Скин сменили на лету — палитру пересчитать.
        ///
        /// Свойство `palette` НЕ `CONSTANT` именно поэтому: без сигнала
        /// привязки QML (`readonly property var pal: page.palette`) не
        /// пересчитываются, и открытая вкладка остаётся в цветах прошлого
        /// скина, пока окно на виджетах уже перекрашено. Толчок приходит из
        /// общего реестра (`crate::skin`), а не отсюда.
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

    // Без #[qinvokable]: инвокабл в наследнике затеняет укороченные клоны
    // сигнатур базы, и rowCount() без аргументов из JS падает.
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
        /// Почему часть кнопок панели неактивна (SteamOS); пусто — все
        /// активны.
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

    // ── API для QML ───────────────────────────────────────────────────
    extern "RustQt" {
        /// Выбрать стол в списке. Заголовки секций не выбираются — как в
        /// оригинале, где у них стоял `NoItemFlags`.
        #[qinvokable]
        fn select(self: Pin<&mut DesktopsModel>, de_id: &QString);

        /// Перечитать состав установленных столов и перерисовать карточку.
        /// Зовётся после каждой успешной операции.
        #[qinvokable]
        #[cxx_name = "refreshInstalled"]
        fn refresh_installed(self: Pin<&mut DesktopsModel>);

        /// Что делать по нажатию «Удалить»: обе защиты решает ядро.
        /// Карта с `kind`/`title`/`text`/`allowed`.
        #[qinvokable]
        #[cxx_name = "removePlan"]
        fn remove_plan(self: &DesktopsModel, de_id: &QString) -> QVariant;

        /// Подтверждение и заголовки хода дела для операции над столом.
        /// `kind` — `install`, `repair`, `switch`, `remove`.
        #[qinvokable]
        #[cxx_name = "operationTexts"]
        fn operation_texts(self: &DesktopsModel, kind: &QString, de_id: &QString) -> QVariant;

        /// Отображаемое имя стола.
        #[qinvokable]
        #[cxx_name = "displayName"]
        fn display_name(self: &DesktopsModel, de_id: &QString) -> QString;

        /// Строка версии из репозиториев; пустая строка — строку скрыть.
        #[qinvokable]
        #[cxx_name = "versionLine"]
        fn version_line(self: &DesktopsModel, version: &QString) -> QString;

        /// Подсказка полноэкранного просмотра «i из n».
        #[qinvokable]
        #[cxx_name = "zoomHint"]
        fn zoom_hint(self: &DesktopsModel, index: i32, count: i32) -> QString;

        /// Точки-индикаторы карусели.
        #[qinvokable]
        #[cxx_name = "carouselDots"]
        fn carousel_dots(self: &DesktopsModel, index: i32, count: i32) -> QString;

        /// Высота рамки скриншота по размеру окна и доступной ширине.
        #[qinvokable]
        #[cxx_name = "carouselHeight"]
        fn carousel_height(self: &DesktopsModel, window_height: i32, available_width: i32) -> i32;

        /// Та же высота, ужатая под вьюпорт карточки: `room` — сколько
        /// высоты остаётся рамке после всего остального в карточке.
        #[qinvokable]
        #[cxx_name = "carouselHeightFitted"]
        fn carousel_height_fitted(self: &DesktopsModel, full: i32, room: i32) -> i32;

        /// Потолок ширины правой панели по высоте окна: дальше лишняя ширина
        /// уходит средней панели («Как в дистрибутиве»), как у сплиттера
        /// оригинала.
        #[qinvokable]
        #[cxx_name = "detailPanelMaxWidth"]
        fn detail_panel_max_width(self: &DesktopsModel, window_height: i32) -> i32;

        /// Ширина средней панели («Как в дистрибутиве») при ширине страницы
        /// `total`: делёж лишнего места между панелями — как у сплиттера
        /// оригинала.
        #[qinvokable]
        #[cxx_name = "looksPanelWidth"]
        fn looks_panel_width(self: &DesktopsModel, total: i32, window_height: i32) -> i32;

        /// Наименьшая ширина средней панели по её же содержимому: подписи
        /// обязаны читаться целиком на штатном минимуме окна. Ширину надписей
        /// меряет по шрифту Qt, числа и арифметика — в ядре.
        #[qinvokable]
        #[cxx_name = "looksPanelMinWidth"]
        fn looks_panel_min_width(
            self: &DesktopsModel,
            header_width: i32,
            column_width: i32,
        ) -> i32;

        /// Встают ли «Как в дистрибутиве» и «Инструменты» рядом в средней
        /// панели шириной `inner_width` (без полей) — решает ядро.
        #[qinvokable]
        #[cxx_name = "looksToolsSideBySide"]
        fn looks_tools_side_by_side(
            self: &DesktopsModel,
            inner_width: i32,
            looks_width: i32,
            tools_width: i32,
        ) -> bool;

        /// Ширина одного блока средней панели (рядом — половина, столбиком —
        /// вся ширина).
        #[qinvokable]
        #[cxx_name = "looksToolsBlockWidth"]
        fn looks_tools_block_width(self: &DesktopsModel, inner_width: i32, side_by_side: bool)
            -> i32;

        /// Левая распорка обёртки карточки при ширине вьюпорта `viewport`:
        /// карточка прижата к правому краю панели, как у оригинала.
        #[qinvokable]
        #[cxx_name = "cardLeftSpacer"]
        fn card_left_spacer(self: &DesktopsModel, viewport: i32) -> i32;

        /// Номер роли по имени (-1, если роли нет) — для проверок без делегата.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &DesktopsModel, name: &QString) -> i32;

        /// Значение роли по номеру строки — проверки без делегата.
        #[qinvokable]
        #[cxx_name = "roleValue"]
        fn role_value(self: &DesktopsModel, row: i32, name: &QString) -> QVariant;
    }

    impl cxx_qt::Initialize for DesktopsModel {}
    // Подписка на смену скина идёт через `CxxQtThread::queue`, и трейт
    // потока обязан быть объявлен В МОСТЕ: cxx-qt печатает по этой
    // строке и сам `qt_thread()`, и его алиас типа.
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

/// Состояние оболочки на ЭТОЙ машине для карточки композитора `wm`: пакет в
/// `PATH`, наш блок в конфиге, отметка режима. Только чтение.
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

/// Блок «Оболочки» карточки списком карт для QML; пусто — блока нет.
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
// Живое окружение: семейство пакетов, текущий стол, снимки экрана
// ============================================================================

/// Семейство пакетов этого дистрибутива. Пробуется один раз на объект:
/// os-release за время работы вкладки не меняется.
fn detect_family() -> PkgFamily {
    DistroDetector::detect_live().family_live()
}

/// Идентификатор стола, в котором работают СЕЙЧАС; пустая строка — не
/// опознан. Тот же ответ даёт `SystemInfo.current_de` на Python.
fn detect_current() -> String {
    let xdg = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let session = std::env::var("DESKTOP_SESSION").unwrap_or_default();
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    system::detect_current_de(&xdg, &session, wayland).unwrap_or_default()
}

/// Что показать в карусели стола: снимки либо одна заглушка.
///
/// Снимки ищет ядро (`carousel_paths_on_disk`): сначала поставляемые в
/// пакете (River), затем скачанные — той же проверкой, по которой его
/// докачка решает, чего не хватает.
fn carousel_images(dir: &str, de_id: &str) -> Vec<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let bundled = screenshots::bundled_screenshot_dirs(&rexon_core::cli::repo_root(), &home);
    let paths = screenshots::carousel_paths_on_disk(dir, &bundled, de_id);
    if !paths.is_empty() {
        return paths;
    }
    // Заглушку и её подпись («загружается» / «появится, когда будет
    // интернет» / «пока нет») выбирает ядро по своей закачке.
    screenshots::placeholder_on_disk(dir, de_id).into_iter().collect()
}

// ============================================================================
// Роли модели
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

/// Роли, меняющиеся от установки или удаления стола.
fn installed_roles() -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    roles.append(ROLE_BASE + ROLE_INSTALLED as i32);
    roles.append(ROLE_BASE + ROLE_TOOLTIP as i32);
    roles
}

// ============================================================================
// Мелочи перевода в Qt
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

/// QSS-цвет ядра в записи, какую понимает QML.
fn qcss(css: &str) -> QVariant {
    qstr(&ui_base::css_color_to_qml(css))
}

/// Доля нейтрального серого (`ui_base::neutral_rgba`) в записи QML.
fn qneutral(alpha: f64) -> QVariant {
    qcss(&ui_base::neutral_rgba(alpha))
}

/// Тон «особой» кнопки — карта, какую понимает `RexonButton.tone`:
/// подложка, подложка под курсором, цвет текста, скругление, жирность и
/// (необязательно) рамка.
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

/// Тоны общего окна операции над столом (`DeOperationDialog.qml`) — в
/// палитру страницы по таблице ядра `qt_desktops::OPERATION_DIALOG_TONES`.
///
/// Зовут ОБЕ страницы с этим окном: «Рабочие столы» и «Сравнение DE».
/// Страница, которая этих ключей не отдаёт, получала «Да» обычной кнопкой
/// скина вместо синей или красной (так и было у «Сравнения»).
pub(crate) fn insert_operation_tones(out: &mut QMap<QMapPair_QString_QVariant>) {
    for (key, t) in qd::OPERATION_DIALOG_TONES {
        out.insert(
            QString::from(key),
            tone(qcss(t.background), qcss(t.hover), t.color, t.radius, t.bold, None),
        );
    }
}

/// Rust-состояние вкладки.
#[derive(Default)]
pub struct DesktopsModelRust {
    /// Номер записи в общем реестре смены скина (`crate::skin`).
    /// 0 — страница ещё не подписана.
    skin_watch: u64,
    rows: Vec<qd::ListRow>,
    selected: String,
    current_de: String,
    family: Option<PkgFamily>,
    screenshot_dir: String,
    loaded: bool,
}

/// Снять подписку на смену скина. Без этого реестр держал бы запись
/// закрытой вкладки до первой рассылки — та её отсеет по
/// `ObjectDestroyed`, но копить мусор незачем.
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
        // С учётом `$XDG_CONFIG_HOME`: снимает скриншоты Python по
        // XDG-осведомлённому пути, и карусель обязана смотреть туда же.
        let xdg = std::env::var("XDG_CONFIG_HOME").ok().filter(|v| !v.is_empty());
        self.screenshot_dir = screenshots::screenshot_dir_xdg(xdg.as_deref(), &home);
        self.rows = qd::list_rows(self.family(), &de_installed);
        // Первым показывается ТЕКУЩИЙ рабочий стол: человек открывает
        // вкладку и видит свой стол, а не первый по списку (у оригинала это
        // был Cinnamon — просто потому, что он стоит первым). Текущего в
        // списке нет — первый выбираемый пункт.
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

    /// Сколько ДРУГИХ столов установлено (сам стол не считается) — вход
    /// второй защиты удаления.
    fn other_installed(&self, de_id: &str) -> usize {
        self.rows
            .iter()
            .filter(|r| r.kind == "de" && r.installed && r.de_id != de_id)
            .count()
    }
}

impl qobject::DesktopsModel {
    // ── Неизменные подписи ────────────────────────────────────────────
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
        // SteamOS: «Проверить мой стол» — это доустановка пакетов, её
        // кнопка видна, но неактивна; причина — строкой под колонкой
        // (`toolsNotice`), подсказка у неактивной кнопки не всплывает.
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
        QString::from(&SteamOsBlock::Desktops.live().unwrap_or_default())
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

    /// Плитки образов: что показать и какие кнопки — ядро
    /// (`qt_desktops::look_tiles`); применён ли — по общему журналу.
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

    /// Подписи и числа карусели скриншотов одной картой.
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

    /// Размеры разметки одной картой: поля панелей, кегли, размеры кнопок.
    /// Все числа — из ядра вкладки, где они выписаны из таблицы стилей
    /// оригинала; страница своих не заводит.
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

    /// Палитра ДЕЙСТВУЮЩЕГО скина (сохранённый `ui_skin`, откат к
    /// стандартному по правилу #17) плюс цвета, которые задаёт сама вкладка
    /// через ядро. Своих цветов страница не заводит.
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
        // Насыщенность вторичного текста (описание, ТТХ, пакеты, версия,
        // точки карусели): у виджетов это `font-weight: 600` из
        // `secondary_text_qss`; без неё QML набирал те же строки обычным
        // начертанием (замер 03.09.2026).
        out.insert(
            QString::from("text_secondary_weight"),
            qint(ui_base::SECONDARY_TEXT_WEIGHT),
        );
        // Заголовок секции списка: у виджетов — роль PlaceholderText палитры
        // скина, а не вторичный текст с прозрачностью.
        out.insert(
            QString::from("placeholder_text"),
            qstr(&ui_base::active_placeholder_text_color()),
        );
        // Список: подложка выбранного пункта и пункта под курсором. Раньше
        // страница брала акцент скина с прозрачностью 0.4 — свою краску
        // вместо синей `rgba(59,130,246,0.4)` оригинала.
        out.insert(QString::from("list_selected"), qcss(qd::LIST_SELECTED_QSS));
        out.insert(
            QString::from("list_selected_text"),
            qstr(qd::LIST_SELECTED_TEXT_COLOR),
        );
        out.insert(QString::from("list_hover"), qcss(qd::list_hover_qss(dark)));
        // Панель инструментов, разделители карточки и рамка карусели — доли
        // нейтрального серого, как в QSS оригинала.
        // Рукоятка сплиттера: без неё QML брал светло-серую от стиля Basic.
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
        // Кнопки действий карточки: синяя, нейтральная с рамкой и красная.
        // Красная — свой тон, а не общая `RexonDangerButton`: у оригинала
        // подложка 0.75 против 0.2 у «Расширений», скругление 6 и жирная
        // подпись, и общий элемент этого не умеет. Синяя и красная — те же
        // `primary_tone`/`destructive_tone`, что берёт окно операции.
        insert_operation_tones(&mut out);
        out.insert(
            QString::from("neutral_tone"),
            tone(
                qneutral(qd::NEUTRAL_BUTTON_BG_ALPHA),
                qneutral(qd::NEUTRAL_BUTTON_HOVER_ALPHA),
                text_primary,
                qd::CARD_BUTTON_RADIUS,
                // Второстепенная кнопка («Проверить / Восстановить») — обычным
                // начертанием: жирная длинная подпись спорила с главной
                // кнопкой и выглядела тяжёлой (замечание Wehrwolfmann 10.10.2026).
                false,
                Some(qneutral(qd::NEUTRAL_BUTTON_BORDER_ALPHA)),
            ),
        );
        // Стрелки и лупа поверх скриншота, кнопки полноэкранного просмотра
        // — круглые тёмные, как в оригинале, а не кнопки скина.
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
        // Затемнение фона под модальным окном — одно на продукт.
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

    // ── Карточка выбранного стола ─────────────────────────────────────
    fn card(&self) -> QMap<QMapPair_QString_QVariant> {
        let state = self.rust();
        let Some(row) = state.row_of(&state.selected) else {
            // Ничего не выбрано — страница показывает приглашение выбрать.
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

        // SteamOS (Steam Deck и Steam Machine): проверенные столы ставятся и
        // удаляются образами systemd-sysext — кнопки живые; у прочих кнопки
        // видны, но неактивны, а причина стоит строкой рядом — кнопку не
        // прячем молча (правило #13). Решает ядро (`steamos_button_block`).
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
                // Ставит набор стола: неактивна, пока он конфликтует с
                // установленным (ответ `packagesReady`, причина — над кнопками).
                ("installsSet", QVariant::from(&qd::installs_desktop_set(button.kind))),
            ])));
        }
        let blocked_notice = reasons.join("\n");

        let main_pkgs = system::get_packages(de_id, state.family()).len();
        let aur_pkgs = system::get_aur_packages(de_id, state.family()).len();
        // Честный счёт даёт пробная транзакция (Arch, в фоне — объект
        // действий, `requestPackages`); до её ответа — «считаю», а не
        // заведомо неверное число имён. Где пробы нет — счёт имён ядра.
        // На SteamOS проба `pacman -Sp` по пакетам самой системы ничего не
        // говорит о том, что соберёт образ systemd-sysext, а её «конфликт»
        // погасил бы живую кнопку «Установить», — её там не делаем.
        let probe = row.available && state.family() == system::PkgFamily::Arch && !steamos;
        let packages_text = if probe {
            qd::packages_counting()
        } else {
            qd::packages_view(row.installed, None, main_pkgs, aur_pkgs).text
        };

        let mut images = QList::<QVariant>::default();
        for path in carousel_images(&state.screenshot_dir, de_id) {
            // QML грузит картинку по адресу, а не по пути.
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
            // Блок «Оболочки» справа: только у Hyprland, niri и sway.
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

    /// Значение одной роли строки. Отдельно от `data`, чтобы тем же путём
    /// отвечал `roleValue`: иначе проверки смотрели бы не туда, куда
    /// смотрит делегат.
    fn role_of(&self, entry: &qd::ListRow, name: Option<&str>) -> QVariant {
        match name {
            Some("kind") => qstr(entry.kind),
            Some("deId") => qstr(&entry.de_id),
            Some("text") => qstr(&entry.text),
            Some("tooltip") => qstr(&entry.tooltip),
            Some("installed") => QVariant::from(&entry.installed),
            Some("available") => QVariant::from(&entry.available),
            // Заголовок секции виден, но не выбирается — в оригинале это был
            // `NoItemFlags`.
            Some("selectable") => QVariant::from(&(entry.kind == "de")),
            Some("isCurrent") => {
                QVariant::from(&(entry.kind == "de" && entry.de_id == self.rust().current_de))
            }
            _ => QVariant::default(),
        }
    }

    // ── API для QML ───────────────────────────────────────────────────
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
        // Образ применён или возвращён — плитки перечитать.
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
            // Тексты переключения — общие с «Сравнением DE»: там они
            // дословно те же, и вторая копия разъехалась бы молча.
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
// Инициализация состояния
// ============================================================================

impl cxx_qt::Initialize for qobject::DesktopsModel {
    /// Список столов собирается сразу: `rowCount` зовётся из QML раньше
    /// любого нашего кода, и пустая модель на первом кадре означала бы
    /// пустую вкладку.
    fn initialize(mut self: Pin<&mut Self>) {
        // Подписка на смену скина: одна строка на страницу, рассылка — общая
        // (`crate::skin`). Снимается в `Drop` того же состояния.
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
    /// Докачать официальные снимки столов, которых нет в кэше.
    ///
    /// Без этого на СВЕЖЕЙ машине карточки навсегда оставались бы с
    /// заглушками: страница читает снимки только с диска, а качал их Python-
    /// хозяин, которого Rust-окно не зовёт. Закачку ведёт ядро
    /// (`auto_download_all_live`) в своём потоке — окно не ждёт; каталог и
    /// имена файлов те же, что у Python, так что кэш у двух окон общий.
    ///
    /// Карточка перечитывается ОДИН раз — когда пачка показанного стола
    /// кончилась: пришли снимки либо заглушка сменила «загружается» на
    /// честную подпись. Смена карточки сбрасывает слайд карусели и заново
    /// спрашивает версию пакета, поэтому на каждый файл её не дёргаем. На
    /// тёплом кэше задач нет вовсе.
    ///
    /// Поток, «одна закачка на процесс» и состояние заглушек — у ядра
    /// (`start_auto_download_live`); здесь только перерисовка.
    fn start_screenshot_download(mut self: Pin<&mut Self>) {
        let dir = self.rust().screenshot_dir.clone();
        let handle = self.as_mut().qt_thread();
        screenshots::start_auto_download_live(&dir, move |outcome| {
            if !outcome.de_finished {
                return;
            }
            let de = outcome.de.clone();
            // Страница могла закрыться — тогда перерисовывать некого.
            let _ = handle.queue(move |mut object| {
                if object.rust().selected == de {
                    object.as_mut().card_changed();
                }
            });
        });
    }
}
