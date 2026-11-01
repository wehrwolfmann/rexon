// CompareModel — вкладка «Сравнение DE» на Rust + cxx-qt + QML.
//
// Виджетные вкладки, чья логика уже в ядре,
// переписываются на QML по одной, и `qt_compare` идёт ПЕРВОЙ — она самая
// маленькая, а её данные и решения давно живут в ядре
// (`rexon_core::qt_compare`). Схема переноса обкатана здесь: решения и
// подписи — ядро, здесь оболочка QObject, в QML только разметка;
// следующие вкладки идут по ней.
//
// Что здесь есть и чего нет:
//   * ЕСТЬ оболочка QObject: модель списка рабочих столов (роли для
//     делегата), неизменные подписи страницы, строки рекомендаций и
//     таблица «X против Y» одним вызовом;
//   * НЕТ ни одной строки продуктовых решений — какие пары предложить, что
//     написать в легенде, какого цвета полоса памяти и кому что
//     порекомендовать, отвечает ядро. Вторая копия этих таблиц разъехалась
//     бы с первой молча, а числа на витрине покупатель сверит раньше нас.
//
// Побочные действия (установка стола, переключение сессии с копией и
// выходом) в этот объект НЕ входят. Страница зовёт их у объекта `actions`:
// в окне на Rust это `DesktopsActions` (src/desktops_actions.rs, исполнитель
// ядра `system_de_exec`, окно хода `qml/DeOperationDialog.qml` — одни на обе
// вкладки), у Python-хозяина — класс `CompareActions` из
// rexon/qt_compare/window.py. Обратно приходит сигнал «состав установленных
// столов изменился».
//
// ⚠️ Состояние «установлен» модель считает САМА, а не принимает снаружи:
// ядро знает, какие исполняемые файлы у стола (`de_installed_binaries`), а
// поиск в PATH — `system::binary_in_path`. Ровно тот же ответ даёт
// `DesktopEnvironment.recheck_installed()` на Python (`shutil.which`),
// поэтому обе вкладки, старая и новая, показывают одинаковые отметки.

