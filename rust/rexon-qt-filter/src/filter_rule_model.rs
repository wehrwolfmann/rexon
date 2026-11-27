// WallpaperFilterRuleModel in Rust via cxx-qt — a trial port of the C++ model
// of the Wallpaper Engine tab filter editor (rexon/qt_wallpaper_engine/
// qml_ui/cpp/filter_rule_model.cpp). Behavior is 1:1 with the original.
//
// The original C++ model (a port of waywallen ui/src/model/filter_rule_model.cpp,
// MIT, (c) hypengw — attribution in NOTICE) is a flat QAbstractListModel
// over QList<WallpaperFilterRule>.
//
// The logic (groups / logical operators / dirty / signals) is entirely in
// Rust. The only thing Rust cannot express is meta-reflection of foreign
// Qt-Protobuf gadgets (their staticMetaObject); it is moved into a generic
// C++ shim rexon_gadget_shim.cpp (without depending on filter.qpb.h).

/// cxx-qt bridge: the QAbstractListModel subclass WallpaperFilterRuleModel.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++Qt" {
        include!(<QtCore/QAbstractListModel>);
        /// Base Qt type.
        #[qobject]
        type QAbstractListModel;
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qhash.h");
        /// QHash<i32, QByteArray> — for roleNames.
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;

        include!("cxx-qt-lib/qvariant.h");
        /// QVariant.
        type QVariant = cxx_qt_lib::QVariant;

        include!("cxx-qt-lib/qmodelindex.h");
        /// QModelIndex.
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvector.h");
        /// QVector<i32> — the set of roles for dataChanged.
        type QVector_i32 = cxx_qt_lib::QVector<i32>;

        include!("cxx-qt-lib/qlist.h");
        /// QVariantList (QList<QVariant>) — items() / replaceState(). The alias name
        /// must match the cxx-qt-lib ExternType-Id: QList_QVariant.
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    // Generic bridge to gadget meta-reflection (see rexon_gadget_shim.h).
    unsafe extern "C++" {
        include!("rexon_gadget_shim.h");
        fn rexon_default_gadget(type_name: &str) -> QVariant;
        fn rexon_coerce_gadget(type_name: &str, value: &QVariant) -> QVariant;
        fn rexon_gadget_read_by_role(gadget: &QVariant, role_index: i32) -> QVariant;
        fn rexon_gadget_set_by_role(gadget: &QVariant, role_index: i32, value: &QVariant)
            -> QVariant;
        fn rexon_gadget_read_int(gadget: &QVariant, prop: &str) -> i32;
        fn rexon_gadget_read_i64(gadget: &QVariant, prop: &str) -> i64;
        fn rexon_gadget_read_named(gadget: &QVariant, prop: &str) -> QVariant;
        fn rexon_gadget_set_int(gadget: &QVariant, prop: &str, value: i32) -> QVariant;
        fn rexon_variant_is_valid(v: &QVariant) -> bool;
        fn rexon_role_names(type_name: &str) -> QHash_i32_QByteArray;
    }

    // Registering the type in QML (qmlRegisterType in "waywallen.ui").
    unsafe extern "C++" {
        include!("rexon_register.h");
        fn rexon_do_register_filter_model();
    }

    extern "RustQt" {
        // Type registration is imperative: extern "C" rexon_register_filter_model
        // (cpp/rexon_register.cpp) calls qmlRegisterType in "waywallen.ui", as
        // the C++ original did. The window loads the .so via dlopen (rexon-gui we_env).
        #[qobject]
        #[base = QAbstractListModel]
        #[qproperty(bool, dirty, READ = dirty, NOTIFY = dirty_changed)]
        // waywallen 0.6.1: the draft can be applied only when every rule and
        // group link is something the daemon accepts.
        #[qproperty(bool, valid, READ = valid, NOTIFY = draft_changed)]
        #[qproperty(QList_QVariant, filter_logics, cxx_name = "filterLogics",
                    READ = filter_logics, NOTIFY = filter_logics_changed)]
        type WallpaperFilterRuleModel = super::WallpaperFilterRuleModelRust;
    }

    // ── Signals ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        /// waywallen 0.6.1: `apply()` is an invokable now; pages listen to
        /// `applied`, which fires only for a valid draft.
        #[qsignal]
        fn applied(self: Pin<&mut WallpaperFilterRuleModel>);
        /// waywallen 0.6.1: the draft changed (NOTIFY of `valid`).
        #[qsignal]
        #[cxx_name = "draftChanged"]
        fn draft_changed(self: Pin<&mut WallpaperFilterRuleModel>);
        /// Reset the filters (QML calls model.reset(), the onReset handler).
        #[qsignal]
        fn reset(self: Pin<&mut WallpaperFilterRuleModel>);
        #[qsignal]
        #[cxx_name = "dirtyChanged"]
        fn dirty_changed(self: Pin<&mut WallpaperFilterRuleModel>);
        #[qsignal]
        #[cxx_name = "filterLogicsChanged"]
        fn filter_logics_changed(self: Pin<&mut WallpaperFilterRuleModel>);

        /// dataChanged of the base class.
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

    // ── Inherited methods of the base class ────────────────────────────────
    extern "RustQt" {
        /// # Safety
        /// Requires a matching end_insert_rows.
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut WallpaperFilterRuleModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        /// # Safety
        /// Pairs with begin_insert_rows.
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut WallpaperFilterRuleModel>);

        /// # Safety
        /// Requires a matching end_remove_rows.
        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        unsafe fn begin_remove_rows(
            self: Pin<&mut WallpaperFilterRuleModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        /// # Safety
        /// Pairs with begin_remove_rows.
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        unsafe fn end_remove_rows(self: Pin<&mut WallpaperFilterRuleModel>);

        /// # Safety
        /// Requires a matching end_reset_model.
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut WallpaperFilterRuleModel>);
        /// # Safety
        /// Pairs with begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut WallpaperFilterRuleModel>);
    }

    // ── QAbstractListModel overrides ─────────────────────────────────
    // Without #[qinvokable]: QML calls rowCount()/setData()/removeRows() through
    // the Q_INVOKABLE of the base QAbstractItemModel, where parent/role have
    // default arguments (moc clones the shortened signatures). An invokable in
    // the subclass shadows the base clones, and rowCount() without arguments from JS
    // fails with "Insufficient arguments" — breaking the dialog's clear_all button.
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

    // ── Property getters ──────────────────────────────────────────────────
    extern "RustQt" {
        fn dirty(self: &WallpaperFilterRuleModel) -> bool;
        fn valid(self: &WallpaperFilterRuleModel) -> bool;
        #[cxx_name = "filterLogics"]
        fn filter_logics(self: &WallpaperFilterRuleModel) -> QList_QVariant;
    }

    // ── Contract of the original (Q_INVOKABLE) ──────────────────────────────────
    extern "RustQt" {
        #[qinvokable]
        fn items(self: &WallpaperFilterRuleModel) -> QList_QVariant;
        #[qinvokable]
        fn apply(self: Pin<&mut WallpaperFilterRuleModel>);
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

// Full metatype names of the gadgets (registered by the waywallen.control module).
const RULE_TYPE: &str = "waywallen::control::v1::WallpaperFilterRule";
const LOGIC_TYPE: &str = "waywallen::control::v1::FilterLogic";
// Original: role == Qt::UserRole + 1 + property_index. Qt::UserRole == 256.
const ROLE_BASE: i32 = 256 + 1;
const STRING_FILTER_TYPE: &str = "waywallen::control::v1::WallpaperStringFilter";
/// `WALLPAPER_FILTER_TYPE_NAME` (filter.proto).
const RULE_TYPE_NAME: i32 = 1;
/// `STRING_CONDITION_CONTAINS` (filter.proto).
const STRING_CONDITION_CONTAINS: i32 = 1;

/// Property index of `name` on the rule gadget (its role minus `ROLE_BASE`).
fn rule_role(rule: &QVariant, name: &str) -> i32 {
    let _ = rule;
    let names = qobject::rexon_role_names(RULE_TYPE);
    for (role, n) in names.iter() {
        if n.as_slice() == name.as_bytes() {
            return *role - ROLE_BASE;
        }
    }
    -1
}

/// What validation needs from one rule. Inactive oneof branches are `None`
/// (their getters are never touched, see `rule_view`).
#[derive(Debug, Clone, Default, PartialEq)]
struct RuleView {
    group: i32,
    ty: i32,
    string_condition: Option<i32>,
    /// (condition, value)
    int_filter: Option<(i32, i64)>,
    tag_condition: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LogicView {
    op: i32,
    group_a: i32,
    group_b: i32,
}

fn rule_view(rule: &QVariant) -> RuleView {
    let read = |name: &str| qobject::rexon_gadget_read_int(rule, name);
    let branch = |has: &str, name: &str| {
        (read(has) != 0).then(|| qobject::rexon_gadget_read_named(rule, name))
    };
    RuleView {
        group: read("group"),
        ty: read("type"),
        string_condition: branch("hasStringFilter", "stringFilter")
            .map(|f| qobject::rexon_gadget_read_int(&f, "condition")),
        int_filter: branch("hasIntFilter", "intFilter").map(|f| {
            (
                qobject::rexon_gadget_read_int(&f, "condition"),
                qobject::rexon_gadget_read_i64(&f, "value"),
            )
        }),
        tag_condition: branch("hasTagFilter", "tagFilter")
            .map(|f| qobject::rexon_gadget_read_int(&f, "condition")),
    }
}

/// One rule is something the daemon accepts: a known type with the payload
/// and condition codes that type takes (waywallen 0.6.1 filter validation).
fn filter_rule_valid(r: &RuleView) -> bool {
    if r.group < 0 {
        return false;
    }
    let string_ok = matches!(r.string_condition, Some(c) if (1..=4).contains(&c));
    let int_ok = matches!(r.int_filter, Some((c, _)) if (1..=6).contains(&c));
    match r.ty {
        // name, author, path
        1 | 2 | 8 => string_ok,
        // size: positive value, only "equal"/"not equal"-style conditions
        3 => int_ok && matches!(r.int_filter, Some((c, v)) if v > 0 && c <= 2),
        5..=7 => int_ok,
        9 => string_ok || matches!(r.tag_condition, Some(c) if (1..=4).contains(&c)),
        _ => false,
    }
}

/// The whole draft: every rule valid, every group link has a known operator
/// and points at groups that still have rules.
fn filter_state_valid(rules: &[RuleView], logics: &[LogicView]) -> bool {
    if !rules.iter().all(filter_rule_valid) {
        return false;
    }
    logics.iter().all(|l| {
        (l.op == 1 || l.op == 2)
            && rules.iter().any(|r| r.group == l.group_a)
            && rules.iter().any(|r| r.group == l.group_b)
    })
}
// LogicOp::LOGIC_OP_AND == 1 (see filter.proto).
const LOGIC_OP_AND: i32 = 1;

type QVariantList = cxx_qt_lib::QList<QVariant>;

/// Exported entry point: the wallpaper tab window loads this cdylib via
/// `dlopen` and takes the symbol by name with `dlsym` (`rexon-gui`,
/// `we_env::register_filter_model`). The export is mandatory: the crate is separate,
/// and the model exists only in the cdylib (Qt-Protobuf gadgets are marshalled only in
/// C++/Rust). Delegates to the C++ shim (qmlRegisterType).
///
/// Call once before QML is loaded, from a thread with a live QGuiApplication.
/// Idempotent (a repeated call is a no-op).
#[no_mangle]
pub extern "C" fn rexon_register_filter_model() {
    qobject::rexon_do_register_filter_model();
}

/// Rust state of the model: rule rows and logical operators — gadgets
/// in QVariant (meta-reflection — through the C++ shim).
#[derive(Default)]
pub struct WallpaperFilterRuleModelRust {
    rows: Vec<QVariant>,
    logics: Vec<QVariant>,
    m_dirty: bool,
    connections: Vec<cxx_qt::QMetaObjectConnectionGuard>,
}

// Post-construction: connect apply/reset to clearing dirty (like the connects in
// the C++ model's constructor).
impl cxx_qt::Initialize for qobject::WallpaperFilterRuleModel {
    fn initialize(mut self: Pin<&mut Self>) {
        // waywallen 0.6.1: `apply` is no longer a signal (see `apply()`).
        let reset_conn = self.as_mut().on_reset(|q| q.clear_dirty());
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

    /// `markDirty` of the original: every edit marks the draft dirty and
    /// re-announces `valid` (waywallen 0.6.1 `draftChanged`).
    fn mark_dirty(mut self: Pin<&mut Self>) {
        self.as_mut().set_dirty(true);
        self.as_mut().draft_changed();
    }

    fn valid(&self) -> bool {
        let rules: Vec<RuleView> = self.rows.iter().map(rule_view).collect();
        let logics: Vec<LogicView> = self
            .logics
            .iter()
            .map(|l| LogicView {
                op: qobject::rexon_gadget_read_int(l, "op"),
                group_a: qobject::rexon_gadget_read_int(l, "groupA"),
                group_b: qobject::rexon_gadget_read_int(l, "groupB"),
            })
            .collect();
        filter_state_valid(&rules, &logics)
    }

    /// waywallen 0.6.1: an invalid draft is never applied.
    fn apply(mut self: Pin<&mut Self>) {
        if !self.valid() {
            return;
        }
        self.as_mut().set_dirty(false);
        self.as_mut().applied();
    }

    // ── filterLogics ──────────────────────────────────────────────────────
    fn filter_logics(&self) -> QVariantList {
        let mut out = QVariantList::default();
        for logic in &self.logics {
            out.append(logic.clone());
        }
        out
    }

    // Notify about a change of the logics + mark dirty (like filterLogicsChanged
    // → markDirty in the original).
    fn touch_logics(mut self: Pin<&mut Self>) {
        self.as_mut().filter_logics_changed();
        self.as_mut().mark_dirty();
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
        // writing the payload changes the has* roles too — notify all roles of the row
        let roles = QVector::<i32>::default();
        self.as_mut().data_changed(index, index, &roles);
        self.as_mut().mark_dirty();
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
        self.as_mut().mark_dirty();
        self.as_mut().drop_orphan_logics();
        true
    }

    /// Remove logics that refer to a group with no rules left.
    ///
    /// The «Clear all» button of the vendor dialog 0.4.4 removes only rows
    /// (`removeRows`), while the logics between groups remain. The waywallen
    /// 0.5.0 daemon answers a logic with a nonexistent group with `InvalidArgument`
    /// for the whole wallpaper list. Upstream added the cleanup in QML in 0.5.0; here
    /// it is in the model — that way both «Clear all» and removing the
    /// last rule of a group by any path get it.
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

    // ── Contract of the original ────────────────────────────────────────────────
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
        self.as_mut().draft_changed();
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
        // waywallen 0.6.1: a new row starts as a complete "Name contains"
        // rule instead of an empty one (which would make the draft invalid).
        rule = qobject::rexon_gadget_set_int(&rule, "type", RULE_TYPE_NAME);
        let mut string_filter = qobject::rexon_default_gadget(STRING_FILTER_TYPE);
        string_filter =
            qobject::rexon_gadget_set_int(&string_filter, "condition", STRING_CONDITION_CONTAINS);
        let role = rule_role(&rule, "stringFilter");
        if role >= 0 {
            let with_filter = qobject::rexon_gadget_set_by_role(&rule, role, &string_filter);
            if qobject::rexon_variant_is_valid(&with_filter) {
                rule = with_filter;
            }
        }
        unsafe {
            self.as_mut()
                .begin_insert_rows(&QModelIndex::default(), row, row);
            self.as_mut().rust_mut().rows.insert(row as usize, rule);
            self.as_mut().end_insert_rows();
        }
        self.as_mut().mark_dirty();
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

    // The sequence of groups in row order (collapsing repeats) —
    // corresponds to orderedGroups() of the original.
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

/// Indices of logics (ascending) whose group `groupA` or `groupB` does not
/// occur among the groups of the remaining rules.
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
    use super::{filter_state_valid, orphan_logic_indices, LogicView, RuleView};

    fn name_contains(group: i32) -> RuleView {
        RuleView { group, ty: 1, string_condition: Some(1), ..RuleView::default() }
    }

    #[test]
    fn empty_draft_is_valid() {
        assert!(filter_state_valid(&[], &[]));
    }

    #[test]
    fn rule_without_its_payload_is_invalid() {
        let bare = RuleView { group: 0, ty: 1, ..RuleView::default() };
        assert!(!filter_state_valid(&[bare], &[]));
        assert!(filter_state_valid(&[name_contains(0)], &[]));
    }

    #[test]
    fn size_needs_positive_value_and_low_condition() {
        let size = |c, v| RuleView { group: 0, ty: 3, int_filter: Some((c, v)), ..RuleView::default() };
        assert!(filter_state_valid(&[size(1, 10)], &[]));
        assert!(!filter_state_valid(&[size(1, 0)], &[]));
        assert!(!filter_state_valid(&[size(3, 10)], &[]));
    }

    #[test]
    fn logic_must_link_existing_groups_with_known_op() {
        let rules = [name_contains(0), name_contains(1)];
        let ok = LogicView { op: 1, group_a: 0, group_b: 1 };
        assert!(filter_state_valid(&rules, &[ok]));
        assert!(!filter_state_valid(&rules, &[LogicView { op: 0, ..ok }]));
        assert!(!filter_state_valid(&rules, &[LogicView { group_b: 5, ..ok }]));
    }

    #[test]
    fn clearing_all_rules_orphans_every_logic() {
        // «Clear all»: there are no rules — no logic survives.
        assert_eq!(orphan_logic_indices(&[], &[(0, 1), (1, 2)]), vec![0, 1]);
    }

    #[test]
    fn only_logics_touching_a_vanished_group_go() {
        // Group 1 became empty: both of its links go away, the 0–2 link remains.
        let groups = [0, 0, 2];
        let logics = [(0, 1), (1, 2), (0, 2)];
        assert_eq!(orphan_logic_indices(&groups, &logics), vec![0, 1]);
        assert!(orphan_logic_indices(&[0, 1], &[(0, 1)]).is_empty());
    }
}
