// WallpaperFilterRuleModel на Rust через cxx-qt — пробный порт C++-модели
// редактора фильтров вкладки Wallpaper Engine (rexon/qt_wallpaper_engine/
// qml_ui/cpp/filter_rule_model.cpp). Поведение 1-в-1 с оригиналом.
//
// Исходная C++-модель (порт waywallen ui/src/model/filter_rule_model.cpp,
// MIT, (c) hypengw — атрибуция в NOTICE) — плоский QAbstractListModel
// поверх QList<WallpaperFilterRule>.
//
// Логика (группы / логические операторы / dirty / сигналы) целиком на
// Rust. Единственное, что Rust выразить не может, — мета-рефлексия чужих
// Qt-Protobuf-гаджетов (их staticMetaObject); она вынесена в обобщённый
// C++-шим rexon_gadget_shim.cpp (без зависимости от filter.qpb.h).

/// Мост cxx-qt: QAbstractListModel-подкласс WallpaperFilterRuleModel.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++Qt" {
        include!(<QtCore/QAbstractListModel>);
        /// Базовый Qt-тип.
        #[qobject]
        type QAbstractListModel;
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qhash.h");
        /// QHash<i32, QByteArray> — для roleNames.
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qvariant.h");
        /// QVariant.
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmodelindex.h");
        /// QModelIndex.
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvector.h");
        /// QVector<i32> — набор ролей для dataChanged.
        type QVector_i32 = cxx_qt_lib::QVector<i32>;

        include!("cxx-qt-lib/qlist.h");
        /// QVariantList (QList<QVariant>) — items() / replaceState(). Имя
        /// алиаса обязано совпадать с ExternType-Id cxx-qt-lib: QList_QVariant.
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    // Обобщённый мост к мета-рефлексии гаджетов (см. rexon_gadget_shim.h).
    unsafe extern "C++" {
        include!("rexon_gadget_shim.h");
        fn rexon_default_gadget(type_name: &str) -> QVariant;
        fn rexon_coerce_gadget(type_name: &str, value: &QVariant) -> QVariant;
        fn rexon_gadget_read_by_role(gadget: &QVariant, role_index: i32) -> QVariant;
        fn rexon_gadget_set_by_role(gadget: &QVariant, role_index: i32, value: &QVariant)
            -> QVariant;
        fn rexon_gadget_read_int(gadget: &QVariant, prop: &str) -> i32;
        fn rexon_gadget_set_int(gadget: &QVariant, prop: &str, value: i32) -> QVariant;
        fn rexon_variant_is_valid(v: &QVariant) -> bool;
        fn rexon_role_names(type_name: &str) -> QHash_i32_QByteArray;
    }

    // Регистрация типа в QML (qmlRegisterType в "waywallen.ui").
    unsafe extern "C++" {
        include!("rexon_register.h");
        fn rexon_do_register_filter_model();
    }

    extern "RustQt" {
        // Регистрация типа — императивная: extern "C" rexon_register_filter_model
        // (cpp/rexon_register.cpp) зовёт qmlRegisterType в "waywallen.ui", как
        // делал C++-оригинал. Окно грузит .so через dlopen (rexon-gui we_env).
        #[qobject]
        #[base = QAbstractListModel]
        #[qproperty(bool, dirty, READ = dirty, NOTIFY = dirty_changed)]
        #[qproperty(QList_QVariant, filter_logics, cxx_name = "filterLogics",
                    READ = filter_logics, NOTIFY = filter_logics_changed)]
        type WallpaperFilterRuleModel = super::WallpaperFilterRuleModelRust;
    }

    // ── Сигналы ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        /// Применить фильтры (QML вызывает model.apply(), обработчик onApply).
        #[qsignal]
        fn apply(self: Pin<&mut WallpaperFilterRuleModel>);
        /// Сбросить фильтры (QML вызывает model.reset(), обработчик onReset).
        #[qsignal]
        fn reset(self: Pin<&mut WallpaperFilterRuleModel>);
        #[qsignal]
        #[cxx_name = "dirtyChanged"]
        fn dirty_changed(self: Pin<&mut WallpaperFilterRuleModel>);
        #[qsignal]
        #[cxx_name = "filterLogicsChanged"]
        fn filter_logics_changed(self: Pin<&mut WallpaperFilterRuleModel>);

        /// dataChanged базового класса.
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut WallpaperFilterRuleModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QVector_i32,
        );
    }

    // ── Наследуемые методы базового класса ────────────────────────────────
    extern "RustQt" {
        /// # Safety
        /// Требует парного end_insert_rows.
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut WallpaperFilterRuleModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        /// # Safety
        /// Парный к begin_insert_rows.
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut WallpaperFilterRuleModel>);

        /// # Safety
        /// Требует парного end_remove_rows.
        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        unsafe fn begin_remove_rows(
            self: Pin<&mut WallpaperFilterRuleModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        /// # Safety
        /// Парный к begin_remove_rows.
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        unsafe fn end_remove_rows(self: Pin<&mut WallpaperFilterRuleModel>);

        /// # Safety
        /// Требует парного end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut WallpaperFilterRuleModel>);
        /// # Safety
        /// Парный к begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut WallpaperFilterRuleModel>);
    }

    // ── Переопределения QAbstractListModel ─────────────────────────────────
    // Без #[qinvokable]: QML зовёт rowCount()/setData()/removeRows() через
    // Q_INVOKABLE базового QAbstractItemModel, где у parent/role есть
    // default-аргументы (moc клонирует укороченные сигнатуры). Инвокабл в
    // наследнике затеняет клоны базы, и rowCount() без аргументов из JS
    // падает «Insufficient arguments» — ломая кнопку clear_all диалога.
    extern "RustQt" {
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &WallpaperFilterRuleModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &WallpaperFilterRuleModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &WallpaperFilterRuleModel, index: &QModelIndex, role: i32) -> QVariant;

        #[cxx_override]
        #[cxx_name = "setData"]
        fn set_data(
            self: Pin<&mut WallpaperFilterRuleModel>,
            index: &QModelIndex,
            value: &QVariant,
            role: i32,
        ) -> bool;

        #[cxx_override]
        #[cxx_name = "removeRows"]
        fn remove_rows(
            self: Pin<&mut WallpaperFilterRuleModel>,
            row: i32,
            count: i32,
            parent: &QModelIndex,
        ) -> bool;
    }

    // ── Property-геттеры ──────────────────────────────────────────────────
    extern "RustQt" {
        fn dirty(self: &WallpaperFilterRuleModel) -> bool;
        #[cxx_name = "filterLogics"]
        fn filter_logics(self: &WallpaperFilterRuleModel) -> QList_QVariant;
    }

    // ── Контракт оригинала (Q_INVOKABLE) ──────────────────────────────────
    extern "RustQt" {
        #[qinvokable]
        fn items(self: &WallpaperFilterRuleModel) -> QList_QVariant;
        #[qinvokable]
        #[cxx_name = "replaceState"]
        fn replace_state(
            self: Pin<&mut WallpaperFilterRuleModel>,
            filters: QList_QVariant,
            logics: QList_QVariant,
        );
        #[qinvokable]
        #[cxx_name = "newGroupId"]
        fn new_group_id(self: &WallpaperFilterRuleModel) -> i32;
        #[qinvokable]
        #[cxx_name = "countInGroup"]
        fn count_in_group(self: &WallpaperFilterRuleModel, group: i32) -> i32;
        #[qinvokable]
        #[cxx_name = "rowIndexInGroup"]
        fn row_index_in_group(self: &WallpaperFilterRuleModel, row: i32) -> i32;
        #[qinvokable]
        #[cxx_name = "rowCountInGroupOf"]
        fn row_count_in_group_of(self: &WallpaperFilterRuleModel, row: i32) -> i32;
        #[qinvokable]
        #[cxx_name = "findInsertPosition"]
        fn find_insert_position(self: &WallpaperFilterRuleModel, group: i32) -> i32;
        #[qinvokable]
        #[cxx_name = "sectionIndexForGroup"]
        fn section_index_for_group(self: &WallpaperFilterRuleModel, group: i32) -> i32;
        #[qinvokable]
        #[cxx_name = "findLogicAt"]
        fn find_logic_at(self: &WallpaperFilterRuleModel, section_index: i32) -> i32;
        #[qinvokable]
        #[cxx_name = "logicOpAt"]
        fn logic_op_at(self: &WallpaperFilterRuleModel, section_index: i32) -> i32;
        #[qinvokable]
        #[cxx_name = "setLogicOpAt"]
        fn set_logic_op_at(self: Pin<&mut WallpaperFilterRuleModel>, section_index: i32, op: i32);
        #[qinvokable]
        #[cxx_name = "appendRuleInGroup"]
        fn append_rule_in_group(self: Pin<&mut WallpaperFilterRuleModel>, group: i32);
        #[qinvokable]
        #[cxx_name = "appendNewGroup"]
        fn append_new_group(self: Pin<&mut WallpaperFilterRuleModel>);
        #[qinvokable]
        #[cxx_name = "deleteGroup"]
        fn delete_group(self: Pin<&mut WallpaperFilterRuleModel>, group: i32);
        #[qinvokable]
        #[cxx_name = "sortByGroup"]
        fn sort_by_group(self: Pin<&mut WallpaperFilterRuleModel>);
    }

    impl cxx_qt::Initialize for WallpaperFilterRuleModel {}
}

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QModelIndex, QVariant, QVector};

