// UserPropertyListModel — редактор свойств обои в панели деталей вкладки
// Wallpaper Engine, перенесённый с Python на Rust/cxx-qt.
// Модель без
// гаджетов Qt-Protobuf и без сети — на вход только две строки JSON
// (`userPropertiesSchema` и `userPropertyOverrides` обоины), на выход строки
// для вендорного QML.
//
// Оригинал — rexon/qt_wallpaper_engine/qml_ui/bridge.py:4180 (сам он порт
// model/user_property_model.cpp waywallen @v0.2.5). Вендорный QML
// (page/WallpaperDetailPanel.qml) НЕ трогаем, поэтому имена типа, свойств,
// сигналов, ролей и строк секций обязаны совпадать посимвольно: секция
// сравнивается в QML строкой (с waywallen 0.3.8 это "property"/"user", до
// того — "Properties"/"User properties").
//
// Разделение слоёв здесь такое же, как в остальном крейте: весь разбор и всё
// состояние живут в чистой структуре `UserProperties` без Qt (её и проверяет
// голден-тест против ЖИВОЙ Python-модели), а QObject только раскладывает
// готовое по ролям и шлёт сигналы.

/// Мост cxx-qt: подкласс QAbstractListModel.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        /// Базовый Qt-тип: обёртки QAbstractListModel в cxx-qt-lib нет, её
        /// объявляет сам мост.
        ///
        /// ⚠️ Здесь он объявлен БЕЗ `#[qobject]`. Причина найдена линкером: на
        /// каждый `#[qobject]`-тип в `extern "C++Qt"` cxx-qt печатает пару
        /// шимов приведения `cxx_qt_ffi_<Тип>_upcastPtr/downcastPtr`, и имя
        /// у них — по имени ТИПА, а не по имени моста. Второй мост в том же
        /// крейте, объявивший ту же базу, даёт `duplicate symbol` на
        /// `cxx_qt_ffi_QAbstractListModel_upcastPtr`. Без `#[qobject]` шимы
        /// не печатаются, а `#[base = QAbstractListModel]`, `#[inherit]` и
        /// `#[cxx_override]` работают: они висят на наследнике.
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

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;

        include!("cxx-qt-lib/qvector.h");
        /// Набор ролей для dataChanged (в Qt6 QVector<int> == QList<int>).
        type QVector_i32 = cxx_qt_lib::QVector<i32>;

        include!("rexon_register_we.h");
        /// Перевод подписи плагина (`localized_text` схемы) синглтоном
        /// `PluginTranslations` — тем же, к которому оригинал ходит через
        /// `PluginTranslationStore::instance()`. Пустая строка означает
        /// «перевода нет», и тогда показывается подпись из схемы.
        #[cxx_name = "rexon_we_plugin_translate"]
        fn plugin_translate(message_json: &QString) -> QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        // Схема и оверрайды приходят из QML привязками к свойствам обоины —
        // отсюда WRITE. Флаги в 0.9 пишутся ЗАГЛАВНЫМИ, и если передан хоть
        // один, READ обязателен явно.
        #[qproperty(QString, schema_json, cxx_name = "schemaJson",
                    READ = schema_json, WRITE = set_schema_json,
                    NOTIFY = schema_json_changed)]
        #[qproperty(QString, overrides_json, cxx_name = "overridesJson",
                    READ = overrides_json, WRITE = set_overrides_json,
                    NOTIFY = overrides_json_changed)]
        #[qproperty(i32, count, READ = count, NOTIFY = count_changed)]
        // Оба флага «есть что сбрасывать» висят на ОДНОМ сигнале — так же,
        // как в оригинале: заголовок секции гасит кнопку сброса по ним.
        #[qproperty(bool, has_predefined_property_overrides,
                    cxx_name = "hasPredefinedPropertyOverrides",
                    READ = has_predefined_property_overrides,
                    NOTIFY = override_state_changed)]
        #[qproperty(bool, has_user_property_overrides,
                    cxx_name = "hasUserPropertyOverrides",
                    READ = has_user_property_overrides,
                    NOTIFY = override_state_changed)]
        type UserPropertyListModel = super::UserPropertyListModelRust;
    }

    // ── Сигналы ───────────────────────────────────────────────────────────
    unsafe extern "RustQt" {
        #[qsignal]
        #[cxx_name = "schemaJsonChanged"]
        fn schema_json_changed(self: Pin<&mut UserPropertyListModel>);
        #[qsignal]
        #[cxx_name = "overridesJsonChanged"]
        fn overrides_json_changed(self: Pin<&mut UserPropertyListModel>);
        #[qsignal]
        #[cxx_name = "countChanged"]
        fn count_changed(self: Pin<&mut UserPropertyListModel>);
        #[qsignal]
        #[cxx_name = "overrideStateChanged"]
        fn override_state_changed(self: Pin<&mut UserPropertyListModel>);

        /// Пользователь изменил значение свойства. Эмитится ТОЛЬКО от
        /// действий пользователя (setValue): QML дебаунсит его таймером и
        /// шлёт WallpaperPropertySetQuery. Программная установка
        /// `overridesJson` этот сигнал НЕ поднимает — иначе ответ демона
        /// закольцевался бы обратно в запись.
        #[qsignal]
        #[cxx_name = "valueChanged"]
        fn value_changed(self: Pin<&mut UserPropertyListModel>, key: QString, value: QString);

        /// Сброс свойства к значению по умолчанию. Отдельный сигнал, а не
        /// `valueChanged` с пустой строкой: с v0.2.5 пустая строка — ВАЛИДНОЕ
        /// значение текстовых свойств, ей сбрасывать нельзя.
        #[qsignal]
        #[cxx_name = "resetRequested"]
        fn reset_requested(self: Pin<&mut UserPropertyListModel>, key: QString);

        /// dataChanged базового класса — точечное обновление строки.
        #[inherit]
        #[qsignal]
        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut UserPropertyListModel>,
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
        unsafe fn begin_reset_model(self: Pin<&mut UserPropertyListModel>);
        /// # Safety
        /// Парный к begin_reset_model.
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut UserPropertyListModel>);
    }

    // index() безопасен для вызова, но cxx-qt требует, чтобы безопасные
    // #[inherit]-функции жили в unsafe-блоке (unsafe тут про соответствие
    // объявления C++-сигнатуре, а не про опасность вызова).
    unsafe extern "RustQt" {
        #[inherit]
        fn index(
            self: &UserPropertyListModel,
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
        fn row_count(self: &UserPropertyListModel, parent: &QModelIndex) -> i32;

        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &UserPropertyListModel) -> QHash_i32_QByteArray;

        #[cxx_override]
        fn data(self: &UserPropertyListModel, index: &QModelIndex, role: i32) -> QVariant;
    }

    // ── Чтение/запись свойств ─────────────────────────────────────────────
    extern "RustQt" {
        #[cxx_name = "schemaJson"]
        fn schema_json(self: &UserPropertyListModel) -> QString;
        #[cxx_name = "setSchemaJson"]
        fn set_schema_json(self: Pin<&mut UserPropertyListModel>, value: &QString);

        #[cxx_name = "overridesJson"]
        fn overrides_json(self: &UserPropertyListModel) -> QString;
        #[cxx_name = "setOverridesJson"]
        fn set_overrides_json(self: Pin<&mut UserPropertyListModel>, value: &QString);

        fn count(self: &UserPropertyListModel) -> i32;

        #[cxx_name = "hasPredefinedPropertyOverrides"]
        fn has_predefined_property_overrides(self: &UserPropertyListModel) -> bool;
        #[cxx_name = "hasUserPropertyOverrides"]
        fn has_user_property_overrides(self: &UserPropertyListModel) -> bool;
    }

    // ── API для QML ───────────────────────────────────────────────────────
    extern "RustQt" {
        /// Запись значения пользователем. Пустая строка — обычное значение,
        /// оверрайд она не снимает (для снятия есть reset*).
        #[qinvokable]
        #[cxx_name = "setValue"]
        fn set_value(self: Pin<&mut UserPropertyListModel>, key: &QString, value: &QString);

        /// Сбросить все свойства к значениям по умолчанию.
        #[qinvokable]
        #[cxx_name = "resetAll"]
        fn reset_all(self: Pin<&mut UserPropertyListModel>);

        /// Сбросить только предопределённые свойства (секция "Properties").
        #[qinvokable]
        #[cxx_name = "resetPredefinedProperties"]
        fn reset_predefined_properties(self: Pin<&mut UserPropertyListModel>);

        /// Сбросить только пользовательские свойства обоины.
        #[qinvokable]
        #[cxx_name = "resetUserProperties"]
        fn reset_user_properties(self: Pin<&mut UserPropertyListModel>);

        /// Номер роли по имени (-1, если роли нет) — чтобы стенд мог сверить,
        /// какая именно роль приехала в dataChanged.
        #[qinvokable]
        #[cxx_name = "roleNumber"]
        fn role_number(self: &UserPropertyListModel, name: &QString) -> i32;

        /// Значение роли по номеру строки — проверки без делегата.
        #[qinvokable]
        #[cxx_name = "roleValue"]
        fn role_value(self: &UserPropertyListModel, row: i32, name: &QString) -> QVariant;
    }

    impl cxx_qt::Initialize for UserPropertyListModel {}
    // Подписка на приход каталогов подписей идёт через `CxxQtThread::queue`,
    // и трейт потока обязан быть объявлен В МОСТЕ: cxx-qt печатает по этой
    // строке и сам `qt_thread()`, и его алиас типа.
    impl cxx_qt::Threading for UserPropertyListModel {}
}

