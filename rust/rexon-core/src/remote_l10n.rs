//! Подписи каталогов обоев на языке интерфейса (Мастерская Steam, Wallhaven) — порт
//! `rexon/qt_wallpaper_engine/qml_ui/remote_l10n.py`.
//!
//! Плагины-источники демона отдают всё по-английски: имя источника,
//! сортировки, названия фильтров, кнопки входа, строки состояния. Покупатель
//! не должен видеть «Top rated» и «Not signed in» (правило #13), но и
//! вендорный QML править ради этого нельзя — обновление вендора идёт
//! `git diff`-ом. Поэтому перевод живёт МЕЖДУ демоном и QML: описание
//! источника пропускается через [`Localizer::translate_source`].
//!
//! Значения фильтров (теги) уходят обратно в Steam/Wallhaven как параметры
//! запроса, поэтому переведённое значение перед отправкой возвращается к
//! оригиналу — [`Localizer::untranslate_tags`]. Обе стороны берут ОДНУ карту
//! [`TAGS`], так что новый тег достаточно вписать один раз.
//!
//! Неизвестная строка остаётся как есть: незнакомая сортировка нового
//! плагина покажется по-английски, но ничего не сломает.
//!
//! # Подписи самого вендорного QML — только через каталог `.qm`
//!
//! Строки страниц зашиты в вендорные `.qml` через `qsTr()`. Правильный (и
//! единственный безопасный) путь — собрать настоящий каталог переводов Qt и
//! отдать его `QTranslator::load()`.
//!
//! ⛔ Переводчик обязан быть НАСТОЯЩИМ каталогом `.qm`, а не `QTranslator` с
//! переопределённым методом `translate()` на стороне Python. Qt спрашивает
//! перевод в том числе из потока QQmlThread (компиляция страницы), а PySide6
//! на вызове Python-метода захватывает GIL; главный поток в этот момент стоит
//! внутри синхронной загрузки QML и GIL не отпускает — оба потока встают
//! навсегда, вкладка «не отвечает» (проверено живьём, коммит 3ed7dd2). Каталог
//! `.qm` ищет строки на C++ и потоков не блокирует.
//!
//! # Что здесь и чего здесь нет
//!
//! Здесь — ЧИСТАЯ логика (детерминированный вход→выход), как в
//! `system_plan.rs` / `importers.rs` / `compliance.rs`:
//!   * все таблицы перевода и поиск по ним;
//!   * перевод описания источника и обратный перевод тегов (с состоянием
//!     карт по источникам — в [`Localizer`]);
//!   * сверка вендорного каталога переводов с вендорными `.qml` и argv
//!     `lrelease` — [`plan_build_qm`] (ПЛАН шага, ничего не исполняет);
//!   * выбор языка вендорных страниц — [`ui_language_codes`],
//!     [`normalize_ui_language`], [`plan_ui_language`].
//!
//! НЕ портировано (исполнение — за мостом): обход дерева `.qml` (`glob`) и
//! чтение файлов, чтение вендорного `.ts`, запуск `lrelease`
//! (`subprocess.run(check=True)`
//! → `FileNotFoundError`/`CalledProcessError` формирует исполнитель),
//! `QCoreApplication.instance()`, `QTranslator::load/installTranslator`,
//! `os.path.isfile` — их РЕЗУЛЬТАТ подаётся параметром.
//!
//! # Починки оригинала (правило #16)
//!
//!   1. `untranslate_tags` брал карту источника через `or` — пустая строка в
//!      карте источника молча проваливалась в общую карту. Здесь проверяется
//!      НАЛИЧИЕ ключа. Починка ЗАЩИТНАЯ: сегодня расхождения не видно, потому
//!      что пустое значение попадает в карту только под пустой подписью, а
//!      общая карта на неё тоже ничего не отвечает; но `or` вместо проверки
//!      ключа — неверный идиом, и первая же подпись с пустым значением
//!      сломала бы фильтр молча.
//!   2. `translate_source` пересобирал элемент `sorts` из двух полей и терял
//!      все остальные ключи (в отличие от `filters`/`actions`, которые
//!      копируются целиком). Здесь лишние ключи сохраняются.
//!   3. Карта обратного перевода источника в Python накапливалась вечно
//!      (`setdefault`): после обновления плагина в ней оставались подписи
//!      исчезнувших тегов. Здесь карта источника пересобирается при каждом
//!      `translate_source`.
//!   4. Сборщик каталога видел только `qsTr("…")` и не замечал `qsTr('…')` —
//!      QML одинаково допускает обе кавычки, и строка молча оставалась без
//!      перевода. Здесь распознаются обе (вендор пишет одинарными, например
//!      `qsTr('Status')` в `Window.qml` v0.3.5).
//!   5. Расхождение каталога и `ui/qml` было невидимым в обе стороны. Теперь
//!      план называет и вендорные строки без перевода
//!      ([`QmBuildPlan::untranslated`]), и записи каталога, которых в `ui/qml`
//!      уже нет ([`QmBuildPlan::stale`]).
//!   6. Путь к `lrelease` был жёстко арчевым (`/usr/lib/qt6/bin/lrelease`).
//!      Кандидаты вынесены в [`LRELEASE_CANDIDATES`] — исполнитель берёт
//!      первый найденный (`which`), жёсткий путь остался последним запасным.
//!   7. Глобальное изменяемое состояние (`_TAGS_BACK_BY_SOURCE`) заменено на
//!      поле [`Localizer`]: две вкладки/два потока больше не делят одну карту.
//!   8. Мусор в описании источника (элемент `filters`/`actions` — не словарь)
//!      ронял `AttributeError` посреди перевода и гасил всю выдачу демона;
//!      теперь такой элемент пропускается, остальные переводятся.
//!   9. Сборщик знал только одиночные строки: вызов со счётчиком
//!      (`qsTr("%n wallpaper(s)", "", n)`) не распознавался ВООБЩЕ — ни как
//!      совпадение, ни как пропажа. Теперь он считается наравне с прочими, а
//!      формы числа приходят готовыми из вендорного каталога
//!      (`numerus`-сообщения).
//!  10. Свой словарь подписей вендорного QML держал 135 строк из вендорных
//!      486 — четыре новые страницы покупатель видел по-английски, а на 55
//!      общих строках наш текст расходился с оригиналом. Источник перевода
//!      теперь ОДИН — вендорный `ui/i18n/waywallen_ru.ts` ([`VENDOR_TS_REL`]),
//!      он вендорится тем же `git diff`-ом, что и `ui/qml`.
//!
//! Сохранённый КВИРК оригинала (менять нельзя): в `fields` и `status` ключи
//! `label`/`description`/`placeholder`/`group`/`value` проставляются ВСЕГДА,
//! даже если их не было во входе (появляется пустая строка). Вендорный QML
//! отличает `undefined` от `""` (`text !== ""`), поэтому «починка» тут сломала
//! бы страницу — поведение сохранено байт-в-байт.

