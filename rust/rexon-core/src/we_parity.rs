//! Сверка UI-паритета вкладки Wallpaper Engine с движком waywallen — порт
//! `rexon/qt_wallpaper_engine/parity.py` (dev-инструмент, не для покупателя).
//!
//! Движок по управляющему протоколу (`RendererPluginList` →
//! `RendererPluginInfo.settings: repeated SettingSchema`) отдаёт ПОЛНУЮ
//! типизированную схему своих renderer-настроек. Это источник правды того,
//! ЧТО движок умеет. Здесь:
//!
//!   * [`REXON_SETTING_REGISTRY`] — машинно-сравнимый реестр того, что UI
//!     Rexon реально показывает (ключи + ожидаемый тип/набор choices).
//!     Настройка, которой в реестре нет, — «дыра» в UI.
//!   * [`compare_schema`] — diff схемы движка с реестром: чего нет в UI,
//!     что устарело в реестре, расхождения по типу/choices, неизвестные UI
//!     типы значений.
//!   * [`render_report_text`] — текстовый дамп отчёта (лог / панель вкладки).
//!
//! Ввод — уже разобранный `RendererPluginListResponse` из [`crate::proto`];
//! всё остальное чистые функции без ввода-вывода. Единственный побочный
//! эффект оригинала — чтение файла `proto/UPSTREAM_TAG` — вынесен в тонкую
//! обёртку [`read_proto_upstream_tag`] над чистой [`proto_upstream_tag`]
//! (та же граница, что у `favorites.rs`: ядро над содержимым, обёртка над
//! путём).
//!
//! ⚠️ Реестр описывает generic-рендер настроек: в Python-эпоху это была форма
//! `_PluginSettingsDialog._make_field`, после QML-миграции — вендорный
//! `ui/qml/component/settings/SettingField.qml` (тот же набор типов 1..5 и
//! те же два спец-ключа `render_node`/`resolution`; неизвестный тип падает
//! в `default: stringField`). Ссылка на удалённый `_PluginSettingsDialog` в
//! оригинале устарела — здесь исправлена.
//!
//! ⚠ правило #16 — починенные при порте дефекты оригинала:
//!   1. `proto_upstream_tag()` открывал файл как UTF-8 и ловил только
//!      `OSError`. Битый байт в `UPSTREAM_TAG` давал `UnicodeDecodeError`
//!      (это `ValueError`, не `OSError`) — исключение улетало наружу через
//!      `_on_parity_resp`, и вся сверка паритета падала вместо честного
//!      «тег не записан». Здесь чтение всегда lossy: тег читается как есть,
//!      мусорные байты заменяются, сверка продолжается.
//!   2. `daemon_matches_snapshot` резал префикс `v` через `str.lstrip("v")`
//!      — это удаление МНОЖЕСТВА ведущих символов из набора, а не префикса:
//!      `"vv0.2.6"` превращалось в `"0.2.6"` и совпадало с версией демона
//!      `0.2.6`. Здесь снимается ровно один ведущий `v`.
//!
//! Сохранённые квирки оригинала (осознанно, поведение не меняется):
//!   * `TYPE_MISMATCH` не выдаётся, когда движок прислал НЕИЗВЕСТНЫЙ тип
//!     (сторож `vt in KNOWN_VALUE_TYPES`) — про такую настройку уже сказано
//!     через `UNKNOWN_TYPE`, а для спец-ключей не сказано ничего;
//!   * сверка `choices` молчит, когда движок не объявил ни одного варианта,
//!     и сравнивает МНОЖЕСТВА (порядок и дубли не важны);
//!   * два renderer'а с одинаковым именем: в `renderers` попадут оба, а в
//!     `engine_keys` останется набор ключей последнего (семантика `dict`).

use crate::proto::control as pb;

/// Известные UI типы значений — зеркало `enum SettingValueType` из proto.
/// Порядок = возрастание номера: он виден в тексте отчёта
/// (`известные: [1, 2, 3, 4, 5]`).
pub const KNOWN_VALUE_TYPES: &[(i32, &str)] = &[
    (1, "u32"),
    (2, "f32"),
    (3, "string"),
    (4, "bool"),
    // SETTING_VALUE_TYPE_I32 (появился к v0.2.x; им демон отдаёт
    // resolution-пресеты)
    (5, "i32"),
];

