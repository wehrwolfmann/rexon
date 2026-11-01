// RemoteListModel — модель выдачи страницы Discover вкладки Wallpaper
// Engine, перенесённая с Python на Rust/cxx-qt.
//
// Эталон —
// rexon/qt_wallpaper_engine/qml_ui/bridge.py:566 (сам он зеркало
// model::RemoteListModel waywallen, роли 1:1 с remote_model.cpp). Вендорный
// QML (page/DiscoverPage.qml, page/RemoteCard.qml) НЕ трогаем, поэтому имена
// типа, ролей, свойств, сигналов и инвокаблов обязаны совпадать посимвольно.
//
// ⚠️ Ради чего вообще затевался уход с PySide6. Именно на этой модели
// исторически падало: пользователь закрывал вкладку, пока ответ
// `remote_search` шёл от демона, колбэк приходил на уже удалённый объект и
// давал «RuntimeError: Internal C++ object (RemoteListModel) already
// deleted», а обработчик исключения добивал вторым — зона SIGSEGV.
// В Python это закрыто вручную: карта `_pending` хранит владельца-QObject и
// при доставке проверяется `shiboken6.isValid(owner)`. Здесь такой ветки нет
// вообще: единственный путь доставки — `CxxQtThread::queue`, а он на мёртвом
// объекте возвращает типизированную ошибку `ObjectDestroyed` и на объекте,
// умершем ПОСЛЕ постановки в очередь, просто ничего не делает. Правило —
// `queue(...).ok()`, без `unwrap()` и без предварительного `is_destroyed()`
// (между проверкой и постановкой объект успевает умереть — гонка описана
// прямо в doc-комментарии cxx-qt). Доказательство — `selftest::run()`.
//
// Разделение слоёв то же, что в остальном крейте: всё состояние и все
// решения «какие сигналы и в каком порядке» живут в чистой структуре
// `RemoteRows` без Qt (её и проверяет голден против ЖИВОЙ Python-модели), а
// QObject только исполняет полученный список событий.

/// Мост cxx-qt: подкласс QAbstractListModel.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// Базовый Qt-тип: обёртки QAbstractListModel в cxx-qt-lib нет.
        ///
        /// ⚠️ Объявлен БЕЗ `#[qobject]` — как и во втором мосте крейта
        /// (user_property_model.rs) и по той же причине: на каждый
        /// `#[qobject]`-тип в `extern "C++Qt"` cxx-qt печатает пару шимов
        /// приведения `cxx_qt_ffi_<Тип>_upcastPtr/downcastPtr`, имя которых
        /// собрано по имени ТИПА, а не моста. Второй такой мост в крейте
        /// даёт `duplicate symbol` на линковке. Без `#[qobject]` шимы не
        /// печатаются, а `#[base]`, `#[inherit]` и `#[cxx_override]`
        /// работают: они висят на наследнике.
        type QAbstractListModel;

        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmap.h");
        /// QVariantMap — им вендорный QML получает строку целиком
        /// (`detailStore.item = searchQuery.model.item(index)`).
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvector.h");
        /// Набор ролей для dataChanged (в Qt6 QVector<int> == QList<int>).
        type QVector_i32 = cxx_qt_lib::QVector<i32>;
    }

    // Фабрика и снос объектов: `new` и `delete` живут только в C++.
    // Нужны стенду поздних колбэков — он создаёт модель без родителя и
    // намеренно убивает её, пока ответ в полёте.
    unsafe extern "C++" {
        // QObject приходится объявить руками. Сам cxx-qt печатает это
        // объявление, только если в мосте есть QObject без базы либо блок
        // `extern "C++Qt"` с `#[qobject]`-типом (cxx-qt-gen/generator/rust:
        // add_qobject_import). У нас ни того, ни другого: база объявлена
        // обычным `extern "C++"` ради ухода от duplicate symbol, — значит,
        // объявление наше.
        #[namespace = ""]
        type QObject = cxx_qt::QObject;

        include!("rexon_gui_factory.h");
        /// # Safety
        /// parent либо nullptr (владение у зовущего), либо живой QObject.
        //
        // Явный allow: cxx переносит объявление в свой блок extern, теряя
        // doc-комментарий, и clippy видит unsafe-функцию «без раздела Safety».
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_new_remote_list_model(parent: *mut QObject) -> *mut QObject;
        /// # Safety
        /// Объект обязан быть создан без родителя и не иметь детей-владельцев.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn rexon_delete_object(object: *mut QObject);
        /// Прокрутить цикл событий не дольше ms миллисекунд.
        fn rexon_process_events(ms: i32);
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        // hasMore читается только: у Python-модели свойство тоже без WRITE,
        // а внутрь его двигает set_has_more/страница выдачи.
        #[qproperty(bool, has_more, cxx_name = "hasMore",
                    READ = has_more, NOTIFY = has_more_changed)]
        type RemoteListModel = super::RemoteListModelRust;
    }

    // ── Сигналы ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut RemoteListModel>);

        /// hasMoreChanged несёт значение — так объявлен и оригинал
        /// (`Q_SIGNAL void hasMoreChanged(bool)` в qextra), и Python-мост.
        #[qsignal]
        #[cxx_name = "hasMoreChanged"]
        fn has_more_changed(self: Pin<&mut RemoteListModel>, value: bool);

        /// Модель просит следующую страницу. Аргумент — номер строки, с
        /// которой продолжать (у оригинала это `reqFetchMore(qint32)`,
        /// который ловит RemoteSearchQuery::fetchMore).
        #[qsignal]
        #[cxx_name = "reqFetchMore"]
        fn req_fetch_more(self: Pin<&mut RemoteListModel>, row: i32);

        /// dataChanged базового класса — точечное обновление одной роли.
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut RemoteListModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );

        // ⚠️ modelReset и rowsInserted сюда НЕ добавлять. Это приватные
        // сигналы Qt: у них есть скрытый параметр
        // QAbstractItemModel::QPrivateSignal, и `#[inherit] #[qsignal]`
        // на них не собирается — cxx-qt берёт указатель на член вида
        // `void (RemoteListModel::*)()`, а настоящий тип
        // `void (QAbstractItemModel::*)(QPrivateSignal)`, компилятор
        // отказывается приводить. Подписаться на них можно из QML
        // (moc такие сигналы отдаёт нормально).
    }

    // ── Наследуемые методы базового класса ────────────────────────────────
    extern "RustQt" {
        /// # Safety
        /// Требует парного end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut RemoteListModel>);
        /// # Safety
        /// Парный к begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut RemoteListModel>);

        /// # Safety
        /// Диапазон обязан быть корректным, требуется парный end_insert_rows.
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut RemoteListModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        /// # Safety
        /// Парный к begin_insert_rows.
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut RemoteListModel>);
    }

    // index() безопасен для вызова, но cxx-qt требует, чтобы безопасные
    // #[inherit]-функции жили в unsafe-блоке (unsafe тут про соответствие
    // объявления C++-сигнатуре, а не про опасность вызова).
    unsafe extern "RustQt" {
        #[inherit]
        fn index(
            self: &RemoteListModel,
            row: i32,
            column: i32,
            parent: &QModelIndex,
        ) -> QModelIndex;
    }

    // ── Переопределения QAbstractListModel ────────────────────────────────
    // Без #[qinvokable]: инвокабл в наследнике затеняет укороченные клоны
    // сигнатур базы, которые moc делает ради default-аргументов, и rowCount()
    // без аргументов из JS падает «Insufficient arguments».
    extern "RustQt" {
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &RemoteListModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &RemoteListModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &RemoteListModel, index: &QModelIndex, role: i32) -> QVariant;

        /// Протокол бесконечной прокрутки: вид спрашивает сам, вручную
        /// loadMore в QML с v0.2.5 больше нет.
        #[cxx_override]
        #[cxx_name = "canFetchMore"]
        fn can_fetch_more(self: &RemoteListModel, parent: &QModelIndex) -> bool;

        #[cxx_override]
        #[cxx_name = "fetchMore"]
        fn fetch_more(self: Pin<&mut RemoteListModel>, parent: &QModelIndex);
    }

    // ── Property-геттеры ──────────────────────────────────────────────────
    extern "RustQt" {
        fn count(self: &RemoteListModel) -> i32;
        #[cxx_name = "hasMore"]
        fn has_more(self: &RemoteListModel) -> bool;
    }

    // ── API для QML ───────────────────────────────────────────────────────
    extern "RustQt" {
        /// Строка целиком как QVariantMap. Вендорный QML зовёт `item(index)`
        /// (DiscoverPage.qml:175), `get` — прежнее имя моста.
        #[qinvokable]
        fn get(self: &RemoteListModel, row: i32) -> QMap_QString_QVariant;
        #[qinvokable]
        fn item(self: &RemoteListModel, row: i32) -> QMap_QString_QVariant;

        /// Совместимость: булев вход → состояние приобретения 3/0.
        #[qinvokable]
        #[cxx_name = "setInstalled"]
        fn set_installed(
            self: Pin<&mut RemoteListModel>,
            source_id: &QString,
            item_id: &QString,
            installed: bool,
        );

        /// Состояние приобретения строки (0 — нет, 1 — в очереди, 2 — идёт,
        /// 3 — есть/подписан). Точечный dataChanged ровно по одной роли.
        #[qinvokable]
        #[cxx_name = "setAcquisitionState"]
        fn set_acquisition_state(
            self: Pin<&mut RemoteListModel>,
            source_id: &QString,
            item_id: &QString,
            state: i32,
        );

        /// Дописать имена авторов в уже показанные карточки: ключ — itemId,
        /// значение — имя. В Python это обычный метод (его зовёт запрос), а
        /// не слот; здесь он инвокабл — так им может пользоваться и стенд,
        /// и будущий Rust-запрос партии 3. Вендорный QML его не знает, так
        /// что расширение поверхности безопасно.
        #[qinvokable]
        #[cxx_name = "setAuthors"]
        fn set_authors(self: Pin<&mut RemoteListModel>, names: &QMap_QString_QVariant);

        /// То же для аватарок авторов: ключ — itemId, значение — адрес
        /// картинки. Отдельным инвокаблом, а не полем в setAuthors, потому
        /// что трогает СВОЮ роль: карточка, у которой аватарка не
        /// скачалась, обязана остаться ровно такой, как была.
        #[qinvokable]
        #[cxx_name = "setAuthorAvatars"]
        fn set_author_avatars(
            self: Pin<&mut RemoteListModel>,
            avatars: &QMap_QString_QVariant,
        );

        /// Положить страницу выдачи. Шов для партии 3 (там страницу будет
        /// подавать Rust-запрос) и вход для стенда: JSON — массив объектов с
        /// семью полями ролей. append=false — полная замена (reset_rows),
        /// true — догрузка (append_rows). Возвращает число разобранных строк.
        #[qinvokable]
        #[cxx_name = "applyPageJson"]
        fn apply_page_json(
            self: Pin<&mut RemoteListModel>,
            json: &QString,
            has_more: bool,
            append: bool,
        ) -> i32;

        /// Номер роли по имени (-1, если роли нет) — чтобы стенд мог сверить,
        /// какая именно роль приехала в dataChanged.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &RemoteListModel, name: &QString) -> i32;

        /// Значение роли по номеру строки — проверки без делегата.
        #[qinvokable]
        #[cxx_name = "roleValue"]
        fn role_value(self: &RemoteListModel, row: i32, name: &QString) -> QVariant;

        /// Разослать состояние приобретения через ОБЩИЙ склад — то есть во
        /// все живые модели сразу, а не только в эту. Зеркало
        /// `_acq_store().set()`. Возвращает число живых получателей.
        #[qinvokable]
        #[cxx_name = "publishAcquisitionState"]
        fn publish_acquisition_state(
            self: &RemoteListModel,
            source_id: &QString,
            item_id: &QString,
            state: i32,
        ) -> i32;
    }

    impl cxx_qt::Threading for RemoteListModel {}
    impl cxx_qt::Initialize for RemoteListModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QMap, QMapPair_QString_QVariant, QModelIndex, QString, QVariant, QVector,
};