// Полные имена метатипов гаджетов (регистрируются модулем waywallen.control).
const RULE_TYPE: &str = "waywallen::control::v1::WallpaperFilterRule";
const LOGIC_TYPE: &str = "waywallen::control::v1::FilterLogic";
// Оригинал: роль == Qt::UserRole + 1 + индекс_свойства. Qt::UserRole == 256.
const ROLE_BASE: i32 = 256 + 1;
// LogicOp::LOGIC_OP_AND == 1 (см. filter.proto).
const LOGIC_OP_AND: i32 = 1;

type QVariantList = cxx_qt_lib::QList<QVariant>;

/// Экспортируемая точка входа: окно вкладки обоев грузит эту cdylib через
/// `dlopen` и берёт символ по имени `dlsym` (`rexon-gui`,
/// `we_env::register_filter_model`). Экспорт обязателен: крейт отдельный,
/// а модель — только в cdylib (гаджеты Qt-Protobuf маршалятся лишь в
/// C++/Rust). Делегирует в C++-шим (qmlRegisterType).
///
/// Вызывать один раз до загрузки QML, из потока с живым QGuiApplication.
/// Идемпотентно (повторный вызов — no-op).
#[no_mangle]
pub extern "C" fn rexon_register_filter_model() {
    qobject::rexon_do_register_filter_model();
}