/// Ключи со спец-виджетом (чипы пресетов разрешения / выбор GPU): UI рисует
/// их корректно даже без «обычного» типа из [`KNOWN_VALUE_TYPES`].
pub const SPECIAL_KEYS: &[&str] = &["render_node", "resolution"];

/// Имя типа значения по номеру; `None` — тип UI неизвестен.
pub fn value_type_name(vt: i32) -> Option<&'static str> {
    KNOWN_VALUE_TYPES
        .iter()
        .find(|(k, _)| *k == vt)
        .map(|(_, name)| *name)
}

fn is_known_type(vt: i32) -> bool {
    value_type_name(vt).is_some()
}

fn is_special_key(key: &str) -> bool {
    SPECIAL_KEYS.contains(&key)
}

/// Что UI Rexon ОЖИДАЕТ увидеть для данного ключа настройки.
///
/// Поля со значением `None` при сверке не проверяются (UI к ним
/// нечувствителен). Реестр намеренно нестрогий: цель — ловить НОВЫЕ
/// настройки и явные несовместимости, а не дублировать всю схему.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedSetting {
    pub key: &'static str,
    /// Ожидаемый `SettingValueType`.
    pub value_type: Option<i32>,
    /// Если важен набор вариантов.
    pub choices: Option<&'static [&'static str]>,
    pub note: &'static str,
    /// Типы обоев, чьи рисовальщики дают эту настройку. Пусто — настройку
    /// дают встроенные рисовальщики демона, и она обязана быть всегда.
    ///
    /// Сцены и веб-обои Wallpaper Engine рисует ОТДЕЛЬНЫЙ плагин: у демона
    /// без него таких рисовальщиков нет вовсе, и спрашивать с него их
    /// настройки не с кого. Такой ключ уходит в
    /// [`ParityReport::unchecked`], а не в расхождения (см. п. 5 в
    /// [`compare_schema`]).
    pub only_types: &'static [&'static str],
}

impl ExpectedSetting {
    const fn new(key: &'static str, value_type: Option<i32>) -> Self {
        Self { key, value_type, choices: None, note: "", only_types: &[] }
    }
    const fn for_types(mut self, types: &'static [&'static str]) -> Self {
        self.only_types = types;
        self
    }
    const fn with_choices(mut self, choices: &'static [&'static str]) -> Self {
        self.choices = Some(choices);
        self
    }
    const fn with_note(mut self, note: &'static str) -> Self {
        self.note = note;
        self
    }
}

/// РЕЕСТР: что UI Rexon реально реализует.
///
/// Перечислены ключи, которые встречаются в схемах движка waywallen 0.2.x и
/// которые UI показывает осознанно. Новый ключ от движка, которого тут нет →
/// отчёт «движок умеет настройку X, в UI Rexon нет» (см. [`compare_schema`]).
/// Порядок записей = порядок, в котором выдаются `STALE_IN_UI`-расхождения
/// (в Python это порядок вставки в `dict`), поэтому он значим.
///
/// Поддерживать вручную ПРОЩЕ, чем кажется: сверка сама подскажет, что
/// добавить, при первом же расхождении.
pub const REXON_SETTING_REGISTRY: &[ExpectedSetting] = &[
    ExpectedSetting::new("enable_audio", Some(4)),
    ExpectedSetting::new("volume", Some(1)),
    ExpectedSetting::new("fps", Some(1)),
    ExpectedSetting::new("hwdec", Some(3))
        .with_choices(&["auto", "vaapi", "vulkan", "none"]),
    // resolution: движок отдаёт спец-тип (enum-пресет), UI рисует чипами
    // пресетов → тип НЕ проверяем (value_type = None). С waywallen 0.5.0
    // у image/video пресеты пришли списком choices (-2 «авто» … 4) и
    // умолчание стало -2 (у сцен и веба плагина OWE — с 0.3.1); набор
    // choices здесь не сверяется — чип «авто» появится с разметкой 0.5.0.
    ExpectedSetting::new("resolution", None).with_note("спец-виджет: чипы пресетов"),
    // С waywallen 0.5.0 «своё разрешение» дают и встроенные рисовальщики
    // image/video, с OWE 0.3.1 — и weweb. Привязка к сценам оставлена: ключ,
    // отданный другими рисовальщиками, сверке уже виден, а «не проверено»
    // нужно лишь демону без них.
    ExpectedSetting::new("custom_extent", Some(1)).for_types(&["scene"]),
    ExpectedSetting::new("msaa", Some(1)).for_types(&["scene"]),
    ExpectedSetting::new("render_node", Some(3)).with_note("спец-виджет: выбор GPU"),
    ExpectedSetting::new("loop_file", Some(3)).with_choices(&["inf", "no"]),
    ExpectedSetting::new("shared_texture_enabled", Some(4)).for_types(&["web"]),
    // remote_debugging_port — порт CDP weweb-рендера (Chromium). Здесь только
    // регистрируем её наличие в UI.
    ExpectedSetting::new("remote_debugging_port", Some(1))
        .with_note("ХУК weweb/Chromium (второй агент)")
        .for_types(&["web"]),
];