use serde::Deserialize;
use serde_json::{Map, Value};

// ============================================================================
// Чистый слой: строки выдачи и решения о сигналах (Qt здесь нет)
// ============================================================================

/// Роли начинаются с Qt::UserRole + 1 (Qt::UserRole == 256) — те же номера,
/// что у Python-модели и у оригинала (remote_model.cpp).
pub const ROLE_BASE: i32 = 256 + 1;

/// Имена ролей в порядке номеров — единственный источник истины и для
/// roleNames, и для data(), и для roleNumber(). Порядок менять нельзя:
/// вендорный QML обращается к ролям по именам, а номера уходят в dataChanged.
/// Две последние роли дописаны В КОНЕЦ намеренно: номера уже показанных
/// ролей сдвигать нельзя, они уходят в dataChanged. `authorAvatarUrl` —
/// имя свойства строки waywallen 0.5.0 (`remote_model.cppm`; до 0.5.0 —
/// наша роль `authorAvatar` для Мастерской, номер тот же), `authorUrl` —
/// НАША ПРАВКА (паритет с Wallpaper Engine: переход к остальным работам
/// автора).
pub const ROLE_NAMES: [&str; 9] = [
    "itemId",
    "sourceId",
    "title",
    "previewUrl",
    "author",
    "wpType",
    "acquisitionState",
    "authorAvatarUrl",
    "authorUrl",
];

/// Строка выдачи Discover. Ровно девять полей — столько же кладёт
/// `_row_from_item` (bridge.py:708) и ветка Мастерской.
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRow {
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub source_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub preview_url: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub wp_type: String,
    #[serde(default)]
    pub acquisition_state: i32,
    #[serde(default)]
    pub author_avatar_url: String,
    #[serde(default)]
    pub author_url: String,
}

impl RemoteRow {
    /// Значение роли по её порядковому номеру в ROLE_NAMES.
    fn role_slot(&self, slot: usize) -> Option<RoleValue<'_>> {
        Some(match *ROLE_NAMES.get(slot)? {
            "itemId" => RoleValue::Text(&self.item_id),
            "sourceId" => RoleValue::Text(&self.source_id),
            "title" => RoleValue::Text(&self.title),
            "previewUrl" => RoleValue::Text(&self.preview_url),
            "author" => RoleValue::Text(&self.author),
            "wpType" => RoleValue::Text(&self.wp_type),
            "acquisitionState" => RoleValue::Number(self.acquisition_state),
            "authorAvatarUrl" => RoleValue::Text(&self.author_avatar_url),
            "authorUrl" => RoleValue::Text(&self.author_url),
            _ => return None,
        })
    }

    /// Строка как JSON-объект — им сверяются get()/item() с голденом.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        for (slot, name) in ROLE_NAMES.iter().enumerate() {
            let value = match self.role_slot(slot) {
                Some(RoleValue::Text(text)) => Value::String(text.to_string()),
                Some(RoleValue::Number(number)) => Value::from(number),
                None => Value::Null,
            };
            map.insert((*name).to_string(), value);
        }
        Value::Object(map)
    }
}

/// Тип значения роли: строк восемь, число одно.
enum RoleValue<'a> {
    Text(&'a str),
    Number(i32),
}

/// Наблюдаемое событие модели. Порядок этих событий — такая же часть
/// контракта, как значения ролей: голден снят с ЖИВОЙ Python-модели именно
/// последовательностью.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelEvent {
    /// Испускает сам QAbstractItemModel в endResetModel.
    ModelReset,
    /// Испускает сам QAbstractItemModel в endInsertRows.
    RowsInserted { first: usize, last: usize },
    CountChanged,
    HasMoreChanged(bool),
    ReqFetchMore(usize),
    /// Всегда РОВНО одна роль: «широкий» dataChanged (пустой набор ролей)
    /// заставил бы вид пересчитать всю карточку — это и есть то, что
    /// стенд считает отдельно и признаёт провалом.
    DataChanged {
        first: usize,
        last: usize,
        role: &'static str,
    },
}

impl ModelEvent {
    /// Структурные события испускает базовый класс в end*-методах, Rust их
    /// не шлёт. Здесь — единственное место, где это знание записано.
    pub fn is_structural(&self) -> bool {
        matches!(self, Self::ModelReset | Self::RowsInserted { .. })
    }
}

/// Состояние модели: строки выдачи и флаг «есть ещё страницы».
#[derive(Debug, Default, Clone)]
pub struct RemoteRows {
    rows: Vec<RemoteRow>,
    has_more: bool,
}

impl RemoteRows {
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn has_more(&self) -> bool {
        self.has_more
    }

    pub fn rows(&self) -> &[RemoteRow] {
        &self.rows
    }

    /// Строка по номеру; None — номера нет (в том числе отрицательного).
    pub fn row(&self, row: i32) -> Option<&RemoteRow> {
        usize::try_from(row).ok().and_then(|r| self.rows.get(r))
    }

    /// canFetchMore: у плоского списка дети есть только у корня, поэтому
    /// по валидному родителю всегда ложь.
    pub fn can_fetch_more(&self, root: bool) -> bool {
        root && self.has_more
    }

