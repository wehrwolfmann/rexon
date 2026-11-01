// WallpaperListModel — модель библиотеки обоев страницы «Обои», перенесённая
// с Python на Rust/cxx-qt.
//
// ⚠️ Эталон здесь НЕ наш Python. Питоновская модель (bridge.py:3064) — это
// список карт и больше ничего: ни выбора, ни ключей, ни роли `selected`.
// Оригинал — `ui/src/model/list_models.cppm` waywallen поверх
// `kstore::QMetaListModel` из QExtra (`include/kstore/qt/meta_list_model.hpp`,
// `src/kstore/meta_list_model.cpp`), и выбор живёт именно В МОДЕЛИ: набор
// ВЫБРАННЫХ КЛЮЧЕЙ, а не индексов. Ключ обои — `id_proto`
// (`ui/src/msg/backend_msg.cppm`, `ItemTrait<Wallpaper>::key`).
//
// Разница не теоретическая, и порт её обязан снять (правило #16 — баги при
// переводе чиним, а не консервируем):
//
//   1. Роли `selected` у нас не было вовсе, а делегат вендора её читает
//      (`page/WallpaperPage.qml:1038`, `selected: model.selected ?? false`).
//      То есть подсветка выделенных карточек в Rexon не работала никогда.
//   2. Выбор по индексам разъезжается при любой перестройке списка:
//      пересканировали библиотеку, сменили сортировку — и выделенными
//      оказываются ЧУЖИЕ обои. По ключам такого не бывает: пропала обоя —
//      пропал и её выбор.
//   3. `selectedKeys()` оригинала отдаёт ключи В ПОРЯДКЕ СТРОК модели, а
//      хвостом — выбранные ключи, которых в списке уже нет (их не
//      выбрасывают: они вернутся, когда фильтр перестанет их скрывать).
//
// Разделение слоёв то же, что в партиях 2-3: состояние и решения «какие
// сигналы и в каком порядке» живут в чистой структуре `WallpaperRows` без
// Qt, а QObject только исполняет полученный список событий.