use crate::i18n::{n_, tr};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

// ============================================================================
// Таблицы перевода (порядок = как в Python-словарях, он важен для TAGS_BACK)
// ============================================================================
//
// ⭐ Значение каждой пары — английский msgid каталога Rexon (`n_`), а не
// готовая подпись: подпись на языке интерфейса даёт [`tr`] в месте показа
// (русский — дословно прежний, в `po/ru.po`). Ключ остаётся тем, что
// присылает источник, и переводу не подлежит.

/// `SOURCE_NAMES` — имена источников.
pub const SOURCE_NAMES: &[(&str, &str)] = &[
    ("wallpaper_engine", n_("Steam Workshop")),
    ("wallhaven", n_("Wallhaven")),
];

/// `WP_TYPES` — тип обоины.
///
/// Ключ — техническая строка движка (она же уходит в запрос миниатюры и в
/// фильтр по типу), значение — подпись для покупателя. Переводится ТОЛЬКО
/// показ: [`wp_type_label`] зовут места вывода, сама строка нигде не
/// подменяется.
pub const WP_TYPES: &[(&str, &str)] = &[
    ("scene", n_("Scene")),
    ("video", n_("Video")),
    ("web", n_("Web")),
    ("image", n_("Image")),
    ("application", n_("Application")),
];

/// `SORTS` — сортировки каталога.
pub const SORTS: &[(&str, &str)] = &[
    // Мастерская Steam
    ("trend_day", n_("Trending (Today)")),
    ("trend_week", n_("Trending (Week)")),
    ("trend_month", n_("Trending (Month)")),
    ("trend_3months", n_("Trending (3 Months)")),
    ("trend_6months", n_("Trending (6 Months)")),
    ("trend_year", n_("Trending (Year)")),
    ("recent", n_("Recently Added")),
    ("most_subscribed", n_("Most Subscribed")),
    ("top_rated", n_("Top Rated")),
    // наши собственные режимы показа поверх каталога Мастерской
    ("rexon_my_items", n_("My Subscriptions")),
    ("rexon_collections", n_("Collections")),
    // Wallhaven
    ("trend", n_("Trending")),
    ("popular", n_("Popular")),
];

/// `FILTER_TITLES` — названия фильтров.
pub const FILTER_TITLES: &[(&str, &str)] = &[
    ("type", n_("Type")),
    ("questionable", n_("12+ (Questionable)")),
    ("mature", n_("18+ (Mature)")),
    ("miscellaneous", n_("Features")),
    ("genre", n_("Genre")),
    ("resolution", n_("Resolution")),
    ("topics", n_("Topics")),
];

/// `FILTER_DESCRIPTIONS` — пояснения к фильтрам.
pub const FILTER_DESCRIPTIONS: &[(&str, &str)] = &[
    (
        "questionable",
        n_("Show wallpapers marked “12+ (Questionable)”."),
    ),
    ("mature", n_("Show wallpapers marked 18+.")),
];

/// `FILTER_CONFIRMATIONS` — вопрос перед включением фильтра.
pub const FILTER_CONFIRMATIONS: &[(&str, &str)] = &[(
    "mature",
    n_("This shows wallpapers marked 18+. Turn it on only if you are an adult. Continue?"),
)];

/// `TAGS` — значения фильтров (они же теги запроса).
///
/// Ключ — то, что уходит в источник; значение — то, что видит покупатель.
/// Числовые разрешения (1920 x 1080) не переводятся и здесь не нужны.
///
/// ⚠️ Порядок записей значим для обратной карты: у Wallhaven жанр называется
/// `CG`, у Мастерской — `CGI`, подпись у обоих одна. Python строил обратную
/// карту словарным включением, где ПОСЛЕДНЯЯ пара побеждает — здесь то же
/// самое, см. [`tag_back`].
pub const TAGS: &[(&str, &str)] = &[
    // тип обоев
    ("Scene", n_("Scene")),
    ("Video", n_("Video")),
    ("Web", n_("Web")),
    ("Application", n_("Application")),
    // возможности
    ("Approved", n_("Approved")),
    ("Audio responsive", n_("Audio responsive")),
    ("3D", n_("3D")),
    ("Customizable", n_("Customizable")),
    ("Puppet Warp", n_("Puppet Warp")),
    ("HDR", n_("HDR")),
    ("Media Integration", n_("Media Integration")),
    ("User Shortcut", n_("User Shortcut")),
    ("Video Texture", n_("Video Texture")),
    ("No Animation", n_("No Animation")),
    // что публикуют в Мастерскую кроме самих обоев
    ("Wallpaper", n_("Wallpaper")),
    ("Preset", n_("Preset")),
    ("Asset", n_("Asset")),
    ("Asset Pack", n_("Asset Pack")),
    // возрастные пометки (они же значения фильтров рейтинга)
    ("Everyone", n_("0+ (Everyone)")),
    ("Questionable", n_("12+ (Questionable)")),
    ("Mature", n_("18+ (Mature)")),
    // жанры
    ("Abstract", n_("Abstract")),
    ("Animal", n_("Animal")),
    ("Anime", n_("Anime")),
    ("Cartoon", n_("Cartoon")),
    ("CG", n_("CGI")),
    ("CGI", n_("CGI")),
    ("Cyberpunk", n_("Cyberpunk")),
    ("Fantasy", n_("Fantasy")),
    ("Game", n_("Game")),
    ("Girls", n_("Girls")),
    ("Guys", n_("Guys")),
    ("Landscape", n_("Landscape")),
    ("Medieval", n_("Medieval")),
    ("Memes", n_("Memes")),
    ("MMD", n_("MikuMikuDance (animation)")),
    ("Music", n_("Music")),
    ("Nature", n_("Nature")),
    ("Pixel art", n_("Pixel art")),
    ("Relaxing", n_("Relaxing")),
    ("Retro", n_("Retro")),
    ("Sci-Fi", n_("Sci-Fi")),
    ("Sports", n_("Sports")),
    ("Technology", n_("Technology")),
    ("Television", n_("Television")),
    ("Vehicle", n_("Vehicle")),
    ("Unspecified", n_("Unspecified genre")),
    // разрешения
    ("Standard Definition", n_("Standard Definition")),
    ("Ultrawide Standard Definition", n_("Ultrawide Standard Definition")),
    ("Ultrawide 2560 x 1080", n_("Ultrawide 2560 x 1080")),
    ("Ultrawide 3440 x 1440", n_("Ultrawide 3440 x 1440")),
    ("Dual Standard Definition", n_("Dual Standard Definition")),
    ("Dual 3840 x 1080", n_("Dual 3840 x 1080")),
    ("Dual 5120 x 1440", n_("Dual 5120 x 1440")),
    ("Dual 7680 x 2160", n_("Dual 7680 x 2160")),
    ("Triple Standard Definition", n_("Triple Standard Definition")),
    ("Triple 4096 x 768", n_("Triple 4096 x 768")),
    ("Triple 5760 x 1080", n_("Triple 5760 x 1080")),
    ("Triple 7680 x 1440", n_("Triple 7680 x 1440")),
    ("Triple 11520 x 2160", n_("Triple 11520 x 2160")),
    ("Portrait Standard Definition", n_("Portrait Standard Definition")),
    ("Portrait 720 x 1280", n_("Portrait 720 x 1280")),
    ("Portrait 1080 x 1920", n_("Portrait 1080 x 1920")),
    ("Portrait 1440 x 2560", n_("Portrait 1440 x 2560")),
    ("Portrait 2160 x 3840", n_("Portrait 2160 x 3840")),
    ("Other resolution", n_("Other resolution")),
    ("Dynamic resolution", n_("Dynamic resolution")),
    ("Oversized", n_("Larger than the screen")),
];