    /// Одиночная смена флага догрузки.
    pub fn set_has_more(&mut self, value: bool) -> Vec<ModelEvent> {
        if self.has_more == value {
            return Vec::new();
        }
        self.has_more = value;
        vec![ModelEvent::HasMoreChanged(value)]
    }

    /// Полная замена выдачи (новый поиск, новая сортировка, новый источник).
    pub fn reset_rows(&mut self, rows: Vec<RemoteRow>, has_more: bool) -> Vec<ModelEvent> {
        // Флаг сравнивается ДО подмены строк — так же, как в оригинале.
        let changed = self.has_more != has_more;
        self.rows = rows;
        self.has_more = has_more;
        let mut events = vec![ModelEvent::ModelReset, ModelEvent::CountChanged];
        if changed {
            events.push(ModelEvent::HasMoreChanged(has_more));
        }
        events
    }

    /// Диапазон строк, который добавит append_rows. None — добавлять нечего,
    /// beginInsertRows звать нельзя. Отдельный метод, потому что Qt требует
    /// объявить вставку ДО правки состояния.
    pub fn insert_range(&self, added: usize) -> Option<(usize, usize)> {
        if added == 0 {
            return None;
        }
        let first = self.rows.len();
        Some((first, first + added - 1))
    }

    /// Догрузка следующей страницы.
    pub fn append_rows(&mut self, rows: Vec<RemoteRow>, has_more: bool) -> Vec<ModelEvent> {
        // Пустая страница строк не добавляет — вставку объявлять нечем,
        // остаётся только флаг догрузки.
        if rows.is_empty() {
            return self.set_has_more(has_more);
        }
        let changed = self.has_more != has_more;
        let (first, last) = self
            .insert_range(rows.len())
            .expect("непустая страница всегда даёт диапазон");
        self.rows.extend(rows);
        self.has_more = has_more;
        let mut events = vec![
            ModelEvent::RowsInserted { first, last },
            ModelEvent::CountChanged,
        ];
        if changed {
            events.push(ModelEvent::HasMoreChanged(has_more));
        }
        events
    }

    /// Протокол бесконечной прокрутки. Флаг гасится ДО запроса — иначе вид
    /// дёргал бы fetchMore повторно, пока страница в полёте.
    pub fn fetch_more(&mut self, root: bool) -> Vec<ModelEvent> {
        if !root || !self.has_more {
            return Vec::new();
        }
        let mut events = self.set_has_more(false);
        events.push(ModelEvent::ReqFetchMore(self.rows.len()));
        events
    }

    /// Состояние приобретения. Совпадать обязаны ОБА идентификатора: один и
    /// тот же предмет может лежать в выдаче двух источников.
    pub fn set_acquisition_state(
        &mut self,
        source_id: &str,
        item_id: &str,
        state: i32,
    ) -> Vec<ModelEvent> {
        let mut events = Vec::new();
        for (row, entry) in self.rows.iter_mut().enumerate() {
            if entry.source_id != source_id || entry.item_id != item_id {
                continue;
            }
            if entry.acquisition_state == state {
                // То же значение — сигнала нет: иначе каждая проверка
                // подписки перерисовывала бы карточку впустую.
                continue;
            }
            entry.acquisition_state = state;
            events.push(ModelEvent::DataChanged {
                first: row,
                last: row,
                role: ROLE_NAMES[ROLE_ACQUISITION_STATE],
            });
        }
        events
    }

    /// Булев вход старого моста: «скачано» → 3, иначе 0.
    pub fn set_installed(
        &mut self,
        source_id: &str,
        item_id: &str,
        installed: bool,
    ) -> Vec<ModelEvent> {
        self.set_acquisition_state(source_id, item_id, if installed { 3 } else { 0 })
    }

    /// Дописать имена авторов в уже показанные карточки. Пустое имя не
    /// затирает показанное (в оригинале это `if name and ...`).
    pub fn set_authors(&mut self, names: &[(String, String)]) -> Vec<ModelEvent> {
        let mut events = Vec::new();
        for (row, entry) in self.rows.iter_mut().enumerate() {
            let Some((_, name)) = names.iter().find(|(id, _)| *id == entry.item_id) else {
                continue;
            };
            if name.is_empty() || entry.author == *name {
                continue;
            }
            entry.author.clone_from(name);
            events.push(ModelEvent::DataChanged {
                first: row,
                last: row,
                role: ROLE_NAMES[ROLE_AUTHOR],
            });
        }
        events
    }

    /// Дописать аватарки авторов — тем же точечным путём, что имена.
    /// Пустой адрес не затирает показанный: аватарка, которая не
    /// скачалась, не имеет права стереть уже показанную.
    pub fn set_author_avatars(&mut self, avatars: &[(String, String)]) -> Vec<ModelEvent> {
        let mut events = Vec::new();
        for (row, entry) in self.rows.iter_mut().enumerate() {
            let Some((_, url)) = avatars.iter().find(|(id, _)| *id == entry.item_id) else {
                continue;
            };
            if url.is_empty() || entry.author_avatar_url == *url {
                continue;
            }
            entry.author_avatar_url.clone_from(url);
            events.push(ModelEvent::DataChanged {
                first: row,
                last: row,
                role: ROLE_NAMES[ROLE_AUTHOR_AVATAR],
            });
        }
        events
    }
}

/// Индексы ролей, которые обновляются точечно.
const ROLE_AUTHOR: usize = 4;
const ROLE_ACQUISITION_STATE: usize = 6;
const ROLE_AUTHOR_AVATAR: usize = 7;

/// Разобрать страницу выдачи из JSON-массива. Битый JSON даёт пустую
/// страницу — молча, как и разбор оверрайдов в редакторе свойств: ошибка
/// разбора не должна валить вкладку.
pub fn parse_rows(json: &str) -> Vec<RemoteRow> {
    serde_json::from_str::<Vec<RemoteRow>>(json).unwrap_or_default()
}

// ============================================================================
// Общий склад состояний приобретения (порт _AcquisitionStore / AppStore)
// ============================================================================

/// Склад состояний приобретения: у апстрима строки выдачи и панель деталей
/// смотрят в ОДНО хранилище, поэтому после подписки кнопка сразу становится
/// «Отписаться» без перезагрузки страницы. Это уже починенный при порте на
/// Python баг, консервировать регресс нельзя — склад переезжает вместе с
/// моделью.
///
/// Устройство склада — главное отличие от Python и смысл всей партии.
/// В Python склад хранит СВЯЗАННЫЙ МЕТОД модели (`changed.connect(
/// self.setAcquisitionState)`), то есть держит модель живой навсегда и при
/// этом ничего не знает про смерть её C++-половины — отсюда исторические
/// «Internal C++ object already deleted». Здесь склад хранит
/// `CxxQtThread<RemoteListModel>`: он `Send + Sync`, доставка идёт очередью
/// цикла событий, а мёртвый объект даёт `ObjectDestroyed` вместо обращения
/// к освобождённой памяти. Запись из реестра снимается либо в Drop
/// Rust-половины, либо при первой же неудачной рассылке.
pub mod acquisition {
    use std::sync::{Mutex, OnceLock};

    /// Доставка состояния ОДНОМУ получателю. `Err(())` — объект умер,
    /// запись пора снять.
    ///
    /// Замыкание, а не `CxxQtThread<RemoteListModel>`: со склада читает не
    /// только модель выдачи, но и держатель одного элемента
    /// (`RemoteStoreItem`, панель деталей Discover) — у апстрима это ОДНО
    /// хранилище на обоих. Обобщить `CxxQtThread` по типу нельзя: cxx-qt
    /// печатает свой алиас на каждый QObject.
    pub type Deliver = Box<dyn Fn(&str, &str, i32) -> Result<(), ()> + Send>;

    #[derive(Default)]
    struct Registry {
        next_id: u64,
        entries: Vec<(u64, Deliver)>,
    }