/// Мост cxx-qt: подкласс QAbstractListModel.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// ⚠️ Без `#[qobject]` — см. remote_list_model.rs: на каждый
        /// `#[qobject]`-тип в `extern "C++Qt"` cxx-qt печатает пару шимов
        /// приведения, собранных по имени ТИПА, и второй такой мост в крейте
        /// даёт `duplicate symbol` на линковке.
        type QAbstractListModel;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvector.h");
        type QVector_i32 = cxx_qt_lib::QVector<i32>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        /// `selectedCount` оригинала — размер набора выбранных ключей.
        #[qproperty(i32, selected_count, cxx_name = "selectedCount",
                    READ = selected_count, NOTIFY = selected_count_changed)]
        /// `selectionEnabled` оригинала. Конструктор модели обоев ставит
        /// его в true (`list_models.cpp:12`) и больше никто не трогает —
        /// вендорный QML этого свойства не пишет вовсе. Оставлено читаемым
        /// свойством, чтобы делегат мог спросить.
        #[qproperty(bool, selection_enabled, cxx_name = "selectionEnabled",
                    READ = selection_enabled, NOTIFY = selection_enabled_changed)]
        type WallpaperListModel = super::WallpaperListModelRust;
    }

    // ── Сигналы ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut WallpaperListModel>);

        #[qsignal]
        #[cxx_name = "selectedCountChanged"]
        fn selected_count_changed(self: Pin<&mut WallpaperListModel>);

        #[qsignal]
        #[cxx_name = "selectionEnabledChanged"]
        fn selection_enabled_changed(self: Pin<&mut WallpaperListModel>);

        /// `selectionChanged` оригинала: «набор выбранного стал другим».
        /// Отдельно от `selectedCountChanged`, потому что состав может
        /// смениться без изменения размера.
        #[qsignal]
        #[cxx_name = "selectionChanged"]
        fn selection_changed(self: Pin<&mut WallpaperListModel>);

        /// waywallen 0.5.0: модель просит следующую страницу (`reqFetchMore`
        /// `QMetaListModel` QExtra). Ловит его `WallpaperListQuery::fetchMore`
        /// — так сетка «Скрытых обоев» догружает строки сама. Аргумент —
        /// число строк, как у оригинала; запрос его не читает.
        #[qsignal]
        #[cxx_name = "reqFetchMore"]
        fn req_fetch_more(self: Pin<&mut WallpaperListModel>, row: i32);

        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut WallpaperListModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );
    }

    // ── Наследуемые методы базового класса ────────────────────────────────
    extern "RustQt" {
        /// # Safety
        /// Требует парного end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut WallpaperListModel>);
        /// # Safety
        /// Парный к begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut WallpaperListModel>);

        /// # Safety
        /// Диапазон обязан быть корректным, требуется парный end_insert_rows.
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut WallpaperListModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        /// # Safety
        /// Парный к begin_insert_rows.
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut WallpaperListModel>);
    }

    unsafe extern "RustQt" {
        #[inherit]
        fn index(
            self: &WallpaperListModel,
            row: i32,
            column: i32,
            parent: &QModelIndex,
        ) -> QModelIndex;
    }

    // ── Переопределения QAbstractListModel ────────────────────────────────
    // Без #[qinvokable]: инвокабл в наследнике затеняет укороченные клоны
    // сигнатур базы, которые moc делает ради default-аргументов.
    extern "RustQt" {
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &WallpaperListModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &WallpaperListModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &WallpaperListModel, index: &QModelIndex, role: i32) -> QVariant;

        /// waywallen 0.5.0: протокол бесконечной прокрутки — вид спрашивает
        /// сам (`QMetaListModel::canFetchMore`/`fetchMore` QExtra).
        #[cxx_override]
        #[cxx_name = "canFetchMore"]
        fn can_fetch_more(self: &WallpaperListModel, parent: &QModelIndex) -> bool;

        #[cxx_override]
        #[cxx_name = "fetchMore"]
        fn fetch_more(self: Pin<&mut WallpaperListModel>, parent: &QModelIndex);
    }

    extern "RustQt" {
        fn count(self: &WallpaperListModel) -> i32;
        #[cxx_name = "selectedCount"]
        fn selected_count(self: &WallpaperListModel) -> i32;
        #[cxx_name = "selectionEnabled"]
        fn selection_enabled(self: &WallpaperListModel) -> bool;
    }

    // ── Инвокаблы (имена — из оригинала, менять нельзя) ───────────────────
    extern "RustQt" {
        /// Строка целиком картой — `WallpaperPage.qml` кладёт её в
        /// `root.selectedWallpaper`, панель деталей читает поля точкой.
        #[qinvokable]
        fn item(self: &WallpaperListModel, row: i32) -> QMap_QString_QVariant;

        /// Размер обои числом. У оригинала инвокабл нужен потому, что
        /// Qt-Protobuf помечает `int64`-свойства как SCRIPTABLE false и QML
        /// не может прочитать `wallpaper.size` напрямую
        /// (`list_models.cppm:35`). У нас строки — карты, но имя и поведение
        /// обязаны совпадать: `WallpaperDetailPanel.qml` зовёт `sizeOf(wp)`.
        #[qinvokable]
        #[cxx_name = "sizeOf"]
        fn size_of(self: &WallpaperListModel, wallpaper: &QMap_QString_QVariant) -> f64;

        #[qinvokable]
        #[cxx_name = "isSelected"]
        fn is_selected(self: &WallpaperListModel, row: i32) -> bool;

        #[qinvokable]
        #[cxx_name = "setSelected"]
        fn set_selected(self: Pin<&mut WallpaperListModel>, row: i32, selected: bool) -> bool;

        #[qinvokable]
        #[cxx_name = "toggleSelected"]
        fn toggle_selected(self: Pin<&mut WallpaperListModel>, row: i32) -> bool;

        #[qinvokable]
        #[cxx_name = "selectRange"]
        fn select_range(
            self: Pin<&mut WallpaperListModel>,
            from: i32,
            to: i32,
            selected: bool,
        ) -> bool;

        #[qinvokable]
        #[cxx_name = "clearSelection"]
        fn clear_selection(self: Pin<&mut WallpaperListModel>);

        #[qinvokable]
        #[cxx_name = "setSelectedKeys"]
        fn set_selected_keys(self: Pin<&mut WallpaperListModel>, keys: &QStringList);

        #[qinvokable]
        #[cxx_name = "selectedKeys"]
        fn selected_keys(self: &WallpaperListModel) -> QStringList;

        /// Ключи выбранных обоей, которые ДОПУСКАЮТ удаление
        /// (`supportsItemRemove`). Оригинал считает это в
        /// `WallpaperSelectStorage::removableSelectedWallpaperIds`, перебирая
        /// `selectedItems()`; у нас признак лежит в строке модели, поэтому
        /// считает модель — а хранилище выбора только спрашивает.
        #[qinvokable]
        #[cxx_name = "removableSelectedKeys"]
        fn removable_selected_keys(self: &WallpaperListModel) -> QStringList;

        /// Выбранные строки целиком — `selectedItems()` оригинала.
        #[qinvokable]
        #[cxx_name = "selectedItems"]
        fn selected_items(self: &WallpaperListModel) -> QList_QVariant;

        /// Роль по имени — ею проверки читают строки, не гадая о номерах.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &WallpaperListModel, name: &QString) -> i32;

        /// Значение роли по имени (для проверок из QML).
        #[qinvokable]
        #[cxx_name = "roleValue"]
        fn role_value(self: &WallpaperListModel, row: i32, name: &QString) -> QVariant;
    }

    impl cxx_qt::Threading for WallpaperListModel {}
    impl cxx_qt::Constructor<()> for WallpaperListModel {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QModelIndex, QString, QStringList, QVariant, QVector};