/// `ACTION_LABELS` — кнопки входа плагинов.
pub const ACTION_LABELS: &[(&str, &str)] = &[
    ("Log in to Steam", n_("Log in to Steam")),
    ("Sign out", n_("Sign out")),
    ("Log in to Wallhaven", n_("Log in to Wallhaven")),
    ("Remove API key", n_("Remove API key")),
    ("API key", n_("API key")),
];

/// `TEXTS` — группы, строки состояния и пояснения плагинов.
pub const TEXTS: &[(&str, &str)] = &[
    // группы и строки состояния
    ("Steam account", n_("Steam account")),
    ("Wallhaven account", n_("Wallhaven account")),
    ("Status", n_("Status")),
    ("Not signed in", n_("Not signed in")),
    ("Guest mode", n_("Guest mode")),
    ("Signed in", n_("Signed in")),
    // пояснения плагинов
    (
        "Waywallen only manages Workshop subscriptions and does not download \
         wallpapers. Keep the Steam desktop client running to download \
         subscribed items.",
        n_("The Steam client downloads the wallpaper files itself — keep it running, and subscribed wallpapers will appear in the list automatically."),
    ),
    (
        "Optional. Use an API key from your Wallhaven account settings. Do not \
         enter your Wallhaven password.",
        n_("Optional. Paste the API key from your Wallhaven account settings. No password is needed."),
    ),
    (
        "Copy the API key from Wallhaven Account Settings.",
        n_("The API key is in your Wallhaven account settings."),
    ),
    ("Wallhaven API key", n_("Wallhaven API key")),
];

/// Подписи самого вендорного QML — ВЕНДОРНЫЙ каталог, не наш словарь.
///
/// Строки страниц зашиты в `.qml` через `qsTr()`, и русский перевод для них
/// уже есть у самого waywallen: `ui/i18n/waywallen_ru.ts`. Он вендорится
/// вместе с `ui/qml` из одного и того же тега ([`VENDOR_TS_REL`]), а мы его
/// только компилируем в `.qm`.
///
/// ⚠ Своего словаря здесь БОЛЬШЕ НЕТ. Он держал 135 строк из вендорных 486 —
/// новые страницы («Мониторы», «Состояние», «Настройки», «О программе»)
/// покупатель видел по-английски, а на 55 общих строках наш текст расходился
/// с вендорным («Открыть страницу в Steam» там, где у оригинала «Open web
/// page»), что и запрещено правилом паритета. Перевод подписей чужой
/// программы место имеет ОДНО — её собственный каталог: тогда он достаётся
/// всем её пользователям, а не только нам.
pub const VENDOR_TS_REL: &str = "waywallen/ui/i18n/waywallen_ru.ts";

// ============================================================================
// Поиск по таблицам
// ============================================================================