    fn registry() -> &'static Mutex<Registry> {
        static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
        REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
    }

    /// Зарегистрировать получателя. Возвращает номер записи для снятия
    /// в Drop.
    pub fn register(deliver: Deliver) -> u64 {
        let mut reg = registry().lock().expect("реестр склада не отравлен");
        reg.next_id += 1;
        let id = reg.next_id;
        reg.entries.push((id, deliver));
        id
    }

    /// Снять запись. Идемпотентно: записи может уже не быть, если рассылка
    /// успела получить ObjectDestroyed раньше деструктора.
    pub fn unregister(id: u64) {
        let mut reg = registry().lock().expect("реестр склада не отравлен");
        reg.entries.retain(|(entry, _)| *entry != id);
    }

    /// Сколько моделей сейчас в реестре (для стенда и отладки).
    pub fn registered() -> usize {
        registry()
            .lock()
            .expect("реестр склада не отравлен")
            .entries
            .len()
    }

    /// Разослать состояние во все живые модели. Возвращает число получателей,
    /// которым удалось поставить задачу в очередь.
    ///
    /// Пустые идентификаторы склад отбрасывает — так же, как оригинал.
    pub fn set(source_id: &str, item_id: &str, state: i32) -> usize {
        if source_id.is_empty() || item_id.is_empty() {
            return 0;
        }
        let mut delivered = 0usize;
        let mut reg = registry().lock().expect("реестр склада не отравлен");
        // retain, а не итерация: запись мёртвого получателя снимается здесь.
        reg.entries.retain(|(_, deliver)| match deliver(source_id, item_id, state) {
            Ok(()) => {
                delivered += 1;
                true
            }
            Err(()) => false,
        });
        delivered
    }
}

// ============================================================================
// Qt-слой
// ============================================================================

/// Rust-состояние модели.
#[derive(Default)]
pub struct RemoteListModelRust {
    rows: RemoteRows,
    /// Номер записи в общем складе; 0 — не зарегистрирована.
    store_id: u64,
}

impl Drop for RemoteListModelRust {
    fn drop(&mut self) {
        if self.store_id != 0 {
            acquisition::unregister(self.store_id);
        }
    }
}

/// Набор ролей из одного номера — то, что уезжает третьим аргументом
/// dataChanged. ПУСТОЙ вектор означал бы «изменились все роли», и вид
/// пересчитал бы карточку целиком; здесь этого не бывает никогда.
fn role_vector(name: &str) -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    if let Some(slot) = ROLE_NAMES.iter().position(|n| *n == name) {
        roles.append(ROLE_BASE + slot as i32);
    }
    roles
}

impl cxx_qt::Initialize for qobject::RemoteListModel {
    fn initialize(mut self: Pin<&mut Self>) {
        // Регистрация в общем складе — в пост-конструкции: qt_thread()
        // требует уже построенного QObject.
        let handle = self.as_ref().get_ref().qt_thread();
        // Ни unwrap(), ни предварительного is_destroyed(): между проверкой
        // и постановкой объект успевает умереть.
        let id = acquisition::register(Box::new(move |source_id, item_id, state| {
            let source = source_id.to_string();
            let item = item_id.to_string();
            match handle.queue(move |model| {
                model.set_acquisition_state(&QString::from(&source), &QString::from(&item), state);
            }) {
                Ok(()) => Ok(()),
                // Объект умер — запись больше не нужна. Прочие ошибки
                // (не удался invokeMethod) объект не хоронят.
                Err(cxx_qt::ThreadingQueueError::ObjectDestroyed) => Err(()),
                Err(_) => Ok(()),
            }
        }));
        self.as_mut().rust_mut().store_id = id;
    }
}

impl qobject::RemoteListModel {
    // ── Свойства ──────────────────────────────────────────────────────────
    // pub: счётчик строк читает стенд запросов партии 3.
    pub fn count(&self) -> i32 {
        self.rows.len() as i32
    }

    // pub: флаг догрузки читает и запрос-владелец (партия 3) — у него
    // свойство `hasMore` не собственное, а пересказ модели, как в мосте
    // (`return self._model.hasMore`).
    pub fn has_more(&self) -> bool {
        self.rows.has_more()
    }

    // ── QAbstractListModel ────────────────────────────────────────────────
    fn row_count(&self, parent: &QModelIndex) -> i32 {
        // У плоского списка дети есть только у корня.
        if parent.is_valid() {
            0
        } else {
            self.rows.len() as i32
        }
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        let mut names = qobject::QHash_i32_QByteArray::default();
        for (slot, name) in ROLE_NAMES.iter().enumerate() {
            names.insert(ROLE_BASE + slot as i32, QByteArray::from(*name));
        }
        names
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        if !index.is_valid() {
            return QVariant::default();
        }
        let Some(entry) = self.rows.row(index.row()) else {
            return QVariant::default();
        };
        let Ok(slot) = usize::try_from(role - ROLE_BASE) else {
            return QVariant::default();
        };
        match entry.role_slot(slot) {
            Some(RoleValue::Text(text)) => QVariant::from(&QString::from(text)),
            Some(RoleValue::Number(number)) => QVariant::from(&number),
            None => QVariant::default(),
        }
    }

    fn can_fetch_more(&self, parent: &QModelIndex) -> bool {
        self.rows.can_fetch_more(!parent.is_valid())
    }

    // pub: догрузку дёргает стенд запросов партии 3 — ровно так, как это
    // делает GridView, через протокол самой модели.
    pub fn fetch_more(mut self: Pin<&mut Self>, parent: &QModelIndex) {
        let root = !parent.is_valid();
        let events = self.as_mut().rust_mut().rows.fetch_more(root);
        self.emit_events(&events);
    }

    // ── API для QML ───────────────────────────────────────────────────────
    // pub: строку целиком читает стенд запросов партии 3.
    pub fn get(&self, row: i32) -> qobject::QMap_QString_QVariant {
        let mut map = QMap::<QMapPair_QString_QVariant>::default();
        let Some(entry) = self.rows.row(row) else {
            // Строки нет — пустая карта, как у оригинала (`return {}`).
            return map;
        };
        for (slot, name) in ROLE_NAMES.iter().enumerate() {
            let value = match entry.role_slot(slot) {
                Some(RoleValue::Text(text)) => QVariant::from(&QString::from(text)),
                Some(RoleValue::Number(number)) => QVariant::from(&number),
                None => QVariant::default(),
            };
            map.insert(QString::from(*name), value);
        }
        map
    }

    fn item(&self, row: i32) -> qobject::QMap_QString_QVariant {
        self.get(row)
    }

    fn set_installed(
        mut self: Pin<&mut Self>,
        source_id: &QString,
        item_id: &QString,
        installed: bool,
    ) {
        let events = self.as_mut().rust_mut().rows.set_installed(
            &source_id.to_string(),
            &item_id.to_string(),
            installed,
        );
        self.emit_events(&events);
    }

    fn set_acquisition_state(
        mut self: Pin<&mut Self>,
        source_id: &QString,
        item_id: &QString,
        state: i32,
    ) {
        let events = self.as_mut().rust_mut().rows.set_acquisition_state(
            &source_id.to_string(),
            &item_id.to_string(),
            state,
        );
        self.emit_events(&events);
    }

    fn set_authors(mut self: Pin<&mut Self>, names: &qobject::QMap_QString_QVariant) {
        let names: Vec<(String, String)> = names
            .iter()
            .map(|(key, value)| {
                let text = value
                    .value::<QString>()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                (key.to_string(), text)
            })
            .collect();
        let events = self.as_mut().rust_mut().rows.set_authors(&names);
        self.emit_events(&events);
    }

    fn set_author_avatars(mut self: Pin<&mut Self>, avatars: &qobject::QMap_QString_QVariant) {
        let avatars: Vec<(String, String)> = avatars
            .iter()
            .map(|(key, value)| {
                let text = value
                    .value::<QString>()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                (key.to_string(), text)
            })
            .collect();
        let events = self.as_mut().rust_mut().rows.set_author_avatars(&avatars);
        self.emit_events(&events);
    }

    fn apply_page_json(
        mut self: Pin<&mut Self>,
        json: &QString,
        has_more: bool,
        append: bool,
    ) -> i32 {
        let rows = parse_rows(&json.to_string());
        let parsed = rows.len() as i32;
        if append {
            self.as_mut().append_page(rows, has_more);
        } else {
            self.as_mut().reset_page(rows, has_more);
        }
        parsed
    }

    fn role_number(&self, name: &QString) -> i32 {
        let name = name.to_string();
        ROLE_NAMES
            .iter()
            .position(|n| *n == name)
            .map(|slot| ROLE_BASE + slot as i32)
            .unwrap_or(-1)
    }

    fn role_value(&self, row: i32, name: &QString) -> QVariant {
        let role = self.role_number(name);
        if role < 0 {
            return QVariant::default();
        }
        let index = self.index(row, 0, &QModelIndex::default());
        self.data(&index, role)
    }

    fn publish_acquisition_state(
        &self,
        source_id: &QString,
        item_id: &QString,
        state: i32,
    ) -> i32 {
        acquisition::set(&source_id.to_string(), &item_id.to_string(), state) as i32
    }

    // ── Rust-API (им будет пользоваться запрос партии 3) ───────────────────
    /// Полная замена выдачи: reset модели, потом сигналы-хвост.
    pub fn reset_page(mut self: Pin<&mut Self>, rows: Vec<RemoteRow>, has_more: bool) {
        let events = unsafe {
            self.as_mut().begin_reset_model();
            let events = self.as_mut().rust_mut().rows.reset_rows(rows, has_more);
            self.as_mut().end_reset_model();
            events
        };
        self.emit_events(&events);
    }