/// Мост cxx-qt: содержимое страницы сравнения. Оно же — модель списка столов.
///
/// Один QObject, а не два (страница + модель), намеренно: список тут
/// фиксированный и целиком принадлежит странице, а разделение потребовало бы
/// связывать объекты через QML — лишний шов на ровном месте. Для вкладок с
/// настоящими списками (расширения, обои) схема остаётся прежней: отдельная
/// модель, страница держит её свойством.
///
/// ⚠️ Тип зовётся `CompareModel`, а не `ComparePage`: .qml-файлы модуля сами
/// становятся QML-типами по имени файла, и `ComparePage` уже занят
/// `qml/ComparePage.qml`. Совпадение имён даёт «Ambiguous type detected» и
/// страницу, которая не находит собственный источник данных. Правило на все
/// вкладки: вид — `<Имя>Page.qml`, объект — `<Имя>Model`.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// Базовый Qt-тип: обёртки QAbstractListModel в cxx-qt-lib нет.
        ///
        /// ⚠️ БЕЗ `#[qobject]` — как в соседних мостах крейта: на каждый
        /// `#[qobject]`-тип в `extern "C++Qt"` cxx-qt печатает шимы
        /// приведения с именем по ТИПУ, и второй мост с той же базой даёт
        /// `duplicate symbol`. Наследование при этом работает: `#[base]`,
        /// `#[inherit]` и `#[cxx_override]` висят на наследнике.
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
        /// Набор ролей для dataChanged (в Qt6 QVector<int> == QList<int>).
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
        // ── Неизменные подписи страницы. CONSTANT, потому что за время
        // жизни вкладки они не меняются: это константы ядра.
        #[qproperty(QString, window_title, cxx_name = "windowTitle",
                    READ = window_title, CONSTANT)]
        #[qproperty(i32, window_min_width, cxx_name = "windowMinWidth",
                    READ = window_min_width, CONSTANT)]
        #[qproperty(i32, window_min_height, cxx_name = "windowMinHeight",
                    READ = window_min_height, CONSTANT)]
        #[qproperty(QString, header_title, cxx_name = "headerTitle",
                    READ = header_title, CONSTANT)]
        #[qproperty(i32, header_title_point_size, cxx_name = "headerTitlePointSize",
                    READ = header_title_point_size, CONSTANT)]
        #[qproperty(QString, install_note_text, cxx_name = "installNoteText",
                    READ = install_note_text, CONSTANT)]
        #[qproperty(QString, pairs_label, cxx_name = "pairsLabel",
                    READ = pairs_label, CONSTANT)]
        #[qproperty(QString, legend_title, cxx_name = "legendTitle",
                    READ = legend_title, CONSTANT)]
        #[qproperty(QString, legend_body, cxx_name = "legendBody",
                    READ = legend_body, CONSTANT)]
        #[qproperty(QString, table_header, cxx_name = "tableHeader",
                    READ = table_header, CONSTANT)]
        #[qproperty(QString, rec_header, cxx_name = "recHeader",
                    READ = rec_header, CONSTANT)]
        #[qproperty(i32, section_point_size, cxx_name = "sectionPointSize",
                    READ = section_point_size, CONSTANT)]
        // Быстрые пары шапки: [{label, a, b}, …].
        #[qproperty(QList_QVariant, quick_pairs, cxx_name = "quickPairs",
                    READ = quick_pairs, CONSTANT)]
        // Палитра стандартного скина приложения (steam_dark, правило #17).
        // Оформление НЕ пишется заново в QML — берётся из ui_base ядра.
        #[qproperty(QMap_QString_QVariant, palette, READ = palette,
                    NOTIFY = palette_changed)]
        // Размеры и подписи диалога «X против Y».
        #[qproperty(i32, pair_min_width, cxx_name = "pairMinWidth",
                    READ = pair_min_width, CONSTANT)]
        #[qproperty(i32, pair_min_height, cxx_name = "pairMinHeight",
                    READ = pair_min_height, CONSTANT)]
        #[qproperty(i32, pair_title_point_size, cxx_name = "pairTitlePointSize",
                    READ = pair_title_point_size, CONSTANT)]
        #[qproperty(QString, pair_close_text, cxx_name = "pairCloseText",
                    READ = pair_close_text, CONSTANT)]
        // ── Меняющееся: строки рекомендаций зависят от состава
        // установленных столов, значит после установки они другие.
        #[qproperty(QList_QVariant, rec_rows, cxx_name = "recRows",
                    READ = rec_rows, NOTIFY = rec_rows_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        type CompareModel = super::CompareModelRust;
    }

    // ── Сигналы ───────────────────────────────────────────────────────────
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
        fn palette_changed(self: Pin<&mut CompareModel>);

        #[qsignal]
        #[cxx_name = "recRowsChanged"]
        fn rec_rows_changed(self: Pin<&mut CompareModel>);

        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut CompareModel>);

        /// dataChanged базового класса — точечное обновление строки.
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut CompareModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );
    }

    // index() безопасен, но cxx-qt требует держать #[inherit] в unsafe-блоке
    // (unsafe тут про соответствие сигнатуре C++, а не про опасность).
    unsafe extern "RustQt" {
        #[inherit]
        fn index(
            self: &CompareModel,
            row: i32,
            column: i32,
            parent: &QModelIndex,
        ) -> QModelIndex;
    }

    // ── Переопределения QAbstractListModel ────────────────────────────────
    // Без #[qinvokable]: инвокабл в наследнике затеняет укороченные клоны
    // сигнатур базы, которые moc делает ради default-аргументов, и
    // rowCount() без аргументов из JS падает «Insufficient arguments».
    extern "RustQt" {
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &CompareModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &CompareModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &CompareModel, index: &QModelIndex, role: i32) -> QVariant;
    }

    // ── Чтение свойств ────────────────────────────────────────────────────
    extern "RustQt" {
        #[cxx_name = "windowTitle"]
        fn window_title(self: &CompareModel) -> QString;
        #[cxx_name = "windowMinWidth"]
        fn window_min_width(self: &CompareModel) -> i32;
        #[cxx_name = "windowMinHeight"]
        fn window_min_height(self: &CompareModel) -> i32;
        #[cxx_name = "headerTitle"]
        fn header_title(self: &CompareModel) -> QString;
        #[cxx_name = "headerTitlePointSize"]
        fn header_title_point_size(self: &CompareModel) -> i32;
        /// Строка под шапкой: какие столы ставятся на SteamOS; пусто — не
        /// SteamOS.
        #[cxx_name = "installNoteText"]
        fn install_note_text(self: &CompareModel) -> QString;
        #[cxx_name = "pairsLabel"]
        fn pairs_label(self: &CompareModel) -> QString;
        #[cxx_name = "legendTitle"]
        fn legend_title(self: &CompareModel) -> QString;
        #[cxx_name = "legendBody"]
        fn legend_body(self: &CompareModel) -> QString;
        #[cxx_name = "tableHeader"]
        fn table_header(self: &CompareModel) -> QString;
        #[cxx_name = "recHeader"]
        fn rec_header(self: &CompareModel) -> QString;
        #[cxx_name = "sectionPointSize"]
        fn section_point_size(self: &CompareModel) -> i32;
        #[cxx_name = "quickPairs"]
        fn quick_pairs(self: &CompareModel) -> QList_QVariant;
        fn palette(self: &CompareModel) -> QMap_QString_QVariant;
        #[cxx_name = "pairMinWidth"]
        fn pair_min_width(self: &CompareModel) -> i32;
        #[cxx_name = "pairMinHeight"]
        fn pair_min_height(self: &CompareModel) -> i32;
        #[cxx_name = "pairTitlePointSize"]
        fn pair_title_point_size(self: &CompareModel) -> i32;
        #[cxx_name = "pairCloseText"]
        fn pair_close_text(self: &CompareModel) -> QString;
        #[cxx_name = "recRows"]
        fn rec_rows(self: &CompareModel) -> QList_QVariant;
        fn count(self: &CompareModel) -> i32;
    }

    // ── API для QML ───────────────────────────────────────────────────────
    extern "RustQt" {
        /// Раскрыть/свернуть карточку стола. Обновляется ТОЛЬКО две роли,
        /// а не вся строка: делегат перерисовывает шапку и показывает тело,
        /// остальное на месте.
        #[qinvokable]
        fn toggle(self: Pin<&mut CompareModel>, row: i32);

        /// Перечитать состав установленных столов.
        ///
        /// Зовётся после успешной установки: без этого карточка продолжала
        /// бы предлагать «Установить» уже установленный стол, а строка
        /// рекомендации — держать кнопку вместо отметки. Ровно эта починка
        /// была сделана в виджетной вкладке (`_refresh_installed`), и
        /// консервировать баг при переносе нельзя (правило #16).
        #[qinvokable]
        #[cxx_name = "refreshInstalled"]
        fn refresh_installed(self: Pin<&mut CompareModel>);

        /// Таблица «X против Y» целиком: заголовок, HTML-таблица, кнопки
        /// установки для ещё не стоящих столов. Пустой QVariant — хотя бы
        /// один стол ядру неизвестен (диалог тогда не открывается).
        #[qinvokable]
        #[cxx_name = "pairView"]
        fn pair_view(self: &CompareModel, a_id: &QString, b_id: &QString) -> QVariant;

        /// Почему кнопка «Установить» стола `de_id` неактивна (SteamOS: стол
        /// не ставится образом systemd-sysext); пусто — активна.
        #[qinvokable]
        #[cxx_name = "installBlockedText"]
        fn install_blocked_text(self: &CompareModel, de_id: &QString) -> QString;

        /// Отображаемое имя стола — для заголовков подтверждений на стороне
        /// побочных действий.
        #[qinvokable]
        #[cxx_name = "displayName"]
        fn display_name(self: &CompareModel, de_id: &QString) -> QString;

        /// Номер роли по имени (-1, если роли нет) — для проверок без
        /// делегата, как в соседних моделях крейта.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &CompareModel, name: &QString) -> i32;

        /// Значение роли по номеру строки — проверки без делегата.
        #[qinvokable]
        #[cxx_name = "roleValue"]
        fn role_value(self: &CompareModel, row: i32, name: &QString) -> QVariant;
    }

    // Объявление трейта обязано быть В МОСТЕ, иначе `impl cxx_qt::Initialize`
    // снаружи не находит конструктора: cxx-qt печатает его по этой строке.
    impl cxx_qt::Initialize for CompareModel {}
    // Подписка на смену скина идёт через `CxxQtThread::queue`, и трейт
    // потока обязан быть объявлен В МОСТЕ: cxx-qt печатает по этой
    // строке и сам `qt_thread()`, и его алиас типа.
    impl cxx_qt::Threading for CompareModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QList, QMap, QMapPair_QString_QVariant, QModelIndex, QString, QVariant, QVector,
};