use std::collections::BTreeSet;

use rexon_core::we_settings::VMap;
use serde_json::Value;

use crate::qvalue::{json_to_variant, variant_to_json};

// ============================================================================
// Чистый слой: строки, выбор и события — без единого Qt-типа
// ============================================================================

/// Ключ строки — `id_proto` (`ItemTrait<Wallpaper>::key` оригинала).
pub const KEY_FIELD: &str = "id_proto";

/// Признак «обою можно удалить» — по нему считается removableSelected*.
pub const REMOVABLE_FIELD: &str = "supportsItemRemove";
/// waywallen 0.4.4: признак «обоина скрыта» в записи библиотеки.
pub const HIDDEN_FIELD: &str = "hidden";

/// Ключи записи библиотеки в том же порядке, в каком их кладёт ядро
/// (`rexon_core::we_wallpaper::entry_to_map`). Роли считаются от этого
/// порядка, поэтому список обязан совпадать по составу с конвертером ядра;
/// имена — свойства ГАДЖЕТА оригинала (camelCase, `id` → `id_proto`).
pub const ENTRY_KEYS: &[&str] = &[
    "id_proto",
    "name",
    "wpType",
    "resource",
    "preview",
    "size",
    "width",
    "height",
    "contentRating",
    "tags",
    "description",
    "externalId",
    "userPropertiesSchema",
    "userPropertyOverrides",
    "wallpaperLayoutOverride",
    "wallpaperLayoutOverrideSet",
    "supportsItemRemove",
    "supportsItemUnsubscribe",
    // v0.3.5: страница обоины на сайте источника.
    "webUrl",
    "metadata",
    // v0.4.4: обоина скрыта (`wallpaper_hide`). В КОНЦЕ списка — номера
    // ролей прежних ключей не сдвигаются.
    "hidden",
];

/// `Qt::UserRole`. Роль выбора у оригинала — ровно она
/// (`QMetaListModel::SelectedRole`), поля записи идут следом.
pub const SELECTED_ROLE: i32 = 0x0100;

/// Роль первого поля записи.
pub const FIRST_ENTRY_ROLE: i32 = SELECTED_ROLE + 1;

/// Что модель обязана сообщить Qt после изменения состояния.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelEvent {
    /// Полная перестройка (begin/endResetModel вокруг).
    Reset,
    /// countChanged.
    CountChanged,
    /// dataChanged по роли выбора на диапазоне строк.
    SelectedRange { first: i32, last: i32 },
    /// selectedCountChanged.
    SelectedCountChanged,
    /// selectionChanged.
    SelectionChanged,
}

/// Строки библиотеки и выбор по ключам.
#[derive(Debug, Default)]
pub struct WallpaperRows {
    rows: Vec<VMap>,
    /// Выбранные КЛЮЧИ. Множество, а не индексы: перестройка списка не
    /// должна переносить выделение на чужие обои.
    selected: BTreeSet<String>,
    selection_enabled: bool,
    /// `hasMore` `QMetaListModel`: у запроса есть следующая страница.
    /// Ставит запрос-владелец по ответу демона (waywallen 0.5.0).
    has_more: bool,
}