use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QByteArray, QModelIndex, QString, QStringList, QVariant, QVector};

use serde_json::{Map, Value};
use std::collections::HashMap;

// ============================================================================
// Чистый слой: разбор схемы и состояние оверрайдов (Qt здесь нет)
// ============================================================================

/// Ключ предопределённого свойства «цвет схемы» — он есть всегда, даже когда
/// схема обоины пуста.
pub const SCHEME_COLOR_KEY: &str = "waywallen.scheme_color";
/// Ключ предопределённого свойства «звук» — строка появляется, только если
/// ключ есть в схеме.
pub const ENABLE_AUDIO_KEY: &str = "waywallen.enable_audio";
/// Ключ предопределённого свойства «скорость воспроизведения» — как и звук,
/// строка появляется только при наличии ключа в схеме.
pub const PLAYBACK_SPEED_KEY: &str = "waywallen.playback_speed";
/// Ключ предопределённого свойства «параллакс от мыши» (waywallen 0.4.2) —
/// строка появляется только при наличии ключа в схеме.
pub const MOUSE_PARALLAX_KEY: &str = "waywallen.mouse_parallax";

/// Все предопределённые ключи: их пропускает разбор пользовательских
/// свойств, иначе они уехали бы во вторую секцию дважды.
const PREDEFINED_KEYS: [&str; 4] = [
    SCHEME_COLOR_KEY,
    ENABLE_AUDIO_KEY,
    PLAYBACK_SPEED_KEY,
    MOUSE_PARALLAX_KEY,
];

/// Названия секций. Вендорный QML сравнивает секцию со строкой и по ней же
/// выбирает заголовок (`WallpaperDetailPanel.qml`), поэтому строки
/// технические и переводу НЕ подлежат.
///
/// ⚠ waywallen 0.3.8 переименовал их из "Properties"/"User properties" в
/// "property"/"user" — ровно в значения [`KIND_PREDEFINED`]/[`KIND_USER`].
/// Причина у вендора та же, что была бы у нас: по этим строкам сравнивают,
/// а показывают перевод `qsTr("Properties")`, и человекочитаемая строка в
/// роли ключа означала бы, что перевод ломает сравнение.
pub const SECTION_PREDEFINED: &str = KIND_PREDEFINED;
pub const SECTION_USER: &str = KIND_USER;
/// Вид записи: предопределённое свойство демона либо свойство самой обоины.
pub const KIND_PREDEFINED: &str = "property";
pub const KIND_USER: &str = "user";

/// Цвет схемы по умолчанию, когда в схеме его нет или он пуст.
const DEFAULT_SCHEME_COLOR: &str = "0.0000 0.0000 0.0000 1.0000";
/// Скорость воспроизведения по умолчанию (проценты) — запасное значение
/// оригинала, когда схема своего не дала.
const DEFAULT_PLAYBACK_SPEED: &str = "100";

/// Типы, для которых в панели деталей есть редактор.
const SUPPORTED_TYPES: [&str; 5] = ["color", "slider", "bool", "textinput", "text"];