use rexon_core::qt_compare as qc;
use rexon_core::system::{binary_in_path, de_installed_binaries, DE_META};
use rexon_core::ui_base;

// ============================================================================
// Чистый слой: строки списка (Qt здесь нет)
// ============================================================================

/// Одна строка списка: идентификатор стола плюс его наблюдаемое состояние.
///
/// Всё остальное (шапка, ТТХ, описание, кнопка) считается ядром на лету —
/// это дешёвые строковые операции над константной таблицей, и держать их
/// копию в состоянии значило бы завести второй источник истины.
#[derive(Debug, Clone, Default)]
pub struct DeRow {
    pub de_id: String,
    pub installed: bool,
    pub expanded: bool,
}

/// Стоит ли стол: у него непустой список исполняемых файлов и все они в PATH.
///
/// Тот же ответ, что у `DesktopEnvironment.recheck_installed()` на Python.
pub fn de_installed(de_id: &str) -> bool {
    let binaries = de_installed_binaries(de_id);
    !binaries.is_empty() && binaries.iter().all(|b| binary_in_path(b))
}

/// Список столов в порядке базы ядра — том же, в котором их выдаёт
/// `DesktopEnvironment.load_all()` виджетной вкладке.
pub fn load_rows() -> Vec<DeRow> {
    DE_META
        .iter()
        .map(|m| DeRow {
            de_id: m.id.to_string(),
            installed: de_installed(m.id),
            expanded: false,
        })
        .collect()
}