impl WallpaperRows {
    /// Модель обоев создаётся с включённым выбором (`list_models.cpp:12`).
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            selected: BTreeSet::new(),
            selection_enabled: true,
            has_more: false,
        }
    }

    pub fn has_more(&self) -> bool {
        self.has_more
    }

    pub fn set_has_more(&mut self, value: bool) {
        self.has_more = value;
    }

    /// `canFetchMore`: у плоского списка дети есть только у корня.
    pub fn can_fetch_more(&self, root: bool) -> bool {
        root && self.has_more
    }

    /// `QMetaListModel::fetchMore`: флаг гасится ДО запроса — иначе вид
    /// дёргал бы догрузку повторно, пока страница в полёте. `true` —
    /// послать `reqFetchMore`.
    pub fn fetch_more(&mut self, root: bool) -> bool {
        if !self.can_fetch_more(root) {
            return false;
        }
        self.has_more = false;
        true
    }

    /// Дописать страницу в хвост (догрузка). Выбор, как и при сбросе, не
    /// трогается: выбранная ранее обоя, пришедшая со страницей, сразу
    /// подсвечена.
    pub fn append_rows(&mut self, rows: Vec<VMap>) -> Vec<ModelEvent> {
        if rows.is_empty() {
            return Vec::new();
        }
        self.rows.extend(rows);
        let mut out = vec![ModelEvent::CountChanged];
        if !self.selected.is_empty() {
            out.push(ModelEvent::SelectionChanged);
        }
        out
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn rows(&self) -> &[VMap] {
        &self.rows
    }

    pub fn row(&self, row: i32) -> Option<&VMap> {
        usize::try_from(row).ok().and_then(|r| self.rows.get(r))
    }

    pub fn selection_enabled(&self) -> bool {
        self.selection_enabled
    }

    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    /// Ключ строки. Пустая строка означает «строки нет или ключа нет» — у
    /// оригинала это `selectionKeyAt`, и по пустому ключу он не выбирает.
    pub fn key_at(&self, row: i32) -> String {
        self.row(row)
            .and_then(|m| m.get(KEY_FIELD))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    }

    pub fn is_selected(&self, row: i32) -> bool {
        if !self.selection_enabled {
            return false;
        }
        let key = self.key_at(row);
        !key.is_empty() && self.selected.contains(&key)
    }

    /// Полная замена строк. Выбор НЕ чистится: у оригинала он живёт в
    /// ключах и переживает перестройку списка (обоя вернулась — вернулось и
    /// её выделение).
    pub fn reset_rows(&mut self, rows: Vec<VMap>) -> Vec<ModelEvent> {
        let count_changed = self.rows.len() != rows.len();
        let selected_before = self.selected_count();
        self.rows = rows;
        let mut out = vec![ModelEvent::Reset];
        if count_changed {
            out.push(ModelEvent::CountChanged);
        }
        // Размер выбора не меняется (ключи те же), но состав ВИДИМОГО
        // выбора мог поехать — страница пересчитывает по selectionChanged.
        if selected_before > 0 {
            out.push(ModelEvent::SelectionChanged);
        }
        out
    }

    pub fn set_selected(&mut self, row: i32, selected: bool) -> (bool, Vec<ModelEvent>) {
        if !self.selection_enabled {
            return (false, Vec::new());
        }
        let key = self.key_at(row);
        if key.is_empty() {
            return (false, Vec::new());
        }
        if self.selected.contains(&key) == selected {
            return (false, Vec::new());
        }
        if selected {
            self.selected.insert(key);
        } else {
            self.selected.remove(&key);
        }
        (
            true,
            vec![
                ModelEvent::SelectedRange {
                    first: row,
                    last: row,
                },
                ModelEvent::SelectedCountChanged,
                ModelEvent::SelectionChanged,
            ],
        )
    }

    pub fn toggle_selected(&mut self, row: i32) -> (bool, Vec<ModelEvent>) {
        let next = !self.is_selected(row);
        self.set_selected(row, next)
    }

    /// Диапазон строк. Границы прижимаются к списку и переставляются
    /// местами, если пришли задом наперёд, — как `std::clamp` + `swap`
    /// оригинала.
    pub fn select_range(&mut self, from: i32, to: i32, selected: bool) -> (bool, Vec<ModelEvent>) {
        if !self.selection_enabled || self.rows.is_empty() {
            return (false, Vec::new());
        }
        let last_row = self.rows.len() as i32 - 1;
        let mut first = from.clamp(0, last_row);
        let mut last = to.clamp(0, last_row);
        if first > last {
            std::mem::swap(&mut first, &mut last);
        }
        let mut changed = false;
        for row in first..=last {
            let key = self.key_at(row);
            if key.is_empty() || self.selected.contains(&key) == selected {
                continue;
            }
            changed = true;
            if selected {
                self.selected.insert(key);
            } else {
                self.selected.remove(&key);
            }
        }
        if !changed {
            return (false, Vec::new());
        }
        (
            true,
            vec![
                ModelEvent::SelectedRange { first, last },
                ModelEvent::SelectedCountChanged,
                ModelEvent::SelectionChanged,
            ],
        )
    }

    pub fn clear_selection(&mut self) -> Vec<ModelEvent> {
        if self.selected.is_empty() {
            return Vec::new();
        }
        self.selected.clear();
        self.all_rows_events()
    }

    pub fn set_selected_keys(&mut self, keys: &[String]) -> Vec<ModelEvent> {
        let next: BTreeSet<String> = keys.iter().filter(|k| !k.is_empty()).cloned().collect();
        if next == self.selected {
            return Vec::new();
        }
        self.selected = next;
        self.all_rows_events()
    }

    /// Ключи выбранного: сперва в порядке строк модели, потом — выбранные
    /// ключи, которых в списке сейчас нет. Порядок ровно как у оригинала;
    /// хвост нужен, потому что фильтр может временно прятать выбранное.
    pub fn selected_keys(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.selected.len());
        let mut seen = BTreeSet::new();
        for row in 0..self.rows.len() as i32 {
            let key = self.key_at(row);
            if !key.is_empty() && self.selected.contains(&key) {
                seen.insert(key.clone());
                out.push(key);
            }
        }
        for key in &self.selected {
            if !seen.contains(key) {
                out.push(key.clone());
            }
        }
        out
    }

    /// Ключи выбранного, у которых стоит `supportsItemRemove`.
    ///
    /// Считается ТОЛЬКО по видимым строкам: признак живёт в записи, а про
    /// ключ, которого в списке нет, мы ничего не знаем — и просить демона
    /// удалить его вслепую нельзя.
    pub fn removable_selected_keys(&self) -> Vec<String> {
        let mut out = Vec::new();
        for row in 0..self.rows.len() as i32 {
            if !self.is_selected(row) {
                continue;
            }
            let removable = self
                .row(row)
                .and_then(|m| m.get(REMOVABLE_FIELD))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let key = self.key_at(row);
            if removable && !key.is_empty() {
                out.push(key);
            }
        }
        out
    }

    /// Сколько выбранных строк скрыто — `hiddenSelectedCount` оригинала
    /// (waywallen 0.4.4). Как и у оригинала, считается по выбранным
    /// ЗАПИСЯМ: ключ, которого в списке нет, ничего о себе не говорит.
    pub fn hidden_selected_count(&self) -> usize {
        self.selected_items()
            .into_iter()
            .filter(|m| m.get(HIDDEN_FIELD).and_then(Value::as_bool).unwrap_or(false))
            .count()
    }

    /// Выбранные строки целиком, в порядке модели.
    pub fn selected_items(&self) -> Vec<&VMap> {
        (0..self.rows.len() as i32)
            .filter(|row| self.is_selected(*row))
            .filter_map(|row| self.row(row))
            .collect()
    }

    fn all_rows_events(&self) -> Vec<ModelEvent> {
        let mut out = Vec::new();
        if !self.rows.is_empty() {
            out.push(ModelEvent::SelectedRange {
                first: 0,
                last: self.rows.len() as i32 - 1,
            });
        }
        out.push(ModelEvent::SelectedCountChanged);
        out.push(ModelEvent::SelectionChanged);
        out
    }
}