/// `dict.get(key)` для таблицы-среза (ключи в наших таблицах уникальны, кроме
/// оговорённого случая обратной карты — см. [`tag_back`]).
fn lookup(table: &[(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Положить наш КЛЮЧЕВОЙ перевод в карту `PluginMessage` рядом с `msgid`.
///
/// С waywallen 0.3.7 вендорный QML показывает не строковое поле, а
/// `*Text: {pluginId, msgid}`, пропущенное через `PluginTranslations`. Наши
/// таблицы сортировок, заголовков фильтров и имён источников — КЛЮЧЕВЫЕ
/// (`trend_day`, `mature`, `wallpaper_engine`), а ключа в `PluginMessage`
/// нет. Поэтому посчитанную здесь подпись мы кладём в ту же карту полем
/// `rexonFallback`: `plugin_l10n::resolve` возьмёт её вторым пунктом —
/// после каталога самого плагина и до голого `msgid`.
///
/// Поле НАШЕ и вендорным QML не читается: лишний ключ в карте безвреден,
/// а подмена самого `msgid` сломала бы поиск по каталогу плагина.
///
/// Ничего не делает, когда перевода нет (`ru == original`) или карты
/// сообщения в описании нет вовсе (демон постарше 0.3.7).
fn put_fallback(item: &mut Map<String, Value>, key: &str, ru: &str, original: &str) {
    if ru == original || ru.is_empty() {
        return;
    }
    let Some(Value::Object(message)) = item.get_mut(key) else {
        return;
    };
    message.insert("rexonFallback".into(), Value::String(ru.to_string()));
}

/// Подпись из таблицы на языке интерфейса (`None` — ключа в таблице нет).
fn lookup_tr(table: &[(&'static str, &'static str)], key: &str) -> Option<String> {
    lookup(table, key).map(tr)
}

/// Тип обоины → подпись для покупателя («scene» → «Scene» / «Сцена»).
///
/// Регистр и пробелы по краям не важны; незнакомый тип возвращается КАК БЫЛ
/// (не обрезанный и не приведённый к нижнему регистру) — как в Python.
pub fn wp_type_label(value: &str) -> String {
    let key = value.trim().to_lowercase();
    lookup_tr(WP_TYPES, &key).unwrap_or_else(|| value.to_string())
}

/// Тег источника → подпись для покупателя.
pub fn tag_label(value: &str) -> String {
    lookup_tr(TAGS, value).unwrap_or_else(|| value.to_string())
}

/// [`tag_label`] по списку.
pub fn tag_labels<S: AsRef<str>>(values: &[S]) -> Vec<String> {
    values
        .iter()
        .map(|v| tag_label(v.as_ref()))
        .collect()
}

/// Подпись → исходный тег по ОБЩЕЙ карте (`_TAGS_BACK`).
///
/// Python строил её словарным включением `{ru: en for en, ru in TAGS}`, где
/// при повторе подписи побеждает ПОСЛЕДНЯЯ пара, — поэтому ищем с конца
/// (`Компьютерная графика` → `CGI`, а не `CG`).
pub fn tag_back(label: &str) -> Option<&'static str> {
    // Подпись сравнивается ПЕРЕВЕДЁННОЙ — той, что видел покупатель.
    TAGS.iter()
        .rev()
        .find(|(_, msgid)| tr(msgid) == label)
        .map(|(tag, _)| *tag)
}

/// Название сортировки; `fallback` — вендорная подпись (`s["label"]`).
pub fn sort_label(key: &str, fallback: &str) -> String {
    lookup_tr(SORTS, key).unwrap_or_else(|| fallback.to_string())
}

/// Имя источника по его `id` (None — источник нам незнаком).
pub fn source_name(source_id: &str) -> Option<String> {
    lookup_tr(SOURCE_NAMES, source_id)
}

/// `_text` — строка состояния/кнопка плагина: сначала [`TEXTS`], потом
/// [`ACTION_LABELS`], иначе как есть.
pub fn text(value: &str) -> String {
    lookup_tr(TEXTS, value)
        .or_else(|| lookup_tr(ACTION_LABELS, value))
        .unwrap_or_else(|| value.to_string())
}

// ============================================================================
// Перевод описания источника
// ============================================================================

/// Ошибка перевода описания источника.
///
/// В Python на не-словаре падал `TypeError` (`'NoneType' object is not
/// iterable` / `object is not iterable`) — здесь тот же случай выражен типом.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum L10nError {
    /// Описание источника — не JSON-объект.
    NotAnObject,
}

/// Переводчик описаний источников с картами обратного перевода тегов.
///
/// Карта обратного перевода ведётся ОТДЕЛЬНО по каждому источнику: её
/// заполняет [`Self::translate_source`] теми значениями, которые источник
/// реально отдал. Причина — живой дефект: у Wallhaven жанр называется `CG`, у
/// Мастерской `CGI`, а подпись у обоих одна («Компьютерная графика»). Одна
/// общая обратная карта возвращала для Wallhaven чужое `CGI`, и фильтр по
/// жанру молча не работал. Общая карта осталась запасной, но сначала
/// спрашиваем тот источник, которому запрос и уходит (правило #12 — чиним
/// механизм, а не конкретную пару тегов).
#[derive(Debug, Default, Clone)]
pub struct Localizer {
    back_by_source: BTreeMap<String, BTreeMap<String, String>>,
}

impl Localizer {
    /// Пустой переводчик (карты источников наполнит `translate_source`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Карта «подпись → исходный тег» конкретного источника (для проверок).
    pub fn source_back_map(&self, source_id: &str) -> Option<&BTreeMap<String, String>> {
        self.back_by_source.get(source_id)
    }

    /// Подписи фильтров → значения, которые понимает ЭТОТ источник.
    ///
    /// Сначала смотрим карту самого источника (её собрал
    /// [`Self::translate_source`] из его же значений), потом общую. Так у двух
    /// источников могут быть разные исходные теги под одной русской подписью —
    /// см. `CG`/`CGI` выше.
    ///
    /// ⚠ Правило #16 (защитно): Python брал значение из карты источника через
    /// `or`, и пустая строка проваливалась в общую карту. Здесь проверяется
    /// НАЛИЧИЕ ключа — см. оговорку в шапке модуля.
    pub fn untranslate_tags<S: AsRef<str>>(&self, values: &[S], source_id: &str) -> Vec<String> {
        let back = self.back_by_source.get(source_id);
        values
            .iter()
            .map(|v| {
                let v = v.as_ref();
                back.and_then(|m| m.get(v))
                    .cloned()
                    .or_else(|| tag_back(v).map(str::to_string))
                    .unwrap_or_else(|| v.to_string())
            })
            .collect()
    }

    /// Копия описания источника с русскими подписями.
    ///
    /// Переводится только то, что видит покупатель. Идентификаторы (`id`
    /// источника, `id` фильтров и действий, ключи сортировок) остаются
    /// нетронутыми — по ним ходит и вендорный QML, и демон.
    ///
    /// Элементы `filters`/`actions`, которые не являются объектами, молча
    /// пропускаются (в Python на них падал `AttributeError`): демон такого не
    /// присылает, а ронять из-за этого всю вкладку незачем.
    pub fn translate_source(&mut self, info: &Value) -> Result<Value, L10nError> {
        let info = info.as_object().ok_or(L10nError::NotAnObject)?;
        let mut out = info.clone();

        let source_id = str_of(info.get("id"));
        if let Some(name) = source_name(&source_id) {
            let original = str_of(info.get("displayName"));
            put_fallback(&mut out, "displayNameText", &name, &original);
            out.insert("displayName".into(), Value::String(name));
        }

        // ── сортировки ──────────────────────────────────────────────────
        //
        // ⚠ Правило #16: Python собирал элемент заново из двух полей и терял
        // все прочие ключи (в отличие от filters/actions, которые копируются
        // целиком). Здесь key/label идут первыми — порядок как у оригинала, —
        // а остальные ключи входа сохраняются следом.
        let sorts: Vec<Value> = array_of(info.get("sorts"))
            .iter()
            .map(|s| {
                let src = s.as_object();
                let key = src.map(|m| str_of(m.get("key"))).unwrap_or_default();
                let label = src.map(|m| str_of(m.get("label"))).unwrap_or_default();
                let mut item = Map::new();
                item.insert("key".into(), Value::String(key.clone()));
                let ru = sort_label(&key, &label);
                item.insert("label".into(), Value::String(ru.clone()));
                if let Some(src) = src {
                    for (k, v) in src {
                        if k != "key" && k != "label" {
                            item.insert(k.clone(), v.clone());
                        }
                    }
                }
                put_fallback(&mut item, "labelText", &ru, &label);
                Value::Object(item)
            })
            .collect();
        out.insert("sorts".into(), Value::Array(sorts));

        out.insert(
            "tags".into(),
            Value::Array(
                str_list(info.get("tags"))
                    .iter()
                    .map(|t| Value::String(tag_label(t)))
                    .collect(),
            ),
        );

        // ⚠ Правило #16: карта источника пересобирается заново. В Python она
        // накапливалась вечно (`setdefault`), и после обновления плагина в ней
        // оставались подписи исчезнувших тегов.
        let mut back: BTreeMap<String, String> = BTreeMap::new();

        // ── фильтры ─────────────────────────────────────────────────────
        let mut filters: Vec<Value> = Vec::new();
        for f in array_of(info.get("filters")) {
            let src = match f.as_object() {
                Some(m) => m,
                None => continue,
            };
            let fid = str_of(src.get("id"));
            let mut item = src.clone();
            let title = str_of(src.get("title"));
            let title_ru = lookup_tr(FILTER_TITLES, &fid).unwrap_or_else(|| title.clone());
            item.insert("title".into(), Value::String(title_ru.clone()));
            put_fallback(&mut item, "titleText", &title_ru, &title);
            let description = str_of(src.get("description"));
            if !description.is_empty() {
                let description_ru =
                    lookup_tr(FILTER_DESCRIPTIONS, &fid).unwrap_or_else(|| text(&description));
                item.insert("description".into(), Value::String(description_ru.clone()));
                put_fallback(&mut item, "descriptionText", &description_ru, &description);
            }
            let confirmation = str_of(src.get("confirmation"));
            if !confirmation.is_empty() {
                let confirmation_ru =
                    lookup_tr(FILTER_CONFIRMATIONS, &fid).unwrap_or_else(|| text(&confirmation));
                item.insert("confirmation".into(), Value::String(confirmation_ru.clone()));
                put_fallback(&mut item, "confirmationText", &confirmation_ru, &confirmation);
            }
            // ── варианты фильтра ────────────────────────────────────────
            //
            // ⭐ waywallen 0.3.8 развёл техническое значение и подпись:
            // вариант приходит тройкой `{value, label, labelText}`, в запрос
            // уходит `value`, человеку показывается подпись. До этого поле
            // было одно (`repeated string values`), и подпись приходилось
            // подменять прямо в нём, а перед отправкой переводить обратно
            // (карта `back_by_source`). Теперь подменяется ТОЛЬКО подпись:
            // значение остаётся тем, что понимает источник.
            let mut options: Vec<Value> = Vec::new();
            for opt in array_of(src.get("options")) {
                let Some(o) = opt.as_object() else { continue };
                let mut out_opt = o.clone();
                let value = str_of(o.get("value"));
                let original = {
                    let label = str_of(o.get("label"));
                    if label.is_empty() { value.clone() } else { label }
                };
                // Словарь тегов ключуется ЗНАЧЕНИЕМ (`Anime`), а не подписью:
                // подпись плагина у разных источников своя, значение — общее.
                let shown = lookup_tr(TAGS, &value).unwrap_or_else(|| original.clone());
                // Карта обратного перевода остаётся: она подхватывает выбор,
                // сохранённый до перехода на 0.3.8 (там в состоянии страницы
                // лежат подписи, а не значения).
                back.insert(shown.clone(), value.clone());
                out_opt.insert("label".into(), Value::String(shown.clone()));
                match out_opt.get("labelText") {
                    // Карта подписи плагина: наш перевод идёт запаской.
                    Some(Value::Object(_)) => {
                        put_fallback(&mut out_opt, "labelText", &shown, &original)
                    }
                    // Строка-запаска (подписи у плагина нет) — заменяем её.
                    Some(_) if shown != original => {
                        out_opt.insert("labelText".into(), Value::String(shown.clone()));
                    }
                    _ => {}
                }
                options.push(Value::Object(out_opt));
            }
            item.insert("options".into(), Value::Array(options));
            filters.push(Value::Object(item));
        }
        out.insert("filters".into(), Value::Array(filters));
        self.back_by_source.insert(source_id, back);

        // ── действия (кнопки входа и настройки источника) ───────────────
        let mut actions: Vec<Value> = Vec::new();
        for a in array_of(info.get("actions")) {
            let src = match a.as_object() {
                Some(m) => m,
                None => continue,
            };
            let mut item = src.clone();
            for key in [
                "label",
                "group",
                "description",
                "browseDescription",
                "browseButtonLabel",
            ] {
                let value = str_of(src.get(key));
                if !value.is_empty() {
                    item.insert(key.into(), Value::String(text(&value)));
                }
            }
            // Квирк оригинала СОХРАНЁН: три ключа проставляются всегда, даже
            // если их не было — вендорный QML отличает undefined от "".
            item.insert(
                "fields".into(),
                Value::Array(
                    array_of(src.get("fields"))
                        .iter()
                        .map(|fld| {
                            let mut field = fld.as_object().cloned().unwrap_or_default();
                            for key in ["label", "description", "placeholder"] {
                                let value = str_of(field.get(key));
                                field.insert(key.into(), Value::String(text(&value)));
                            }
                            Value::Object(field)
                        })
                        .collect(),
                ),
            );
            actions.push(Value::Object(item));
        }
        out.insert("actions".into(), Value::Array(actions));

        // ── строки состояния ────────────────────────────────────────────
        out.insert(
            "status".into(),
            Value::Array(
                array_of(info.get("status"))
                    .iter()
                    .map(|row| {
                        let mut item = row.as_object().cloned().unwrap_or_default();
                        for key in ["label", "group", "value"] {
                            let value = str_of(item.get(key));
                            item.insert(key.into(), Value::String(text(&value)));
                        }
                        Value::Object(item)
                    })
                    .collect(),
            ),
        );

        let hint = str_of(info.get("remoteHint"));
        if !hint.is_empty() {
            out.insert("remoteHint".into(), Value::String(text(&hint)));
        }
        Ok(Value::Object(out))
    }
}

/// `str(value or "")` для поля JSON-объекта: строка как есть, отсутствие и
/// прочие типы — пустая строка (в описании источника все переводимые поля
/// строковые, их собирает `_source_info` из protobuf).
fn str_of(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_string()
}

/// `info.get(key) or []` — массив или пусто.
fn array_of(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// Массив строк (нестроковые элементы отбрасываются — в описании источника
/// `tags`/`values` протоколом объявлены `repeated string`).
fn str_list(v: Option<&Value>) -> Vec<String> {
    array_of(v)
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

// ============================================================================
// Сборка каталога переводов (.ts → lrelease → .qm)
// ============================================================================

/// Имя собранного каталога рядом с модулем UI.
pub const QM_FILE_NAME: &str = "waywallen_ru.qm";

/// Каталог вендорных каталогов переводов (`.ts`) относительно модуля UI.
/// С 0.5.0 их у апстрима два — `waywallen_ru.ts` и `waywallen_de.ts`, и
/// каждый собирается в свой `waywallen_<язык>.qm` рядом с модулем
/// ([`qm_file_name`]): тогда немецкий Rexon видит вендорные страницы
/// по-немецки, а не исходными английскими строками.
pub const VENDOR_I18N_REL: &str = "waywallen/ui/i18n";

/// Языки вендорных каталогов по именам файлов `waywallen_<язык>.ts`, по
/// коду, без повторов.
pub fn vendor_catalog_languages<S: AsRef<str>>(file_names: &[S]) -> Vec<String> {
    let mut codes: BTreeSet<String> = BTreeSet::new();
    for name in file_names {
        let code = name
            .as_ref()
            .strip_prefix(QM_PREFIX)
            .and_then(|rest| rest.strip_suffix(".ts"))
            .filter(|code| !code.is_empty());
        if let Some(code) = code {
            codes.insert(code.to_string());
        }
    }
    codes.into_iter().collect()
}

/// Путь вендорного каталога переводов языка `code` относительно модуля UI.
pub fn vendor_ts_path_for(module_dir: &str, code: &str) -> String {
    join(module_dir, &format!("{VENDOR_I18N_REL}/{QM_PREFIX}{code}.ts"))
}

/// Путь собранного каталога языка `code` рядом с модулем UI.
pub fn qm_path_for(module_dir: &str, code: &str) -> String {
    join(module_dir, &qm_file_name(code))
}

/// Каталог вендорного QML относительно модуля UI.
pub const QML_DIR_REL: &str = "waywallen/ui/qml";

/// Жёсткий путь `lrelease` из оригинала (Arch/CachyOS).
pub const DEFAULT_LRELEASE: &str = "/usr/lib/qt6/bin/lrelease";

/// ⚠ Правило #16: кандидаты `lrelease` по убыванию предпочтения — исполнитель
/// берёт первый найденный через `which`. В Python путь был жёстко арчевым, и
/// на любом другом дистрибутиве сборка каталога падала `FileNotFoundError`.
/// Жёсткий путь остался последним запасным.
pub const LRELEASE_CANDIDATES: &[&str] = &[
    "lrelease-qt6",
    "lrelease6",
    "lrelease",
    DEFAULT_LRELEASE,
];

/// Каталог модуля UI внутри пакета `rexon/`.
///
/// Каталоги `.qm` лежат рядом с `remote_l10n.py`, а крейт знает только
/// каталог пакета (его сообщает Python при загрузке `.so`,
/// `we_env::pkg_dir`). Путь между ними держит ядро — второй его копии в
/// мосте быть не должно.
pub const UI_MODULE_REL: &str = "qt_wallpaper_engine/qml_ui";

/// Каталог модуля UI по каталогу пакета `rexon/`.
pub fn ui_module_dir(pkg_dir: &str) -> String {
    join(pkg_dir, UI_MODULE_REL)
}

/// Путь собранного каталога рядом с модулем UI (`QM_PATH`).
pub fn qm_path(module_dir: &str) -> String {
    join(module_dir, QM_FILE_NAME)
}

/// Каталог вендорного QML по умолчанию.
pub fn default_qml_dir(module_dir: &str) -> String {
    join(module_dir, QML_DIR_REL)
}

/// Путь вендорного каталога переводов относительно модуля UI.
pub fn vendor_ts_path(module_dir: &str) -> String {
    join(module_dir, VENDOR_TS_REL)
}

/// План сборки каталога: что скомпилировать и чем.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QmBuildPlan {
    /// Исходник каталога — вендорный `.ts`. Мы его не пишем, а только читаем:
    /// он приходит с обновлением вендора вместе с `ui/qml`.
    pub ts_path: String,
    /// Куда `lrelease` положит скомпилированный каталог.
    pub qm_path: String,
    /// Команда компиляции (`check=True`: ненулевой код — ошибка).
    pub lrelease_argv: Vec<String>,
    /// ⚠ Строки, которые вендорный QML отдаёт на перевод, а вендорного
    /// перевода для них нет, — покупатель увидит их по-английски (правило
    /// #13). Не ноль означает, что каталог отстал от `ui/qml`: чинится
    /// наверху, у waywallen, а не подпоркой у нас.
    pub untranslated: Vec<String>,
    /// ⚠ Обратный перекос: в каталоге строка есть, а в `ui/qml` её больше
    /// нет. Сам по себе безвреден (лишняя запись в `.qm`), но означает, что
    /// вендорные `qml` и `i18n` вендорены с разных коммитов.
    pub stale: Vec<String>,
}

/// Все строки, которые вендорный QML отдаёт на перевод, — первые аргументы
/// всех `qsTr` дерева.
///
/// ⚠ Правило #16: распознаются обе формы записи, `qsTr("…")` и `qsTr('…')` —
/// QML допускает любые кавычки, а Python видел только двойные; вызов со
/// счётчиком `qsTr("…", "", n)`, которого Python не видел вовсе; и экранировка
/// внутри строки. Последнее не мелочь: вендор пишет
/// `qsTr("Deleted \"%1\"")` и `qsTr("\nRelated display: #%1")`, а в каталоге
/// эти строки лежат уже РАЗОБРАННЫМИ — без разбора они не совпадут ни с одной
/// записью, и девять живых подписей числились бы непереведёнными.
///
/// ⭐ И вторая функция перевода — `qsTranslate("Контекст", "…")`: строка у
/// неё ВТОРЫМ аргументом. Разметка 0.5.0 впервые зовёт её (вынесенный из
/// диалога фильтров `WallpaperFilterRules.qml` держит прежний контекст
/// `WallpaperFilterDialog`: «Group %1», «AND», «OR»), и без её разбора три
/// живые подписи числились лишними записями каталога.
pub fn vendor_sources<P: AsRef<str>, T: AsRef<str>>(files: &[(P, T)]) -> Vec<String> {
    let mut found: BTreeSet<String> = BTreeSet::new();
    for (_, text) in files {
        let text = text.as_ref();
        for (at, _) in text.match_indices("qsTr(") {
            if !starts_word(text, at) {
                continue;
            }
            if let Some(literal) = call_literal(&text[at + "qsTr(".len()..]) {
                found.insert(literal);
            }
        }
        for (at, _) in text.match_indices("qsTranslate(") {
            if !starts_word(text, at) {
                continue;
            }
            let rest = &text[at + "qsTranslate(".len()..];
            let Some((_context, tail)) = leading_literal(rest) else {
                continue;
            };
            let Some(tail) = tail.trim_start().strip_prefix(',') else {
                continue;
            };
            if let Some(literal) = call_literal(tail.trim_start()) {
                found.insert(literal);
            }
        }
    }
    found.into_iter().collect()
}

/// Имя функции стоит отдельным словом: `myqsTr(` — чужая функция.
fn starts_word(text: &str, at: usize) -> bool {
    !text[..at]
        .chars()
        .next_back()
        .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

/// Строковый литерал в начале `rest` и остаток после него.
fn leading_literal(rest: &str) -> Option<(String, &str)> {
    let quote = match rest.chars().next() {
        Some(q @ ('"' | '\'')) => q,
        _ => return None,
    };
    js_string(&rest[quote.len_utf8()..], quote)
}

/// Литерал — аргумент вызова: за ним закрывающая скобка или следующий
/// аргумент (счётчик, комментарий). Иначе это выражение (`qsTr(a + b)`), и
/// строку для каталога из него не вынуть.
fn call_literal(rest: &str) -> Option<String> {
    let (literal, tail) = leading_literal(rest)?;
    matches!(tail.chars().next(), Some(')' | ',')).then_some(literal)
}

/// Разобрать строковый литерал JS до закрывающей кавычки `quote`: вернуть его
/// значение (с раскрытой экранировкой) и остаток текста после кавычки.
///
/// Неизвестная экранировка (`\d`) отдаёт сам символ — как в JS для всего, что
/// не в таблице; шестнадцатеричные формы (`\xNN`, `\uNNNN`) вендор в `qsTr`
/// не использует, и разбирать их было бы догадкой.
fn js_string(body: &str, quote: char) -> Option<(String, &str)> {
    let mut out = String::new();
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == quote {
            return Some((out, &body[i + c.len_utf8()..]));
        }
        if c != '\\' {
            out.push(c);
            continue;
        }
        let (_, escaped) = chars.next()?;
        out.push(match escaped {
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '0' => '\0',
            other => other,
        });
    }
    None
}

/// Строки, для которых в каталоге ЕСТЬ готовый перевод.
///
/// Разбор нарочно грубый — по тегам `<source>` и `<translation>`, без XML-
/// парсера: каталог пишет `lupdate` у вендора, формат у него один и тот же, а
/// лишняя зависимость ради четырёх тегов не окупается. Незавершённый перевод
/// (`type="unfinished"`) переводом НЕ считается: Qt отдаст исходную строку, и
/// покупатель увидит английский.
pub fn catalog_translated(ts: &str) -> BTreeSet<String> {
    let mut done = BTreeSet::new();
    let mut pending: Option<String> = None;
    for (at, _) in ts.match_indices('<') {
        let rest = &ts[at..];
        if let Some(body) = tag_body(rest, "source") {
            pending = Some(xml_unescape(body));
        } else if rest.starts_with("<translation") {
            let head_end = match rest.find('>') {
                Some(i) => i,
                None => continue,
            };
            let unfinished = rest[..head_end].contains("unfinished");
            if let (Some(source), false) = (pending.take(), unfinished) {
                done.insert(source);
            }
        }
    }
    done
}

/// Тело простого тега `<name>…</name>` в начале среза.
fn tag_body<'a>(rest: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let body = rest.strip_prefix(&open)?;
    let end = body.find(&close)?;
    Some(&body[..end])
}

/// Обратное к `xml.sax.saxutils.escape`; `&amp;` разбирается последним, чтобы
/// `&amp;lt;` остался текстом `&lt;`, а не превратился в `<`.
fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Вендорные строки без вендорного перевода — покупатель видит их
/// по-английски (правило #13). Появляются, когда `ui/qml` обновили, а
/// `ui/i18n` нет.
pub fn untranslated_vendor_sources<P: AsRef<str>, T: AsRef<str>>(
    files: &[(P, T)],
    ts: &str,
) -> Vec<String> {
    let translated = catalog_translated(ts);
    vendor_sources(files)
        .into_iter()
        .filter(|source| !translated.contains(source))
        .collect()
}

/// Собрать `waywallen_ru.qm` из ВЕНДОРНОГО каталога.
///
/// `files` — пары «путь → текст» всех `.qml` вендора (в исполнителе это
/// `glob("**/*.qml", recursive=True)` + чтение), `ts` — содержимое вендорного
/// `.ts`. Файлы нужны не для сборки, а для сверки: план заодно докладывает,
/// где каталог и `ui/qml` разошлись. `lrelease` — компилятор каталогов из Qt:
/// есть на машине разработчика, покупателю не нужен — в сборку едет готовый
/// `.qm`.
pub fn plan_build_qm<P: AsRef<str>, T: AsRef<str>>(
    files: &[(P, T)],
    ts: &str,
    ts_path: &str,
    qm_path: &str,
    lrelease: &str,
) -> QmBuildPlan {
    let translated = catalog_translated(ts);
    let sources: BTreeSet<String> = vendor_sources(files).into_iter().collect();
    QmBuildPlan {
        lrelease_argv: vec![
            lrelease.to_string(),
            ts_path.to_string(),
            "-qm".to_string(),
            qm_path.to_string(),
        ],
        untranslated: sources.difference(&translated).cloned().collect(),
        stale: translated.difference(&sources).cloned().collect(),
        ts_path: ts_path.to_string(),
        qm_path: qm_path.to_string(),
    }
}

// ============================================================================
// Язык интерфейса вендорных страниц
// ============================================================================
//
// Порт `ui/src/ui_language.cpp` waywallen. Страница настроек вендора держит
// комбобокс «Язык», а `App` отдаёт ему три члена — `availableUiLanguages`,
// `uiLanguage`, `setUiLanguage()`. Здесь чистая часть их устройства: какие
// языки считать доступными, как привести выбор к поддерживаемому коду и
// какой каталог грузить. Сам `QTranslator`, подписи локалей и пересборка
// привязок QML — за исполнителем (шим `cpp/rexon_we_singletons.cpp`).
//
// ⚠️ Второго механизма языка здесь НЕ заводится: доступные языки — это ровно
// те каталоги `.qm`, которые уже собирает [`plan_build_qm`] из вендорного
// `.ts`. Новое здесь одно — каталог перестал быть намертво русским, и
// прежнее «поставить русский переводчик» (`plan_install_translator`,
// `remote_l10n.install_translator()`) удалено целиком: у переводчика теперь
// один хозяин — синглтон `App`.

/// Ключ выбора языка в `QSettings`. Тот же, что у оригинала
/// (`ui_language.cpp`, `kSettingsKey`): настройка одна на программу, и
/// расходиться ей не с чем.
pub const UI_LANGUAGE_KEY: &str = "ui/language";

/// «Как в системе» — значение по умолчанию (`kSystemLanguage` оригинала).
pub const UI_LANGUAGE_SYSTEM: &str = "system";

/// Английский. Каталога для него нет и не нужно: исходные строки `qsTr()`
/// в вендорном QML английские (`kEnglishLanguage` оригинала).
pub const UI_LANGUAGE_ENGLISH: &str = "en";

/// Подпись английского языка — его собственное самоназвание. У оригинала
/// она тоже записана строкой (`QStringLiteral("English")`), а не берётся у
/// QLocale: каталога для английского нет, и спрашивать локаль не о чем.
pub const UI_LANGUAGE_ENGLISH_LABEL: &str = "English";

/// Начало и конец имени каталога переводов вендора.
const QM_PREFIX: &str = "waywallen_";
const QM_SUFFIX: &str = ".qm";

/// Имя каталога переводов для кода языка: `ru` → `waywallen_ru.qm`.
pub fn qm_file_name(code: &str) -> String {
    format!("{QM_PREFIX}{code}{QM_SUFFIX}")
}

/// Код языка из имени каталога; `None` — файл не наш.
///
/// Ветка `discoverLanguages` оригинала, только там имена приходят из
/// ресурса `:/i18n`, а у нас — из каталога рядом с модулем UI.
pub fn qm_file_language(file_name: &str) -> Option<String> {
    let rest = file_name.strip_prefix(QM_PREFIX)?;
    let code = rest.strip_suffix(QM_SUFFIX)?;
    if code.is_empty() {
        return None;
    }
    Some(code.to_string())
}

/// Языки, у которых рядом с модулем есть собранный каталог.
///
/// Английский отброшен, как у оригинала: его строки и так исходные, и
/// каталог для него означал бы перевод английского на английский. Порядок —
/// по коду (у оригинала `QDir::Name`), повторы снимаются.
pub fn ui_language_codes<S: AsRef<str>>(file_names: &[S]) -> Vec<String> {
    let mut codes: BTreeSet<String> = BTreeSet::new();
    for name in file_names {
        let Some(code) = qm_file_language(name.as_ref()) else {
            continue;
        };
        if base_language(&code) == UI_LANGUAGE_ENGLISH {
            continue;
        }
        codes.insert(code);
    }
    codes.into_iter().collect()
}

/// Базовый язык кода локали: `ru_RU` → `ru`, `zh-CN` → `zh`.
fn base_language(code: &str) -> String {
    let cut = code
        .find(['_', '-', '.', '@'])
        .unwrap_or(code.len());
    code[..cut].to_ascii_lowercase()
}

/// Привести выбор к поддерживаемому коду; `None` — такого языка у нас нет.
///
/// Порядок ровно как в `normalizePreference` оригинала: «как в системе» и
/// английский принимаются всегда, дальше точное совпадение с собранным
/// каталогом, дальше — совпадение по базовому языку (`ru_RU` берёт каталог
/// `ru`, а `zh` — каталог `zh_CN`).
pub fn normalize_ui_language(preference: &str, codes: &[String]) -> Option<String> {
    let pref = preference.trim();
    if pref.is_empty() {
        return None;
    }
    if pref == UI_LANGUAGE_SYSTEM {
        return Some(UI_LANGUAGE_SYSTEM.to_string());
    }
    let base = base_language(pref);
    if base == UI_LANGUAGE_ENGLISH {
        return Some(UI_LANGUAGE_ENGLISH.to_string());
    }
    if let Some(exact) = codes.iter().find(|code| code.as_str() == pref) {
        return Some(exact.clone());
    }
    codes
        .iter()
        .find(|code| base_language(code) == base)
        .cloned()
}

/// Решение [`plan_ui_language`]: что показать и что для этого загрузить.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiLanguagePlan {
    /// Код, который станет выбором покупателя (`system`, `en`, `ru`).
    pub preference: String,
    /// Имя локали Qt для `QTranslator::load` (`ru_RU`, `en`); пусто —
    /// системная локаль.
    pub locale_name: String,
    /// Имя каталога рядом с модулем; пусто — переводить нечем и не нужно.
    pub qm_file: String,
    /// Каталог обязателен: без него смена языка считается неудачной и
    /// вендорный QML показывает тост «Не удалось изменить язык».
    pub required: bool,
}