    /// Одиночная смена флага догрузки. Ей пользуется запрос-владелец партии
    /// 3: оригинал гасит `hasMore` ДО отправки страницы, иначе GridView
    /// дёргает `fetchMore` повторно, пока страница в полёте.
    pub fn set_page_has_more(mut self: Pin<&mut Self>, value: bool) {
        let events = self.as_mut().rust_mut().rows.set_has_more(value);
        self.emit_events(&events);
    }

    /// Догрузка страницы: объявить вставку, дописать, закрыть, потом хвост.
    pub fn append_page(mut self: Pin<&mut Self>, rows: Vec<RemoteRow>, has_more: bool) {
        // Диапазон считается ДО правки состояния: beginInsertRows обязан
        // знать его заранее.
        let range = self.rows.insert_range(rows.len());
        let events = unsafe {
            if let Some((first, last)) = range {
                self.as_mut().begin_insert_rows(
                    &QModelIndex::default(),
                    first as i32,
                    last as i32,
                );
            }
            let events = self.as_mut().rust_mut().rows.append_rows(rows, has_more);
            if range.is_some() {
                self.as_mut().end_insert_rows();
            }
            events
        };
        self.emit_events(&events);
    }

    // ── Внутреннее ────────────────────────────────────────────────────────
    /// Единственное место, где события чистого слоя превращаются в сигналы
    /// Qt. Структурные (modelReset/rowsInserted) пропускаются: их испускает
    /// сам QAbstractItemModel в end_reset_model / end_insert_rows.
    ///
    /// ⚠️ Значения берутся ЖИВЫЕ, а не запланированные, и это не придирка.
    /// GridView спрашивает `canFetchMore`/`fetchMore` прямо из обработчика
    /// `modelReset`, то есть РЕЕНТРАНТНО — ещё внутри `end_reset_model()`,
    /// до того как reset успел досказать свой хвост сигналов. К моменту,
    /// когда очередь доходит до хвостового `hasMoreChanged`, флаг уже сбит
    /// догрузкой в false, и отправить запланированный `true` значило бы
    /// соврать виду. Оригинал шлёт именно текущее состояние
    /// (`self.hasMoreChanged.emit(self._has_more)`, bridge.py:691) — здесь
    /// то же самое.
    fn emit_events(mut self: Pin<&mut Self>, events: &[ModelEvent]) {
        for event in events {
            match event {
                ModelEvent::ModelReset | ModelEvent::RowsInserted { .. } => {}
                ModelEvent::CountChanged => self.as_mut().count_changed(),
                ModelEvent::HasMoreChanged(_) => {
                    let live = self.has_more();
                    self.as_mut().has_more_changed(live);
                }
                ModelEvent::ReqFetchMore(_) => {
                    let live = self.count();
                    self.as_mut().req_fetch_more(live);
                }
                ModelEvent::DataChanged { first, last, role } => {
                    let roles = role_vector(role);
                    let root = QModelIndex::default();
                    let top_left = self.index(*first as i32, 0, &root);
                    let bottom_right = self.index(*last as i32, 0, &root);
                    self.as_mut().data_changed(&top_left, &bottom_right, &roles);
                }
            }
        }
    }
}

// ============================================================================
// Голден: разбор эталона, снятого с ЖИВОЙ Python-модели
// ============================================================================

/// Разбор `tests/golden/remote_list_cases.json`. Живёт не под `#[cfg(test)]`,
/// потому что тем же эталоном пользуется стенд `selftest::run()`: чистый слой
/// проверяется `cargo test`, а Qt-слой — настоящим QObject'ом.
pub mod golden {
    use super::{ModelEvent, RemoteRow, RemoteRows};
    use serde::Deserialize;
    use serde_json::Value;

    /// Эталон целиком.
    #[derive(Debug, Deserialize)]
    pub struct Golden {
        pub cases: Vec<Case>,
    }

    #[derive(Debug, Deserialize)]
    pub struct Case {
        pub name: String,
        pub models: usize,
        pub steps: Vec<Step>,
        pub events: Vec<Event>,
        pub final_: Vec<Final>,
    }

    // «final» — ключевое слово Rust только как идентификатор поля не мешает,
    // но serde-имя в голдене именно final, отсюда переименование.
    #[derive(Debug, Deserialize)]
    pub struct Step {
        pub op: String,
        #[serde(default)]
        pub model: usize,
        #[serde(default)]
        pub rows: Vec<RemoteRow>,
        #[serde(default, rename = "hasMore")]
        pub has_more: bool,
        #[serde(default, rename = "sourceId")]
        pub source_id: String,
        #[serde(default, rename = "itemId")]
        pub item_id: String,
        #[serde(default)]
        pub state: i32,
        #[serde(default)]
        pub installed: bool,
        #[serde(default)]
        pub names: serde_json::Map<String, Value>,
    }

    #[derive(Debug, Deserialize)]
    pub struct Event {
        pub model: usize,
        pub sig: String,
        #[serde(default)]
        pub first: usize,
        #[serde(default)]
        pub last: usize,
        #[serde(default)]
        pub roles: Vec<String>,
        #[serde(default)]
        pub value: bool,
        #[serde(default)]
        pub row: usize,
    }

    #[derive(Debug, Deserialize)]
    pub struct Final {
        pub count: usize,
        #[serde(rename = "hasMore")]
        pub has_more: bool,
        #[serde(rename = "canFetchMore")]
        pub can_fetch_more: bool,
        #[serde(rename = "canFetchMoreChild")]
        pub can_fetch_more_child: bool,
        pub rows: Vec<RemoteRow>,
        #[serde(rename = "getFirst")]
        pub get_first: Value,
        #[serde(rename = "getOutOfRange")]
        pub get_out_of_range: Value,
        #[serde(rename = "getNegative")]
        pub get_negative: Value,
        #[serde(rename = "itemFirst")]
        pub item_first: Value,
    }

    /// Голден в исходниках крейта — тот самый файл, который печатает
    /// tests/golden/remote_list_cases.py с живого Python-моста.
    pub const GOLDEN_JSON: &str = include_str!("../tests/golden/remote_list_cases.json");

    pub fn load() -> Golden {
        // serde не умеет ключ «final» без переименования, а голден пишет
        // именно его: правим на лету, чтобы не заводить второй формат.
        let raw: Value = serde_json::from_str(GOLDEN_JSON).expect("голден читается");
        let mut fixed = raw;
        if let Some(cases) = fixed.get_mut("cases").and_then(Value::as_array_mut) {
            for case in cases {
                if let Some(object) = case.as_object_mut() {
                    if let Some(value) = object.remove("final") {
                        object.insert("final_".to_string(), value);
                    }
                }
            }
        }
        serde_json::from_value(fixed).expect("голден разбирается в структуры")
    }

    /// Ожидаемое событие голдена → событие модели. Здесь же ловится
    /// «широкий» dataChanged: у эталона он ВСЕГДА ровно с одной ролью, и
    /// набор из нуля или двух ролей уронит разбор.
    pub fn expected_event(event: &Event) -> (usize, ModelEvent) {
        let model = event.model;
        let value = match event.sig.as_str() {
            "modelReset" => ModelEvent::ModelReset,
            "rowsInserted" => ModelEvent::RowsInserted {
                first: event.first,
                last: event.last,
            },
            "countChanged" => ModelEvent::CountChanged,
            "hasMoreChanged" => ModelEvent::HasMoreChanged(event.value),
            "reqFetchMore" => ModelEvent::ReqFetchMore(event.row),
            "dataChanged" => {
                assert_eq!(
                    event.roles.len(),
                    1,
                    "голден: dataChanged обязан нести РОВНО одну роль, а несёт {:?}",
                    event.roles
                );
                let name = super::ROLE_NAMES
                    .iter()
                    .find(|n| **n == event.roles[0])
                    .unwrap_or_else(|| panic!("голден: неизвестная роль {}", event.roles[0]));
                ModelEvent::DataChanged {
                    first: event.first,
                    last: event.last,
                    role: name,
                }
            }
            other => panic!("голден: неизвестный сигнал {other}"),
        };
        (model, value)
    }