// ============================================================================
// Роли модели
// ============================================================================

/// Роли начинаются с Qt::UserRole + 1 (Qt::UserRole == 256).
const ROLE_BASE: i32 = 256 + 1;

/// Имена ролей в порядке номеров — единственный источник истины и для
/// roleNames, и для data(), и для roleNumber().
const ROLE_NAMES: [&str; 16] = [
    "deId",
    "displayName",
    "icon",
    "installed",
    "expanded",
    "headerText",
    "ramLabel",
    "ramMax",
    "ramValue",
    "ramColor",
    "starsLabels",
    "specs",
    "description",
    "actionKind",
    "actionText",
    "actionAccent",
];

const ROLE_INSTALLED: usize = 3;
const ROLE_EXPANDED: usize = 4;
const ROLE_HEADER_TEXT: usize = 5;
const ROLE_ACTION_KIND: usize = 13;
const ROLE_ACTION_TEXT: usize = 14;
const ROLE_ACTION_ACCENT: usize = 15;

/// Роли, меняющиеся от раскрытия карточки.
fn toggle_roles() -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    roles.append(ROLE_BASE + ROLE_EXPANDED as i32);
    roles.append(ROLE_BASE + ROLE_HEADER_TEXT as i32);
    roles
}

/// Роли, меняющиеся от установки стола.
///
/// Пустой вектор («изменились все роли») здесь был бы дороже и заставил бы
/// делегат пересчитать даже ТТХ, которые не менялись.
fn installed_roles() -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    roles.append(ROLE_BASE + ROLE_INSTALLED as i32);
    roles.append(ROLE_BASE + ROLE_HEADER_TEXT as i32);
    roles.append(ROLE_BASE + ROLE_ACTION_KIND as i32);
    roles.append(ROLE_BASE + ROLE_ACTION_TEXT as i32);
    roles.append(ROLE_BASE + ROLE_ACTION_ACCENT as i32);
    roles
}

// ============================================================================
// Мелочи перевода в Qt
// ============================================================================