/// Что сделать, чтобы вендорные страницы заговорили на выбранном языке.
///
/// `system_locale` — имя системной локали Qt (`ru_RU`), его знает только
/// исполнитель. `codes` — ответ [`ui_language_codes`].
///
/// `None` — язык не поддерживается; у оригинала это же значит «вернуть
/// `false` из `setUiLanguage`».
pub fn plan_ui_language(
    preference: &str,
    system_locale: &str,
    codes: &[String],
) -> Option<UiLanguagePlan> {
    let preference = normalize_ui_language(preference, codes)?;
    if preference == UI_LANGUAGE_SYSTEM {
        // «Как в системе» каталога не требует: нет русского каталога —
        // страницы останутся английскими, и это не ошибка.
        let qm_file = normalize_ui_language(system_locale, codes)
            .filter(|code| code != UI_LANGUAGE_SYSTEM && code != UI_LANGUAGE_ENGLISH)
            .map(|code| qm_file_name(&code))
            .unwrap_or_default();
        return Some(UiLanguagePlan {
            preference,
            locale_name: String::new(),
            qm_file,
            required: false,
        });
    }
    if preference == UI_LANGUAGE_ENGLISH {
        return Some(UiLanguagePlan {
            locale_name: preference.clone(),
            preference,
            qm_file: String::new(),
            required: false,
        });
    }
    Some(UiLanguagePlan {
        qm_file: qm_file_name(&preference),
        locale_name: preference.clone(),
        preference,
        required: true,
    })
}

// ============================================================================
// Хелперы путей (семантика os.path)
// ============================================================================

fn join(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        b.to_string()
    } else if a.is_empty() || a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}