    /// Имена авторов из голдена в порядке записи.
    pub fn names_of(step: &Step) -> Vec<(String, String)> {
        step.names
            .iter()
            .map(|(key, value)| {
                (
                    key.clone(),
                    value.as_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    }

    /// Прогнать случай через ЧИСТЫЙ слой и вернуть состояние моделей вместе
    /// с журналом событий в порядке возникновения.
    pub fn replay_pure(case: &Case) -> (Vec<RemoteRows>, Vec<(usize, ModelEvent)>) {
        let mut models: Vec<RemoteRows> = (0..case.models).map(|_| RemoteRows::default()).collect();
        let mut journal: Vec<(usize, ModelEvent)> = Vec::new();

        for step in &case.steps {
            let index = step.model;
            let events = match step.op.as_str() {
                "resetRows" => models[index].reset_rows(step.rows.clone(), step.has_more),
                "appendRows" => models[index].append_rows(step.rows.clone(), step.has_more),
                "fetchMore" => models[index].fetch_more(true),
                "setAcquisitionState" => {
                    models[index].set_acquisition_state(&step.source_id, &step.item_id, step.state)
                }
                "setInstalled" => {
                    models[index].set_installed(&step.source_id, &step.item_id, step.installed)
                }
                "setAuthors" => models[index].set_authors(&names_of(step)),
                "setAuthorAvatars" => models[index].set_author_avatars(&names_of(step)),
                "storeSet" => {
                    // Склад — рассылка во ВСЕ живые модели; пустые
                    // идентификаторы он отбрасывает.
                    if step.source_id.is_empty() || step.item_id.is_empty() {
                        Vec::new()
                    } else {
                        for (i, model) in models.iter_mut().enumerate() {
                            let events = model.set_acquisition_state(
                                &step.source_id,
                                &step.item_id,
                                step.state,
                            );
                            journal.extend(events.into_iter().map(|e| (i, e)));
                        }
                        Vec::new()
                    }
                }
                other => panic!("голден: неизвестное действие {other}"),
            };
            journal.extend(events.into_iter().map(|e| (index, e)));
        }
        (models, journal)
    }
}

// ============================================================================
// Стенд: поздние колбэки и Qt-слой на живом QObject
// ============================================================================

/// Самопроверка Qt-слоя без QML и без окна.
///
/// Здесь доказывается то, ради чего партия существует: модель удаляется,
/// пока колбэк ещё в полёте, — и это не падение, а `ObjectDestroyed`.
/// Заодно все случаи голдена прогоняются через НАСТОЯЩИЙ QObject, а не
/// только через чистый слой: сверяются полный порядок сигналов (включая
/// modelReset/rowsInserted самого QAbstractItemModel), номера ролей в
/// dataChanged, значения ролей через data() и содержимое get()/item().
///
/// Возврат: 0 — всё сошлось, 1 — есть расхождения. Молчаливого успеха по
/// таймауту нет: стенд не ждёт ничего, чего не может дождаться.
pub mod selftest {
    use super::golden;
    use super::qobject::{
        rexon_delete_object, rexon_new_remote_list_model, rexon_process_events, RemoteListModel,
    };
    use super::{acquisition, ModelEvent, RemoteRow, ROLE_BASE, ROLE_NAMES};
    use core::pin::Pin;
    use cxx_qt::{Threading, ThreadingQueueError};
    use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QModelIndex, QString, QVariant};
    use std::sync::{Arc, Mutex};

    pub const ARG: &str = "--remote-selftest";

    /// Живой объект-модель, созданный фабрикой без родителя. Snимается
    /// только явным `destroy()` — стенду нужна управляемая смерть.
    struct OwnedModel {
        raw: *mut super::qobject::QObject,
    }

    impl OwnedModel {
        fn new() -> Self {
            let raw = unsafe { rexon_new_remote_list_model(core::ptr::null_mut()) };
            assert!(!raw.is_null(), "фабрика вернула nullptr");
            Self { raw }
        }

        /// Указатель на наследника: одиночное наследование, адрес тот же.
        fn model(&self) -> *mut RemoteListModel {
            self.raw.cast()
        }

        // Изменяемая ссылка из неизменяемой — так и задумано: владеет
        // объектом Qt, а не Rust, и «изменяемость» здесь про C++-объект,
        // а не про поле raw.
        #[allow(clippy::mut_from_ref)]
        fn pinned(&self) -> Pin<&mut RemoteListModel> {
            unsafe { Pin::new_unchecked(&mut *self.model()) }
        }

        fn destroy(self) {
            unsafe { rexon_delete_object(self.raw) };
        }
    }

    /// Собранный журнал сигналов живого QObject.
    type Journal = Arc<Mutex<Vec<ModelEvent>>>;

    /// Подписаться на наблюдаемые сигналы модели. Возвращаемые «сторожа»
    /// связей обязаны жить, пока идёт случай: их Drop разрывает связь.
    ///
    /// Структурных сигналов (modelReset, rowsInserted) здесь нет и быть не
    /// может: они приватные (QPrivateSignal), из Rust на них не подписаться.
    /// Их порядок сверяет чистый слой в `cargo test` и живой вид в
    /// QML-стенде — см. шапку модуля.
    fn connect_journal(
        model: Pin<&mut RemoteListModel>,
        journal: &Journal,
    ) -> Vec<cxx_qt::QMetaObjectConnectionGuard> {
        let mut guards = Vec::new();
        let raw = unsafe { model.get_unchecked_mut() as *mut RemoteListModel };
        let pin = || unsafe { Pin::new_unchecked(&mut *raw) };

        let sink = Arc::clone(journal);
        guards.push(pin().on_count_changed(move |_| {
            sink.lock().expect("журнал").push(ModelEvent::CountChanged);
        }));

        let sink = Arc::clone(journal);
        guards.push(pin().on_has_more_changed(move |_, value| {
            sink.lock()
                .expect("журнал")
                .push(ModelEvent::HasMoreChanged(value));
        }));

        let sink = Arc::clone(journal);
        guards.push(pin().on_req_fetch_more(move |_, row| {
            sink.lock()
                .expect("журнал")
                .push(ModelEvent::ReqFetchMore(row as usize));
        }));

        let sink = Arc::clone(journal);
        guards.push(pin().on_data_changed(move |_, top_left, bottom_right, roles| {
            // Роли приезжают номерами — переводим в имена ТЕМ ЖЕ способом,
            // каким их печатает голден. Пустой или многоролевой набор здесь
            // не превратится в одну роль: он попадёт в журнал как «широкий»
            // и не совпадёт с эталоном.
            let names: Vec<&'static str> = (0..roles.len())
                .filter_map(|i| roles.get(i).copied())
                .filter_map(|role| {
                    usize::try_from(role - ROLE_BASE)
                        .ok()
                        .and_then(|slot| ROLE_NAMES.get(slot).copied())
                })
                .collect();
            let role = if names.len() == 1 { names[0] } else { "<широкий>" };
            sink.lock().expect("журнал").push(ModelEvent::DataChanged {
                first: top_left.row() as usize,
                last: bottom_right.row() as usize,
                role,
            });
        }));

        guards
    }

    /// Значение роли живой модели по имени — через настоящий data().
    fn role_text(model: &RemoteListModel, row: i32, name: &str) -> String {
        let value = model.role_value(row, &QString::from(name));
        variant_text(&value)
    }

    fn variant_text(value: &QVariant) -> String {
        if let Some(text) = value.value::<QString>() {
            return text.to_string();
        }
        match value.value::<i32>() {
            Some(number) => number.to_string(),
            None => String::new(),
        }
    }

    /// Строка живой модели в том же JSON-виде, что печатает голден.
    fn map_to_json(map: &QMap<QMapPair_QString_QVariant>) -> serde_json::Value {
        let mut object = serde_json::Map::new();
        for (key, value) in map.iter() {
            let name = key.to_string();
            let json = if name == "acquisitionState" {
                serde_json::Value::from(value.value::<i32>().unwrap_or_default())
            } else {
                serde_json::Value::String(variant_text(value))
            };
            object.insert(name, json);
        }
        serde_json::Value::Object(object)
    }

    struct Report {
        failures: Vec<String>,
        wide_updates: usize,
        cases: usize,
        events: usize,
    }

    impl Report {
        fn check(&mut self, condition: bool, message: impl FnOnce() -> String) {
            if !condition {
                self.failures.push(message());
            }
        }
    }

    /// Прогон одного случая голдена через живой QObject.
    fn run_case(case: &golden::Case, report: &mut Report) {
        let models: Vec<OwnedModel> = (0..case.models).map(|_| OwnedModel::new()).collect();
        let journals: Vec<Journal> = (0..case.models)
            .map(|_| Arc::new(Mutex::new(Vec::new())))
            .collect();
        let _guards: Vec<_> = models
            .iter()
            .zip(&journals)
            .map(|(model, journal)| connect_journal(model.pinned(), journal))
            .collect();

        for step in &case.steps {
            let model = models[step.model].pinned();
            match step.op.as_str() {
                "resetRows" => model.reset_page(step.rows.clone(), step.has_more),
                "appendRows" => model.append_page(step.rows.clone(), step.has_more),
                "fetchMore" => model.fetch_more(&QModelIndex::default()),
                "setAcquisitionState" => model.set_acquisition_state(
                    &QString::from(&step.source_id),
                    &QString::from(&step.item_id),
                    step.state,
                ),
                "setInstalled" => model.set_installed(
                    &QString::from(&step.source_id),
                    &QString::from(&step.item_id),
                    step.installed,
                ),
                "setAuthors" => {
                    let mut names = QMap::<QMapPair_QString_QVariant>::default();
                    for (key, value) in golden::names_of(step) {
                        names.insert(QString::from(&key), QVariant::from(&QString::from(&value)));
                    }
                    model.set_authors(&names);
                }
                // Аватарки приезжают вторым проходом, ОТДЕЛЬНЫМ инвокаблом:
                // имя уже отрисовано, картинка догоняет своей ролью.
                "setAuthorAvatars" => {
                    let mut avatars = QMap::<QMapPair_QString_QVariant>::default();
                    for (key, value) in golden::names_of(step) {
                        avatars
                            .insert(QString::from(&key), QVariant::from(&QString::from(&value)));
                    }
                    model.set_author_avatars(&avatars);
                }
                "storeSet" => {
                    model.publish_acquisition_state(
                        &QString::from(&step.source_id),
                        &QString::from(&step.item_id),
                        step.state,
                    );
                    // Склад доставляет очередью цикла событий: в Python это
                    // прямая связь, здесь — очередь, и её надо прокрутить.
                    rexon_process_events(50);
                }
                other => panic!("стенд: неизвестное действие {other}"),
            }
        }
        // Хвост очереди склада на случай, если последний шаг был storeSet.
        rexon_process_events(50);

        // ── сверка журналов ────────────────────────────────────────────────
        let mut expected: Vec<Vec<ModelEvent>> = vec![Vec::new(); case.models];
        for event in &case.events {
            let (index, value) = golden::expected_event(event);
            // Структурные сигналы из ожидаемого ряда выбрасываются: на них
            // из Rust не подписаться (см. connect_journal). Их порядок
            // проверяется чистым слоем в cargo test и живым видом в QML.
            if value.is_structural() {
                continue;
            }
            expected[index].push(value);
        }
        for (index, journal) in journals.iter().enumerate() {
            let actual = journal.lock().expect("журнал").clone();
            report.events += actual.len();
            report.wide_updates += actual
                .iter()
                .filter(|e| matches!(e, ModelEvent::DataChanged { role, .. } if *role == "<широкий>"))
                .count();
            let want = &expected[index];
            report.check(actual == *want, || {
                format!(
                    "{} · модель {index}: порядок сигналов разошёлся\n  получено: {actual:?}\n  ожидалось: {want:?}",
                    case.name
                )
            });
        }

        // ── сверка состояния ──────────────────────────────────────────────
        for (index, want) in case.final_.iter().enumerate() {
            let model = models[index].pinned();
            let at = format!("{} · модель {index}", case.name);
            report.check(model.count() == want.count as i32, || {
                format!("{at}: count {} вместо {}", model.count(), want.count)
            });
            report.check(model.has_more() == want.has_more, || {
                format!("{at}: hasMore {} вместо {}", model.has_more(), want.has_more)
            });
            let root = QModelIndex::default();
            report.check(model.can_fetch_more(&root) == want.can_fetch_more, || {
                format!("{at}: canFetchMore разошёлся")
            });
            if want.count > 0 {
                let child = model.index(0, 0, &root);
                report.check(
                    model.can_fetch_more(&child) == want.can_fetch_more_child,
                    || format!("{at}: canFetchMore по строке-родителю разошёлся"),
                );
            }
            for (row, row_want) in want.rows.iter().enumerate() {
                let at = format!("{at} · строка {row} ({})", row_want.item_id);
                for name in ROLE_NAMES {
                    let actual = role_text(&model, row as i32, name);
                    let expected = expected_role_text(row_want, name);
                    report.check(actual == expected, || {
                        format!("{at}: роль {name} = «{actual}», ожидалась «{expected}»")
                    });
                }
            }
            report.check(map_to_json(&model.get(0)) == want.get_first, || {
                format!("{at}: get(0) разошёлся")
            });
            report.check(map_to_json(&model.item(0)) == want.item_first, || {
                format!("{at}: item(0) разошёлся")
            });
            report.check(
                map_to_json(&model.get(want.count as i32)) == want.get_out_of_range,
                || format!("{at}: get(за границей) разошёлся"),
            );
            report.check(map_to_json(&model.get(-1)) == want.get_negative, || {
                format!("{at}: get(-1) разошёлся")
            });
            // Роли, которой нет, обязаны давать -1 и пустой QVariant.
            report.check(model.role_number(&QString::from("такойРолиНет")) == -1, || {
                format!("{at}: несуществующая роль дала не -1")
            });
        }

        for model in models {
            model.destroy();
        }
    }

    fn expected_role_text(row: &RemoteRow, name: &str) -> String {
        match name {
            "itemId" => row.item_id.clone(),
            "sourceId" => row.source_id.clone(),
            "title" => row.title.clone(),
            "previewUrl" => row.preview_url.clone(),
            "author" => row.author.clone(),
            "wpType" => row.wp_type.clone(),
            "acquisitionState" => row.acquisition_state.to_string(),
            "authorAvatarUrl" => row.author_avatar_url.clone(),
            "authorUrl" => row.author_url.clone(),
            other => panic!("стенд: неизвестная роль {other}"),
        }
    }

    /// Страница-образец для сценариев поздних колбэков.
    fn probe_page() -> Vec<RemoteRow> {
        vec![RemoteRow {
            item_id: "late".to_string(),
            source_id: "wallhaven".to_string(),
            title: "Обоина под поздний колбэк".to_string(),
            ..RemoteRow::default()
        }]
    }

    /// ⭐ Сердцевина партии: поздние колбэки на удалённую модель.
    fn run_late_callbacks(report: &mut Report) {
        // ── 1. Модель ЖИВА: ответ доезжает ────────────────────────────────
        let alive = OwnedModel::new();
        alive.pinned().reset_page(probe_page(), false);
        let handle = unsafe { (*alive.model()).qt_thread() };
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(80));
            let result = worker.queue(|model| {
                model.set_acquisition_state(
                    &QString::from("wallhaven"),
                    &QString::from("late"),
                    3,
                );
            });
            tx.send(result.is_ok()).ok();
        });
        rexon_process_events(400);
        report.check(rx.recv().unwrap_or(false), || {
            "живая модель: постановка в очередь не удалась".to_string()
        });
        report.check(role_text(&alive.pinned(), 0, "acquisitionState") == "3", || {
            "живая модель: ответ из фонового потока не доехал".to_string()
        });
        alive.destroy();