/// Rust-состояние модели: строки-правила и логические операторы — гаджеты
/// в QVariant (мета-рефлексия — через C++-шим).
#[derive(Default)]
pub struct WallpaperFilterRuleModelRust {
    rows: Vec<QVariant>,
    logics: Vec<QVariant>,
    m_dirty: bool,
    connections: Vec<cxx_qt::QMetaObjectConnectionGuard>,
}

// Пост-конструкция: связать apply/reset со сбросом dirty (как коннекты в
// конструкторе C++-модели).
impl cxx_qt::Initialize for qobject::WallpaperFilterRuleModel {
    fn initialize(mut self: Pin<&mut Self>) {
        let apply_conn = self.as_mut().on_apply(|q| q.clear_dirty());
        let reset_conn = self.as_mut().on_reset(|q| q.clear_dirty());
        self.as_mut().rust_mut().connections.push(apply_conn);
        self.as_mut().rust_mut().connections.push(reset_conn);
    }
}

impl qobject::WallpaperFilterRuleModel {
    // ── dirty ─────────────────────────────────────────────────────────────
    fn dirty(&self) -> bool {
        self.m_dirty
    }

    fn set_dirty(mut self: Pin<&mut Self>, value: bool) {
        if self.m_dirty == value {
            return;
        }
        self.as_mut().rust_mut().m_dirty = value;
        self.as_mut().dirty_changed();
    }

    fn clear_dirty(self: Pin<&mut Self>) {
        self.set_dirty(false);
    }

    // ── filterLogics ──────────────────────────────────────────────────────
    fn filter_logics(&self) -> QVariantList {
        let mut out = QVariantList::default();
        for logic in &self.logics {
            out.append(logic.clone());
        }
        out
    }