/// Номер роли по имени. `selected` — роль выбора, остальные — поля записи.
pub fn role_of(name: &str) -> Option<i32> {
    if name == "selected" {
        return Some(SELECTED_ROLE);
    }
    ENTRY_KEYS
        .iter()
        .position(|k| *k == name)
        .map(|i| FIRST_ENTRY_ROLE + i as i32)
}

/// Имя поля записи по номеру роли.
pub fn key_of(role: i32) -> Option<&'static str> {
    usize::try_from(role - FIRST_ENTRY_ROLE)
        .ok()
        .and_then(|i| ENTRY_KEYS.get(i).copied())
}

// ============================================================================
// Qt-слой
// ============================================================================

/// Rust-состояние модели.
pub struct WallpaperListModelRust {
    rows: WallpaperRows,
}

impl Default for WallpaperListModelRust {
    fn default() -> Self {
        Self {
            rows: WallpaperRows::new(),
        }
    }
}

impl cxx_qt::Initialize for qobject::WallpaperListModel {
    fn initialize(self: Pin<&mut Self>) {
        // Состояние по умолчанию уже правильное (выбор включён); метод
        // нужен, потому что #[base]-объекты cxx-qt конструирует через
        // Initialize.
        let _ = self;
    }
}

fn role_vector(role: i32) -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    roles.append(role);
    roles
}