/// Строка редактора свойств. Всё, кроме текущего значения, посчитано один раз
/// при разборе схемы: data() зовётся на каждую перерисовку делегата.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    pub label: String,
    pub kind_of_editor: String,
    pub section: &'static str,
    pub kind: &'static str,
    pub supported: bool,
    pub min_val: f64,
    pub max_val: f64,
    /// Шаг ползунка из схемы (`step`). Ноль означает «шага нет»: разметка
    /// сама подбирает его по величине максимума.
    pub step_val: f64,
    /// Приписка к числу ползунка из схемы (`suffix`), например «%».
    pub value_suffix: String,
    /// Значение по умолчанию из схемы, приведённое к проводному виду.
    pub default_wire: String,
    pub option_labels: Vec<String>,
    pub option_values: Vec<String>,
    /// Подпись плагина (`localized_text`) сырым JSON: переводом её делает
    /// синглтон `PluginTranslations`, а не чистый слой.
    pub localized_label: String,
    /// То же для подписей вариантов combo (`localized_label` у опции).
    pub localized_option_labels: Vec<String>,
    /// Порядок сортировки пользовательских свойств.
    pub order: f64,
}

/// Состояние модели: разобранная схема + оверрайды пользователя.
#[derive(Debug, Default, Clone)]
pub struct UserProperties {
    entries: Vec<Entry>,
    overrides: HashMap<String, String>,
}

impl UserProperties {
    /// Пересобрать строки по схеме обоины.
    pub fn set_schema(&mut self, schema_json: &str) {
        self.entries = build_entries(schema_json);
    }

    /// Разобрать карту оверрайдов. Значения-нестроки игнорируются, битый JSON
    /// даёт пустую карту (как в оригинале — молча, без ошибки в UI).
    pub fn set_overrides(&mut self, overrides_json: &str) {
        self.overrides.clear();
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(overrides_json) else {
            return;
        };
        for (key, value) in map {
            if let Value::String(text) = value {
                self.overrides.insert(key, text);
            }
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Текущее значение строки: оверрайд, если ключ ЕСТЬ в карте (пустая
    /// строка тоже значение), иначе значение по умолчанию из схемы.
    pub fn current_value(&self, row: usize) -> &str {
        let Some(entry) = self.entries.get(row) else {
            return "";
        };
        match self.overrides.get(&entry.key) {
            Some(value) => value,
            None => &entry.default_wire,
        }
    }

    /// Есть ли у цвета альфа-канал — по числу компонент проводной строки.
    pub fn has_alpha(&self, row: usize) -> bool {
        self.current_value(row).split_whitespace().count() >= 4
    }

    /// Есть ли хоть один оверрайд у записей данного вида — этим гасятся
    /// кнопки сброса в заголовках секций.
    pub fn has_overrides_kind(&self, kind: &str) -> bool {
        self.entries
            .iter()
            .any(|e| e.kind == kind && self.overrides.contains_key(&e.key))
    }

    /// Записать значение. Возвращает номер строки, которую надо обновить;
    /// None — такой строки нет (значение всё равно запоминается, ровно как в
    /// оригинале: карта оверрайдов шире набора строк).
    pub fn set_value(&mut self, key: &str, value: &str) -> Option<usize> {
        self.overrides.insert(key.to_string(), value.to_string());
        self.row_of(key)
    }

    /// Снять оверрайды у записей вида `kind` (None — у всех).
    /// Возвращает пары «строка, ключ» в порядке строк — в этом же порядке
    /// оригинал шлёт dataChanged и resetRequested.
    pub fn reset_kind(&mut self, kind: Option<&str>) -> Vec<(usize, String)> {
        let mut reset = Vec::new();
        for (row, entry) in self.entries.iter().enumerate() {
            if let Some(kind) = kind {
                if entry.kind != kind {
                    continue;
                }
            }
            if self.overrides.remove(&entry.key).is_some() {
                reset.push((row, entry.key.clone()));
            }
        }
        reset
    }

    fn row_of(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.key == key)
    }
}

/// Питоновская истинность значения JSON: пустые строка/список/словарь, ноль,
/// false и null — ложь. Оригинал опирается на неё (`v.get("text") or key`).
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Формат `%g` (шесть значащих цифр, хвостовые нули убраны, показатель для
/// очень больших и очень малых) — им оригинал печатает числа схемы, и от него
/// зависит проводная строка: 1234567 обязан стать `1.23457e+06`, а не
/// `1234567`.
fn format_g(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    // Научная запись с 5 знаками после точки = ровно 6 значащих цифр; из неё
    // берётся показатель уже ПОСЛЕ округления, как того требует %g.
    let scientific = format!("{value:.5e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("научная запись всегда содержит e");
    let exponent: i32 = exponent.parse().unwrap_or(0);
    // Диапазон показателя, при котором %g печатает обычную запись, а не
    // научную: от -4 до 6 (не включая).
    if !(-4..6).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{}e{}{:02}",
            trim_trailing_zeros(mantissa),
            sign,
            exponent.abs()
        )
    } else {
        let decimals = (5 - exponent).max(0) as usize;
        trim_trailing_zeros(&format!("{value:.decimals$}"))
    }
}

fn trim_trailing_zeros(text: &str) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text.to_string()
    }
}