        // ── 2. Модель УМЕРЛА, ответ ещё в полёте ──────────────────────────
        // Ровно тот сценарий, что давал «Internal C++ object
        // (RemoteListModel) already deleted»: вкладку закрыли, пока ответ
        // remote_search шёл от демона.
        let doomed = OwnedModel::new();
        doomed.pinned().reset_page(probe_page(), false);
        let handle = unsafe { (*doomed.model()).qt_thread() };
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(120));
            // queue(...).ok() — без unwrap() и без is_destroyed(): между
            // проверкой и постановкой объект успевает умереть.
            let outcome = match worker.queue(|model| {
                model.set_acquisition_state(
                    &QString::from("wallhaven"),
                    &QString::from("late"),
                    3,
                );
            }) {
                Ok(()) => "доставлено".to_string(),
                Err(ThreadingQueueError::ObjectDestroyed) => "объект уничтожен".to_string(),
                Err(other) => format!("иная ошибка: {other}"),
            };
            tx.send(outcome).ok();
        });
        // Модель умирает ДО того, как фоновый поток проснулся.
        doomed.destroy();
        rexon_process_events(500);
        let verdict = rx.recv().unwrap_or_else(|_| "поток не ответил".to_string());
        report.check(verdict == "объект уничтожен", || {
            format!("поздний колбэк на удалённую модель дал «{verdict}», а обязан был дать «объект уничтожен»")
        });
        // Дошли сюда живыми — значит, обращения к освобождённой памяти не
        // было: в Python это место падало RuntimeError и добивалось вторым
        // исключением в обработчике.
        report.check(handle.is_destroyed(), || {
            "CxxQtThread не считает удалённую модель уничтоженной".to_string()
        });

        // ── 3. Ответ поставлен в очередь, модель умерла ДО его исполнения ──
        let racing = OwnedModel::new();
        racing.pinned().reset_page(probe_page(), false);
        let handle = unsafe { (*racing.model()).qt_thread() };
        let queued = handle.queue(|model| {
            model.set_acquisition_state(&QString::from("wallhaven"), &QString::from("late"), 3);
        });
        report.check(queued.is_ok(), || {
            "живая модель: постановка в очередь до удаления не удалась".to_string()
        });
        racing.destroy();
        // Задача осталась в очереди события уже мёртвого объекта: cxx-qt
        // печатает qWarning и ничего не делает. Проверяем, что цикл событий
        // это переживает.
        rexon_process_events(300);

        // ── 4. Общий склад: мёртвый получатель не мешает живому ───────────
        let live = OwnedModel::new();
        live.pinned().reset_page(probe_page(), false);
        let dead = OwnedModel::new();
        dead.pinned().reset_page(probe_page(), false);
        let registered_before = acquisition::registered();
        report.check(registered_before >= 2, || {
            format!("склад: зарегистрировано {registered_before} моделей, ожидалось не меньше двух")
        });
        dead.destroy();
        let delivered = acquisition::set("wallhaven", "late", 2);
        rexon_process_events(300);
        report.check(delivered == 1, || {
            format!("склад: рассылка дошла до {delivered} моделей, ожидалась ровно одна живая")
        });
        report.check(role_text(&live.pinned(), 0, "acquisitionState") == "2", || {
            "склад: живая модель не получила состояние".to_string()
        });
        live.destroy();
        let registered_after = acquisition::registered();
        report.check(registered_after == 0, || {
            format!("склад: после сноса моделей осталось {registered_after} записей")
        });
    }

    /// Точка входа стенда. Код возврата — приговор.
    pub fn run() -> i32 {
        // Стенду хватает ядра приложения: ни окон, ни QML здесь нет.
        let mut app = cxx_qt_lib::QCoreApplication::new();
        if let Some(app) = app.as_mut() {
            app.set_application_name(&QString::from("Rexon (стенд Discover)"));
        }

        let golden = golden::load();
        let mut report = Report {
            failures: Vec::new(),
            wide_updates: 0,
            cases: golden.cases.len(),
            events: 0,
        };

        // ⭐ Пустая выборка = провал, а не успех: голден без случаев прогнал бы
        // перебор НИ РАЗУ и объявил стенд зелёным, не проверив ничего. Тот же
        // класс, на котором попался QML-стенд: сценарий Мастерской шёл на пустой
        // модели и сверял ноль с нулём.
        report.check(!golden.cases.is_empty(), || {
            "в эталоне нет ни одного случая — стенду нечего проверять".to_string()
        });
        for case in &golden.cases {
            run_case(case, &mut report);
        }
        run_late_callbacks(&mut report);

        println!(
            "REXON-REMOTE-SELFTEST cases={} events={} wideUpdates={} failures={}",
            report.cases,
            report.events,
            report.wide_updates,
            report.failures.len()
        );
        for failure in &report.failures {
            println!("REXON-REMOTE-SELFTEST-FAIL {failure}");
        }
        // «Широких» dataChanged быть не должно ни одного: точечное
        // обновление роли не имеет права вырождаться в «изменились все».
        let ok = report.failures.is_empty() && report.wide_updates == 0;
        println!(
            "REXON-REMOTE-SELFTEST-VERDICT {} failures={}",
            if ok { "OK" } else { "FAIL" },
            report.failures.len()
        );
        i32::from(!ok)
    }
}