/// Собрать QVariantMap из пар. QML читает такую карту точкой (`row.label`).
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

/// Rust-состояние страницы.
#[derive(Default)]
pub struct CompareModelRust {
    /// Номер записи в общем реестре смены скина (`crate::skin`).
    /// 0 — страница ещё не подписана.
    skin_watch: u64,
    rows: Vec<DeRow>,
    /// Состояние собирается при первом обращении, а не в конструкторе:
    /// объявленный конструктор в cxx-qt отменяет дефолтный с родителем,
    /// а PATH на момент создания объекта уже такой же.
    loaded: bool,
}

/// Снять подписку на смену скина. Без этого реестр держал бы запись
/// закрытой вкладки до первой рассылки — та её отсеет по
/// `ObjectDestroyed`, но копить мусор незачем.
impl Drop for CompareModelRust {
    fn drop(&mut self) {
        crate::skin::unwatch(self.skin_watch);
    }
}

impl CompareModelRust {
    fn ensure_loaded(&mut self) {
        if !self.loaded {
            self.rows = load_rows();
            self.loaded = true;
        }
    }

    /// Список идентификаторов установленных столов — вход рекомендаций.
    fn installed_ids(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|r| r.installed)
            .map(|r| r.de_id.clone())
            .collect()
    }
}

impl qobject::CompareModel {
    // ── Неизменные подписи ────────────────────────────────────────────────
    fn window_title(&self) -> QString {
        QString::from(&qc::window_title())
    }

    fn window_min_width(&self) -> i32 {
        qc::WINDOW_MIN_SIZE.0 as i32
    }

    fn window_min_height(&self) -> i32 {
        qc::WINDOW_MIN_SIZE.1 as i32
    }

    fn header_title(&self) -> QString {
        QString::from(&qc::header_title())
    }

    fn header_title_point_size(&self) -> i32 {
        qc::HEADER_TITLE_POINT_SIZE
    }

    /// SteamOS (Steam Deck и Steam Machine): какие столы ставятся образами
    /// systemd-sysext — одной фразой под шапкой (та же сводка, что в окне
    /// отказа). Не SteamOS — пусто.
    fn install_note_text(&self) -> QString {
        if !rexon_core::distro_compat::running_on_steamos() {
            return QString::default();
        }
        QString::from(&rexon_core::i18n::tr(rexon_core::distro_compat::STEAMOS_SUMMARY_DESKTOPS))
    }

    /// SteamOS: проверенные столы ставятся — пусто; прочим кнопка видна, но
    /// неактивна, и причина та же, что у карточки вкладки «Рабочие столы»
    /// (`distro_compat::steamos_desktop_card_block`, одна на продукт).
    fn install_blocked_text(&self, de_id: &QString) -> QString {
        let steamos = rexon_core::distro_compat::running_on_steamos();
        QString::from(
            &rexon_core::distro_compat::steamos_desktop_card_block(&de_id.to_string(), steamos)
                .unwrap_or_default(),
        )
    }

    fn pairs_label(&self) -> QString {
        QString::from(&qc::pairs_label())
    }

    fn legend_title(&self) -> QString {
        QString::from(&qc::legend_title())
    }

    fn legend_body(&self) -> QString {
        QString::from(&qc::legend_body())
    }

    fn table_header(&self) -> QString {
        QString::from(&qc::table_header())
    }

    fn rec_header(&self) -> QString {
        QString::from(&qc::rec_header())
    }

    fn section_point_size(&self) -> i32 {
        qc::SECTION_POINT_SIZE
    }

    fn quick_pairs(&self) -> QList<QVariant> {
        let mut out = QList::<QVariant>::default();
        for (label, a, b) in qc::QUICK_PAIRS {
            out.append(QVariant::from(&vmap(vec![
                ("label", qstr(label)),
                ("a", qstr(a)),
                ("b", qstr(b)),
            ])));
        }
        out
    }