impl qobject::WallpaperListModel {
    pub fn count(&self) -> i32 {
        self.rows.len() as i32
    }

    pub fn selected_count(&self) -> i32 {
        self.rows.selected_count() as i32
    }

    pub fn selection_enabled(&self) -> bool {
        self.rows.selection_enabled()
    }

    pub fn row_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() {
            0
        } else {
            self.count()
        }
    }

    pub fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut names = qobject::QHash_i32_QByteArray::default();
        // `selected` объявляется только при включённом выборе — так же, как
        // у оригинала (`meta_list_model.cpp:278`).
        if self.rows.selection_enabled() {
            names.insert(SELECTED_ROLE, cxx_qt_lib::QByteArray::from("selected"));
        }
        for (i, key) in ENTRY_KEYS.iter().enumerate() {
            names.insert(
                FIRST_ENTRY_ROLE + i as i32,
                cxx_qt_lib::QByteArray::from(*key),
            );
        }
        names
    }

    pub fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let row = index.row();
        if !index.is_valid() || self.rows.row(row).is_none() {
            return QVariant::default();
        }
        if role == SELECTED_ROLE {
            return QVariant::from(&self.rows.is_selected(row));
        }
        let Some(key) = key_of(role) else {
            return QVariant::default();
        };
        match self.rows.row(row).and_then(|m| m.get(key)) {
            Some(value) => json_to_variant(value),
            None => QVariant::default(),
        }
    }

    pub fn item(&self, row: i32) -> qobject::QMap_QString_QVariant {
        let mut out = QMap::<QMapPair_QString_QVariant>::default();
        if let Some(map) = self.rows.row(row) {
            for (key, value) in map {
                out.insert(QString::from(key), json_to_variant(value));
            }
        }
        out
    }

    pub fn size_of(&self, wallpaper: &qobject::QMap_QString_QVariant) -> f64 {
        // Карта приходит из `item()`, то есть значение уже число; но QML
        // может отдать и строку (панель деталей зовёт sizeOf на том, что
        // положил в свойство), поэтому разбираем оба вида.
        let Some(value) = wallpaper.get(&QString::from("size")) else {
            return 0.0;
        };
        match variant_to_json(&value) {
            Value::Number(n) => n.as_f64().unwrap_or_default(),
            Value::String(s) => s.parse::<f64>().unwrap_or_default(),
            _ => 0.0,
        }
    }

    pub fn is_selected(&self, row: i32) -> bool {
        self.rows.is_selected(row)
    }

    pub fn set_selected(mut self: Pin<&mut Self>, row: i32, selected: bool) -> bool {
        let (changed, events) = self.as_mut().rust_mut().rows.set_selected(row, selected);
        self.emit_events(&events);
        changed
    }

    pub fn toggle_selected(mut self: Pin<&mut Self>, row: i32) -> bool {
        let (changed, events) = self.as_mut().rust_mut().rows.toggle_selected(row);
        self.emit_events(&events);
        changed
    }

    pub fn select_range(mut self: Pin<&mut Self>, from: i32, to: i32, selected: bool) -> bool {
        let (changed, events) = self
            .as_mut()
            .rust_mut()
            .rows
            .select_range(from, to, selected);
        self.emit_events(&events);
        changed
    }

    pub fn clear_selection(mut self: Pin<&mut Self>) {
        let events = self.as_mut().rust_mut().rows.clear_selection();
        self.emit_events(&events);
    }

    pub fn set_selected_keys(mut self: Pin<&mut Self>, keys: &QStringList) {
        let keys: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        let events = self.as_mut().rust_mut().rows.set_selected_keys(&keys);
        self.emit_events(&events);
    }

    pub fn selected_keys(&self) -> QStringList {
        QStringList::from_iter(self.rows.selected_keys().iter().map(QString::from))
    }

    pub fn removable_selected_keys(&self) -> QStringList {
        QStringList::from_iter(self.rows.removable_selected_keys().iter().map(QString::from))
    }

    pub fn selected_items(&self) -> qobject::QList_QVariant {
        let mut out = qobject::QList_QVariant::default();
        for map in self.rows.selected_items() {
            out.append(json_to_variant(&Value::Object(map.clone())));
        }
        out
    }

    pub fn role_number(&self, name: &QString) -> i32 {
        role_of(&name.to_string()).unwrap_or(-1)
    }

    pub fn role_value(&self, row: i32, name: &QString) -> QVariant {
        let Some(role) = role_of(&name.to_string()) else {
            return QVariant::default();
        };
        let index = self.index(row, 0, &QModelIndex::default());
        self.data(&index, role)
    }

    fn can_fetch_more(&self, parent: &QModelIndex) -> bool {
        self.rows.can_fetch_more(!parent.is_valid())
    }

    // pub: догрузку дёргает стенд QML так же, как это делает сетка.
    pub fn fetch_more(mut self: Pin<&mut Self>, parent: &QModelIndex) {
        if self.as_mut().rust_mut().rows.fetch_more(!parent.is_valid()) {
            let rows = self.count();
            self.req_fetch_more(rows);
        }
    }

    // ── Rust-API для запроса-владельца ────────────────────────────────────

    /// Флаг «есть следующая страница» (`setHasMore` оригинала).
    pub fn set_page_has_more(mut self: Pin<&mut Self>, value: bool) {
        self.as_mut().rust_mut().rows.set_has_more(value);
    }

    /// Строк в модели — запрос дописывает ей только недостающий хвост.
    pub fn row_total(&self) -> usize {
        self.rows.len()
    }

    /// Догрузка страницы: объявить вставку, дописать, закрыть, потом хвост
    /// сигналов.
    pub fn append_rows(mut self: Pin<&mut Self>, rows: Vec<VMap>) {
        if rows.is_empty() {
            return;
        }
        let first = self.rows.len() as i32;
        let last = first + rows.len() as i32 - 1;
        let events = unsafe {
            self.as_mut()
                .begin_insert_rows(&QModelIndex::default(), first, last);
            let events = self.as_mut().rust_mut().rows.append_rows(rows);
            self.as_mut().end_insert_rows();
            events
        };
        self.emit_events(&events);
    }

    /// Полная замена строк библиотеки (ответ демона на `wallpaper_list`).
    pub fn reset_rows(mut self: Pin<&mut Self>, rows: Vec<VMap>) {
        let events = unsafe {
            self.as_mut().begin_reset_model();
            let events = self.as_mut().rust_mut().rows.reset_rows(rows);
            self.as_mut().end_reset_model();
            events
        };
        self.emit_events(&events);
    }

    /// Ключи выбранного — ими пользуется хранилище выбора.
    pub fn selected_keys_vec(&self) -> Vec<String> {
        self.rows.selected_keys()
    }

    pub fn removable_selected_keys_vec(&self) -> Vec<String> {
        self.rows.removable_selected_keys()
    }

    pub fn selected_count_value(&self) -> i32 {
        self.rows.selected_count() as i32
    }

    pub fn hidden_selected_count_value(&self) -> i32 {
        self.rows.hidden_selected_count() as i32
    }

    pub fn emit_events(mut self: Pin<&mut Self>, events: &[ModelEvent]) {
        for event in events {
            match event {
                // Сброс уже объявлен begin/endResetModel вокруг правки.
                ModelEvent::Reset => {}
                ModelEvent::CountChanged => self.as_mut().count_changed(),
                ModelEvent::SelectedRange { first, last } => {
                    let top = self.as_ref().index(*first, 0, &QModelIndex::default());
                    let bottom = self.as_ref().index(*last, 0, &QModelIndex::default());
                    self.as_mut()
                        .data_changed(&top, &bottom, &role_vector(SELECTED_ROLE));
                }
                ModelEvent::SelectedCountChanged => self.as_mut().selected_count_changed(),
                ModelEvent::SelectionChanged => self.as_mut().selection_changed(),
            }
        }
    }
}