#[cfg(test)]
mod tests {
    use super::golden;
    use super::*;

    /// Голден снят с ЖИВОЙ Python-модели (tests/golden/remote_list_cases.py).
    /// Здесь он прогоняется через ЧИСТЫЙ слой порта: строки, значения ролей,
    /// has_more/canFetchMore и — главное — единая последовательность сигналов
    /// (порядок, строки, роли). Qt-слой той же моделью проверяет стенд
    /// `selftest::run()`.
    #[test]
    fn golden_parity_with_python_model() {
        let golden = golden::load();
        assert!(!golden.cases.is_empty(), "голден пуст");

        for case in &golden.cases {
            let name = &case.name;
            let (models, journal) = golden::replay_pure(case);
            let expected: Vec<(usize, ModelEvent)> =
                case.events.iter().map(golden::expected_event).collect();
            assert_eq!(journal, expected, "{name}: последовательность сигналов");

            assert_eq!(models.len(), case.final_.len(), "{name}: число моделей");
            for (index, want) in case.final_.iter().enumerate() {
                let model = &models[index];
                let at = format!("{name} · модель {index}");
                assert_eq!(model.len(), want.count, "{at}: число строк");
                assert_eq!(model.has_more(), want.has_more, "{at}: hasMore");
                assert_eq!(
                    model.can_fetch_more(true),
                    want.can_fetch_more,
                    "{at}: canFetchMore"
                );
                assert!(
                    !model.can_fetch_more(false),
                    "{at}: canFetchMore по валидному родителю обязан быть ложью"
                );
                assert_eq!(model.rows(), want.rows.as_slice(), "{at}: строки");

                let first = model
                    .row(0)
                    .map(RemoteRow::to_json)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                assert_eq!(first, want.get_first, "{at}: get(0)");
                assert_eq!(first, want.item_first, "{at}: item(0)");
                assert!(
                    model.row(model.len() as i32).is_none(),
                    "{at}: строка за границей"
                );
                assert!(model.row(-1).is_none(), "{at}: отрицательная строка");
            }
        }
    }

    #[test]
    fn role_numbers_match_the_original() {
        // Номера ролей — часть контракта, а не деталь реализации:
        // bridge.py:577-586 (и remote_model.cpp за ним) нумерует их
        // Qt::UserRole + 1..9 ИМЕННО в этом порядке. Вендорный QML ходит по
        // именам и перестановки бы не заметил, но заметил бы всякий, кто
        // сверяет кадр протокола или пишет прокси-модель поверх нашей.
        // Последние две — наши (аватарка и работы автора), дописаны в конец.
        for (number, name) in [
            (257, "itemId"),
            (258, "sourceId"),
            (259, "title"),
            (260, "previewUrl"),
            (261, "author"),
            (262, "wpType"),
            (263, "acquisitionState"),
            (264, "authorAvatarUrl"),
            (265, "authorUrl"),
        ] {
            let slot = ROLE_NAMES
                .iter()
                .position(|n| *n == name)
                .unwrap_or_else(|| panic!("роли {name} нет"));
            assert_eq!(ROLE_BASE + slot as i32, number, "номер роли {name}");
        }
    }

    #[test]
    fn roles_are_unique_and_indexed() {
        let mut seen = std::collections::HashSet::new();
        for name in ROLE_NAMES {
            assert!(seen.insert(name), "роль {name} объявлена дважды");
        }
        assert_eq!(ROLE_NAMES[ROLE_AUTHOR], "author");
        assert_eq!(ROLE_NAMES[ROLE_ACQUISITION_STATE], "acquisitionState");
        assert_eq!(ROLE_NAMES[ROLE_AUTHOR_AVATAR], "authorAvatarUrl");
    }

    #[test]
    fn every_data_changed_carries_exactly_one_role() {
        // Отдельная проверка того же, что стенд считает счётчиком
        // «широких»: точечное обновление не имеет права вырождаться
        // в «изменились все роли».
        let golden = golden::load();
        for case in &golden.cases {
            let (_, journal) = golden::replay_pure(case);
            for (_, event) in &journal {
                if let ModelEvent::DataChanged { role, .. } = event {
                    assert!(
                        ROLE_NAMES.contains(role),
                        "{}: dataChanged с ролью «{role}» вне набора",
                        case.name
                    );
                }
            }
        }
    }

    #[test]
    fn broken_page_json_is_silent() {
        // Битый JSON страницы не должен валить вкладку: пустая выдача и
        // никаких паник (тот же принцип, что у оверрайдов в партии 1).
        assert!(parse_rows("{это не json").is_empty());
        assert!(parse_rows("[]").is_empty());
        assert_eq!(
            parse_rows(r#"[{"itemId":"a","acquisitionState":2}]"#),
            vec![RemoteRow {
                item_id: "a".to_string(),
                acquisition_state: 2,
                ..RemoteRow::default()
            }]
        );
    }

    #[test]
    fn insert_range_is_empty_for_empty_page() {
        let mut rows = RemoteRows::default();
        assert_eq!(rows.insert_range(0), None);
        rows.append_rows(parse_rows(r#"[{"itemId":"a"},{"itemId":"b"}]"#), true);
        assert_eq!(rows.insert_range(2), Some((2, 3)));
    }
}