/// Компоненты цвета печатаются с четырьмя знаками — так их ждёт демон.
fn components_wire(items: &[Value]) -> String {
    items
        .iter()
        // Оригинал зовёт float(x) и падает на нечисловой компоненте
        // (TypeError прямо в data() модели). При порте это чиним: нечисловая
        // компонента даёт 0.0, схема с мусором просто рисуется чёрным.
        .map(|item| format!("{:.4}", item.as_f64().unwrap_or(0.0)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// JSON-значение → проводная строка (`jsonValueToWireString` оригинала).
fn json_wire(value: &Value) -> String {
    match value {
        Value::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        Value::Number(n) => format_g(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => s.clone(),
        Value::Array(items) => components_wire(items),
        _ => String::new(),
    }
}

/// Значение по умолчанию из схемы → проводная строка с учётом типа
/// (`coerceDefaultWireString` оригинала).
fn coerce_default_wire(value: &Value, type_name: &str) -> String {
    if type_name == "color" {
        match value {
            Value::Array(items) => return components_wire(items),
            Value::String(s) => return s.clone(),
            _ => {}
        }
    }
    if type_name == "bool" {
        return if truthy(value) { "true" } else { "false" }.to_string();
    }
    if type_name == "slider" {
        // Оригинал приводит к числу только int/float, всё остальное — 0.0
        // (bool в Python наследник int, поэтому true даёт «1»).
        let number = match value {
            Value::Number(n) => n.as_f64().unwrap_or(0.0),
            Value::Bool(b) => f64::from(u8::from(*b)),
            _ => 0.0,
        };
        return format_g(number);
    }
    json_wire(value)
}

/// Число из поля схемы с запасным значением. Оригинал пишет
/// `float(v.get("max", 1.0) or 1.0)`: ложное значение (в том числе ноль!)
/// подменяется запасным — это заметный квирк, максимум `0` превращается в `1`.
fn number_or(value: Option<&Value>, fallback: f64) -> f64 {
    let Some(value) = value else {
        return fallback;
    };
    if !truthy(value) {
        return fallback;
    }
    match value {
        Value::Number(n) => n.as_f64().unwrap_or(fallback),
        Value::Bool(_) => 1.0,
        // Оригинал зовёт float("...") и падает на нечисловой строке; при
        // порте вместо исключения берём запасное значение.
        Value::String(s) => s.trim().parse::<f64>().unwrap_or(fallback),
        _ => fallback,
    }
}

/// Подпись записи: текст из схемы, а если его нет — сам ключ.
fn label_or_key(fields: &Map<String, Value>, key: &str) -> String {
    match fields.get("text") {
        Some(value) if truthy(value) => match value {
            Value::String(s) => s.clone(),
            other => json_wire(other),
        },
        _ => key.to_string(),
    }
}

/// Тип свойства в нижнем регистре (в схемах встречается «Slider»).
fn type_name(fields: &Map<String, Value>) -> String {
    match fields.get("type") {
        None => String::new(),
        Some(Value::String(s)) => s.to_lowercase(),
        Some(other) => json_wire(other).to_lowercase(),
    }
}

/// Пустая карта полей для записей схемы, которые оказались не словарём.
fn fields_of(value: &Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    }
}

/// Разобрать схему обоины в строки редактора.
///
/// Порядок строк: сначала предопределённые свойства (цвет схемы, затем звук),
/// потом пользовательские, отсортированные по полю `order`. Сортировка
/// устойчивая — при равных `order` порядок остаётся тем, в каком ключи лежат
/// в JSON (за это отвечает feature `preserve_order` у serde_json).
pub fn build_entries(schema_json: &str) -> Vec<Entry> {
    let schema = match serde_json::from_str::<Value>(schema_json) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    };

    let mut entries = predefined_entries(&schema);
    let mut users = Vec::new();

    for (key, value) in &schema {
        if PREDEFINED_KEYS.contains(&key.as_str()) {
            continue;
        }
        let fields = fields_of(value);
        let editor = type_name(&fields);

        let mut option_labels = Vec::new();
        let mut option_values = Vec::new();
        let mut localized_option_labels = Vec::new();
        if let Some(Value::Array(options)) = fields.get("options") {
            for option in options {
                let option = fields_of(option);
                let value = json_wire(option.get("value").unwrap_or(&Value::Null));
                let label = match option.get("label") {
                    Some(text) if truthy(text) => json_wire(text),
                    _ => value.clone(),
                };
                option_values.push(value);
                option_labels.push(label);
                localized_option_labels.push(localized_of(&option, "localized_label"));
            }
        }

        let supported = SUPPORTED_TYPES.contains(&editor.as_str())
            || (editor == "combo" && !option_values.is_empty());

        users.push(Entry {
            label: label_or_key(&fields, key),
            section: SECTION_USER,
            kind: KIND_USER,
            supported,
            min_val: number_or(fields.get("min"), 0.0),
            max_val: number_or(fields.get("max"), 1.0),
            default_wire: coerce_default_wire(
                fields.get("value").unwrap_or(&Value::Null),
                &editor,
            ),
            option_labels,
            option_values,
            localized_label: localized_of(&fields, "localized_text"),
            localized_option_labels,
            order: number_or(fields.get("order"), 0.0),
            step_val: step_of(&fields),
            value_suffix: text_of(&fields, "suffix"),
            kind_of_editor: editor,
            key: key.clone(),
        });
    }

    users.sort_by(|a, b| a.order.total_cmp(&b.order));
    entries.extend(users);
    entries
}

/// Предопределённые свойства демона — они не из схемы обоины, а из протокола.
///
/// ⚠️ Числовые поля берутся ИЗ СХЕМЫ, а не выдумываются: у «скорости
/// воспроизведения» демон присылает `min: 10`, `max: 400`, `step: 10` и
/// `suffix: "%"`, и без них ползунок оказался бы от 0 до 1 без процентов.
fn predefined_entries(schema: &Map<String, Value>) -> Vec<Entry> {
    let make = |key: &str,
                fields: &Map<String, Value>,
                label: &str,
                editor: &str,
                default_wire: &str| {
        let mut kind_of_editor = type_name(fields);
        if kind_of_editor.is_empty() {
            kind_of_editor = editor.to_string();
        }
        let mut wire = coerce_default_wire(
            fields.get("value").unwrap_or(&Value::Null),
            &kind_of_editor,
        );
        if wire.is_empty() {
            wire = default_wire.to_string();
        }
        Entry {
            key: key.to_string(),
            label: label_or_key_default(fields, label),
            supported: SUPPORTED_TYPES.contains(&kind_of_editor.as_str()),
            kind_of_editor,
            section: SECTION_PREDEFINED,
            kind: KIND_PREDEFINED,
            min_val: number_or(fields.get("min"), 0.0),
            max_val: number_or(fields.get("max"), 1.0),
            step_val: step_of(fields),
            value_suffix: text_of(fields, "suffix"),
            default_wire: wire,
            option_labels: Vec::new(),
            option_values: Vec::new(),
            localized_label: localized_of(fields, "localized_text"),
            localized_option_labels: Vec::new(),
            order: 0.0,
        }
    };

    let scheme = fields_of(schema.get(SCHEME_COLOR_KEY).unwrap_or(&Value::Null));
    let mut entries = vec![make(
        SCHEME_COLOR_KEY,
        &scheme,
        "Scheme color",
        "color",
        DEFAULT_SCHEME_COLOR,
    )];

    // Строка звука появляется, только если ключ есть в схеме: обои без звука
    // не должны показывать выключатель, которым нечего выключать.
    if schema.contains_key(ENABLE_AUDIO_KEY) {
        let audio = fields_of(schema.get(ENABLE_AUDIO_KEY).unwrap_or(&Value::Null));
        entries.push(make(ENABLE_AUDIO_KEY, &audio, "Enable audio", "bool", "true"));
    }

    // Скорость воспроизведения — тем же порядком и на тех же условиях.
    if schema.contains_key(PLAYBACK_SPEED_KEY) {
        let speed = fields_of(schema.get(PLAYBACK_SPEED_KEY).unwrap_or(&Value::Null));
        entries.push(make(
            PLAYBACK_SPEED_KEY,
            &speed,
            "Playback speed",
            "slider",
            DEFAULT_PLAYBACK_SPEED,
        ));
    }

    // waywallen 0.4.2 (`user_property_model.cpp`): параллакс от мыши —
    // выключатель, по умолчанию включён, тем же порядком после скорости.
    if schema.contains_key(MOUSE_PARALLAX_KEY) {
        let parallax = fields_of(schema.get(MOUSE_PARALLAX_KEY).unwrap_or(&Value::Null));
        entries.push(make(MOUSE_PARALLAX_KEY, &parallax, "Mouse parallax", "bool", "true"));
    }
    entries
}

/// Шаг ползунка из схемы (`step`). Отсутствие и нечисловое поле дают ноль —
/// разметка вендора трактует его как «шага нет».
fn step_of(fields: &Map<String, Value>) -> f64 {
    match fields.get("step") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Строковое поле схемы. Нестроковое значение — пустая строка, ровно как у
/// `QJsonValue::toString()` оригинала.
fn text_of(fields: &Map<String, Value>, name: &str) -> String {
    match fields.get(name) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Подпись плагина (`{pluginId, msgid}`) сырым JSON. Пусто, когда поля нет
/// или оно не объект: переводить тогда нечего.
fn localized_of(fields: &Map<String, Value>, name: &str) -> String {
    match fields.get(name) {
        Some(value @ Value::Object(map)) if !map.is_empty() => value.to_string(),
        _ => String::new(),
    }
}

/// То же, что label_or_key, но запасной вариант — не ключ, а готовая подпись
/// протокола (у предопределённых свойств ключ пользователю не показывают).
fn label_or_key_default(fields: &Map<String, Value>, fallback: &str) -> String {
    match fields.get("text") {
        Some(value) if truthy(value) => match value {
            Value::String(s) => s.clone(),
            other => json_wire(other),
        },
        _ => fallback.to_string(),
    }
}

// ============================================================================
// Qt-слой
// ============================================================================

/// Роли начинаются с Qt::UserRole + 1 (Qt::UserRole == 256) — те же номера,
/// что у Python-модели и у оригинала.
const ROLE_BASE: i32 = 256 + 1;

/// Имена ролей в порядке номеров — единственный источник истины и для
/// roleNames, и для data(), и для roleNumber().
/// ⚠️ Порядок ролей — тот же, что у перечисления `Roles` вендора
/// (`ui/src/model/user_property_model.cppm`), и он же задаёт их номера.
/// Роли `stepVal` и `valueSuffix` появились у вендора вместе с ползунком
/// «скорость воспроизведения»; делегат панели деталей объявляет их
/// `required property`, а такой делегат без роли просто НЕ СОЗДАЁТСЯ — обе
/// секции свойств тогда пропадают целиком.
const ROLE_NAMES: [&str; 14] = [
    "key",
    "label",
    "type",
    "supported",
    "minVal",
    "maxVal",
    "stepVal",
    "valueSuffix",
    "currentValue",
    "hasAlpha",
    "optionLabels",
    "optionValues",
    "section",
    "kind",
];

/// Индексы ролей, зависящих от оверрайда: ровно они летят в dataChanged при
/// правке значения. Остальные роли берутся из схемы и не меняются.
const ROLE_CURRENT_VALUE: usize = 8;
const ROLE_HAS_ALPHA: usize = 9;

/// Индексы ролей, зависящих от КАТАЛОГА ПОДПИСЕЙ, а не от схемы: подпись
/// строки и подписи вариантов combo. Обе считаются в `data()` через
/// `PluginTranslations`, поэтому приход каталогов обязан их обновить.
const ROLE_LABEL: usize = 1;
const ROLE_OPTION_LABELS: usize = 10;

/// Rust-состояние модели.
#[derive(Default)]
pub struct UserPropertyListModelRust {
    schema_json: String,
    overrides_json: String,
    props: UserProperties,
    /// Номер подписки на каталоги подписей плагинов; `0` — не подписана.
    translations_watch: u64,
}

impl Drop for UserPropertyListModelRust {
    fn drop(&mut self) {
        crate::plugin_translations::subscribers::unwatch(self.translations_watch);
    }
}

/// Набор ролей «изменилось текущее значение» — две роли, не пустой вектор.
/// Пустой означал бы «изменились ВСЕ роли», и QML пересчитал бы весь делегат.
fn current_value_roles() -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    roles.append(ROLE_BASE + ROLE_CURRENT_VALUE as i32);
    roles.append(ROLE_BASE + ROLE_HAS_ALPHA as i32);
    roles
}

/// Набор ролей «приехал каталог подписей» — тоже две, и тоже не пустой:
/// значение свойства при смене языка не трогается, дёргать из-за подписи
/// весь делегат незачем.
fn label_roles() -> QVector<i32> {
    let mut roles = QVector::<i32>::default();
    roles.append(ROLE_BASE + ROLE_LABEL as i32);
    roles.append(ROLE_BASE + ROLE_OPTION_LABELS as i32);
    roles
}

impl cxx_qt::Initialize for qobject::UserPropertyListModel {
    /// Подписка на каталоги подписей плагинов.
    ///
    /// ⭐ Без неё панель свойств показывала покупателю «Enable audio» и
    /// «Playback speed» по-английски в русском окне: подпись этих
    /// предопределённых свойств демон отдаёт парой `{pluginId, msgid}`,
    /// перевод ищется по каталогам, а каталоги грузятся ЛЕНИВО — первым
    /// же вопросом их и заказывает сам делегат панели. Ответ приходит
    /// позже, роль `label` к тому времени уже прочитана, и без толчка
    /// английские слова оставались на экране до пересборки модели.
    fn initialize(mut self: Pin<&mut Self>) {
        let watch = crate::plugin_translations::subscribers::watch(
            self.as_ref().get_ref().qt_thread(),
            qobject::UserPropertyListModel::on_translations_changed,
        );
        self.as_mut().rust_mut().translations_watch = watch;
    }
}

impl qobject::UserPropertyListModel {
    // ── Свойства ──────────────────────────────────────────────────────────
    fn schema_json(&self) -> QString {
        QString::from(&self.schema_json)
    }

    fn overrides_json(&self) -> QString {
        QString::from(&self.overrides_json)
    }

    fn count(&self) -> i32 {
        self.props.len() as i32
    }

    fn has_predefined_property_overrides(&self) -> bool {
        self.props.has_overrides_kind(KIND_PREDEFINED)
    }

    fn has_user_property_overrides(&self) -> bool {
        self.props.has_overrides_kind(KIND_USER)
    }

    /// Смена схемы = полная замена содержимого, поэтому reset, а не
    /// insert/remove: строк может стать любое число.
    fn set_schema_json(mut self: Pin<&mut Self>, value: &QString) {
        let value = value.to_string();
        if value == self.schema_json {
            return;
        }
        self.as_mut().rust_mut().schema_json = value.clone();
        self.as_mut().schema_json_changed();

        unsafe {
            self.as_mut().begin_reset_model();
            self.as_mut().rust_mut().props.set_schema(&value);
            self.as_mut().end_reset_model();
        }
        self.as_mut().count_changed();
        self.as_mut().override_state_changed();
    }

    /// Оверрайды приходят от демона целиком одной строкой. Строки при этом не
    /// добавляются и не исчезают — меняются только два зависимых от значения
    /// поля, поэтому reset не нужен, хватает dataChanged по диапазону.
    fn set_overrides_json(mut self: Pin<&mut Self>, value: &QString) {
        let value = value.to_string();
        if value == self.overrides_json {
            return;
        }
        {
            let mut state = self.as_mut().rust_mut();
            state.overrides_json = value.clone();
        }
        self.as_mut().overrides_json_changed();
        self.as_mut().rust_mut().props.set_overrides(&value);

        let rows = self.props.len();
        if rows > 0 {
            let roles = current_value_roles();
            let first = self.index(0, 0, &QModelIndex::default());
            let last = self.index(rows as i32 - 1, 0, &QModelIndex::default());
            self.as_mut().data_changed(&first, &last, &roles);
        }
        self.as_mut().override_state_changed();
    }

    /// Каталоги подписей приехали (или сменился язык) — перечитать подписи.
    ///
    /// Строки те же и в том же порядке, меняется только текст двух ролей,
    /// поэтому reset не нужен: хватает `dataChanged` по всему диапазону.
    pub fn on_translations_changed(mut self: Pin<&mut Self>) {
        let rows = self.props.len();
        if rows == 0 {
            return;
        }
        let roles = label_roles();
        let first = self.index(0, 0, &QModelIndex::default());
        let last = self.index(rows as i32 - 1, 0, &QModelIndex::default());
        self.as_mut().data_changed(&first, &last, &roles);
    }

    // ── QAbstractListModel ────────────────────────────────────────────────
    fn row_count(&self, parent: &QModelIndex) -> i32 {
        // У плоского списка дети есть только у корня.
        if parent.is_valid() {
            0
        } else {
            self.props.len() as i32
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
        if !index.is_valid() || row < 0 || row as usize >= self.props.len() {
            return QVariant::default();
        }
        let row = row as usize;
        let entry = &self.props.entries()[row];
        let Ok(slot) = usize::try_from(role - ROLE_BASE) else {
            return QVariant::default();
        };
        match ROLE_NAMES.get(slot).copied() {
            Some("key") => QVariant::from(&QString::from(&entry.key)),
            Some("label") => {
                let translated = plugin_label(&entry.localized_label);
                QVariant::from(&QString::from(if translated.is_empty() {
                    &entry.label
                } else {
                    &translated
                }))
            }
            Some("type") => QVariant::from(&QString::from(&entry.kind_of_editor)),
            Some("supported") => QVariant::from(&entry.supported),
            Some("minVal") => QVariant::from(&entry.min_val),
            Some("maxVal") => QVariant::from(&entry.max_val),
            Some("stepVal") => QVariant::from(&entry.step_val),
            Some("valueSuffix") => QVariant::from(&QString::from(&entry.value_suffix)),
            Some("currentValue") => {
                QVariant::from(&QString::from(self.props.current_value(row)))
            }
            Some("hasAlpha") => QVariant::from(&self.props.has_alpha(row)),
            Some("optionLabels") => {
                let mut labels = entry.option_labels.clone();
                for (index, label) in labels.iter_mut().enumerate() {
                    let Some(localized) = entry.localized_option_labels.get(index) else {
                        break;
                    };
                    let translated = plugin_label(localized);
                    if !translated.is_empty() {
                        *label = translated;
                    }
                }
                QVariant::from(&string_list(&labels))
            }
            Some("optionValues") => QVariant::from(&string_list(&entry.option_values)),
            Some("section") => QVariant::from(&QString::from(entry.section)),
            Some("kind") => QVariant::from(&QString::from(entry.kind)),
            _ => QVariant::default(),
        }
    }

    // ── API для QML ───────────────────────────────────────────────────────
    fn set_value(mut self: Pin<&mut Self>, key: &QString, value: &QString) {
        let key = key.to_string();
        let value = value.to_string();
        let row = self.as_mut().rust_mut().props.set_value(&key, &value);
        if let Some(row) = row {
            self.as_mut().notify_current_value(row);
        }
        self.as_mut().override_state_changed();
        // valueChanged — последним: QML в обработчике читает уже обновлённое
        // состояние модели.
        self.as_mut()
            .value_changed(QString::from(&key), QString::from(&value));
    }

    fn reset_all(self: Pin<&mut Self>) {
        self.reset_kind(None);
    }

    fn reset_predefined_properties(self: Pin<&mut Self>) {
        self.reset_kind(Some(KIND_PREDEFINED));
    }

    fn reset_user_properties(self: Pin<&mut Self>) {
        self.reset_kind(Some(KIND_USER));
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
        let role = self.role_number(name);
        if role < 0 {
            return QVariant::default();
        }
        let index = self.index(row, 0, &QModelIndex::default());
        self.data(&index, role)
    }

    // ── Внутреннее ────────────────────────────────────────────────────────
    /// Точечное оповещение об изменении значения одной строки.
    fn notify_current_value(mut self: Pin<&mut Self>, row: usize) {
        let roles = current_value_roles();
        let index = self.index(row as i32, 0, &QModelIndex::default());
        self.as_mut().data_changed(&index, &index, &roles);
    }

    fn reset_kind(mut self: Pin<&mut Self>, kind: Option<&str>) {
        let reset = self.as_mut().rust_mut().props.reset_kind(kind);
        for (row, key) in reset {
            self.as_mut().notify_current_value(row);
            // Сброс уезжает отдельным сигналом: QML шлёт демону reset=true,
            // а не запись пустой строки.
            self.as_mut().reset_requested(QString::from(&key));
        }
        self.as_mut().override_state_changed();
    }
}

/// Перевод подписи плагина по её сырому JSON. Пустой вход — пустой выход:
/// у большинства свойств `localized_text` в схеме нет вовсе.
fn plugin_label(localized_json: &str) -> String {
    if localized_json.is_empty() {
        return String::new();
    }
    qobject::plugin_translate(&QString::from(localized_json)).to_string()
}

/// Список строк для ролей-списков. Отдельная модель под варианты combo не
/// нужна: QML читает их как обычный массив (`optionValues[index]`).
fn string_list(items: &[String]) -> QStringList {
    let mut list = QStringList::default();
    for item in items {
        list.append(QString::from(item));
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Голден снят с ЖИВОЙ Python-модели (см. tests/golden/user_property_cases.py).
    /// Здесь он прогоняется через чистый слой порта: строки, значения ролей,
    /// флаги оверрайдов и — главное — сигналы (какие строки и в каком порядке
    /// попадают в dataChanged/resetRequested).
    #[derive(serde::Deserialize)]
    struct Golden {
        cases: Vec<Case>,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        #[serde(rename = "schemaJson")]
        schema_json: String,
        #[serde(rename = "overridesJson")]
        overrides_json: String,
        actions: Vec<Action>,
        count: usize,
        rows: Vec<Row>,
        #[serde(rename = "hasPredefinedPropertyOverrides")]
        has_predefined: bool,
        #[serde(rename = "hasUserPropertyOverrides")]
        has_user: bool,
        #[serde(rename = "dataChanged")]
        data_changed: Vec<DataChanged>,
        #[serde(rename = "valueChanged")]
        value_changed: Vec<ValueChanged>,
        #[serde(rename = "resetRequested")]
        reset_requested: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct Action {
        op: String,
        #[serde(default)]
        key: String,
        #[serde(default)]
        value: String,
    }

    #[derive(serde::Deserialize)]
    struct Row {
        key: String,
        label: String,
        #[serde(rename = "type")]
        editor: String,
        supported: bool,
        #[serde(rename = "minVal")]
        min_val: f64,
        #[serde(rename = "maxVal")]
        max_val: f64,
        #[serde(rename = "currentValue")]
        current_value: String,
        #[serde(rename = "hasAlpha")]
        has_alpha: bool,
        #[serde(rename = "optionLabels")]
        option_labels: Vec<String>,
        #[serde(rename = "optionValues")]
        option_values: Vec<String>,
        section: String,
        kind: String,
    }

    #[derive(serde::Deserialize)]
    struct DataChanged {
        first: usize,
        last: usize,
        roles: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct ValueChanged {
        key: String,
        value: String,
    }

    /// Что порт «испустил бы» сигналами: те же события, что снимает голден с
    /// Python-модели, но собранные из возвратов чистого слоя.
    #[derive(Default)]
    struct Emitted {
        data_changed: Vec<(usize, usize, Vec<String>)>,
        value_changed: Vec<(String, String)>,
        reset_requested: Vec<String>,
    }

    const VALUE_ROLES: [&str; 2] = ["currentValue", "hasAlpha"];

    /// Имя секции эталона Python-эпохи → имя секции waywallen 0.3.8.
    fn section_of(golden: &str) -> &'static str {
        match golden {
            "Properties" => SECTION_PREDEFINED,
            "User properties" => SECTION_USER,
            other => panic!("незнакомая секция эталона: {other}"),
        }
    }

    fn replay(case: &Case) -> (UserProperties, Emitted) {
        let mut props = UserProperties::default();
        let mut out = Emitted::default();

        // Оба присваивания идут через проверку «значение не менялось» — она
        // живёт в set_schema_json/set_overrides_json и её видно в голдене:
        // у случая с пустой схемой модель остаётся вообще без строк, потому
        // что пустая строка равна начальному значению свойства.
        if !case.schema_json.is_empty() {
            props.set_schema(&case.schema_json);
        }
        // Голден снимает сигналы, начиная с установки оверрайдов: сборка
        // схемы идёт через reset модели, а не через dataChanged.
        if !case.overrides_json.is_empty() {
            props.set_overrides(&case.overrides_json);
            if !props.is_empty() {
                out.data_changed.push((
                    0,
                    props.len() - 1,
                    VALUE_ROLES.iter().map(|s| s.to_string()).collect(),
                ));
            }
        }

        for action in &case.actions {
            match action.op.as_str() {
                "setValue" => {
                    if let Some(row) = props.set_value(&action.key, &action.value) {
                        out.data_changed.push((
                            row,
                            row,
                            VALUE_ROLES.iter().map(|s| s.to_string()).collect(),
                        ));
                    }
                    out.value_changed
                        .push((action.key.clone(), action.value.clone()));
                }
                op => {
                    let kind = match op {
                        "resetAll" => None,
                        "resetPredefinedProperties" => Some(KIND_PREDEFINED),
                        "resetUserProperties" => Some(KIND_USER),
                        other => panic!("неизвестное действие голдена: {other}"),
                    };
                    for (row, key) in props.reset_kind(kind) {
                        out.data_changed.push((
                            row,
                            row,
                            VALUE_ROLES.iter().map(|s| s.to_string()).collect(),
                        ));
                        out.reset_requested.push(key);
                    }
                }
            }
        }
        (props, out)
    }

    #[test]
    fn golden_parity_with_python_model() {
        let golden: Golden =
            serde_json::from_str(include_str!("../tests/golden/user_property_cases.json"))
                .expect("голден читается");
        assert!(!golden.cases.is_empty());

        for case in &golden.cases {
            let name = &case.name;
            let (props, emitted) = replay(case);

            assert_eq!(props.len(), case.count, "{name}: число строк");
            assert_eq!(props.len(), case.rows.len(), "{name}: длина голдена строк");

            for (row, expected) in case.rows.iter().enumerate() {
                let entry = &props.entries()[row];
                let at = format!("{name}: строка {row} ({})", expected.key);
                assert_eq!(entry.key, expected.key, "{at}: key");
                assert_eq!(entry.label, expected.label, "{at}: label");
                assert_eq!(entry.kind_of_editor, expected.editor, "{at}: type");
                assert_eq!(entry.supported, expected.supported, "{at}: supported");
                assert_eq!(entry.min_val, expected.min_val, "{at}: minVal");
                assert_eq!(entry.max_val, expected.max_val, "{at}: maxVal");
                // ⚠ Эталон снят с ЖИВОЙ Python-модели времён waywallen
                // ≤0.3.7, где секция звалась человеческой строкой. С 0.3.8
                // вендор переименовал её в технический ключ, а заголовок
                // научился переводить сам — поэтому эталон приводится к
                // новым именам, а не переписывается руками.
                assert_eq!(
                    entry.section,
                    section_of(&expected.section),
                    "{at}: section"
                );
                assert_eq!(entry.kind, expected.kind, "{at}: kind");
                assert_eq!(
                    entry.option_labels, expected.option_labels,
                    "{at}: optionLabels"
                );
                assert_eq!(
                    entry.option_values, expected.option_values,
                    "{at}: optionValues"
                );
                assert_eq!(
                    props.current_value(row),
                    expected.current_value,
                    "{at}: currentValue"
                );
                assert_eq!(props.has_alpha(row), expected.has_alpha, "{at}: hasAlpha");
            }

            assert_eq!(
                props.has_overrides_kind(KIND_PREDEFINED),
                case.has_predefined,
                "{name}: hasPredefinedPropertyOverrides"
            );
            assert_eq!(
                props.has_overrides_kind(KIND_USER),
                case.has_user,
                "{name}: hasUserPropertyOverrides"
            );

            let expected_dc: Vec<(usize, usize, Vec<String>)> = case
                .data_changed
                .iter()
                .map(|d| (d.first, d.last, d.roles.clone()))
                .collect();
            assert_eq!(emitted.data_changed, expected_dc, "{name}: dataChanged");

            let expected_vc: Vec<(String, String)> = case
                .value_changed
                .iter()
                .map(|v| (v.key.clone(), v.value.clone()))
                .collect();
            assert_eq!(emitted.value_changed, expected_vc, "{name}: valueChanged");
            assert_eq!(
                emitted.reset_requested, case.reset_requested,
                "{name}: resetRequested"
            );
        }
    }

    #[test]
    fn format_g_matches_python() {
        // Значения снизу — вывод `"%g" % v` в Python (проверено вручную на
        // тех же числах, что участвуют в схемах обоев).
        for (value, expected) in [
            (0.0, "0"),
            (0.5, "0.5"),
            (1.0, "1"),
            (100.0, "100"),
            (2.5, "2.5"),
            (1234567.0, "1.23457e+06"),
            (0.00001, "1e-05"),
            (0.0001, "0.0001"),
            (-0.25, "-0.25"),
            (123456.0, "123456"),
            (1.0 / 3.0, "0.333333"),
        ] {
            assert_eq!(format_g(value), expected, "%g для {value}");
        }
    }

    #[test]
    fn empty_override_is_a_value_not_a_reset() {
        // Регресс v0.2.5: пустая строка — валидное значение текстового
        // свойства, и она НЕ должна означать «вернуть значение по умолчанию».
        let mut props = UserProperties::default();
        props.set_schema(r#"{"caption": {"type": "textinput", "value": "по умолчанию"}}"#);
        let row = props.entries().len() - 1;
        assert_eq!(props.current_value(row), "по умолчанию");
        props.set_value("caption", "");
        assert_eq!(props.current_value(row), "");
        assert!(props.has_overrides_kind(KIND_USER));
        props.reset_kind(Some(KIND_USER));
        assert_eq!(props.current_value(row), "по умолчанию");
    }

    /// Сторож договора с вендорной разметкой.
    ///
    /// ЗАЧЕМ. Делегат панели деталей объявляет свои поля `required property`,
    /// и роль, которой у модели нет, — это не «пустое значение», а отказ
    /// СОЗДАТЬ делегат: обе секции свойств пропадают с экрана целиком,
    /// молча. Ровно так и случилось с `stepVal`/`valueSuffix`: они приехали
    /// с вендорным QML v0.3.5, а модель о них не узнала, и панель свойств
    /// была пуста с 18.08.2026. Проверка снимает список прямо с вендорного
    /// файла, поэтому следующее такое поле поймается на `cargo test`, а не
    /// на живом окне.
    #[test]
    fn delegate_required_properties_are_covered_by_roles() {
        const PANEL_QML: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/qt_wallpaper_engine/qml_ui/waywallen/ui/qml/page/",
            "WallpaperDetailPanel.qml"
        ));

        // Делегат модели свойств опознаётся по своему `id`: подстрока
        // `delegate: ColumnLayout` встречается и у делегата СЕКЦИИ
        // (`section.delegate:`), а у того объявление ровно одно.
        let start = PANEL_QML
            .find("id: m_prop_delegate")
            .expect("в панели деталей обязан быть делегат свойств");
        let block = &PANEL_QML[start..];
        let mut required: Vec<&str> = Vec::new();
        for line in block.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("required property ") else {
                if !required.is_empty() && !line.is_empty() {
                    // Объявления идут одним куском: первая же строка после
                    // них закрывает список.
                    break;
                }
                continue;
            };
            let name = rest
                .split_whitespace()
                .nth(1)
                .expect("у объявления есть тип и имя");
            required.push(name);
        }
        assert!(
            required.len() >= 10,
            "объявлений required property найдено {} — разбор разъехался с \
             разметкой",
            required.len()
        );
        for name in required {
            assert!(
                ROLE_NAMES.contains(&name),
                "делегат вендора требует роль `{name}`, а модель её не отдаёт: \
                 делегат не создастся и секции свойств исчезнут"
            );
        }
    }

    /// Три предопределённых свойства демона — те же, что у вендора
    /// (`isPredefinedKey`): цвет схемы, звук, скорость воспроизведения.
    #[test]
    fn playback_speed_is_a_predefined_row_with_schema_limits() {
        let mut props = UserProperties::default();
        props.set_schema(
            r#"{"waywallen.scheme_color": {"type": "color", "value": "0.1 0.2 0.3"},
                "waywallen.enable_audio": {"type": "bool", "value": true},
                "waywallen.playback_speed": {"text": "Playback speed",
                                             "type": "slider", "value": 100,
                                             "min": 10, "max": 400, "step": 10,
                                             "suffix": "%"},
                "brightness": {"type": "slider", "value": 0.5, "order": 1}}"#,
        );
        let keys: Vec<&str> = props.entries().iter().map(|e| e.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                SCHEME_COLOR_KEY,
                ENABLE_AUDIO_KEY,
                PLAYBACK_SPEED_KEY,
                "brightness"
            ],
            "предопределённые идут первыми и не дублируются в секции обоины"
        );
        let speed = &props.entries()[2];
        assert_eq!(speed.kind, KIND_PREDEFINED);
        assert_eq!(speed.min_val, 10.0);
        assert_eq!(speed.max_val, 400.0);
        assert_eq!(speed.step_val, 10.0);
        assert_eq!(speed.value_suffix, "%");
        assert_eq!(props.current_value(2), "100");
    }

    #[test]
    fn role_names_are_unique_and_indexed() {
        let mut seen = std::collections::HashSet::new();
        for name in ROLE_NAMES {
            assert!(seen.insert(name), "роль {name} объявлена дважды");
        }
        assert_eq!(ROLE_NAMES[ROLE_CURRENT_VALUE], "currentValue");
        assert_eq!(ROLE_NAMES[ROLE_HAS_ALPHA], "hasAlpha");
    }
}