/// Найти ожидание реестра по ключу настройки.
pub fn expected_for(key: &str) -> Option<&'static ExpectedSetting> {
    REXON_SETTING_REGISTRY.iter().find(|e| e.key == key)
}

// ─────────────────────────────────────────────────────────────────────────
//  Результат сверки
// ─────────────────────────────────────────────────────────────────────────

/// Вид расхождения (значения строк — как в `IssueKind` Python).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// Движок умеет, UI не знает.
    MissingInUi,
    /// UI знает, движок не отдаёт.
    StaleInUi,
    /// Тип значения вне [`KNOWN_VALUE_TYPES`].
    UnknownType,
    /// Тип ≠ ожидаемого реестром.
    TypeMismatch,
    /// Набор choices разошёлся.
    ChoicesMismatch,
}

impl IssueKind {
    /// Строковое значение перечисления (`IssueKind.MISSING_IN_UI.value`).
    pub fn as_str(self) -> &'static str {
        match self {
            IssueKind::MissingInUi => "missing_in_ui",
            IssueKind::StaleInUi => "stale_in_ui",
            IssueKind::UnknownType => "unknown_type",
            IssueKind::TypeMismatch => "type_mismatch",
            IssueKind::ChoicesMismatch => "choices_mismatch",
        }
    }
}

/// Одно расхождение отчёта.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParityIssue {
    pub kind: IssueKind,
    /// Имя renderer-плагина (или пусто для глобальных).
    pub renderer: String,
    pub key: String,
    /// Человекочитаемое пояснение.
    pub detail: String,
}

impl ParityIssue {
    /// MISSING/UNKNOWN — настройка может «пропасть» из UI → `warning`;
    /// STALE/типы — скорее `info` (UI просто покажет лишнее/иначе).
    pub fn severity(&self) -> &'static str {
        match self.kind {
            IssueKind::MissingInUi | IssueKind::UnknownType => "warning",
            _ => "info",
        }
    }
}

/// Отчёт о сверке.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParityReport {
    pub supported_types: Vec<String>,
    pub renderers: Vec<String>,
    pub issues: Vec<ParityIssue>,
    /// Все ключи настроек, реально пришедшие от движка: renderer → keys.
    /// Упорядоченная карта с семантикой Python `dict`: повторная запись по
    /// существующему имени заменяет значение, НЕ меняя позицию.
    pub engine_keys: Vec<(String, Vec<String>)>,
    /// Настройки, которые сверить НЕ С ЧЕМ: тип обоев → ключи реестра.
    /// Рисовальщика этого типа обоев у движка нет (плагин не установлен),
    /// поэтому молчание движка о них — не расхождение, но и не «сошлось».
    /// Порядок — как в реестре.
    pub unchecked: Vec<(String, Vec<String>)>,
}

impl ParityReport {
    pub fn has_drift(&self) -> bool {
        !self.issues.is_empty()
    }

    pub fn warning_count(&self) -> usize {
        self.issues.iter().filter(|i| i.severity() == "warning").count()
    }

    /// Ключи настроек данного renderer'а.
    pub fn engine_keys_of(&self, renderer: &str) -> Option<&[String]> {
        self.engine_keys
            .iter()
            .find(|(name, _)| name == renderer)
            .map(|(_, keys)| keys.as_slice())
    }