    /// Палитра ДЕЙСТВУЮЩЕГО скина (сохранённый `ui_skin`, откат к
    /// стандартному по правилу #17). Оформление QML-вкладки берётся отсюда,
    /// а не пишется заново: скин один на весь продукт, и второй его копией
    /// в .qml мы бы развели вкладку с остальным Rexon.
    fn palette(&self) -> QMap<QMapPair_QString_QVariant> {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        let colors = ui_base::active_qml_palette();
        // Тёмный ли скин — тем же критерием, что и весь продукт, а не
        // «steam_dark в названии»: скин выбирает покупатель, и «Светлый»
        // обязан считаться светлым.
        let dark = colors
            .get("bg_primary")
            .and_then(|c| ui_base::parse_hex(c))
            .map(|(r, g, b)| ui_base::is_dark_rgb(r, g, b))
            .unwrap_or(true);
        for (key, value) in &colors {
            out.insert(QString::from(key), qstr(value));
        }
        // Цвета, которые задаёт сама вкладка через ядро: отметка «стоит» в
        // рекомендациях и пороги полосы памяти живут в qt_compare.
        out.insert(
            QString::from("rec_badge"),
            qstr(qc::REC_BADGE_COLOR),
        );
        // Янтарная строка «почему неактивно» — та же краска, что у
        // «недоступен» на вкладке «Рабочие столы».
        out.insert(
            QString::from("status_unavailable"),
            qstr(rexon_core::qt_desktops::STATUS_UNAVAILABLE_COLOR),
        );
        out.insert(QString::from("spec_good"), qstr(qc::GOOD_COLOR));
        out.insert(QString::from("spec_bad"), qstr(qc::BAD_COLOR));
        // Канавка полосы памяти. Скин страницы тёмный (правило #17), но
        // спрашиваем ядро по теме, а не берём тёмный вариант руками:
        // светлый скин иначе получил бы канавку от тёмного.
        out.insert(QString::from("ram_track"), qstr(qc::ram_track_color(dark)));
        // Насыщенность вторичного текста (подпись пар, тело легенды,
        // описание стола, подстрочник рекомендации). У виджетной вкладки
        // это `font-weight: 600` из `secondary_text_qss`; без неё QML
        // набирал те же строки обычным начертанием (замер 03.09.2026:
        // описание KDE у виджетов 426 точек шириной, у QML — 417).
        out.insert(
            QString::from("text_secondary_weight"),
            QVariant::from(&ui_base::SECONDARY_TEXT_WEIGHT),
        );
        // Тон синей кнопки установки — карта, какую понимает RexonButton
        // (`tone`). Краски те же, что в `INSTALL_BUTTON_QSS` виджетной
        // вкладки; раньше QML брал `highlighted` — акцент скина с другой
        // прозрачностью, то есть свою краску вместо краски ядра.
        out.insert(
            QString::from("install_tone"),
            QVariant::from(&vmap(vec![
                ("background", qstr(qc::INSTALL_BUTTON_COLOR)),
                ("hover", qstr(qc::INSTALL_BUTTON_HOVER_COLOR)),
                ("radius", QVariant::from(&qc::INSTALL_BUTTON_RADIUS)),
                ("bold", QVariant::from(&true)),
            ])),
        );
        // «Да»/«Прервать» общего окна операции (`DeOperationDialog.qml`) —
        // тоны «Рабочих столов» по той же таблице ядра. Без них «Да» выходила
        // обычной кнопкой скина, а не синей или красной по операции.
        crate::desktops_page::insert_operation_tones(&mut out);
        // Затемнение фона под модальным окном. Своего оттенка страница не
        // заводит: гашение фона одно на продукт (`ui_base::MODAL_DIM_RGBA`),
        // тем же цветом гасит окно обучение. Следующим QML-вкладкам эту
        // строку повторять как есть.
        out.insert(
            QString::from("modal_dim"),
            qstr(&ui_base::modal_dim_argb_hex()),
        );
        out
    }

    fn pair_min_width(&self) -> i32 {
        qc::PAIR_MIN_SIZE.0 as i32
    }

    fn pair_min_height(&self) -> i32 {
        qc::PAIR_MIN_SIZE.1 as i32
    }

    fn pair_title_point_size(&self) -> i32 {
        qc::PAIR_TITLE_POINT_SIZE
    }

    fn pair_close_text(&self) -> QString {
        QString::from(&qc::pair_close_text())
    }