    // Оповестить об изменении логик + пометить dirty (как filterLogicsChanged
    // → markDirty в оригинале).
    fn touch_logics(mut self: Pin<&mut Self>) {
        self.as_mut().filter_logics_changed();
        self.as_mut().set_dirty(true);
    }

    // ── QAbstractListModel ────────────────────────────────────────────────
    fn row_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() {
            0
        } else {
            self.rows.len() as i32
        }
    }

    fn role_names(&self) -> qobject::QHash_i32_QByteArray {
        qobject::rexon_role_names(RULE_TYPE)
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let row = index.row();
        if !index.is_valid() || row < 0 || row as usize >= self.rows.len() {
            return QVariant::default();
        }
        let i = role - ROLE_BASE;
        if i < 0 {
            return QVariant::default();
        }
        qobject::rexon_gadget_read_by_role(&self.rows[row as usize], i)
    }

    fn set_data(mut self: Pin<&mut Self>, index: &QModelIndex, value: &QVariant, role: i32) -> bool {
        let row = index.row();
        if !index.is_valid() || row < 0 || row as usize >= self.rows.len() {
            return false;
        }
        let i = role - ROLE_BASE;
        if i < 0 {
            return false;
        }
        let updated = qobject::rexon_gadget_set_by_role(&self.rows[row as usize], i, value);
        if !qobject::rexon_variant_is_valid(&updated) {
            return false;
        }
        self.as_mut().rust_mut().rows[row as usize] = updated;
        // запись payload меняет и has*-роли — оповещаем все роли строки
        let roles = QVector::<i32>::default();
        self.as_mut().data_changed(index, index, &roles);
        self.as_mut().set_dirty(true);
        true
    }

    fn remove_rows(mut self: Pin<&mut Self>, row: i32, count: i32, parent: &QModelIndex) -> bool {
        if parent.is_valid()
            || row < 0
            || count <= 0
            || (row + count) as usize > self.rows.len()
        {
            return false;
        }
        unsafe {
            self.as_mut()
                .begin_remove_rows(&QModelIndex::default(), row, row + count - 1);
            let start = row as usize;
            self.as_mut()
                .rust_mut()
                .rows
                .drain(start..start + count as usize);
            self.as_mut().end_remove_rows();
        }
        self.as_mut().set_dirty(true);
        self.as_mut().drop_orphan_logics();
        true
    }

    /// Убрать логики, ссылающиеся на группу, в которой не осталось правил.
    ///
    /// Кнопка «очистить всё» вендорного диалога 0.4.4 удаляет только строки
    /// (`removeRows`), а логики между группами остаются. Демон waywallen
    /// 0.5.0 на логику с несуществующей группой отвечает `InvalidArgument`
    /// на весь список обоев. Апстрим в 0.5.0 дописал очистку в QML; здесь
    /// она в модели — так её получают и «очистить всё», и удаление
    /// последнего правила группы любым путём.
    fn drop_orphan_logics(mut self: Pin<&mut Self>) {
        let groups: Vec<i32> = self
            .rows
            .iter()
            .map(|r| qobject::rexon_gadget_read_int(r, "group"))
            .collect();
        let pairs: Vec<(i32, i32)> = self
            .logics
            .iter()
            .map(|l| {
                (
                    qobject::rexon_gadget_read_int(l, "groupA"),
                    qobject::rexon_gadget_read_int(l, "groupB"),
                )
            })
            .collect();
        let orphans = orphan_logic_indices(&groups, &pairs);
        if orphans.is_empty() {
            return;
        }
        for i in orphans.into_iter().rev() {
            self.as_mut().rust_mut().logics.remove(i);
        }
        self.as_mut().touch_logics();
    }

    // ── Контракт оригинала ────────────────────────────────────────────────
    fn items(&self) -> QVariantList {
        let mut out = QVariantList::default();
        for rule in &self.rows {
            out.append(rule.clone());
        }
        out
    }

    fn replace_state(mut self: Pin<&mut Self>, filters: QVariantList, logics: QVariantList) {
        unsafe {
            self.as_mut().begin_reset_model();
            {
                let mut m = self.as_mut().rust_mut();
                m.rows.clear();
                for i in 0..filters.len() {
                    if let Some(v) = filters.get(i) {
                        m.rows.push(qobject::rexon_coerce_gadget(RULE_TYPE, v));
                    }
                }
            }
            self.as_mut().end_reset_model();
        }

        {
            let mut m = self.as_mut().rust_mut();
            m.logics.clear();
            for i in 0..logics.len() {
                if let Some(v) = logics.get(i) {
                    m.logics.push(qobject::rexon_coerce_gadget(LOGIC_TYPE, v));
                }
            }
        }
        self.as_mut().filter_logics_changed();

        self.as_mut().sort_by_group();
        self.as_mut().set_dirty(false);
    }

    fn new_group_id(&self) -> i32 {
        let mut max = -1;
        for rule in &self.rows {
            max = max.max(qobject::rexon_gadget_read_int(rule, "group"));
        }
        max + 1
    }

    fn count_in_group(&self, group: i32) -> i32 {
        self.rows
            .iter()
            .filter(|r| qobject::rexon_gadget_read_int(r, "group") == group)
            .count() as i32
    }

    fn row_index_in_group(&self, row: i32) -> i32 {
        if row < 0 || row as usize >= self.rows.len() {
            return 0;
        }
        let group = qobject::rexon_gadget_read_int(&self.rows[row as usize], "group");
        let mut count = 0;
        for i in 0..row as usize {
            if qobject::rexon_gadget_read_int(&self.rows[i], "group") == group {
                count += 1;
            }
        }
        count
    }

    fn row_count_in_group_of(&self, row: i32) -> i32 {
        if row < 0 || row as usize >= self.rows.len() {
            return 0;
        }
        let group = qobject::rexon_gadget_read_int(&self.rows[row as usize], "group");
        self.count_in_group(group)
    }

    fn find_insert_position(&self, group: i32) -> i32 {
        let mut pos = 0;
        let mut seen = false;
        for i in 0..self.rows.len() {
            let current = qobject::rexon_gadget_read_int(&self.rows[i], "group");
            if current <= group {
                pos = i as i32 + 1;
                seen = true;
            } else if seen {
                break;
            }
        }
        pos
    }

    fn section_index_for_group(&self, group: i32) -> i32 {
        self.ordered_groups()
            .iter()
            .position(|&g| g == group)
            .map(|p| p as i32)
            .unwrap_or(-1)
    }

    fn find_logic_at(&self, section_index: i32) -> i32 {
        if section_index <= 0 {
            return -1;
        }
        let groups = self.ordered_groups();
        if section_index as usize >= groups.len() {
            return -1;
        }
        let previous = groups[section_index as usize - 1];
        let current = groups[section_index as usize];
        for (i, logic) in self.logics.iter().enumerate() {
            if qobject::rexon_gadget_read_int(logic, "groupA") == previous
                && qobject::rexon_gadget_read_int(logic, "groupB") == current
            {
                return i as i32;
            }
        }
        -1
    }

    fn logic_op_at(&self, section_index: i32) -> i32 {
        let idx = self.find_logic_at(section_index);
        if idx < 0 {
            return -1;
        }
        qobject::rexon_gadget_read_int(&self.logics[idx as usize], "op")
    }

    fn set_logic_op_at(mut self: Pin<&mut Self>, section_index: i32, op: i32) {
        let idx = self.find_logic_at(section_index);
        if idx < 0 {
            let groups = self.ordered_groups();
            if section_index <= 0 || section_index as usize >= groups.len() {
                return;
            }
            let mut logic = qobject::rexon_default_gadget(LOGIC_TYPE);
            logic = qobject::rexon_gadget_set_int(&logic, "op", op);
            logic = qobject::rexon_gadget_set_int(&logic, "groupA", groups[section_index as usize - 1]);
            logic = qobject::rexon_gadget_set_int(&logic, "groupB", groups[section_index as usize]);
            self.as_mut().rust_mut().logics.push(logic);
        } else {
            if qobject::rexon_gadget_read_int(&self.logics[idx as usize], "op") == op {
                return;
            }
            let updated = qobject::rexon_gadget_set_int(&self.logics[idx as usize], "op", op);
            self.as_mut().rust_mut().logics[idx as usize] = updated;
        }
        self.as_mut().touch_logics();
    }

    fn append_rule_in_group(mut self: Pin<&mut Self>, group: i32) {
        let row = self.find_insert_position(group);
        let mut rule = qobject::rexon_default_gadget(RULE_TYPE);
        rule = qobject::rexon_gadget_set_int(&rule, "group", group);
        unsafe {
            self.as_mut()
                .begin_insert_rows(&QModelIndex::default(), row, row);
            self.as_mut().rust_mut().rows.insert(row as usize, rule);
            self.as_mut().end_insert_rows();
        }
        self.as_mut().set_dirty(true);
    }

    fn append_new_group(mut self: Pin<&mut Self>) {
        let new_id = self.new_group_id();
        let previous_max = self
            .rows
            .last()
            .map(|r| qobject::rexon_gadget_read_int(r, "group"))
            .unwrap_or(-1);
        self.as_mut().append_rule_in_group(new_id);
        if previous_max >= 0 {
            let mut logic = qobject::rexon_default_gadget(LOGIC_TYPE);
            logic = qobject::rexon_gadget_set_int(&logic, "op", LOGIC_OP_AND);
            logic = qobject::rexon_gadget_set_int(&logic, "groupA", previous_max);
            logic = qobject::rexon_gadget_set_int(&logic, "groupB", new_id);
            self.as_mut().rust_mut().logics.push(logic);
            self.as_mut().touch_logics();
        }
    }

    fn delete_group(mut self: Pin<&mut Self>, group: i32) {
        for i in (0..self.rows.len()).rev() {
            if qobject::rexon_gadget_read_int(&self.rows[i], "group") == group {
                self.as_mut().remove_rows(i as i32, 1, &QModelIndex::default());
            }
        }
        let mut changed = false;
        for i in (0..self.logics.len()).rev() {
            let a = qobject::rexon_gadget_read_int(&self.logics[i], "groupA");
            let b = qobject::rexon_gadget_read_int(&self.logics[i], "groupB");
            if a == group || b == group {
                self.as_mut().rust_mut().logics.remove(i);
                changed = true;
            }
        }
        if changed {
            self.as_mut().touch_logics();
        }
    }

    fn sort_by_group(mut self: Pin<&mut Self>) {
        unsafe {
            self.as_mut().begin_reset_model();
            self.as_mut().rust_mut().rows.sort_by(|a, b| {
                qobject::rexon_gadget_read_int(a, "group")
                    .cmp(&qobject::rexon_gadget_read_int(b, "group"))
            });
            self.as_mut().end_reset_model();
        }
    }

    // Последовательность групп в порядке строк (схлопывая повторы) —
    // соответствует orderedGroups() оригинала.
    fn ordered_groups(&self) -> Vec<i32> {
        let mut groups: Vec<i32> = Vec::new();
        for rule in &self.rows {
            let group = qobject::rexon_gadget_read_int(rule, "group");
            if groups.last() != Some(&group) {
                groups.push(group);
            }
        }
        groups
    }
}

/// Номера логик (по возрастанию), чья группа `groupA` или `groupB` не
/// встречается среди групп оставшихся правил.
fn orphan_logic_indices(groups: &[i32], logics: &[(i32, i32)]) -> Vec<usize> {
    logics
        .iter()
        .enumerate()
        .filter(|(_, (a, b))| !groups.contains(a) || !groups.contains(b))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::orphan_logic_indices;

    #[test]
    fn clearing_all_rules_orphans_every_logic() {
        // «Очистить всё»: правил нет — ни одна логика не уцелеет.
        assert_eq!(orphan_logic_indices(&[], &[(0, 1), (1, 2)]), vec![0, 1]);
    }

    #[test]
    fn only_logics_touching_a_vanished_group_go() {
        // Группа 1 опустела: уходят обе её связи, связь 0–2 остаётся.
        let groups = [0, 0, 2];
        let logics = [(0, 1), (1, 2), (0, 2)];
        assert_eq!(orphan_logic_indices(&groups, &logics), vec![0, 1]);
        assert!(orphan_logic_indices(&[0, 1], &[(0, 1)]).is_empty());
    }
}