    fn set_engine_keys(&mut self, renderer: &str, keys: Vec<String>) {
        match self.engine_keys.iter_mut().find(|(name, _)| name == renderer) {
            Some(slot) => slot.1 = keys,
            None => self.engine_keys.push((renderer.to_string(), keys)),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Сверка
// ─────────────────────────────────────────────────────────────────────────

/// Сверить `RendererPluginListResponse` с реестром UI Rexon.
pub fn compare_schema(resp: &pb::RendererPluginListResponse) -> ParityReport {
    let mut report = ParityReport {
        supported_types: resp.supported_types.clone(),
        ..Default::default()
    };
    let mut seen_keys: Vec<&str> = Vec::new();

    for ri in &resp.renderers {
        report.renderers.push(ri.name.clone());
        report.set_engine_keys(&ri.name, ri.settings.iter().map(|s| s.key.clone()).collect());
        for s in &ri.settings {
            if !seen_keys.contains(&s.key.as_str()) {
                seen_keys.push(&s.key);
            }
            let vt = s.r#type;

            // 1) Неизвестный UI тип значения.
            if !is_known_type(vt) && !is_special_key(&s.key) {
                report.issues.push(ParityIssue {
                    kind: IssueKind::UnknownType,
                    renderer: ri.name.clone(),
                    key: s.key.clone(),
                    detail: crate::i18n::trf(
                        "value type %1 is unknown to the UI (known: %2); \
the widget may be drawn as a plain field",
                        &[&vt, &known_types_repr()],
                    ),
                });
            }

            let Some(exp) = expected_for(&s.key) else {
                // 2) Настройка есть в движке, но UI её не регистрирует.
                report.issues.push(ParityIssue {
                    kind: IssueKind::MissingInUi,
                    renderer: ri.name.clone(),
                    key: s.key.clone(),
                    detail: crate::i18n::trf(
                        "the engine supports “%1” (type %2, group %3), but the Rexon UI registry does not have it",
                        &[
                            &s.key,
                            &type_label(vt),
                            &if s.group.is_empty() { "—" } else { s.group.as_str() },
                        ],
                    ),
                });
                continue;
            };

            // 3) Тип разошёлся с ожиданием реестра.
            if let Some(want) = exp.value_type {
                if is_known_type(vt) && vt != want {
                    report.issues.push(ParityIssue {
                        kind: IssueKind::TypeMismatch,
                        renderer: ri.name.clone(),
                        key: s.key.clone(),
                        detail: crate::i18n::trf(
                            "the UI expects type %1, the engine returns %2",
                            // `KNOWN_VALUE_TYPES.get(exp.value_type)` без
                            // значения по умолчанию: неизвестный тип в самом
                            // реестре печатался бы как `None`.
                            &[&value_type_name(want).unwrap_or("None"), &type_label(vt)],
                        ),
                    });
                }
            }

            // 4) Набор choices разошёлся.
            if let Some(want) = exp.choices {
                let eng: Vec<&str> = s.choices.iter().map(String::as_str).collect();
                if !eng.is_empty() && !same_set(&eng, want) {
                    report.issues.push(ParityIssue {
                        kind: IssueKind::ChoicesMismatch,
                        renderer: ri.name.clone(),
                        key: s.key.clone(),
                        detail: crate::i18n::trf(
                            "the UI expects choices %1, the engine returns %2",
                            &[&py_str_list(want), &py_str_list(&eng)],
                        ),
                    });
                }
            }
        }
    }

    // 5) Реестр UI знает ключ, которого движок больше не отдаёт нигде.
    //    Исключение — настройка рисовальщика, которого у движка НЕТ: её
    //    некому отдавать, и это «не проверено», а не «удалена». Иначе
    //    демон без плагина сцен и веб-обоев давал четыре расхождения на
    //    ровном месте (снимок стенда 10.10.2026).
    for exp in REXON_SETTING_REGISTRY {
        if !seen_keys.contains(&exp.key) {
            if !exp.only_types.is_empty()
                && !exp.only_types.iter().any(|t| serves_type(resp, t))
            {
                let label = exp.only_types.join(", ");
                match report.unchecked.iter_mut().find(|(types, _)| *types == label) {
                    Some(slot) => slot.1.push(exp.key.to_string()),
                    None => report.unchecked.push((label, vec![exp.key.to_string()])),
                }
                continue;
            }
            report.issues.push(ParityIssue {
                kind: IssueKind::StaleInUi,
                renderer: String::new(),
                key: exp.key.to_string(),
                detail: crate::i18n::trf(
                    "the Rexon UI expects “%1”, but no engine renderer returns it \
any more (the setting may have been removed)",
                    &[&exp.key],
                ),
            });
        }
    }

    report
}

/// Есть ли у движка рисовальщик обоев данного типа: тип назван либо в общем
/// списке ответа, либо у самого рисовальщика.
fn serves_type(resp: &pb::RendererPluginListResponse, wallpaper_type: &str) -> bool {
    resp.supported_types.iter().any(|t| t == wallpaper_type)
        || resp
            .renderers
            .iter()
            .any(|r| r.types.iter().any(|t| t == wallpaper_type))
}

/// `KNOWN_VALUE_TYPES.get(vt, vt)` — имя типа либо сам номер.
fn type_label(vt: i32) -> String {
    value_type_name(vt).map_or_else(|| vt.to_string(), str::to_string)
}

/// `sorted(KNOWN_VALUE_TYPES)` в f-строке — список номеров по возрастанию.
fn known_types_repr() -> String {
    let mut nums: Vec<i32> = KNOWN_VALUE_TYPES.iter().map(|(k, _)| *k).collect();
    nums.sort_unstable();
    let body: Vec<String> = nums.iter().map(i32::to_string).collect();
    format!("[{}]", body.join(", "))
}

/// Сравнение как `set(a) != set(b)` в Python: порядок и дубли не важны.
fn same_set(a: &[&str], b: &[&str]) -> bool {
    a.iter().all(|x| b.contains(x)) && b.iter().all(|x| a.contains(x))
}

/// `repr(list[str])` Python: `['auto', 'vaapi']`. Кавычка выбирается как в
/// Python (одинарные, но двойные — если внутри есть `'` и нет `"`).
fn py_str_list(items: &[&str]) -> String {
    let body: Vec<String> = items.iter().map(|s| py_repr_str(s)).collect();
    format!("[{}]", body.join(", "))
}

fn py_repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(quote);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Текстовый дамп отчёта (для логов / CLI / тестов).
pub fn render_report_text(report: &ParityReport) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(crate::i18n::trf(
        "Engine wallpaper types: %1",
        &[&join_or_dash(&report.supported_types)],
    ));
    lines.push(crate::i18n::trf("Renderers: %1", &[&join_or_dash(&report.renderers)]));
    if report.issues.is_empty() {
        lines.push(crate::i18n::tr("UI parity: no differences found ✅"));
    } else {
        lines.push(crate::i18n::trf(
            "Differences: %1 (warnings: %2)",
            &[&report.issues.len(), &report.warning_count()],
        ));
    }
    for it in &report.issues {
        let tag = if it.severity() == "warning" { "⚠️" } else { "•" };
        let where_ = if it.renderer.is_empty() {
            String::new()
        } else {
            format!("[{}] ", it.renderer)
        };
        lines.push(format!("  {tag} {where_}{}: {}", it.key, it.detail));
    }
    // Не расхождение и не «сошлось»: сверять было не с чем — говорим словами.
    for (types, keys) in &report.unchecked {
        lines.push(crate::i18n::trf(
            "Not checked: the engine has no renderer for “%1” wallpapers, \
so there is nothing to compare these settings with: %2",
            &[types, &keys.join(", ")],
        ));
    }
    lines.join("\n")
}

/// `', '.join(items) or '—'`: пустая строка (в т.ч. от пустого списка) —
/// прочерк.
fn join_or_dash(items: &[String]) -> String {
    let joined = items.join(", ");
    if joined.is_empty() {
        "—".to_string()
    } else {
        joined
    }
}

// ─────────────────────────────────────────────────────────────────────────
//  Тег proto-снапшота
// ─────────────────────────────────────────────────────────────────────────

/// Тег апстрима waywallen из содержимого файла `proto/UPSTREAM_TAG`.
///
/// Файл пишется при каждом прогоне `scripts/sync-waywallen-proto.py` (или
/// вручную при resync), формат «v0.2.4». `None` (файла нет) и пустое
/// содержимое дают пустую строку — снапшот неизвестного возраста.
///
/// ⚠ правило #16: декодирование lossy. Оригинал открывал файл строгим UTF-8
/// и ловил только `OSError`, поэтому единственный битый байт ронял всю
/// сверку паритета `UnicodeDecodeError`'ом.
pub fn proto_upstream_tag(raw: Option<&[u8]>) -> String {
    match raw {
        Some(bytes) => String::from_utf8_lossy(bytes).trim().to_string(),
        None => String::new(),
    }
}

/// Путь к файлу тега относительно каталога пакета `qt_wallpaper_engine`.
pub fn upstream_tag_path(package_dir: &std::path::Path) -> std::path::PathBuf {
    package_dir.join("proto").join("UPSTREAM_TAG")
}

/// Тонкая обёртка с реальным чтением: ошибка ввода-вывода → пустой тег.
pub fn read_proto_upstream_tag(package_dir: &std::path::Path) -> String {
    let path = upstream_tag_path(package_dir);
    proto_upstream_tag(std::fs::read(path).ok().as_deref())
}

/// Каталог пакета вкладки внутри пакета `rexon/`.
///
/// ⚠️ Отдельный шаг, и он не формальность. Питоновская сверка считает
/// `package_dir` от `parity.py`, то есть от каталога ВКЛАДКИ, а крейту
/// Python сообщает каталог пакета `rexon/` целиком (`we_env::pkg_dir`) —
/// на уровень выше. Без этого шага тег снапшота не находился, отчёт писал
/// «(тег не записан)», а бейдж не мог сказать, что схему сверяли по
/// устаревшему протоколу: расхождение версий просто не считалось. Поймано
/// снимком стенда 06.09.2026 — у виджетной версии «v0.3.7 (9540f8c)», у
/// QML пусто. Путь между двумя каталогами держит ядро, как и у каталога
/// `.qm` (`remote_l10n::UI_MODULE_REL`).
pub const TAB_PACKAGE_REL: &str = "qt_wallpaper_engine";

/// Тег proto-снапшота по каталогу пакета `rexon/`.
pub fn read_proto_upstream_tag_from_pkg(pkg_dir: &str) -> String {
    read_proto_upstream_tag(&std::path::Path::new(pkg_dir).join(TAB_PACKAGE_REL))
}

/// Совпадает ли версия живого демона с версией proto-снапшота.
///
/// `None` — сравнить нечем (нет версии демона или тега). Сравнение по
/// «числовой» части: `v0.2.4` == `0.2.4`.
///
/// ⚠ правило #16: у оригинала префикс `v` снимался через `lstrip("v")` —
/// это удаление всех ведущих символов из набора, из-за чего `vv0.2.6`
/// считался равным `0.2.6`. Здесь снимается ровно один ведущий `v`.
///
/// ⚠ правило #16, вторая починка (06.09.2026): в файле тега рядом с версией
/// живёт хеш коммита — `v0.3.7 (9540f8c)`, так его пишет рука при resync'е.
/// Оригинал сравнивал строку целиком, поэтому бейдж докладывал расхождение
/// ВСЕГДА, даже когда демон и снапшот одной версии, — «✓ схема» на странице
/// «Разработчик» не мог показаться в принципе. Хвост с хешем отрезается —
/// всё остальное поведение оригинала (в том числе то, что тег НЕ обрезается
/// по краям, а версия демона обрезается) сохранено как было.
pub fn daemon_matches_snapshot(tag_file: &str, daemon_version: &str) -> Option<bool> {
    let tag = strip_v(strip_commit(tag_file));
    let ver = strip_v(strip_commit(daemon_version.trim()));
    if tag.is_empty() || ver.is_empty() {
        return None;
    }
    Some(tag == ver)
}

/// Отрезать хвост «` (хеш)`», которым в файле тега подписан коммит апстрима.
///
/// Скобки обязательны обе: строка без них — обычная версия, её трогать
/// нельзя (иначе `0.3.8 dirty` молча стал бы `0.3.8`).
fn strip_commit(s: &str) -> &str {
    match s.find(" (") {
        Some(i) if s.trim_end().ends_with(')') => &s[..i],
        _ => s,
    }
}

fn strip_v(s: &str) -> &str {
    s.strip_prefix('v').unwrap_or(s)
}