    // ── Рекомендации ──────────────────────────────────────────────────────
    fn rec_rows(&self) -> QList<QVariant> {
        // Читающий геттер не может дозагрузить состояние (у него &self), но
        // список столов от PATH не зависит по составу — только по отметкам,
        // а их считает `installed_ids` уже загруженного состояния. До первого
        // обращения к модели QML сюда не попадает: свойство читает та же
        // страница, что и делегаты.
        let installed = self.rust().installed_ids();
        let mut out = QList::<QVariant>::default();
        for row in qc::rec_rows(&installed) {
            let color = if row.mark == "badge" {
                qc::REC_BADGE_COLOR
            } else {
                ""
            };
            out.append(QVariant::from(&vmap(vec![
                ("labelHtml", qstr(&row.label_html)),
                ("sub", qstr(&row.sub)),
                ("deId", qstr(row.de_id)),
                ("mark", qstr(row.mark)),
                ("text", qstr(&row.text)),
                ("color", qstr(color)),
            ])));
        }
        out
    }

    fn count(&self) -> i32 {
        self.rust().rows.len() as i32
    }

    // ── QAbstractListModel ────────────────────────────────────────────────
    fn row_count(&self, parent: &QModelIndex) -> i32 {
        // У плоского списка дети есть только у корня.
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
    /// отвечал `roleValue` — иначе проверки смотрели бы не туда, куда
    /// смотрит делегат.
    fn role_of(&self, entry: &DeRow, name: Option<&str>) -> QVariant {
        // Тело карточки ядро отдаёт одной структурой: ram_label, полоса,
        // звёзды, ТТХ, описание и кнопка считаются вместе.
        // `dark` = true: вкладка идёт под стандартным скином steam_dark, а
        // от темы в теле зависит только QSS-подложка полосы, которой в QML
        // нет вовсе (цвет заполнения от темы не зависит).
        let body = qc::card_body(&entry.de_id, entry.installed, true);
        match name {
            Some("deId") => qstr(&entry.de_id),
            Some("displayName") => qstr(self.display_name_of(&entry.de_id)),
            Some("icon") => qstr(qc::de_icon(&entry.de_id)),
            Some("installed") => QVariant::from(&entry.installed),
            Some("expanded") => QVariant::from(&entry.expanded),
            Some("headerText") => {
                let text = qc::card_header(&entry.de_id, entry.expanded, entry.installed)
                    .unwrap_or_else(|| self.display_name_of(&entry.de_id).to_string());
                qstr(&text)
            }
            Some("ramLabel") => match &body {
                Some(b) => qstr(&b.ram_label),
                None => QVariant::default(),
            },
            Some("ramMax") => QVariant::from(&(qc::RAM_SCALE_MAX as i32)),
            Some("ramValue") => match &body {
                Some(b) => QVariant::from(&(b.ram_bar.value as i32)),
                None => QVariant::from(&0_i32),
            },
            Some("ramColor") => match &body {
                Some(b) => qstr(b.ram_bar.color),
                None => QVariant::default(),
            },
            Some("starsLabels") => {
                let mut out = QList::<QVariant>::default();
                if let Some(b) = &body {
                    for label in &b.stars_labels {
                        out.append(qstr(label));
                    }
                }
                QVariant::from(&out)
            }
            Some("specs") => {
                let mut out = QList::<QVariant>::default();
                for spec in qc::specs(&entry.de_id) {
                    out.append(QVariant::from(&vmap(vec![
                        ("label", qstr(&spec.label)),
                        ("valueHtml", qstr(&spec.value_html)),
                        ("color", qstr(spec.color)),
                    ])));
                }
                QVariant::from(&out)
            }
            Some("description") => match &body {
                Some(b) => qstr(&b.description),
                None => qstr(""),
            },
            Some("actionKind") => qstr(qc::card_action(&entry.de_id, entry.installed).kind),
            Some("actionText") => qstr(&qc::card_action(&entry.de_id, entry.installed).text),
            // Выделенная (синяя) кнопка — только у установки: у ядра она
            // помечена непустым QSS, здесь это флаг для QML.
            Some("actionAccent") => {
                let action = qc::card_action(&entry.de_id, entry.installed);
                QVariant::from(&!action.qss.is_empty())
            }
            _ => QVariant::default(),
        }
    }

    /// Отображаемое имя по идентификатору; сам идентификатор — если стола в
    /// базе нет (в интерфейсе это лучше пустой строки).
    fn display_name_of<'a>(&self, de_id: &'a str) -> &'a str {
        DE_META
            .iter()
            .find(|m| m.id == de_id)
            .map(|m| m.display_name)
            .unwrap_or(de_id)
    }

    // ── API для QML ───────────────────────────────────────────────────────
    fn toggle(mut self: Pin<&mut Self>, row: i32) {
        let Ok(idx) = usize::try_from(row) else {
            return;
        };
        if idx >= self.rust().rows.len() {
            return;
        }
        {
            let mut state = self.as_mut().rust_mut();
            state.rows[idx].expanded = !state.rows[idx].expanded;
        }
        let index = self.index(row, 0, &QModelIndex::default());
        let roles = toggle_roles();
        self.as_mut().data_changed(&index, &index, &roles);
    }

    fn refresh_installed(mut self: Pin<&mut Self>) {
        let rows = {
            let mut state = self.as_mut().rust_mut();
            state.ensure_loaded();
            for row in state.rows.iter_mut() {
                row.installed = de_installed(&row.de_id);
            }
            state.rows.len()
        };
        if rows > 0 {
            let first = self.index(0, 0, &QModelIndex::default());
            let last = self.index(rows as i32 - 1, 0, &QModelIndex::default());
            let roles = installed_roles();
            self.as_mut().data_changed(&first, &last, &roles);
        }
        // Строки рекомендаций считаются от состава установленных — после
        // установки у них другая правая часть (кнопка → отметка).
        self.as_mut().rec_rows_changed();
        self.as_mut().count_changed();
    }

    fn pair_view(&self, a_id: &QString, b_id: &QString) -> QVariant {
        let a_id = a_id.to_string();
        let b_id = b_id.to_string();
        let rows = &self.rust().rows;
        let installed = |id: &str| rows.iter().any(|r| r.de_id == id && r.installed);
        let Some(view) = qc::pair_view(&a_id, &b_id, installed(&a_id), installed(&b_id)) else {
            // Пара ядру неизвестна — виджетная вкладка в этом случае просто
            // не открывала диалог; пустой QVariant значит для QML то же.
            return QVariant::default();
        };
        let mut buttons = QList::<QVariant>::default();
        for button in &view.buttons {
            buttons.append(QVariant::from(&vmap(vec![
                ("deId", qstr(&button.de_id)),
                ("text", qstr(&button.text)),
                ("accent", QVariant::from(&!button.qss.is_empty())),
            ])));
        }
        QVariant::from(&vmap(vec![
            ("windowTitle", qstr(&view.window_title)),
            ("heading", qstr(&view.heading)),
            ("html", qstr(&view.html)),
            ("buttons", QVariant::from(&buttons)),
            ("closeText", qstr(&view.close_text)),
            ("minWidth", QVariant::from(&(view.min_size.0 as i32))),
            ("minHeight", QVariant::from(&(view.min_size.1 as i32))),
            ("titlePointSize", QVariant::from(&view.title_point_size)),
        ]))
    }

    fn display_name(&self, de_id: &QString) -> QString {
        QString::from(self.display_name_of(&de_id.to_string()))
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

impl cxx_qt::Initialize for qobject::CompareModel {
    /// Список столов собирается сразу при создании объекта: `rowCount`
    /// зовётся из QML раньше любого нашего кода, и пустая модель на первом
    /// кадре означала бы пустую вкладку до первого `refreshInstalled`.
    fn initialize(mut self: Pin<&mut Self>) {
        // Подписка на смену скина: одна строка на страницу, рассылка — общая
        // (`crate::skin`). Снимается в `Drop` того же состояния.
        let watch = crate::skin::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::CompareModel::palette_changed,
        );
        self.as_mut().rust_mut().skin_watch = watch;
        self.as_mut().rust_mut().ensure_loaded();
    }
}
