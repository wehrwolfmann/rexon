//! Магазин Cinnamon Spices — ядро источника `spices` для пяти разделов
//! Cinnamon вкладки «Оформление» (апплеты, десклеты, расширения, темы,
//! действия Nemo).
//!
//! Устройство сайта (замер 17.10.2026, `https://cinnamon-spices.linuxmint.com`):
//! * **Индекс** — пять статических JSON (`/json/<тип>s.json`), словарь
//!   «ключ → запись». Ключ — uuid, у тем — имя каталога темы (у части тем
//!   оно расходится с `name`: `Adara-Dark` ↔ «Adara Dark»). Ставить и
//!   включать надо по КЛЮЧУ. Версии Cinnamon в индексе нет — она только в
//!   `metadata.json` внутри архива.
//! * **Архив** — `/files/<тип>s/<ключ>.zip`, внутри каталог `<ключ>/` (у
//!   действий рядом ещё `<ключ>.nemo_action`).
//! * **Нагрузка.** Условий использования нет, перед сайтом стоит WAF. Индекс
//!   берётся не чаще раза в сутки условным запросом (`If-None-Match` /
//!   `If-Modified-Since` → 304), без «ломающего кэш» параметра времени;
//!   картинки — лениво; архив — только по кнопке.
//!
//! Здесь только решения: разбор индекса, витрина, кэш, совместимость с
//! версией Cinnamon, сборка `.mo` из `.po` и ПЛАН установки. Исполнение
//! плана (скачивание, распаковка, копирование, gsettings) — отдельным шагом.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::i18n::{tr, trf};
use crate::models::ThemeItem;

/// Адрес сайта.
pub const HOME_URL: &str = "https://cinnamon-spices.linuxmint.com";
/// Ключ источника в переключателе «Магазин:».
pub const SOURCE_KEY: &str = "spices";
/// Префикс id карточки и записи журнала установленного:
/// `spices:<тип>:<ключ>`.
pub const ID_PREFIX: &str = "spices:";
/// `provider_id` журнала установленного.
pub const PROVIDER_ID: &str = "cinnamon-spices";
/// Срок жизни индекса: не чаще раза в сутки на тип.
pub const CACHE_TTL_SECS: i64 = 24 * 3600;
/// После неудачного похода за индексом (сеть, 5xx, мусор) — не стучаться
/// снова раньше этого срока (кроме кнопки «Обновить»): иначе каждое
/// открытие раздела без сети било бы в сайт заново.
pub const FAIL_BACKOFF_SECS: i64 = 15 * 60;
/// Версия формата файла кэша: выросла — старый файл читается как протухший.
pub const CACHE_VERSION: u32 = 1;
/// Таймаут запроса индекса, секунды (на соединение и на каждое чтение).
pub const INDEX_TIMEOUT_SECS: u64 = 15;
/// Больше этого индекс не читаем: самый крупный (апплеты) весит ~0,7 МБ.
pub const MAX_INDEX_BYTES: u64 = 16 << 20;

/// `User-Agent` запросов — тот же, что у клиента KDE Store.
pub fn user_agent() -> &'static str {
    crate::ocs::USER_AGENT
}

// ═══════════════════════════════════════════════════════════════════════
//  Типы спайсов
// ═══════════════════════════════════════════════════════════════════════

/// Тип спайса. Каталоги установки и способ включения НЕ дублируются здесь:
/// они в матрице `de_customization` (строка по [`SpiceType::kind`]), номер
/// раздела — в `categories::DE_SECTIONS`.
#[derive(Debug, PartialEq, Eq)]
pub struct SpiceType {
    /// `applet`, `desklet`, `extension`, `theme`, `action`.
    pub key: &'static str,
    /// Ключ раздела «Оформления».
    pub section: &'static str,
    /// Имя индекса и каталога на сайте: `/json/<json>.json`, `/files/<json>/`.
    pub json: &'static str,
    /// Главный файл кода, без которого Cinnamon спайс не грузит.
    pub main_js: Option<&'static str>,
    /// Тип для DBus-вызова `org.Cinnamon.ReloadXlet` (только xlet'ы).
    pub xlet: Option<&'static str>,
}

/// Пять типов — по одной строке на раздел.
pub const TYPES: &[SpiceType] = &[
    SpiceType { key: "applet", section: "cinnamonapplets", json: "applets", main_js: Some("applet.js"), xlet: Some("APPLET") },
    SpiceType { key: "desklet", section: "cinnamondesklets", json: "desklets", main_js: Some("desklet.js"), xlet: Some("DESKLET") },
    SpiceType { key: "extension", section: "cinnamonext", json: "extensions", main_js: Some("extension.js"), xlet: Some("EXTENSION") },
    SpiceType { key: "theme", section: "cinnamontheme", json: "themes", main_js: None, xlet: None },
    SpiceType { key: "action", section: "nemoactions", json: "actions", main_js: None, xlet: None },
];

impl SpiceType {
    /// Тип оформления раздела (`cinnamon-applet`, `theme-cinnamon`…) —
    /// из `categories::category_kind`.
    pub fn kind(&self) -> &'static str {
        crate::categories::category_kind(self.section).unwrap_or("")
    }

    /// Номер раздела (909, 915–918) — из `categories::DE_SECTIONS`.
    pub fn cat_id(&self) -> i64 {
        crate::categories::DE_SECTIONS
            .iter()
            .find(|(_, k)| *k == self.section)
            .map(|(id, _)| *id)
            .unwrap_or(0)
    }

    /// Строка матрицы кастомизации этого типа.
    pub fn row(&self) -> Option<&'static crate::de_customization::Row> {
        crate::de_customization::row(self.kind())
    }

    pub fn is_theme(&self) -> bool {
        self.key == "theme"
    }

    pub fn is_action(&self) -> bool {
        self.key == "action"
    }
}

/// Тип по ключу раздела «Оформления».
pub fn type_for_section(section: &str) -> Option<&'static SpiceType> {
    TYPES.iter().find(|t| t.section == section)
}

/// Тип по собственному ключу (`applet`…).
pub fn type_for_key(key: &str) -> Option<&'static SpiceType> {
    TYPES.iter().find(|t| t.key == key)
}

/// Адрес индекса. Без параметра времени: он делает каждый запрос
/// уникальным и ломает кэш сервера.
pub fn index_url(t: &SpiceType) -> String {
    format!("{HOME_URL}/json/{}.json", t.json)
}

/// Страница спайса на сайте («Открыть страницу»). `spices-id` у тем —
/// строка, у остальных — число.
pub fn page_url(t: &SpiceType, spices_id: &Value) -> String {
    let id = match spices_id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    };
    format!("{HOME_URL}/{}/view/{id}", t.json)
}

// ═══════════════════════════════════════════════════════════════════════
//  Индекс
// ═══════════════════════════════════════════════════════════════════════

/// Запись индекса.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Spice {
    /// Ключ словаря индекса: uuid, у тем — имя каталога.
    pub key: String,
    /// Номер (у тем — строка) для ссылки на страницу сайта.
    pub spices_id: Value,
    pub name: String,
    pub description: String,
    /// `name_<яз>` / `description_<яз>`; у тем поля нет.
    pub translations: Map<String, Value>,
    /// Число «лайков» на сайте (не шкала 0–100).
    pub score: i64,
    /// Unix-секунды.
    pub created: i64,
    /// Unix-секунды; это и есть версия для проверки обновлений.
    pub last_edited: i64,
    pub last_commit: String,
    pub last_commit_subject: String,
    /// Путь архива от корня сайта.
    pub file: String,
    /// Размер архива, байты.
    pub file_size: i64,
    /// Путь значка от корня сайта (у тем пусто).
    pub icon: String,
    /// Путь снимка от корня сайта (у действий пусто).
    pub screenshot: String,
    /// Ник автора; `none`/`unknown` сайта здесь уже пусты.
    pub author: String,
}

/// Годится ли ключ в имя каталога: `[A-Za-z0-9@._+-]+`, не `.` и не `..`.
/// Замер 17.10.2026: этому правилу отвечают все 572 ключа сайта.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key != "."
        && key != ".."
        && key.len() <= 255
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b"@._+-".contains(&b))
}

/// Путь от корня сайта, который безопасно приклеить к [`HOME_URL`]:
/// начинается с `/` (иначе `HOME_URL` + `@evil/…` увёл бы на чужой
/// адрес), без `..`, `//`, `?`, `#` и пробелов.
fn site_path_ok(p: &str) -> bool {
    p.starts_with('/')
        && !p.starts_with("//")
        && !p.contains("..")
        && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"@._+-/%".contains(&b))
}

fn str_field(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn int_field(v: &Value, key: &str) -> i64 {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn site_path_field(v: &Value, key: &str) -> String {
    let p = str_field(v, key);
    if site_path_ok(&p) { p } else { String::new() }
}

/// Разобрать индекс. Терпим к пустому `{}`, к отсутствию полей и к записям
/// чужого вида; записи с негодным ключом или путём архива отбрасываются
/// (ключ станет именем каталога, путь — адресом скачивания). Порядок —
/// как в файле.
pub fn parse_index(body: &str) -> Result<Vec<Spice>, String> {
    let root: Value = serde_json::from_str(body).map_err(|e| format!("индекс не разобран: {e}"))?;
    let obj = root.as_object().ok_or_else(|| "индекс — не словарь".to_string())?;
    let mut out = Vec::with_capacity(obj.len());
    for (key, v) in obj {
        if !v.is_object() || !valid_key(key) {
            continue;
        }
        let file = site_path_field(v, "file");
        if file.is_empty() {
            continue;
        }
        let author = str_field(v, "author_user");
        let author = if matches!(author.as_str(), "none" | "unknown") { String::new() } else { author };
        out.push(Spice {
            key: key.clone(),
            spices_id: v.get("spices-id").cloned().unwrap_or(Value::Null),
            name: str_field(v, "name"),
            description: str_field(v, "description"),
            translations: v.get("translations").and_then(Value::as_object).cloned().unwrap_or_default(),
            score: int_field(v, "score"),
            created: int_field(v, "created"),
            last_edited: int_field(v, "last_edited"),
            last_commit: str_field(v, "last_commit"),
            last_commit_subject: str_field(v, "last_commit_subject"),
            file,
            file_size: int_field(v, "file_size"),
            icon: site_path_field(v, "icon"),
            screenshot: site_path_field(v, "screenshot"),
            author,
        });
    }
    Ok(out)
}

/// Язык интерфейса без кодировки и модификатора: `ru_RU.UTF-8@x` → `ru_RU`.
fn bare_lang(lang: &str) -> &str {
    let end = lang.find(['.', '@']).unwrap_or(lang.len());
    &lang[..end]
}

fn translated(s: &Spice, field: &str, lang_long: &str) -> Option<String> {
    let long = bare_lang(lang_long);
    let short = long.split('_').next().unwrap_or(long);
    let mut keys = vec![format!("{field}_{long}")];
    if short != long {
        keys.push(format!("{field}_{short}"));
    }
    keys.iter()
        .filter(|_| !long.is_empty())
        .find_map(|k| s.translations.get(k).and_then(Value::as_str).filter(|t| !t.trim().is_empty()))
        .map(str::to_string)
}

/// Имя и описание на языке интерфейса: сначала `<поле>_<яз_СТРАНА>`
/// (`name_zh_TW`), потом `<поле>_<яз>` (`name_ru`), иначе исходное.
pub fn localized(s: &Spice, lang_long: &str) -> (String, String) {
    (
        translated(s, "name", lang_long).unwrap_or_else(|| s.name.clone()),
        translated(s, "description", lang_long).unwrap_or_else(|| s.description.clone()),
    )
}

/// Страница витрины: поиск (подстрока без учёта регистра в имени,
/// описании — исходных и переведённых, — ключе и авторе), сортировка,
/// срез. `(страница, сколько всего подошло)`.
///
/// Сортировки Rexon → данные сайта: `new` — `last_edited` по убыванию,
/// `high` и `down` — `score` по убыванию (чисел скачиваний у сайта нет),
/// `alpha` — показанное имя. Неизвестная — как `new`. Равные — в порядке
/// индекса.
pub fn filter_sort_page(
    list: &[Spice],
    query: &str,
    sort: &str,
    page: usize,
    size: usize,
    lang: &str,
) -> (Vec<Spice>, usize) {
    let q = query.trim().to_lowercase();
    let mut hits: Vec<(&Spice, String)> = list
        .iter()
        .map(|s| (s, localized(s, lang)))
        .filter(|(s, (name, desc))| {
            q.is_empty()
                || [name.as_str(), desc.as_str(), &s.name, &s.description, &s.key, &s.author]
                    .iter()
                    .any(|f| f.to_lowercase().contains(&q))
        })
        .map(|(s, (name, _))| (s, name))
        .collect();
    match sort {
        "high" | "down" => hits.sort_by(|a, b| b.0.score.cmp(&a.0.score)),
        "alpha" => hits.sort_by_cached_key(|(_, name)| name.to_lowercase()),
        _ => hits.sort_by(|a, b| b.0.last_edited.cmp(&a.0.last_edited)),
    }
    let total = hits.len();
    let size = size.max(1);
    let page = hits.into_iter().skip(page.saturating_mul(size)).take(size).map(|(s, _)| s.clone()).collect();
    (page, total)
}

/// Дата `YYYY.MM.DD` из unix-секунд — так сайт и Cinnamon показывают
/// версию спайса.
pub fn version_label(unix: i64) -> String {
    if unix <= 0 {
        return String::new();
    }
    crate::engine_update::now_iso(unix)[..10].replace('-', ".")
}

fn iso(unix: i64) -> String {
    if unix <= 0 { String::new() } else { crate::engine_update::now_iso(unix) }
}

fn site_url(path: &str) -> String {
    if path.is_empty() { String::new() } else { format!("{HOME_URL}{path}") }
}

/// id карточки: `spices:<тип>:<ключ>`.
pub fn item_id(t: &SpiceType, key: &str) -> String {
    format!("{ID_PREFIX}{}:{key}", t.key)
}

/// Разобрать id карточки обратно: `(тип, ключ)`.
pub fn parse_item_id(id: &str) -> Option<(&'static SpiceType, &str)> {
    let rest = id.strip_prefix(ID_PREFIX)?;
    let (tk, key) = rest.split_once(':')?;
    let t = type_for_key(tk)?;
    valid_key(key).then_some((t, key))
}

/// Лицензия для карточки: у сайта её поля нет, код спайсов лежит в
/// репозиториях Cinnamon Spices под GPL.
pub fn license_label() -> String {
    tr("GPL (Cinnamon Spices repository)")
}

/// Маленькая картинка карточки: у тем — миниатюра сайта (около 7 КБ, а не
/// снимок в 700 КБ), у действий — значок (снимков нет), у остальных —
/// снимок.
pub fn thumb_url(t: &SpiceType, s: &Spice) -> String {
    if t.is_theme() {
        return format!("{HOME_URL}/uploads/themes/thumbs/{}.png", s.key);
    }
    if t.is_action() || s.screenshot.is_empty() {
        return site_url(&s.icon);
    }
    site_url(&s.screenshot)
}

/// Карточка витрины. `rating` и `downloads` нулевые: «лайки» сайта не по
/// шкале 0–100 (их показывают отдельной строкой «★ N»), чисел скачиваний
/// нет.
pub fn theme_item(t: &SpiceType, s: &Spice, lang: &str) -> ThemeItem {
    let (name, description) = localized(s, lang);
    let mut gallery = Vec::new();
    for p in [&s.screenshot, &s.icon] {
        let u = site_url(p);
        if !u.is_empty() && !gallery.contains(&u) {
            gallery.push(u);
        }
    }
    ThemeItem {
        id: item_id(t, &s.key),
        name,
        description,
        author: s.author.clone(),
        license: license_label(),
        version: version_label(s.last_edited),
        rating: 0,
        downloads: 0,
        preview_url: thumb_url(t, s),
        preview_urls: gallery,
        download_url: site_url(&s.file),
        download_name: s.file.rsplit('/').next().unwrap_or("").to_string(),
        download_size: (s.file_size.max(0) + 1023) / 1024,
        category_id: t.cat_id(),
        created: iso(s.created),
        updated: iso(s.last_edited),
        source_name: if localized(s, lang).0 != s.name { s.name.clone() } else { String::new() },
        ..Default::default()
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    fn fixture(json: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/tests/data/cinnamon_spices/{json}.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn load(t: &SpiceType) -> Vec<Spice> {
        parse_index(&fixture(t.json)).unwrap()
    }

    #[test]
    fn types_line_up_with_sections_and_matrix() {
        let want = [
            ("applet", 915, "cinnamon-applet", ".local/share/cinnamon/applets"),
            ("desklet", 916, "cinnamon-desklet", ".local/share/cinnamon/desklets"),
            ("extension", 917, "cinnamon-extension", ".local/share/cinnamon/extensions"),
            ("theme", 909, "theme-cinnamon", ".themes"),
            ("action", 918, "nemo-action", ".local/share/nemo/actions"),
        ];
        assert_eq!(TYPES.len(), want.len());
        for (key, id, kind, dest) in want {
            let t = type_for_key(key).unwrap();
            assert_eq!(t.cat_id(), id, "{key}");
            assert_eq!(t.kind(), kind, "{key}");
            assert_eq!(t.row().unwrap().dest, dest, "{key}");
            assert_eq!(type_for_section(t.section), Some(t));
        }
        assert_eq!(type_for_section("gnomeext"), None);
    }

    #[test]
    fn urls() {
        let a = type_for_key("applet").unwrap();
        let th = type_for_key("theme").unwrap();
        assert_eq!(index_url(a), "https://cinnamon-spices.linuxmint.com/json/applets.json");
        assert!(!index_url(a).contains('?'));
        assert_eq!(page_url(a, &Value::from(17)), "https://cinnamon-spices.linuxmint.com/applets/view/17");
        assert_eq!(page_url(th, &Value::from("Adapta")), "https://cinnamon-spices.linuxmint.com/themes/view/Adapta");
    }

    #[test]
    fn all_five_indexes_parse() {
        for t in TYPES {
            let list = load(t);
            assert_eq!(list.len(), 6, "{}", t.key);
            for s in &list {
                assert!(valid_key(&s.key));
                assert_eq!(s.file, format!("/files/{}/{}.zip", t.json, s.key), "{}", s.key);
                assert!(s.last_edited > 1_500_000_000 || t.is_theme(), "{}", s.key);
                assert!(s.file_size > 0);
                if t.is_theme() {
                    assert!(s.icon.is_empty() && s.translations.is_empty());
                    assert!(s.spices_id.is_string());
                } else {
                    assert_eq!(s.icon, format!("/files/{}/{}.png", t.json, s.key));
                    assert!(s.spices_id.is_number());
                }
                if t.is_action() {
                    assert!(s.screenshot.is_empty());
                }
            }
        }
        let applets = load(type_for_key("applet").unwrap());
        let weather = applets.iter().find(|s| s.key == "weather@mockturtl").unwrap();
        assert_eq!(weather.author, "", "author_user = none прячется");
        assert_eq!(weather.score, 270);
        assert_eq!(weather.screenshot, "/git/applets/weather@mockturtl/screenshot.png");
        let empty = applets.iter().find(|s| s.key == "1440@jvlianodorneles").unwrap();
        assert!(empty.translations.is_empty());
        assert_eq!(empty.author, "jvlianodorneles");
        // Порядок — как в файле.
        assert_eq!(applets[0].key, "1440@jvlianodorneles");
    }

    #[test]
    fn theme_key_differs_from_name() {
        let themes = load(type_for_key("theme").unwrap());
        let adara = themes.iter().find(|s| s.key == "Adara-Dark").unwrap();
        assert_eq!(adara.name, "Adara Dark");
        let th = type_for_key("theme").unwrap();
        let item = theme_item(th, adara, "ru");
        assert_eq!(item.id, "spices:theme:Adara-Dark");
        assert_eq!(item.preview_url, "https://cinnamon-spices.linuxmint.com/uploads/themes/thumbs/Adara-Dark.png");
        assert_eq!(item.download_url, "https://cinnamon-spices.linuxmint.com/files/themes/Adara-Dark.zip");
        assert_eq!(parse_item_id(&item.id), Some((th, "Adara-Dark")));
    }

    #[test]
    fn tolerant_parse_and_hostile_entries() {
        assert_eq!(parse_index("{}").unwrap(), vec![]);
        assert!(parse_index("[]").is_err());
        assert!(parse_index("<html>").is_err());
        let body = r#"{
            "ok@x": {"file": "/files/applets/ok@x.zip"},
            "../evil": {"file": "/files/applets/evil.zip"},
            "host@x": {"file": "@evil.example/x.zip"},
            "dots@x": {"file": "/files/../../x.zip"},
            "num@x": {"file": "/files/applets/num@x.zip", "score": "12", "last_edited": 1.7e9, "icon": "//evil/x.png"},
            "notobj": 5
        }"#;
        let list = parse_index(body).unwrap();
        let keys: Vec<&str> = list.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["ok@x", "num@x"]);
        assert_eq!(list[0].name, "");
        assert_eq!(list[1].score, 12);
        assert_eq!(list[1].last_edited, 1_700_000_000);
        assert_eq!(list[1].icon, "", "адрес на чужой хост отброшен");
    }

    #[test]
    fn valid_key_rule() {
        for k in ["weather@mockturtl", "Adara-Dark", "a+b_c.d", "AlbumArt3.0@claudiux"] {
            assert!(valid_key(k), "{k}");
        }
        for k in ["", ".", "..", "a/b", "a b", "ключ", "a\\b", "a:b"] {
            assert!(!valid_key(k), "{k}");
        }
    }

    #[test]
    fn localized_prefers_long_then_short() {
        let mut s = Spice { name: "Weather".into(), description: "D".into(), ..Default::default() };
        s.translations.insert("name_zh".into(), "zh".into());
        s.translations.insert("name_zh_TW".into(), "zh_TW".into());
        s.translations.insert("name_ru".into(), "Погода".into());
        s.translations.insert("description_ru".into(), "  ".into());
        assert_eq!(localized(&s, "zh_TW").0, "zh_TW");
        assert_eq!(localized(&s, "zh_CN").0, "zh");
        assert_eq!(localized(&s, "ru_RU.UTF-8"), ("Погода".to_string(), "D".to_string()), "пустой перевод не берётся");
        assert_eq!(localized(&s, "ru").0, "Погода");
        assert_eq!(localized(&s, "de").0, "Weather");
        assert_eq!(localized(&s, "").0, "Weather");

        // Настоящая запись: zh_CN и zh_TW рядом.
        let applets = load(type_for_key("applet").unwrap());
        let a = applets.iter().find(|s| s.key == "AutostartPrograms@spacy01").unwrap();
        let tw = a.translations["name_zh_TW"].as_str().unwrap();
        let cn = a.translations["name_zh_CN"].as_str().unwrap();
        assert_eq!(localized(a, "zh_TW").0, tw);
        assert_eq!(localized(a, "zh_CN").0, cn);
        let w = applets.iter().find(|s| s.key == "weather@mockturtl").unwrap();
        assert_eq!(localized(w, "ru").0, w.translations["name_ru"].as_str().unwrap());
    }

    #[test]
    fn sorting_and_search() {
        let applets = load(type_for_key("applet").unwrap());
        let keys = |v: &[Spice]| v.iter().map(|s| s.key.clone()).collect::<Vec<_>>();

        let (top, total) = filter_sort_page(&applets, "", "high", 0, 2, "en");
        assert_eq!(total, 6);
        assert_eq!(top[0].key, "weather@mockturtl");
        assert!(top[0].score >= top[1].score);
        assert_eq!(keys(&filter_sort_page(&applets, "", "down", 0, 10, "en").0), keys(&filter_sort_page(&applets, "", "high", 0, 10, "en").0));

        let (new, _) = filter_sort_page(&applets, "", "new", 0, 10, "en");
        assert!(new.windows(2).all(|w| w[0].last_edited >= w[1].last_edited));
        assert_eq!(keys(&filter_sort_page(&applets, "", "???", 0, 10, "en").0), keys(&new));

        let (alpha, _) = filter_sort_page(&applets, "", "alpha", 0, 10, "en");
        let names: Vec<String> = alpha.iter().map(|s| s.name.to_lowercase()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);

        // Поиск: по исходному имени, по переводу, по ключу, по автору.
        assert_eq!(keys(&filter_sort_page(&applets, "WEATHER", "new", 0, 10, "ru").0), ["weather@mockturtl"]);
        let w = applets.iter().find(|s| s.key == "weather@mockturtl").unwrap();
        let ru = localized(w, "ru").0;
        assert_eq!(keys(&filter_sort_page(&applets, &ru, "new", 0, 10, "ru").0), ["weather@mockturtl"]);
        assert_eq!(keys(&filter_sort_page(&applets, "mockturtl", "new", 0, 10, "en").0), ["weather@mockturtl"]);
        assert_eq!(filter_sort_page(&applets, "jvlianodorneles", "new", 0, 10, "en").1, 1);
        assert_eq!(filter_sort_page(&applets, "no-such-thing", "new", 0, 10, "en").1, 0);

        // Срез страниц.
        let (p1, t) = filter_sort_page(&applets, "", "new", 1, 4, "en");
        assert_eq!((p1.len(), t), (2, 6));
        assert!(filter_sort_page(&applets, "", "new", 9, 4, "en").0.is_empty());
    }

    #[test]
    fn theme_item_field_by_field() {
        let a = type_for_key("applet").unwrap();
        let w = load(a).into_iter().find(|s| s.key == "weather@mockturtl").unwrap();
        let item = theme_item(a, &w, "ru");
        let (name, desc) = localized(&w, "ru");
        let want = ThemeItem {
            id: "spices:applet:weather@mockturtl".into(),
            name: name.clone(),
            description: desc,
            author: String::new(),
            license: license_label(),
            version: version_label(w.last_edited),
            rating: 0,
            downloads: 0,
            preview_url: "https://cinnamon-spices.linuxmint.com/git/applets/weather@mockturtl/screenshot.png".into(),
            preview_urls: vec![
                "https://cinnamon-spices.linuxmint.com/git/applets/weather@mockturtl/screenshot.png".into(),
                "https://cinnamon-spices.linuxmint.com/files/applets/weather@mockturtl.png".into(),
            ],
            download_url: "https://cinnamon-spices.linuxmint.com/files/applets/weather@mockturtl.zip".into(),
            download_name: "weather@mockturtl.zip".into(),
            download_size: (1_519_150 + 1023) / 1024,
            category_id: 915,
            created: crate::engine_update::now_iso(w.created),
            updated: crate::engine_update::now_iso(w.last_edited),
            source_name: "Weather".into(),
            ..Default::default()
        };
        assert_eq!(item, want);
        assert_ne!(name, "Weather");
        assert_eq!(item.version.len(), 10);
        assert_eq!(version_label(1_784_985_259), "2026.07.25");
        assert_eq!(version_label(0), "");

        // Действие: картинка — значок, снимка нет.
        let act = type_for_key("action").unwrap();
        let b = load(act).into_iter().find(|s| s.key == "backup-this-file@h3nr1ke").unwrap();
        let item = theme_item(act, &b, "en");
        assert_eq!(item.preview_url, "https://cinnamon-spices.linuxmint.com/files/actions/backup-this-file@h3nr1ke.png");
        assert_eq!(item.preview_urls, vec![item.preview_url.clone()]);
        assert_eq!(item.category_id, 918);
        assert_eq!(item.source_name, "");
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Совместимость с версией Cinnamon
// ═══════════════════════════════════════════════════════════════════════

/// Версия `major.minor[.patch]`; недостающая часть — 0. `6.6.9` →
/// `(6, 6, 9)`, `6.6` → `(6, 6, 0)`. Хвост после чисел (`6.6.9-1.1`,
/// `6.6.9+mint`) отбрасывается.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim();
    let core = s.split(|c: char| !(c.is_ascii_digit() || c == '.')).next().unwrap_or("");
    let mut parts = core.split('.').filter(|p| !p.is_empty()).map(|p| p.parse::<u32>().ok());
    let major = parts.next()??;
    let minor = parts.next()??;
    let patch = parts.next().unwrap_or(Some(0)).unwrap_or(0);
    Some((major, minor, patch))
}

/// Версия установленного Cinnamon: `cinnamon --version` («Cinnamon 6.6.9»,
/// второе слово), иначе версия пакета (pacman, dpkg, rpm). `run` —
/// запуск команды, отдаёт stdout при успехе.
pub fn running_cinnamon_version(run: &dyn Fn(&[&str]) -> Option<String>) -> Option<(u32, u32, u32)> {
    if let Some(v) = run(&["cinnamon", "--version"]).and_then(|o| o.split_whitespace().nth(1).and_then(parse_version)) {
        return Some(v);
    }
    let probes: &[(&[&str], usize)] = &[
        (&["pacman", "-Q", "cinnamon"], 1),
        (&["dpkg-query", "-W", "-f=${Version}", "cinnamon"], 0),
        (&["rpm", "-q", "--qf", "%{VERSION}", "cinnamon"], 0),
    ];
    for (argv, word) in probes {
        if let Some(v) = run(argv).and_then(|o| o.split_whitespace().nth(*word).and_then(parse_version)) {
            return Some(v);
        }
    }
    None
}

/// [`running_cinnamon_version`] на живой машине.
pub fn live_cinnamon_version() -> Option<(u32, u32, u32)> {
    running_cinnamon_version(&|argv: &[&str]| {
        let out = std::process::Command::new(argv[0]).args(&argv[1..]).stdin(std::process::Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    })
}

/// Начальные цифры строки как число (`"6abc"` → 6, `"x"` → нет) — так
/// Cinnamon читает части версий из `metadata.json`.
fn leading_int(s: &str) -> Option<u32> {
    let s = s.trim_start();
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// `cinnamon-version` — список МИНИМАЛЬНЫХ версий: совместимо, если хоть
/// одна запись `major.minor` не новее текущей. Пустой список — несовместимо
/// (так его понимает и сама оболочка, и её окно настроек); неразборная
/// запись не подходит.
pub fn version_check(required: &[String], current: (u32, u32, u32)) -> bool {
    required.iter().any(|r| {
        let mut it = r.split('.');
        let (Some(maj), Some(min)) = (it.next().and_then(leading_int), it.next().and_then(leading_int)) else {
            return false;
        };
        current.0 > maj || (current.0 == maj && current.1 >= min)
    })
}

/// Список `cinnamon-version` из `metadata.json`: `None` — поля нет.
/// Числа и одиночная строка тоже принимаются (встречаются у старых
/// спайсов).
pub fn cinnamon_versions(meta: &Value) -> Option<Vec<String>> {
    let v = meta.get("cinnamon-version")?;
    let one = |x: &Value| match x {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    Some(match v {
        Value::Array(a) => a.iter().filter_map(one).collect(),
        other => one(other).into_iter().collect(),
    })
}

fn version_dir(name: &str) -> Option<(u32, u32, u32)> {
    // ^[1-9][0-9]*\.[0-9]+(\.[0-9]+)?$
    let parts: Vec<&str> = name.split('.').collect();
    if !(2..=3).contains(&parts.len())
        || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
        || parts[0].starts_with('0')
    {
        return None;
    }
    parse_version(name)
}

/// Подкаталог мультиверсионного спайса, который возьмёт Cinnamon: среди
/// имён вида `3.8` / `4.0.1` — наибольшее, не новее текущей версии
/// (`3.8` = `3.8.0`). `None` — код лежит в корне.
///
/// Части сравниваются как ЧИСЛА. Сама оболочка 6.6.9 при выборе сравнивает
/// их как строки (`"10" < "8"`), окно настроек — как числа; расходятся они
/// только на частях от 10 и выше, которых в каталоге сегодня нет.
pub fn pick_version_subdir(subdirs: &[String], current: (u32, u32, u32)) -> Option<String> {
    subdirs
        .iter()
        .filter_map(|n| version_dir(n).map(|v| (v, n)))
        .filter(|(v, _)| *v <= current)
        // Равные (`6.6` и `6.6.0`) — более подробное имя, как у окна настроек.
        .max_by_key(|(v, n)| (*v, n.len()))
        .map(|(_, n)| n.clone())
}

#[cfg(test)]
mod version_tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parse_versions() {
        assert_eq!(parse_version("6.6.9"), Some((6, 6, 9)));
        assert_eq!(parse_version("6.6"), Some((6, 6, 0)));
        assert_eq!(parse_version("6.6.9-1.1"), Some((6, 6, 9)));
        assert_eq!(parse_version(" 5.8.4+wilma\n"), Some((5, 8, 4)));
        assert_eq!(parse_version("6"), None);
        assert_eq!(parse_version("Cinnamon"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn running_version_probes_in_order() {
        let calls = std::cell::RefCell::new(Vec::new());
        let run = |argv: &[&str]| {
            calls.borrow_mut().push(argv[0].to_string());
            match argv[0] {
                "cinnamon" => Some("Cinnamon 6.6.9\n".to_string()),
                _ => None,
            }
        };
        assert_eq!(running_cinnamon_version(&run), Some((6, 6, 9)));
        assert_eq!(calls.borrow().as_slice(), ["cinnamon"]);

        let pkg = |argv: &[&str]| match argv[0] {
            "pacman" => Some("cinnamon 6.6.9-1.1\n".to_string()),
            _ => None,
        };
        assert_eq!(running_cinnamon_version(&pkg), Some((6, 6, 9)));
        let deb = |argv: &[&str]| match argv[0] {
            "dpkg-query" => Some("6.4.13+zara".to_string()),
            _ => None,
        };
        assert_eq!(running_cinnamon_version(&deb), Some((6, 4, 13)));
        assert_eq!(running_cinnamon_version(&|_: &[&str]| None), None);
    }

    /// Таблица случаев с поведением оболочки 6.6.9.
    #[test]
    fn version_check_table() {
        let cur = (6, 6, 9);
        let weather = s(&["2.2", "2.4", "2.6", "2.8", "3.0", "3.2", "3.4", "3.6", "3.8", "4.0", "4.2", "4.4", "4.6", "4.8"]);
        assert!(version_check(&weather, cur), "последняя 4.8 ≤ 6.6");
        assert!(!version_check(&s(&["6.8"]), cur));
        assert!(version_check(&s(&["6.6"]), cur));
        assert!(version_check(&s(&["6.6"]), (6, 6, 0)));
        assert!(!version_check(&s(&["6.6"]), (6, 4, 13)));
        assert!(version_check(&s(&["5.8", "7.0"]), cur), "хватает одной подходящей");
        assert!(version_check(&s(&["6.6.12"]), cur), "третья часть не сравнивается");
        assert!(!version_check(&s(&[]), cur), "пустой список — несовместимо");
        assert!(!version_check(&s(&["abc", "6"]), cur));
        assert!(version_check(&s(&["4.x2", "6.0beta"]), cur), "как parseInt: начальные цифры");
    }

    #[test]
    fn versions_from_metadata() {
        let m: Value = serde_json::from_str(r#"{"cinnamon-version": ["3.6", 4.0, "6.6"]}"#).unwrap();
        assert_eq!(cinnamon_versions(&m), Some(s(&["3.6", "4.0", "6.6"])));
        let one: Value = serde_json::from_str(r#"{"cinnamon-version": "6.0"}"#).unwrap();
        assert_eq!(cinnamon_versions(&one), Some(s(&["6.0"])));
        assert_eq!(cinnamon_versions(&serde_json::json!({})), None);
    }

    #[test]
    fn subdir_table() {
        let cur = (6, 6, 9);
        // weather@mockturtl на 6.6.9 берёт 3.8.
        assert_eq!(pick_version_subdir(&s(&["3.0", "po", "icons", "3.8", "arrow-icons"]), cur).as_deref(), Some("3.8"));
        assert_eq!(pick_version_subdir(&s(&["po", "icons"]), cur), None);
        assert_eq!(pick_version_subdir(&s(&["6.8", "7.0"]), cur), None, "все новее — корень");
        assert_eq!(pick_version_subdir(&s(&["6.6", "6.6.9", "6.6.10"]), cur).as_deref(), Some("6.6.9"));
        assert_eq!(pick_version_subdir(&s(&["6.6", "6.6.0"]), (6, 6, 0)).as_deref(), Some("6.6.0"));
        assert_eq!(pick_version_subdir(&s(&["6.6.0", "6.6"]), (6, 6, 0)).as_deref(), Some("6.6.0"));
        assert_eq!(pick_version_subdir(&s(&["5.4", "6.10", "6.2"]), cur).as_deref(), Some("6.2"), "6.10 > 6.6 по числам");
        assert_eq!(pick_version_subdir(&s(&["4.0", "4.10"]), (4, 10, 0)).as_deref(), Some("4.10"));
        for bad in ["06.0", "6", "6.", ".6", "6.0.1.2", "6.a", "v6.0"] {
            assert_eq!(pick_version_subdir(&s(&[bad]), cur), None, "{bad}");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  .po → .mo
// ═══════════════════════════════════════════════════════════════════════
//
// Переводы спайсов приходят исходниками `po/<яз>.po`, а gettext читает
// только собранный `.mo`. Собираем сами, без внешнего `msgfmt` (его может
// не быть в системе). Раскладка файла — по описанию формата в руководстве
// GNU gettext («The Format of GNU MO Files»): шапка, две таблицы
// (длина, смещение), таблица хэшей, строки с завершающим нулём. Совпадение
// с `msgfmt` 1.0 побайтовое (сверка в тестах на настоящих .po спайсов).

/// Одна запись каталога: ключ (`контекст\x04msgid[\0msgid_plural]`) и
/// перевод (формы через `\0`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct PoEntry {
    key: Vec<u8>,
    value: Vec<u8>,
}

/// Строка в кавычках: `"…"` с C-экранированием.
fn po_unquote(s: &str, line_no: usize) -> Result<Vec<u8>, String> {
    let s = s.trim();
    let inner = s
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .filter(|_| s.len() >= 2)
        .ok_or_else(|| format!("строка {line_no}: ожидалась строка в кавычках"))?;
    let b = inner.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'"' {
            return Err(format!("строка {line_no}: кавычка внутри строки"));
        }
        if c != b'\\' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        let e = *b.get(i).ok_or_else(|| format!("строка {line_no}: обрыв экранирования"))?;
        i += 1;
        match e {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'v' => out.push(11),
            b'\\' | b'"' | b'\'' | b'?' => out.push(e),
            b'0'..=b'7' => {
                let mut v = u32::from(e - b'0');
                for _ in 0..2 {
                    match b.get(i) {
                        Some(d @ b'0'..=b'7') => {
                            v = v * 8 + u32::from(d - b'0');
                            i += 1;
                        }
                        _ => break,
                    }
                }
                out.push(v as u8);
            }
            b'x' => {
                let start = i;
                let mut v: u32 = 0;
                while let Some(d) = b.get(i).and_then(|d| (*d as char).to_digit(16)) {
                    v = (v << 4 | d) & 0xff;
                    i += 1;
                }
                if i == start {
                    return Err(format!("строка {line_no}: \\x без цифр"));
                }
                out.push(v as u8);
            }
            other => return Err(format!("строка {line_no}: неизвестное экранирование \\{}", other as char)),
        }
    }
    Ok(out)
}

#[derive(Default)]
struct PoDraft {
    ctx: Option<Vec<u8>>,
    id: Option<Vec<u8>>,
    plural: Option<Vec<u8>>,
    strs: Vec<(usize, Vec<u8>)>,
    fuzzy: bool,
    started_at: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum PoField {
    Ctx,
    Id,
    Plural,
    Str(usize),
}

/// Разобрать `.po` в записи, которые попадут в `.mo`: без устаревших
/// (`#~`), без неточных (`#, fuzzy`, кроме шапки) и без непереведённых
/// (пустая первая форма). Порядок — как в файле.
fn parse_po(po: &str) -> Result<Vec<PoEntry>, String> {
    let mut out = Vec::new();
    let mut d = PoDraft::default();
    let mut field: Option<PoField> = None;
    let mut fuzzy_pending = false;

    fn finish(d: &mut PoDraft, out: &mut Vec<PoEntry>) -> Result<(), String> {
        let draft = std::mem::take(d);
        let Some(id) = draft.id else {
            if draft.ctx.is_some() || !draft.strs.is_empty() {
                return Err(format!("строка {}: запись без msgid", draft.started_at));
            }
            return Ok(());
        };
        if draft.strs.is_empty() {
            return Err(format!("строка {}: запись без msgstr", draft.started_at));
        }
        let mut forms = draft.strs;
        forms.sort_by_key(|(n, _)| *n);
        if forms.iter().enumerate().any(|(i, (n, _))| i != *n) {
            return Err(format!("строка {}: формы msgstr[] не по порядку", draft.started_at));
        }
        if draft.plural.is_none() && forms.len() != 1 {
            return Err(format!("строка {}: msgstr[] без msgid_plural", draft.started_at));
        }
        let is_header = id.is_empty() && draft.ctx.is_none();
        if (draft.fuzzy && !is_header) || forms[0].1.is_empty() {
            return Ok(());
        }
        let mut key = Vec::new();
        if let Some(c) = &draft.ctx {
            key.extend_from_slice(c);
            key.push(4);
        }
        key.extend_from_slice(&id);
        if let Some(p) = &draft.plural {
            key.push(0);
            key.extend_from_slice(p);
        }
        let mut value = Vec::new();
        for (i, (_, f)) in forms.iter().enumerate() {
            if i > 0 {
                value.push(0);
            }
            value.extend_from_slice(f);
        }
        if is_header {
            // Как у msgfmt: дата создания шаблона в собранный каталог не идёт
            // (иначе одна и та же сборка давала бы разные файлы).
            let text = String::from_utf8_lossy(&value).into_owned();
            let kept: String = text.split_inclusive('\n').filter(|l| !l.starts_with("POT-Creation-Date:")).collect();
            value = kept.into_bytes();
        }
        out.push(PoEntry { key, value });
        Ok(())
    }

    for (n, raw) in po.lines().enumerate() {
        let line_no = n + 1;
        let line = raw.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            if line.starts_with("#~") || line.starts_with("#|") {
                continue;
            }
            if d.id.is_some() && !d.strs.is_empty() {
                finish(&mut d, &mut out)?;
                field = None;
            }
            if let Some(flags) = line.strip_prefix("#,") {
                if flags.split(',').any(|f| f.trim() == "fuzzy") {
                    fuzzy_pending = true;
                }
            }
            continue;
        }
        if line.starts_with('"') {
            let bytes = po_unquote(line, line_no)?;
            let target = match field {
                Some(PoField::Ctx) => d.ctx.as_mut(),
                Some(PoField::Id) => d.id.as_mut(),
                Some(PoField::Plural) => d.plural.as_mut(),
                Some(PoField::Str(i)) => d.strs.iter_mut().find(|(k, _)| *k == i).map(|(_, v)| v),
                None => None,
            }
            .ok_or_else(|| format!("строка {line_no}: продолжение без ключевого слова"))?;
            target.extend_from_slice(&bytes);
            continue;
        }
        let (kw, rest) = line.split_once(|c: char| c.is_whitespace() || c == '"').map_or((line, ""), |(k, _)| (k, &line[k.len()..]));
        let value = po_unquote(rest, line_no)?;
        let new_entry = matches!(kw, "msgctxt" | "msgid") && (!d.strs.is_empty() || (kw == "msgctxt" && d.id.is_some()));
        if new_entry {
            finish(&mut d, &mut out)?;
        }
        if d.ctx.is_none() && d.id.is_none() && d.strs.is_empty() {
            d.started_at = line_no;
            d.fuzzy = std::mem::take(&mut fuzzy_pending);
        }
        match kw {
            "msgctxt" => {
                if d.ctx.is_some() || d.id.is_some() {
                    return Err(format!("строка {line_no}: лишний msgctxt"));
                }
                d.ctx = Some(value);
                field = Some(PoField::Ctx);
            }
            "msgid" => {
                if d.id.is_some() {
                    return Err(format!("строка {line_no}: второй msgid подряд"));
                }
                d.id = Some(value);
                field = Some(PoField::Id);
            }
            "msgid_plural" => {
                if d.id.is_none() || d.plural.is_some() || !d.strs.is_empty() {
                    return Err(format!("строка {line_no}: msgid_plural не на месте"));
                }
                d.plural = Some(value);
                field = Some(PoField::Plural);
            }
            _ => {
                let idx = if kw == "msgstr" {
                    if d.plural.is_some() {
                        return Err(format!("строка {line_no}: у формы по числу нужен msgstr[N]"));
                    }
                    0
                } else if let Some(num) = kw.strip_prefix("msgstr[").and_then(|r| r.strip_suffix(']')) {
                    if d.plural.is_none() {
                        return Err(format!("строка {line_no}: msgstr[N] без msgid_plural"));
                    }
                    num.parse::<usize>().map_err(|_| format!("строка {line_no}: номер формы «{num}»"))?
                } else {
                    return Err(format!("строка {line_no}: неизвестное ключевое слово «{kw}»"));
                };
                if d.id.is_none() || d.strs.iter().any(|(k, _)| *k == idx) {
                    return Err(format!("строка {line_no}: msgstr не на месте"));
                }
                d.strs.push((idx, value));
                field = Some(PoField::Str(idx));
            }
        }
    }
    finish(&mut d, &mut out)?;
    Ok(out)
}

/// Хэш строки для таблицы `.mo` (из описания формата GNU gettext).
fn mo_hash(s: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &c in s {
        h = (h << 4).wrapping_add(u32::from(c));
        let g = h & 0xf000_0000;
        if g != 0 {
            h ^= g >> 24;
            h ^= g;
        }
    }
    h
}

fn next_prime(mut n: u32) -> u32 {
    let is_prime = |n: u32| n >= 2 && (2..).take_while(|d| d * d <= n).all(|d| n % d != 0);
    n |= 1;
    while !is_prime(n) {
        n += 2;
    }
    n
}

/// Собрать `.mo` (порядок байтов — младший первым) из текста `.po`.
/// Ошибка — файл испорчен (повтор msgid, мусорная строка, обрыв
/// экранирования): такой язык пропускается, остальные ставятся.
pub fn po_to_mo(po: &str) -> Result<Vec<u8>, String> {
    let mut entries = parse_po(po)?;
    // Ключ поиска — до первого нуля (без msgid_plural).
    let lookup = |e: &PoEntry| -> Vec<u8> { e.key.split(|b| *b == 0).next().unwrap_or(&[]).to_vec() };
    entries.sort_by_key(lookup);
    if let Some(w) = entries.windows(2).find(|w| lookup(&w[0]) == lookup(&w[1])) {
        return Err(format!("повтор записи «{}»", String::from_utf8_lossy(&lookup(&w[0]))));
    }
    let n = entries.len() as u32;
    let hash_size = if n == 0 { 0 } else { next_prime(n * 4 / 3).max(3) };
    let orig_at: u32 = 28;
    let trans_at = orig_at + n * 8;
    let hash_at = trans_at + n * 8;
    let mut strings_at = hash_at + hash_size * 4;

    let mut orig_table = Vec::new();
    let mut trans_table = Vec::new();
    let mut blob = Vec::new();
    for e in &entries {
        orig_table.push((e.key.len() as u32, strings_at));
        strings_at += e.key.len() as u32 + 1;
    }
    for e in &entries {
        trans_table.push((e.value.len() as u32, strings_at));
        strings_at += e.value.len() as u32 + 1;
    }
    for e in &entries {
        blob.extend_from_slice(&e.key);
        blob.push(0);
    }
    for e in &entries {
        blob.extend_from_slice(&e.value);
        blob.push(0);
    }
    let mut hash = vec![0u32; hash_size as usize];
    if hash_size > 2 {
        for (i, e) in entries.iter().enumerate() {
            let h = mo_hash(&lookup(e));
            let mut idx = h % hash_size;
            let incr = 1 + h % (hash_size - 2);
            while hash[idx as usize] != 0 {
                idx = if idx >= hash_size - incr { idx - (hash_size - incr) } else { idx + incr };
            }
            hash[idx as usize] = i as u32 + 1;
        }
    }

    let mut out = Vec::with_capacity(strings_at as usize);
    for w in [0x9504_12de_u32, 0, n, orig_at, trans_at, hash_size, hash_at] {
        out.extend_from_slice(&w.to_le_bytes());
    }
    for (len, off) in orig_table.iter().chain(trans_table.iter()) {
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&off.to_le_bytes());
    }
    for h in hash {
        out.extend_from_slice(&h.to_le_bytes());
    }
    out.extend_from_slice(&blob);
    Ok(out)
}

/// Код языка из имени `.po`: всё до первой точки (`pt_BR.po` → `pt_BR`).
/// `None` — имя не годится в каталог (`../x.po`, пустое, не `.po`).
pub fn po_language(file_name: &str) -> Option<&str> {
    if !file_name.ends_with(".po") {
        return None;
    }
    let lang = file_name.split('.').next().unwrap_or("");
    let ok = !lang.is_empty() && lang.bytes().all(|b| b.is_ascii_alphanumeric() || b"_@-".contains(&b));
    ok.then_some(lang)
}

#[cfg(test)]
mod po_tests {
    use super::*;
    use crate::i18n::Catalog;

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/po/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    const FIXTURES: &[&str] = &["powerman-cs.po", "1440-da.po", "backup-uk.po", "albumart-tr_TR.po"];

    const EDGE: &str = r#"
# шапка без пометок
msgid ""
msgstr ""
"Project-Id-Version: x\n"
"POT-Creation-Date: 2025-01-01 00:00+0000\n"
"Content-Type: text/plain; charset=UTF-8\n"
"Plural-Forms: nplurals=3; plural=(n==1 ? 0 : n>=2 && n<=4 ? 1 : 2);\n"

msgid "allempty"
msgid_plural "allempties"
msgstr[0] ""
msgstr[1] ""
msgstr[2] ""

msgid "partempty"
msgid_plural "partempties"
msgstr[0] "a"
msgstr[1] ""
msgstr[2] "c"

msgid "firstempty"
msgid_plural "firstempties"
msgstr[0] ""
msgstr[1] "b"
msgstr[2] "c"

msgctxt "ctx"
msgid ""
msgstr "ctx-empty-id"

msgctxt "menu"
msgid "Open"
msgstr "Открыть (меню)"

msgid "Open"
msgstr "Открыть"

#, fuzzy, javascript-format
msgid "fuzzy one"
msgstr "неточно"

msgid "esc"
msgstr "a\tb\\c\"d\101\x42"

msgid ""
"multi "
"line"
msgstr ""
"много"
"строк"

#~ msgid "obsolete"
#~ msgstr "old"
"#;

    #[test]
    fn roundtrip_through_our_reader() {
        let cat = Catalog::parse_mo(&po_to_mo(EDGE).unwrap()).unwrap();
        assert_eq!(cat.get(None, "Open"), Some("Открыть"));
        assert_eq!(cat.get(Some("menu"), "Open"), Some("Открыть (меню)"));
        assert_eq!(cat.get(Some("ctx"), ""), Some("ctx-empty-id"));
        assert_eq!(cat.get(None, "esc"), Some("a\tb\\c\"dAB"));
        assert_eq!(cat.get(None, "multi line"), Some("многострок"));
        assert_eq!(cat.get(None, "fuzzy one"), None, "неточное не берётся");
        assert_eq!(cat.get(None, "obsolete"), None);
        assert_eq!(cat.get(None, "allempty"), None);
        assert_eq!(cat.get(None, "firstempty"), None, "первая форма пуста — не переведено");
        assert_eq!(cat.plural_lookup("partempty", 1).as_deref(), Some("a"));
        assert_eq!(cat.plural_lookup("partempty", 7).as_deref(), Some("c"));
        assert_eq!(cat.nplurals(), 3);

        for name in FIXTURES {
            let po = fixture(name);
            let cat = Catalog::parse_mo(&po_to_mo(&po).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
            // Каждая непустая нефаззи-запись нашлась.
            let entries = parse_po(&po).unwrap();
            assert!(entries.len() > 1, "{name}");
            for e in entries.iter().filter(|e| !e.key.is_empty()) {
                let key = String::from_utf8(e.key.clone()).unwrap();
                let (ctx, id) = match key.split_once('\u{4}') {
                    Some((c, i)) => (Some(c), i),
                    None => (None, key.as_str()),
                };
                let id = id.split('\0').next().unwrap();
                let first = String::from_utf8(e.value.split(|b| *b == 0).next().unwrap().to_vec()).unwrap();
                assert_eq!(cat.get(ctx, id), Some(first.as_str()), "{name}: {id}");
            }
        }
        let cs = Catalog::parse_mo(&po_to_mo(&fixture("powerman-cs.po")).unwrap()).unwrap();
        assert_eq!(cs.plural_lookup("hour", 1).as_deref(), Some("hodina"));
        assert_eq!(cs.plural_lookup("hour", 3).as_deref(), Some("hodiny"));
        assert_eq!(cs.plural_lookup("hour", 5).as_deref(), Some("hodin"));
        assert_eq!(cs.get(Some("time of battery remaining"), "%d:%02d"), Some("%d:%02d"));
        // Неточные выкинуты: в tr_TR их четыре, шапка — сохранена.
        let tr_po = fixture("albumart-tr_TR.po");
        let kept = parse_po(&tr_po).unwrap();
        assert!(kept.iter().any(|e| e.key.is_empty()));
        let headers = kept.iter().find(|e| e.key.is_empty()).unwrap();
        assert!(!String::from_utf8_lossy(&headers.value).contains("POT-Creation-Date"));
    }

    /// Побайтовая сверка с `msgfmt` (если он есть в системе).
    #[test]
    fn byte_identical_with_msgfmt() {
        let Ok(probe) = std::process::Command::new("msgfmt").arg("--version").output() else {
            eprintln!("msgfmt нет — сверка НЕ ПРОВЕРЕНА");
            return;
        };
        if !probe.status.success() {
            eprintln!("msgfmt не запускается — сверка НЕ ПРОВЕРЕНА");
            return;
        }
        let dir = std::env::temp_dir().join(format!("rexon-spices-po-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut cases: Vec<(String, String)> = FIXTURES.iter().map(|n| (n.to_string(), fixture(n))).collect();
        cases.push(("edge.po".into(), EDGE.into()));
        for (name, po) in cases {
            let src = dir.join(&name);
            let dst = dir.join(format!("{name}.mo"));
            std::fs::write(&src, &po).unwrap();
            let st = std::process::Command::new("msgfmt").arg("-o").arg(&dst).arg(&src).status().unwrap();
            assert!(st.success(), "{name}: msgfmt отказал");
            let want = std::fs::read(&dst).unwrap();
            let got = po_to_mo(&po).unwrap();
            assert_eq!(got, want, "{name}: .mo расходится с msgfmt");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ручной прогон: каждый `.po` под `REXON_SPICES_PO_DIR` (например
    /// распакованные архивы спайсов) против `msgfmt`, побайтово.
    #[test]
    #[ignore]
    fn msgfmt_on_directory() {
        let Ok(root) = std::env::var("REXON_SPICES_PO_DIR") else { return };
        let out = std::env::temp_dir().join(format!("rexon-spices-po-dir-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        let mut stack = vec![std::path::PathBuf::from(root)];
        let (mut same, mut total) = (0, 0);
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.extension().and_then(|x| x.to_str()) != Some("po") {
                    continue;
                }
                total += 1;
                let dst = out.join("x.mo");
                let ok = std::process::Command::new("msgfmt").arg("-o").arg(&dst).arg(&p).status().unwrap().success();
                let ours = po_to_mo(&std::fs::read_to_string(&p).unwrap());
                match (ok, ours) {
                    (true, Ok(m)) if m == std::fs::read(&dst).unwrap() => same += 1,
                    (false, Err(_)) => same += 1,
                    (ok, ours) => eprintln!("РАСХОДИТСЯ {}: msgfmt={ok} наш={:?}", p.display(), ours.map(|m| m.len())),
                }
            }
        }
        let _ = std::fs::remove_dir_all(&out);
        eprintln!("совпало {same} из {total}");
        assert_eq!(same, total);
    }

    #[test]
    fn broken_po_is_an_error() {
        for bad in [
            "msgid \"a\"\nmsgstr \"b\"\nmsgid \"a\"\nmsgstr \"c\"\n",
            "msgid \"a\"\nmsgstr \"b\\q\"\n",
            "msgid \"a\nmsgstr \"b\"\n",
            "garbage line\n",
            "msgid \"a\"\n",
            "msgid \"a\"\nmsgstr[0] \"b\"\n",
            "msgid \"a\"\nmsgid_plural \"as\"\nmsgstr \"b\"\n",
            "\"orphan\"\n",
        ] {
            assert!(po_to_mo(bad).is_err(), "{bad:?}");
        }
        assert!(po_to_mo("").unwrap().len() == 28, "пустой каталог — одна шапка");
    }

    #[test]
    fn language_from_file_name() {
        assert_eq!(po_language("pt_BR.po"), Some("pt_BR"));
        assert_eq!(po_language("tr_TR.po"), Some("tr_TR"));
        assert_eq!(po_language("sr@latin.po"), Some("sr@latin"));
        assert_eq!(po_language("ru.utf8.po"), Some("ru"));
        assert_eq!(po_language("weather@mockturtl.pot"), None);
        assert_eq!(po_language(".po"), None);
        assert_eq!(po_language("..po"), None);
        assert_eq!(po_language("a b.po"), None);
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Кэш индекса
// ═══════════════════════════════════════════════════════════════════════

/// Файл кэша одного индекса: тело как пришло плюс то, что нужно условному
/// запросу.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CachedIndex {
    pub version: u32,
    /// Когда индекс последний раз подтверждён сервером (200 или 304),
    /// unix-секунды. 0 — ни разу.
    pub fetched: i64,
    /// Когда последний поход за индексом не удался (0 — не было).
    pub failed: i64,
    pub etag: String,
    pub last_modified: String,
    /// Тело индекса (JSON сайта) как есть.
    pub body: String,
}

/// Путь кэша: `$XDG_CACHE_HOME/rexon/cinnamon-spices/<json>.json`.
pub fn cache_path(xdg_cache_home: Option<&str>, home: &str, t: &SpiceType) -> String {
    format!("{}/rexon/cinnamon-spices/{}.json", crate::utils::cache_home(xdg_cache_home, home), t.json)
}

/// [`cache_path`] этого процесса.
pub fn live_cache_path(t: &SpiceType) -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    cache_path(std::env::var("XDG_CACHE_HOME").ok().as_deref(), &home, t)
}

/// Прочитать кэш. Чужая версия, битый файл и тело, которое не
/// разбирается, — как будто кэша нет (то есть он протух).
pub fn load_cache(path: &Path) -> Option<(CachedIndex, Vec<Spice>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let c: CachedIndex = serde_json::from_str(&text).ok()?;
    if c.version != CACHE_VERSION {
        return None;
    }
    let list = if c.body.is_empty() { None } else { parse_index(&c.body).ok() };
    match list {
        Some(list) => Some((c, list)),
        // Тела нет (только отметка неудачи) — валидаторы не годятся.
        None => Some((CachedIndex { etag: String::new(), last_modified: String::new(), body: String::new(), fetched: 0, ..c }, Vec::new())),
    }
}

/// Записать кэш (через временный файл — половинчатого не бывает).
pub fn save_cache(path: &Path, c: &CachedIndex) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string(c).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Ответ на условный запрос.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CondReply {
    /// 304: у нас последняя версия.
    NotModified,
    /// 200 с телом и новыми валидаторами (пустые — сервер их не дал).
    Body { body: Vec<u8>, etag: String, last_modified: String },
}

/// Транспорт условного GET. В общем [`crate::net::NetIo`] нет ни заголовков
/// ответа, ни заголовков запроса на вызов, а без них `ETag`/`304` не
/// сделать — поэтому здесь свой узкий трейт: в сверках он отвечает
/// записанными ответами, в бою — [`UreqConditional`].
pub trait ConditionalNet {
    /// `GET url` с `If-None-Match`/`If-Modified-Since` (пустые не
    /// посылаются). Ответ вне 2xx и не 304 — `NetError::Status`.
    fn conditional_get(
        &mut self,
        url: &str,
        user_agent: &str,
        etag: &str,
        last_modified: &str,
        timeout_secs: u64,
    ) -> Result<CondReply, crate::net::NetError>;
}

/// Чем кончилось обновление индекса.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshSource {
    /// Кэш свежий, в сеть не ходили.
    Cache,
    /// Сервер ответил 304 — кэш продлён.
    NotModified,
    /// Пришёл новый индекс.
    Downloaded,
    /// Сеть/сервер подвели — показан прежний кэш (возможно, пустой).
    Stale,
}

/// Итог [`refresh`].
#[derive(Debug, Clone, PartialEq)]
pub struct Refresh {
    pub spices: Vec<Spice>,
    pub source: RefreshSource,
    /// Когда список последний раз подтверждён сервером (0 — ни разу).
    pub fetched: i64,
    /// Что сказать человеку; пусто — всё в порядке.
    pub note: String,
}

/// Сети нет / сервер не ответил.
pub fn offline_message(have_saved: bool) -> String {
    if have_saved {
        tr("Couldn't reach the Cinnamon Spices website — the saved list is shown.")
    } else {
        tr("Couldn't reach the Cinnamon Spices website. Check your internet connection and try again.")
    }
}

/// Индекс для показа. Свежий кэш (моложе суток) — без сети. Иначе (или по
/// `force` — кнопка «Обновить») — условный запрос с валидаторами кэша:
/// 304 продлевает кэш, 200 заменяет его. Сбой сети, ответ сервера с
/// ошибкой или мусор вместо JSON — прежний кэш с пометкой, а повтор не
/// раньше [`FAIL_BACKOFF_SECS`] (кроме `force`).
pub fn refresh(net: &mut dyn ConditionalNet, path: &Path, t: &SpiceType, force: bool, now: i64) -> Refresh {
    let cached = load_cache(path);
    let (meta, saved) = cached.unwrap_or_else(|| (CachedIndex { version: CACHE_VERSION, ..Default::default() }, Vec::new()));
    let have_body = !meta.body.is_empty();
    let fresh = have_body && meta.fetched > 0 && now >= meta.fetched && now - meta.fetched < CACHE_TTL_SECS;
    let backing_off = meta.failed > 0 && now >= meta.failed && now - meta.failed < FAIL_BACKOFF_SECS;
    if !force && (fresh || backing_off) {
        let note = if fresh { String::new() } else { offline_message(have_body) };
        let source = if fresh { RefreshSource::Cache } else { RefreshSource::Stale };
        return Refresh { spices: saved, source, fetched: meta.fetched, note };
    }

    let (etag, lm) = if have_body { (meta.etag.as_str(), meta.last_modified.as_str()) } else { ("", "") };
    let reply = net.conditional_get(&index_url(t), user_agent(), etag, lm, INDEX_TIMEOUT_SECS);
    let fail = |meta: CachedIndex, saved: Vec<Spice>| {
        let have = !meta.body.is_empty();
        let fetched = meta.fetched;
        let _ = save_cache(path, &CachedIndex { failed: now, ..meta });
        Refresh { spices: saved, source: RefreshSource::Stale, fetched, note: offline_message(have) }
    };
    match reply {
        Ok(CondReply::NotModified) if have_body => {
            let _ = save_cache(path, &CachedIndex { fetched: now, failed: 0, ..meta });
            Refresh { spices: saved, source: RefreshSource::NotModified, fetched: now, note: String::new() }
        }
        Ok(CondReply::Body { body, etag, last_modified }) => {
            let text = String::from_utf8_lossy(&body).into_owned();
            match parse_index(&text) {
                Ok(list) => {
                    let c = CachedIndex { version: CACHE_VERSION, fetched: now, failed: 0, etag, last_modified, body: text };
                    let _ = save_cache(path, &c);
                    Refresh { spices: list, source: RefreshSource::Downloaded, fetched: now, note: String::new() }
                }
                // Мусор (страница ошибки вместо JSON) кэш не затирает.
                Err(_) => fail(meta, saved),
            }
        }
        // 304 без кэша (мы валидаторов не посылали) — ответ странный.
        Ok(CondReply::NotModified) | Err(_) => fail(meta, saved),
    }
}

/// Боевой транспорт условного запроса: `ureq` с теми же настройками, что
/// у общего сетевого слоя (`crate::net::UreqNet`): системное хранилище
/// корней через native-tls, таймаут на соединение и каждое чтение, до 10
/// переадресаций. Без `Accept-Encoding` — сервер отдаёт как есть.
pub struct UreqConditional {
    agent: Option<(u64, ureq::Agent)>,
    /// Больше этого тело не читается (после распаковки).
    limit: u64,
    /// Просить сжатый ответ (`Accept-Encoding: gzip`) и распаковывать его:
    /// дерево репозитория тем у GitHub весит ~9 МБ, сжатое — ~1,2 МБ.
    gzip: bool,
}

impl Default for UreqConditional {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqConditional {
    pub fn new() -> Self {
        UreqConditional { agent: None, limit: MAX_INDEX_BYTES, gzip: false }
    }

    /// Сжатый ответ и свой потолок размера (для [`THEME_TREE_URL`]).
    pub fn gzip_up_to(limit: u64) -> Self {
        UreqConditional { agent: None, limit, gzip: true }
    }

    fn agent(&mut self, timeout_secs: u64) -> ureq::Agent {
        if let Some((t, a)) = &self.agent {
            if *t == timeout_secs {
                return a.clone();
            }
        }
        let t = std::time::Duration::from_secs(timeout_secs);
        let config = ureq::Agent::config_builder()
            .timeout_connect(Some(t))
            .timeout_send_request(Some(t))
            .timeout_recv_response(Some(t))
            .timeout_recv_body(Some(t))
            .max_redirects(10)
            .http_status_as_error(false)
            .tls_config(
                ureq::tls::TlsConfig::builder()
                    .provider(ureq::tls::TlsProvider::NativeTls)
                    .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                    .build(),
            )
            .build();
        let agent = ureq::Agent::new_with_config(config);
        self.agent = Some((timeout_secs, agent.clone()));
        agent
    }
}

impl ConditionalNet for UreqConditional {
    fn conditional_get(
        &mut self,
        url: &str,
        user_agent: &str,
        etag: &str,
        last_modified: &str,
        timeout_secs: u64,
    ) -> Result<CondReply, crate::net::NetError> {
        use crate::net::NetError;
        use std::io::Read;
        let mut req = self.agent(timeout_secs).get(url).header("User-Agent", user_agent);
        if !etag.is_empty() {
            req = req.header("If-None-Match", etag);
        }
        if !last_modified.is_empty() {
            req = req.header("If-Modified-Since", last_modified);
        }
        if self.gzip {
            req = req.header("Accept-Encoding", "gzip");
        }
        let resp = req.call().map_err(|e| NetError::Transport { detail: format!("<urlopen error {e}>") })?;
        let code = resp.status().as_u16();
        if code == 304 {
            return Ok(CondReply::NotModified);
        }
        if !(200..300).contains(&code) {
            return Err(NetError::Status { code, reason: resp.status().canonical_reason().unwrap_or("").to_string() });
        }
        let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or("").trim().to_string();
        let (etag, last_modified) = (header("etag"), header("last-modified"));
        let gzipped = self.gzip && header("content-encoding").eq_ignore_ascii_case("gzip");
        let limit = self.limit;
        let mut body = Vec::new();
        let reader = resp.into_body().into_reader();
        let read = if gzipped {
            flate2::read::GzDecoder::new(reader.take(limit)).take(limit).read_to_end(&mut body)
        } else {
            reader.take(limit).read_to_end(&mut body)
        };
        read.map_err(|e| NetError::Transport { detail: e.to_string() })?;
        Ok(CondReply::Body { body, etag, last_modified })
    }
}

/// [`refresh`] на живой машине (сеть, кэш этого процесса).
pub fn live_refresh(t: &SpiceType, force: bool) -> Refresh {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    refresh(&mut UreqConditional::new(), Path::new(&live_cache_path(t)), t, force, now)
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use crate::net::NetError;

    /// Записанные ответы по порядку; запросы складываются для сверки.
    struct Replay {
        answers: Vec<Result<CondReply, NetError>>,
        asked: Vec<(String, String, String, String)>,
    }

    impl ConditionalNet for Replay {
        fn conditional_get(&mut self, url: &str, ua: &str, etag: &str, lm: &str, _t: u64) -> Result<CondReply, NetError> {
            self.asked.push((url.into(), ua.into(), etag.into(), lm.into()));
            assert!(!self.answers.is_empty(), "лишний запрос к сайту");
            self.answers.remove(0)
        }
    }

    fn replay(answers: Vec<Result<CondReply, NetError>>) -> Replay {
        Replay { answers, asked: Vec::new() }
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-spices-cache-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("rexon/cinnamon-spices/applets.json")
    }

    fn body() -> Vec<u8> {
        std::fs::read(format!("{}/tests/data/cinnamon_spices/applets.json", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    fn ok200(etag: &str) -> Result<CondReply, NetError> {
        Ok(CondReply::Body { body: body(), etag: etag.into(), last_modified: "Fri, 16 Oct 2026 22:15:06 GMT".into() })
    }

    const T0: i64 = 1_792_000_000;

    fn applets() -> &'static SpiceType {
        type_for_key("applet").unwrap()
    }

    #[test]
    fn path_layout() {
        assert_eq!(cache_path(None, "/home/u", applets()), "/home/u/.cache/rexon/cinnamon-spices/applets.json");
        assert_eq!(cache_path(Some("/c"), "/home/u", type_for_key("theme").unwrap()), "/c/rexon/cinnamon-spices/themes.json");
    }

    #[test]
    fn first_fetch_then_day_of_silence_then_304() {
        let p = tmp("flow");
        let mut net = replay(vec![ok200("W/\"ddfd-1\"")]);
        let r = refresh(&mut net, &p, applets(), false, T0);
        assert_eq!((r.source, r.spices.len(), r.fetched), (RefreshSource::Downloaded, 6, T0));
        assert!(r.note.is_empty());
        assert_eq!(net.asked[0], ("https://cinnamon-spices.linuxmint.com/json/applets.json".into(), "Rexon/5.0".into(), String::new(), String::new()));

        // Моложе суток — ни одного запроса.
        let mut quiet = replay(vec![]);
        let r = refresh(&mut quiet, &p, applets(), false, T0 + CACHE_TTL_SECS - 1);
        assert_eq!((r.source, r.spices.len()), (RefreshSource::Cache, 6));
        assert!(quiet.asked.is_empty());

        // Сутки прошли — условный запрос с валидаторами; 304 продлевает.
        let mut net = replay(vec![Ok(CondReply::NotModified)]);
        let r = refresh(&mut net, &p, applets(), false, T0 + CACHE_TTL_SECS);
        assert_eq!((r.source, r.spices.len(), r.fetched), (RefreshSource::NotModified, 6, T0 + CACHE_TTL_SECS));
        assert_eq!(net.asked[0].2, "W/\"ddfd-1\"");
        assert_eq!(net.asked[0].3, "Fri, 16 Oct 2026 22:15:06 GMT");
        assert!(!net.asked[0].0.contains('?'), "без параметра времени");
        let (c, _) = load_cache(&p).unwrap();
        assert_eq!(c.fetched, T0 + CACHE_TTL_SECS);

        // «Обновить» — в сеть даже при свежем кэше; новое тело заменяет.
        let mut net = replay(vec![ok200("W/\"ddfd-2\"")]);
        let r = refresh(&mut net, &p, applets(), true, T0 + CACHE_TTL_SECS + 5);
        assert_eq!(r.source, RefreshSource::Downloaded);
        assert_eq!(load_cache(&p).unwrap().0.etag, "W/\"ddfd-2\"");
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn server_error_and_broken_connection_keep_old_cache_and_back_off() {
        for (name, err) in [
            ("500", NetError::Status { code: 500, reason: "Internal Server Error".into() }),
            ("eof", NetError::Transport { detail: "<urlopen error connection closed>".into() }),
        ] {
            let p = tmp(name);
            refresh(&mut replay(vec![ok200("e1")]), &p, applets(), false, T0);
            let later = T0 + CACHE_TTL_SECS + 10;
            let mut net = replay(vec![Err(err)]);
            let r = refresh(&mut net, &p, applets(), false, later);
            assert_eq!((r.source, r.spices.len(), r.fetched), (RefreshSource::Stale, 6, T0), "{name}");
            assert_eq!(r.note, offline_message(true));
            // Сразу снова — не стучимся (пауза после неудачи).
            let mut quiet = replay(vec![]);
            let r = refresh(&mut quiet, &p, applets(), false, later + 60);
            assert_eq!((r.source, r.spices.len()), (RefreshSource::Stale, 6));
            assert!(quiet.asked.is_empty());
            // «Обновить» пробует сразу; после паузы — тоже.
            let mut net = replay(vec![Ok(CondReply::NotModified)]);
            assert_eq!(refresh(&mut net, &p, applets(), true, later + 61).source, RefreshSource::NotModified);
            let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap().parent().unwrap());
        }
    }

    #[test]
    fn garbage_body_does_not_replace_cache() {
        let p = tmp("garbage");
        refresh(&mut replay(vec![ok200("e1")]), &p, applets(), false, T0);
        let later = T0 + CACHE_TTL_SECS;
        let html = Ok(CondReply::Body { body: b"<html>Access Denied - Sucuri Website Firewall</html>".to_vec(), etag: "x".into(), last_modified: String::new() });
        let r = refresh(&mut replay(vec![html]), &p, applets(), false, later);
        assert_eq!((r.source, r.spices.len()), (RefreshSource::Stale, 6));
        let (c, list) = load_cache(&p).unwrap();
        assert_eq!((c.etag.as_str(), list.len(), c.failed), ("e1", 6, later));
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn no_cache_and_no_network() {
        let p = tmp("offline");
        let mut net = replay(vec![Err(NetError::Transport { detail: "dns".into() })]);
        let r = refresh(&mut net, &p, applets(), false, T0);
        assert_eq!((r.source, r.spices.len(), r.fetched), (RefreshSource::Stale, 0, 0));
        assert_eq!(r.note, offline_message(false));
        // Отметка неудачи не мешает потом скачать начисто, без валидаторов.
        let mut net = replay(vec![ok200("e")]);
        let r = refresh(&mut net, &p, applets(), false, T0 + FAIL_BACKOFF_SECS);
        assert_eq!(r.source, RefreshSource::Downloaded);
        assert_eq!((net.asked[0].2.as_str(), net.asked[0].3.as_str()), ("", ""));
        let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap().parent().unwrap());
    }

    #[test]
    fn broken_cache_file_counts_as_expired() {
        for junk in ["not json", r#"{"version": 99, "fetched": 1, "body": "{}"}"#, r#"{"version": 1, "fetched": 1792000000, "etag": "e", "body": "[broken"}"#] {
            let p = tmp("junk");
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, junk).unwrap();
            let mut net = replay(vec![ok200("e2")]);
            let r = refresh(&mut net, &p, applets(), false, T0 + 1);
            assert_eq!(r.source, RefreshSource::Downloaded, "{junk}");
            assert_eq!(net.asked[0].2, "", "валидаторы испорченного кэша не посылаются: {junk}");
            let _ = std::fs::remove_dir_all(p.parent().unwrap().parent().unwrap().parent().unwrap());
        }
    }

    /// Боевой транспорт на местном сервере: заголовки запроса и разбор
    /// 200 / 304 / 500.
    #[test]
    fn live_transport_sends_validators_and_reads_them_back() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let answers = [
                "HTTP/1.1 200 OK\r\nETag: W/\"a1\"\r\nLast-Modified: Fri, 16 Oct 2026 22:15:06 GMT\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                "HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n",
                "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            ];
            let mut heads = Vec::new();
            for a in answers {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = vec![0u8; 4096];
                let mut got = Vec::new();
                while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = s.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    got.extend_from_slice(&buf[..n]);
                }
                heads.push(String::from_utf8_lossy(&got).to_lowercase());
                s.write_all(a.as_bytes()).unwrap();
            }
            heads
        });
        let url = format!("http://127.0.0.1:{port}/json/applets.json");
        let mut net = UreqConditional::new();
        let first = net.conditional_get(&url, "Rexon/5.0", "", "", 5).unwrap();
        assert_eq!(first, CondReply::Body { body: b"{}".to_vec(), etag: "W/\"a1\"".into(), last_modified: "Fri, 16 Oct 2026 22:15:06 GMT".into() });
        assert_eq!(net.conditional_get(&url, "Rexon/5.0", "W/\"a1\"", "Fri, 16 Oct 2026 22:15:06 GMT", 5).unwrap(), CondReply::NotModified);
        assert!(matches!(net.conditional_get(&url, "Rexon/5.0", "", "", 5), Err(NetError::Status { code: 500, .. })));
        let heads = server.join().unwrap();
        assert!(heads[0].contains("user-agent: rexon/5.0"));
        assert!(!heads[0].contains("if-none-match") && !heads[0].contains("if-modified-since"));
        assert!(!heads[0].contains("accept-encoding"), "сжатие не просим");
        assert!(heads[1].contains("if-none-match: w/\"a1\""));
        assert!(heads[1].contains("if-modified-since: fri, 16 oct 2026 22:15:06 gmt"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Проверка архива и план установки
// ═══════════════════════════════════════════════════════════════════════

use crate::installed_scan::FsSnapshot;

/// Почему спайс не ставится. Текст для человека — [`SpiceRefusal::message`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpiceRefusal {
    /// Ключ из индекса не годится в имя каталога.
    InvalidKey,
    /// Запись архива уводит за каталог распаковки (`../`, абсолютный путь).
    UnsafePath { entry: String },
    /// Внутри нет каталога `<ключ>/`.
    NotZipLayout,
    /// Нет `metadata.json` (xlet'ы и действия).
    NoMetadata,
    /// `metadata.json` не разбирается как объект JSON.
    BadMetadata,
    /// `uuid` в `metadata.json` не совпадает с каталогом: Cinnamon такой не грузит.
    UuidMismatch { found: String },
    /// Нет обязательного поля `metadata.json` (`name`, `description`).
    MissingProperty { name: String },
    /// Ни одна из версий `cinnamon-version` не подходит.
    Incompatible { need: Vec<String>, current: (u32, u32, u32) },
    /// У расширения нет `cinnamon-version`: Cinnamon такое не грузит.
    MissingCinnamonVersion,
    /// Нет главного файла (`applet.js`…) в каталоге, который возьмёт Cinnamon.
    NoMainJs { file: String },
    /// У темы нет `cinnamon/cinnamon.css`.
    NoCinnamonCss,
    /// У действия нет `<uuid>.nemo_action`.
    NoActionFile,
}

/// «Нужен Cinnamon N или новее, а здесь M» — отказ на установке
/// несовместимого спайса (решение Wehrwolfmann 17.10.2026: в витрине
/// показываются все, проверка — при установке).
pub fn incompatible_message(name: &str, need: &[String], current: (u32, u32, u32)) -> String {
    let lowest = need
        .iter()
        .filter_map(|v| parse_version(v).map(|p| (p, v)))
        .min_by_key(|(p, _)| *p)
        .map(|(_, v)| v.clone());
    let cur = format!("{}.{}.{}", current.0, current.1, current.2);
    match lowest {
        Some(v) if parse_version(&v).is_some_and(|p| p > current) => trf(
            "%1 needs Cinnamon %2 or newer, and this computer has Cinnamon %3. Update Cinnamon or choose another item.",
            &[&name, &v, &cur],
        ),
        _ => trf(
            "%1 doesn't support Cinnamon %2 (the version on this computer), so Cinnamon wouldn't load it.",
            &[&name, &cur],
        ),
    }
}

impl SpiceRefusal {
    /// Объяснение для человека; `name` — имя спайса на карточке.
    pub fn message(&self, name: &str) -> String {
        match self {
            SpiceRefusal::InvalidKey => trf("%1 has a name that can't be used as a folder name, so it can't be installed.", &[&name]),
            SpiceRefusal::UnsafePath { entry } => {
                trf("The archive of %1 contains an unsafe path (%2), so it wasn't installed.", &[&name, entry])
            }
            SpiceRefusal::NotZipLayout => trf("The archive of %1 isn't laid out the way Cinnamon expects, so it wasn't installed.", &[&name]),
            SpiceRefusal::NoMetadata => trf("%1 is damaged: its description file metadata.json is missing.", &[&name]),
            SpiceRefusal::BadMetadata => trf("%1 is damaged: its description file metadata.json can't be read.", &[&name]),
            SpiceRefusal::UuidMismatch { found } => trf(
                "%1 is damaged: its description names it «%2», so Cinnamon wouldn't load it.",
                &[&name, found],
            ),
            SpiceRefusal::MissingProperty { name: field } => trf(
                "%1 is damaged: its description file has no «%2» field, so Cinnamon wouldn't load it.",
                &[&name, field],
            ),
            SpiceRefusal::Incompatible { need, current } => incompatible_message(name, need, *current),
            SpiceRefusal::MissingCinnamonVersion => trf(
                "%1 doesn't say which Cinnamon versions it supports, and Cinnamon doesn't load such extensions.",
                &[&name],
            ),
            SpiceRefusal::NoMainJs { file } => trf("%1 is damaged: the file %2 is missing.", &[&name, file]),
            SpiceRefusal::NoCinnamonCss => trf("%1 isn't a Cinnamon theme: it has no cinnamon/cinnamon.css.", &[&name]),
            SpiceRefusal::NoActionFile => trf("%1 is damaged: the Nemo action file is missing.", &[&name]),
        }
    }
}

/// Все записи архива остаются внутри каталога распаковки. Проверять ДО
/// распаковки (та же проверка, что у общего распаковщика оформления).
pub fn check_entry_names(names: &[String]) -> Result<(), SpiceRefusal> {
    const PROBE: &str = "/rexon-spice-extract";
    match names.iter().find(|n| n.contains('\\') || !crate::appearance_installer::safe_extract_entry(PROBE, n)) {
        Some(bad) => Err(SpiceRefusal::UnsafePath { entry: bad.clone() }),
        None => Ok(()),
    }
}

/// Что в архиве нашлось — для плана.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// `metadata.json` (у темы без него — пустой объект).
    pub meta: Value,
    /// Подкаталог версии, который возьмёт Cinnamon (`3.8`), либо корень.
    pub version_subdir: Option<String>,
    /// Имена `po/<яз>.po` в корне спайса, по алфавиту (у тем — пусто).
    pub po_files: Vec<String>,
}

fn snap_join(a: &str, b: &str) -> String {
    format!("{}/{b}", a.trim_end_matches('/'))
}

/// Проверить распакованный архив (`root` — каталог распаковки, в нём
/// `<ключ>/`). `current` — версия Cinnamon на машине; `None` (Cinnamon не
/// найден) — версия не проверяется, главный файл ищется в корне и в
/// подкаталогах версий.
pub fn check_layout(
    fs: &FsSnapshot,
    root: &str,
    t: &SpiceType,
    key: &str,
    current: Option<(u32, u32, u32)>,
) -> Result<Layout, SpiceRefusal> {
    if !valid_key(key) {
        return Err(SpiceRefusal::InvalidKey);
    }
    let dir = snap_join(root, key);
    if !fs.is_dir(&dir) {
        return Err(SpiceRefusal::NotZipLayout);
    }
    let meta_path = snap_join(&dir, "metadata.json");
    let meta: Value = match fs.read_text(&meta_path) {
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(v) if v.is_object() => v,
            _ => return Err(SpiceRefusal::BadMetadata),
        },
        None if t.is_theme() => Value::Object(Map::new()),
        None => return Err(SpiceRefusal::NoMetadata),
    };

    if t.is_theme() {
        if !fs.is_file(&snap_join(&dir, "cinnamon/cinnamon.css")) {
            return Err(SpiceRefusal::NoCinnamonCss);
        }
        return Ok(Layout { meta, version_subdir: None, po_files: Vec::new() });
    }

    let uuid = meta.get("uuid").and_then(Value::as_str).unwrap_or("");
    if uuid != key {
        return Err(SpiceRefusal::UuidMismatch { found: uuid.to_string() });
    }
    if t.is_action() && !fs.is_file(&snap_join(root, &format!("{key}.nemo_action"))) {
        return Err(SpiceRefusal::NoActionFile);
    }

    let mut version_subdir = None;
    if let Some(main) = t.main_js {
        for field in ["name", "description"] {
            if meta.get(field).is_none() {
                return Err(SpiceRefusal::MissingProperty { name: field.to_string() });
            }
        }
        match (cinnamon_versions(&meta), current) {
            (None, _) if t.key == "extension" => return Err(SpiceRefusal::MissingCinnamonVersion),
            (Some(need), Some(cur)) if !version_check(&need, cur) => {
                return Err(SpiceRefusal::Incompatible { need, current: cur });
            }
            _ => {}
        }
        let subdirs: Vec<String> = fs.scandir(&dir).iter().filter(|n| fs.is_dir(&snap_join(&dir, n))).cloned().collect();
        let candidates: Vec<Option<String>> = match current {
            Some(cur) => vec![pick_version_subdir(&subdirs, cur)],
            // Версия неизвестна: годится любой подкаталог версии или корень.
            None => {
                let mut c: Vec<Option<String>> = subdirs.iter().filter(|n| version_dir(n).is_some()).cloned().map(Some).collect();
                c.sort_by_key(|n| std::cmp::Reverse(n.as_deref().and_then(version_dir)));
                c.push(None);
                c
            }
        };
        let found = candidates.iter().find(|sub| {
            let code = sub.as_deref().map_or(dir.clone(), |s| snap_join(&dir, s));
            fs.is_file(&snap_join(&code, main))
        });
        match found {
            Some(sub) => version_subdir = sub.clone(),
            None => {
                let shown = candidates[0].as_deref().map_or(main.to_string(), |s| format!("{s}/{main}"));
                return Err(SpiceRefusal::NoMainJs { file: shown });
            }
        }
    }

    let po_dir = snap_join(&dir, "po");
    let mut po_files: Vec<String> = fs
        .scandir(&po_dir)
        .iter()
        .filter(|n| po_language(n).is_some() && fs.is_file(&snap_join(&po_dir, n)))
        .cloned()
        .collect();
    po_files.sort();
    Ok(Layout { meta, version_subdir, po_files })
}

/// Где живёт человек: всё ставится под `home` (как и ищет Cinnamon:
/// `~/.local/share/cinnamon/…`, `~/.themes`, `~/.local/share/nemo/actions`,
/// переводы — `~/.local/share/locale`). Настройки спайсов Cinnamon держит
/// в каталоге настроек XDG (`$XDG_CONFIG_HOME`, обычно `~/.config`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiceEnv {
    pub home: String,
    /// Каталог настроек XDG: `$XDG_CONFIG_HOME` либо `~/.config`.
    pub config: String,
}

impl SpiceEnv {
    /// Под домашним каталогом `home`, настройки — в `~/.config`.
    pub fn at_home(home: &str) -> Self {
        let home = home.trim_end_matches('/').to_string();
        SpiceEnv { config: format!("{home}/.config"), home }
    }

    /// Под `home`: живой каталог настроек, если это домашний каталог
    /// процесса, иначе `home/.config`.
    pub fn for_home(home: &str) -> Self {
        let live = Self::live();
        if live.home == home.trim_end_matches('/') {
            live
        } else {
            Self::at_home(home)
        }
    }

    /// Живая машина: `$HOME` и `$XDG_CONFIG_HOME` (пустой — как не заданный).
    pub fn live() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        let mut env = Self::at_home(&home);
        if let Some(c) = std::env::var("XDG_CONFIG_HOME").ok().filter(|c| c.starts_with('/')) {
            env.config = c.trim_end_matches('/').to_string();
        }
        env
    }
}

/// Обстоятельства установки.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InstallContext {
    /// Спайс уже стоит (каталог есть) — это обновление.
    pub is_update: bool,
    /// Спайс сейчас включён (есть в списке включённых).
    pub enabled: bool,
    /// Версия Cinnamon на машине.
    pub cinnamon: Option<(u32, u32, u32)>,
}

/// Шаг плана установки — один побочный эффект для исполнителя.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum SpiceStep {
    /// `mkdir -p`.
    MakeDirs { path: String },
    /// Удалить каталог, если есть.
    RemoveTree { path: String },
    /// Удалить файл, если есть.
    RemoveFile { path: String },
    /// Скопировать дерево (права файлов — как в архиве).
    CopyTree { from: String, to: String },
    /// Скопировать файл.
    CopyFile { from: String, to: String },
    /// Собрать `.mo` из `.po` ([`po_to_mo`]); неудача — строка в журнал и
    /// пропуск языка, установка продолжается.
    CompilePo { po: String, mo: String },
    /// Дописать `"last-edited": ts` в `metadata.json` (порядок прочих
    /// ключей сохранить, отступ 4). `create` — файла может не быть (тема):
    /// тогда создать из одного этого поля.
    WriteLastEdited { meta: String, ts: i64, create: bool },
    /// Добавить строку в список gsettings, если её там нет.
    GsettingsListAdd { schema: String, key: String, item: String },
    /// Убрать из списка gsettings все записи спайса ([`list_without`]):
    /// `field` — номер поля записи через `:` (у апплетов 3, у десклетов 0),
    /// `None` — запись целиком (расширения, действия Nemo).
    GsettingsListDrop { schema: String, key: String, item: String, field: Option<u32> },
    /// Сбросить ключ gsettings к умолчанию, если он сейчас равен `value`
    /// (снимаемая тема оболочки выбрана сейчас).
    GsettingsResetIf { schema: String, key: String, value: String },
    /// Удалить каталог, только если он пуст (каталоги языков переводов).
    RemoveDirIfEmpty { path: String },
    /// Забыть запись журнала установленного.
    Forget { content_id: String },
    /// `org.Cinnamon.ReloadXlet(uuid, тип)` — включённый xlet подхватывает
    /// новый код без перезапуска оболочки.
    ReloadXlet { uuid: String, xlet: String },
    /// Запись в журнал установленного.
    Record { content_id: String, provider: String, web_url: String },
}

/// Каталог установки типа (`~/.themes`, `~/.local/share/cinnamon/applets`…).
pub fn install_root(env: &SpiceEnv, t: &SpiceType) -> String {
    let dest = t.row().map_or("", |r| r.dest);
    snap_join(&env.home, dest)
}

/// Каталог переводов: `~/.local/share/locale/<яз>/LC_MESSAGES`.
pub fn locale_dir(env: &SpiceEnv, lang: &str) -> String {
    format!("{}/.local/share/locale/{lang}/LC_MESSAGES", env.home.trim_end_matches('/'))
}

/// План установки распакованного архива `extracted` (в нём `<ключ>/`, у
/// действий ещё `<ключ>.nemo_action`). Отказ — до первого побочного
/// эффекта.
pub fn plan_install(
    env: &SpiceEnv,
    t: &SpiceType,
    s: &Spice,
    fs: &FsSnapshot,
    extracted: &str,
    ctx: &InstallContext,
) -> Result<Vec<SpiceStep>, SpiceRefusal> {
    let key = s.key.as_str();
    let layout = check_layout(fs, extracted, t, key, ctx.cinnamon)?;
    let root = install_root(env, t);
    let dest = snap_join(&root, key);
    let src = snap_join(extracted, key);
    let mut steps = vec![SpiceStep::MakeDirs { path: root.clone() }, SpiceStep::RemoveTree { path: dest.clone() }];
    let action_file = format!("{key}.nemo_action");
    if t.is_action() {
        steps.push(SpiceStep::RemoveFile { path: snap_join(&root, &action_file) });
    }
    steps.push(SpiceStep::CopyTree { from: src.clone(), to: dest.clone() });
    if t.is_action() {
        steps.push(SpiceStep::CopyFile { from: snap_join(extracted, &action_file), to: snap_join(&root, &action_file) });
    }
    steps.push(SpiceStep::WriteLastEdited { meta: snap_join(&dest, "metadata.json"), ts: s.last_edited, create: t.is_theme() });
    for po in &layout.po_files {
        let lang = po_language(po).unwrap_or_default();
        steps.push(SpiceStep::CompilePo {
            po: snap_join(&snap_join(&src, "po"), po),
            mo: snap_join(&locale_dir(env, lang), &format!("{key}.mo")),
        });
    }
    if t.is_action() && !ctx.is_update {
        // Свежее действие кладётся ВЫКЛЮЧЕННЫМ: Nemo показывает в меню всё,
        // чего нет в этом списке, а включает его человек сам.
        let (schema, list_key) = crate::de_apply::NEMO_DISABLED;
        steps.push(SpiceStep::GsettingsListAdd { schema: schema.into(), key: list_key.into(), item: action_file.clone() });
    }
    if let (Some(xlet), true) = (t.xlet, ctx.enabled) {
        steps.push(SpiceStep::ReloadXlet { uuid: key.to_string(), xlet: xlet.to_string() });
    }
    steps.push(SpiceStep::Record { content_id: item_id(t, key), provider: PROVIDER_ID.into(), web_url: page_url(t, &s.spices_id) });
    Ok(steps)
}

/// Снимок дерева из списка записей zip (`каталог/` — каталоги) с текстами
/// нужных файлов (`metadata.json`, `.nemo_action`); остальные файлы —
/// пустые. Родительские каталоги появляются сами.
pub fn snapshot_from_entries(root: &str, entries: &[String], texts: &Map<String, Value>) -> FsSnapshot {
    let root = root.trim_end_matches('/').to_string();
    let mut dirs: Map<String, Value> = Map::new();
    let mut files: Map<String, Value> = Map::new();
    fn add_child(dirs: &mut Map<String, Value>, parent: &str, name: &str) {
        let list = dirs.entry(parent.to_string()).or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(a) = list {
            if !a.iter().any(|v| v.as_str() == Some(name)) {
                a.push(Value::from(name));
            }
        }
    }
    dirs.insert(root.clone(), Value::Array(Vec::new()));
    for e in entries {
        let is_dir = e.ends_with('/');
        let parts: Vec<&str> = e.trim_end_matches('/').split('/').filter(|p| !p.is_empty()).collect();
        let mut cur = root.clone();
        for (i, p) in parts.iter().enumerate() {
            add_child(&mut dirs, &cur, p);
            let next = format!("{cur}/{p}");
            if i + 1 < parts.len() || is_dir {
                dirs.entry(next.clone()).or_insert_with(|| Value::Array(Vec::new()));
            } else {
                let text = texts.get(e.as_str()).and_then(Value::as_str).unwrap_or("");
                files.insert(next.clone(), Value::from(text));
            }
            cur = next;
        }
    }
    FsSnapshot::from_json(&serde_json::json!({ "dirs": dirs, "files": files }))
}

/// Снимок настоящего каталога (распакованный архив): все каталоги и
/// файлы, тексты — только у `metadata.json` и `*.nemo_action` (не больше
/// 1 МБ). Ссылки не разворачиваются и считаются файлами.
pub fn live_snapshot(root: &Path) -> FsSnapshot {
    fn walk(dir: &Path, dirs: &mut Map<String, Value>, files: &mut Map<String, Value>) {
        let mut names = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                let path = e.path();
                let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
                names.push(Value::from(name.clone()));
                if md.is_dir() {
                    walk(&path, dirs, files);
                } else {
                    let wants = name == "metadata.json" || name.ends_with(".nemo_action");
                    let text = if wants && md.is_file() && md.len() <= 1 << 20 {
                        std::fs::read_to_string(&path).unwrap_or_default()
                    } else {
                        String::new()
                    };
                    files.insert(path.to_string_lossy().into_owned(), Value::from(text));
                }
            }
        }
        dirs.insert(dir.to_string_lossy().into_owned(), Value::Array(names));
    }
    let mut dirs = Map::new();
    let mut files = Map::new();
    walk(root, &mut dirs, &mut files);
    FsSnapshot::from_json(&serde_json::json!({ "dirs": dirs, "files": files }))
}

// ── Включение апплета (для вкладки; решение Wehrwolfmann 17.10.2026) ────

/// Добавить апплет в НАЧАЛО правой зоны первой панели, как это делает
/// окно настроек Cinnamon: записи той же панели и зоны с порядком ≥ 0
/// сдвигаются на один, новая — `panel1:right:0:<uuid>:<номер>`.
/// Возвращает новый список и новое значение `next-applet-id`.
///
/// Номер экземпляра берётся не меньше, чем на единицу больше уже занятых:
/// у штатных записей счётчик бывает 0 при занятых номерах, и тогда два
/// апплета получили бы один номер и общие настройки.
pub fn applet_enable_front(entries: &[String], uuid: &str, next_id: i64) -> (Vec<String>, i64) {
    let mut max_used = -1i64;
    let mut out: Vec<String> = entries
        .iter()
        .map(|e| {
            let mut parts: Vec<String> = e.split(':').map(str::to_string).collect();
            if let Some(id) = parts.get(4).and_then(|p| p.parse::<i64>().ok()) {
                max_used = max_used.max(id);
            }
            if parts.len() >= 4 && parts[0] == "panel1" && parts[1] == "right" {
                if let Ok(pos) = parts[2].parse::<i64>() {
                    if pos >= 0 {
                        parts[2] = (pos + 1).to_string();
                    }
                }
            }
            parts.join(":")
        })
        .collect();
    let id = next_id.max(max_used + 1).max(0);
    out.push(format!("panel1:right:0:{uuid}:{id}"));
    (out, id + 1)
}

// ═══════════════════════════════════════════════════════════════════════
//  Обновления и удаление
// ═══════════════════════════════════════════════════════════════════════

/// Отметка установленной версии — поле `last-edited` в `metadata.json`
/// (его дописывает установка, и Rexon, и окно настроек Cinnamon). Число
/// либо строка с числом; нет поля — версия неизвестна.
pub fn installed_last_edited(meta: &Value) -> Option<i64> {
    match meta.get("last-edited")? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Есть ли обновление: отметка установленного старше правки в индексе.
/// Отметки нет (спайс положен руками) — обновления не предлагаем, как и
/// окно настроек Cinnamon: сравнивать не с чем.
pub fn has_update(installed: Option<i64>, index_last_edited: i64) -> bool {
    installed.is_some_and(|i| i < index_last_edited)
}

/// Записи списка включённых без экземпляров спайса `item`. `field` — номер
/// поля записи через `:` (апплет `panel1:right:0:<uuid>:<номер>` — 3,
/// десклет `<uuid>:<номер>:<x>:<y>` — 0), `None` — запись целиком. Ведущий
/// `!` (выключенный экземпляр) при сравнении не учитывается. Сравнение
/// точное: `a@b` не задевает `xa@b`.
pub fn list_without(entries: &[String], item: &str, field: Option<u32>) -> Vec<String> {
    entries
        .iter()
        .filter(|e| {
            let part = match field {
                Some(i) => e.split(':').nth(i as usize).unwrap_or(""),
                None => e.as_str(),
            };
            part.trim_start_matches('!') != item
        })
        .cloned()
        .collect()
}

/// Каталог переводов всех языков: `~/.local/share/locale`.
pub fn locale_root(env: &SpiceEnv) -> String {
    format!("{}/.local/share/locale", env.home.trim_end_matches('/'))
}

/// План удаления спайса `key`. `fs` — снимок `~/.local/share/locale` (по
/// нему находятся `.mo` спайса во всех языках). Сначала спайс выключается
/// (запись уходит из списка включённых, выбранная тема сбрасывается к
/// умолчанию), потом снимаются его файлы: каталог, у действия ещё
/// `.nemo_action`, переводы (опустевшие каталоги языков — тоже), настройки
/// `~/.config/cinnamon/spices/<uuid>` и прежние `~/.cinnamon/configs/<uuid>`;
/// в конце — запись журнала установленного.
///
/// Действие Nemo «выключено» — это его строка в `disabled-actions`; после
/// удаления файла она не нужна и тоже убирается.
pub fn plan_uninstall(env: &SpiceEnv, t: &SpiceType, key: &str, fs: &FsSnapshot) -> Result<Vec<SpiceStep>, SpiceRefusal> {
    use crate::de_customization::{Activate, EntryFmt};
    if !valid_key(key) {
        return Err(SpiceRefusal::InvalidKey);
    }
    let root = install_root(env, t);
    let mut steps = Vec::new();
    match t.row().map(|r| &r.activate) {
        Some(Activate::GsettingsList { schema, key: list, entry, .. }) => {
            let field = match entry {
                EntryFmt::CinnamonApplet => Some(3),
                EntryFmt::CinnamonDesklet => Some(0),
                EntryFmt::Uuid => None,
            };
            steps.push(SpiceStep::GsettingsListDrop { schema: (*schema).into(), key: (*list).into(), item: key.into(), field });
        }
        Some(Activate::Gsettings { schema, key: k }) => {
            steps.push(SpiceStep::GsettingsResetIf { schema: (*schema).into(), key: (*k).into(), value: key.into() });
        }
        _ => {}
    }
    let action_file = format!("{key}.nemo_action");
    if t.is_action() {
        steps.push(SpiceStep::RemoveFile { path: snap_join(&root, &action_file) });
    }
    steps.push(SpiceStep::RemoveTree { path: snap_join(&root, key) });
    if t.is_action() {
        let (schema, list) = crate::de_apply::NEMO_DISABLED;
        steps.push(SpiceStep::GsettingsListDrop { schema: schema.into(), key: list.into(), item: action_file, field: None });
    }
    if !t.is_theme() {
        let locales = locale_root(env);
        let mut langs: Vec<String> = fs.scandir(&locales).to_vec();
        langs.sort();
        for lang in langs.iter().filter(|l| valid_key(l)) {
            let dir = locale_dir(env, lang);
            let mo = snap_join(&dir, &format!("{key}.mo"));
            if fs.is_file(&mo) {
                steps.push(SpiceStep::RemoveFile { path: mo });
                steps.push(SpiceStep::RemoveDirIfEmpty { path: dir });
                steps.push(SpiceStep::RemoveDirIfEmpty { path: snap_join(&locales, lang) });
            }
        }
        steps.push(SpiceStep::RemoveTree { path: format!("{}/cinnamon/spices/{key}", env.config.trim_end_matches('/')) });
        steps.push(SpiceStep::RemoveTree { path: format!("{}/.cinnamon/configs/{key}", env.home.trim_end_matches('/')) });
    }
    steps.push(SpiceStep::Forget { content_id: item_id(t, key) });
    Ok(steps)
}

#[cfg(test)]
mod plan_tests {
    use super::*;

    struct Tree {
        snap: FsSnapshot,
        names: Vec<String>,
    }

    /// Деревья шести настоящих архивов (снято 17.10.2026): имена записей
    /// zip, их права и тексты `metadata.json` / `.nemo_action`.
    fn tree(archive: &str) -> Tree {
        let all: Value = serde_json::from_str(
            &std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/trees.json", env!("CARGO_MANIFEST_DIR"))).unwrap(),
        )
        .unwrap();
        let t = &all[archive];
        let names: Vec<String> = t["entries"].as_array().unwrap().iter().map(|e| e[0].as_str().unwrap().to_string()).collect();
        let snap = snapshot_from_entries(X, &names, t["texts"].as_object().unwrap());
        Tree { snap, names }
    }

    const X: &str = "/tmp/x";
    const CUR: Option<(u32, u32, u32)> = Some((6, 6, 9));

    fn env() -> SpiceEnv {
        SpiceEnv::at_home("/home/u")
    }

    fn spice(key: &str, id: Value) -> Spice {
        Spice { key: key.into(), spices_id: id, last_edited: 1_780_583_449, ..Default::default() }
    }

    fn ctx(is_update: bool, enabled: bool) -> InstallContext {
        InstallContext { is_update, enabled, cinnamon: CUR }
    }

    fn po_steps(steps: &[SpiceStep]) -> Vec<(String, String)> {
        steps
            .iter()
            .filter_map(|s| match s {
                SpiceStep::CompilePo { po, mo } => Some((po.clone(), mo.clone())),
                _ => None,
            })
            .collect()
    }

    fn no_po(steps: &[SpiceStep]) -> Vec<SpiceStep> {
        steps.iter().filter(|s| !matches!(s, SpiceStep::CompilePo { .. })).cloned().collect()
    }

    fn all_archives() -> [&'static str; 7] {
        [
            "1440@jvlianodorneles",
            "weather@mockturtl",
            "AlbumArt3.0@claudiux",
            "ArrangementOfButtonsOnWindowTitleBar@claudiux",
            "Adara-Dark",
            "backup-this-file@h3nr1ke",
            "powerman@dr.drummie",
        ]
    }

    #[test]
    fn real_archives_have_safe_names() {
        for a in all_archives() {
            check_entry_names(&tree(a).names).unwrap_or_else(|e| panic!("{a}: {e:?}"));
        }
    }

    #[test]
    fn simple_applet() {
        let a = type_for_key("applet").unwrap();
        let tr = tree("1440@jvlianodorneles");
        let s = spice("1440@jvlianodorneles", Value::from(427));
        let steps = plan_install(&env(), a, &s, &tr.snap, X, &ctx(false, false)).unwrap();
        let d = "/home/u/.local/share/cinnamon/applets";
        assert_eq!(
            no_po(&steps),
            vec![
                SpiceStep::MakeDirs { path: d.into() },
                SpiceStep::RemoveTree { path: format!("{d}/1440@jvlianodorneles") },
                SpiceStep::CopyTree { from: "/tmp/x/1440@jvlianodorneles".into(), to: format!("{d}/1440@jvlianodorneles") },
                SpiceStep::WriteLastEdited { meta: format!("{d}/1440@jvlianodorneles/metadata.json"), ts: 1_780_583_449, create: false },
                SpiceStep::Record {
                    content_id: "spices:applet:1440@jvlianodorneles".into(),
                    provider: "cinnamon-spices".into(),
                    web_url: "https://cinnamon-spices.linuxmint.com/applets/view/427".into(),
                },
            ]
        );
        let po = po_steps(&steps);
        let langs: Vec<&str> = tr.names.iter().filter_map(|n| n.strip_prefix("1440@jvlianodorneles/po/")).filter_map(po_language).collect();
        assert_eq!(po.len(), langs.len());
        assert!(po.len() >= 3);
        assert!(po.contains(&(
            "/tmp/x/1440@jvlianodorneles/po/da.po".into(),
            "/home/u/.local/share/locale/da/LC_MESSAGES/1440@jvlianodorneles.mo".into()
        )));
        assert!(po.iter().all(|(p, _)| !p.ends_with(".pot")));
        // Без cinnamon-version у апплета — совместим; версии нет вовсе — тоже.
        let unknown = InstallContext { cinnamon: None, ..ctx(false, false) };
        assert!(plan_install(&env(), a, &s, &tr.snap, X, &unknown).is_ok());
    }

    #[test]
    fn multiversion_applet_picks_3_8_and_reloads_when_enabled() {
        let a = type_for_key("applet").unwrap();
        let tr = tree("weather@mockturtl");
        let lay = check_layout(&tr.snap, X, a, "weather@mockturtl", CUR).unwrap();
        assert_eq!(lay.version_subdir.as_deref(), Some("3.8"));
        assert!(lay.po_files.iter().any(|p| p == "ru.po"));
        assert!(lay.po_files.windows(2).all(|w| w[0] < w[1]));
        // На старом Cinnamon — 3.0; ниже 3.0 — корня без applet.js не хватит.
        assert_eq!(check_layout(&tr.snap, X, a, "weather@mockturtl", Some((3, 6, 0))).unwrap().version_subdir.as_deref(), Some("3.0"));
        assert_eq!(
            check_layout(&tr.snap, X, a, "weather@mockturtl", Some((2, 8, 0))),
            Err(SpiceRefusal::NoMainJs { file: "applet.js".into() })
        );
        // Версия неизвестна — годится любой подкаталог.
        assert_eq!(check_layout(&tr.snap, X, a, "weather@mockturtl", None).unwrap().version_subdir.as_deref(), Some("3.8"));

        let s = spice("weather@mockturtl", Value::from(17));
        let steps = plan_install(&env(), a, &s, &tr.snap, X, &ctx(true, true)).unwrap();
        assert_eq!(
            steps.iter().rev().nth(1),
            Some(&SpiceStep::ReloadXlet { uuid: "weather@mockturtl".into(), xlet: "APPLET".into() })
        );
        assert!(!steps.iter().any(|s| matches!(s, SpiceStep::GsettingsListAdd { .. })));
        let not_enabled = plan_install(&env(), a, &s, &tr.snap, X, &ctx(true, false)).unwrap();
        assert!(!not_enabled.iter().any(|s| matches!(s, SpiceStep::ReloadXlet { .. })));
    }

    #[test]
    fn multiversion_flag_without_subdirs() {
        // powerman: "multiversion": true, но подкаталогов нет; версии 6.4/6.6.
        let a = type_for_key("applet").unwrap();
        let tr = tree("powerman@dr.drummie");
        let lay = check_layout(&tr.snap, X, a, "powerman@dr.drummie", CUR).unwrap();
        assert_eq!(lay.version_subdir, None);
        match check_layout(&tr.snap, X, a, "powerman@dr.drummie", Some((6, 2, 0))) {
            Err(SpiceRefusal::Incompatible { need, current }) => {
                assert_eq!(need, vec!["6.4".to_string(), "6.6".to_string()]);
                assert_eq!(current, (6, 2, 0));
                let msg = SpiceRefusal::Incompatible { need, current }.message("PowerMan");
                assert!(msg.contains("6.4") && msg.contains("6.2.0"), "{msg}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn desklet() {
        let d = type_for_key("desklet").unwrap();
        let tr = tree("AlbumArt3.0@claudiux");
        let s = spice("AlbumArt3.0@claudiux", Value::from(73));
        let steps = plan_install(&env(), d, &s, &tr.snap, X, &ctx(false, true)).unwrap();
        let root = "/home/u/.local/share/cinnamon/desklets";
        assert_eq!(steps[0], SpiceStep::MakeDirs { path: root.into() });
        assert_eq!(steps[2], SpiceStep::CopyTree { from: "/tmp/x/AlbumArt3.0@claudiux".into(), to: format!("{root}/AlbumArt3.0@claudiux") });
        assert!(steps.contains(&SpiceStep::ReloadXlet { uuid: "AlbumArt3.0@claudiux".into(), xlet: "DESKLET".into() }));
        assert!(!po_steps(&steps).is_empty());
        // Без desklet.js — отказ.
        let mut names = tr.names.clone();
        names.retain(|n| n != "AlbumArt3.0@claudiux/desklet.js");
        let all: Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/trees.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
        let snap = snapshot_from_entries(X, &names, all["AlbumArt3.0@claudiux"]["texts"].as_object().unwrap());
        assert_eq!(plan_install(&env(), d, &s, &snap, X, &ctx(false, false)), Err(SpiceRefusal::NoMainJs { file: "desklet.js".into() }));
    }

    #[test]
    fn extension_and_its_mandatory_version() {
        let e = type_for_key("extension").unwrap();
        let key = "ArrangementOfButtonsOnWindowTitleBar@claudiux";
        let tr = tree(key);
        let s = spice(key, Value::from(107));
        let steps = plan_install(&env(), e, &s, &tr.snap, X, &ctx(false, false)).unwrap();
        assert_eq!(steps[1], SpiceStep::RemoveTree { path: format!("/home/u/.local/share/cinnamon/extensions/{key}") });
        assert_eq!(steps.last().unwrap(), &SpiceStep::Record {
            content_id: format!("spices:extension:{key}"),
            provider: PROVIDER_ID.into(),
            web_url: "https://cinnamon-spices.linuxmint.com/extensions/view/107".into(),
        });
        // На Cinnamon 3.4 (до 3.6) — несовместимо.
        assert!(matches!(check_layout(&tr.snap, X, e, key, Some((3, 4, 0))), Err(SpiceRefusal::Incompatible { .. })));
        // Расширение без cinnamon-version — отказ; апплет без него — можно.
        let meta = format!("{{\"uuid\": \"{key}\", \"name\": \"n\", \"description\": \"d\"}}");
        let mut texts = Map::new();
        texts.insert(format!("{key}/metadata.json"), Value::from(meta));
        let snap = snapshot_from_entries(X, &tr.names, &texts);
        assert_eq!(check_layout(&snap, X, e, key, CUR), Err(SpiceRefusal::MissingCinnamonVersion));
    }

    #[test]
    fn theme_goes_to_dot_themes_by_key() {
        let th = type_for_key("theme").unwrap();
        let tr = tree("Adara-Dark");
        let s = spice("Adara-Dark", Value::from("Adara-Dark"));
        let steps = plan_install(&env(), th, &s, &tr.snap, X, &ctx(false, false)).unwrap();
        assert_eq!(
            steps,
            vec![
                SpiceStep::MakeDirs { path: "/home/u/.themes".into() },
                SpiceStep::RemoveTree { path: "/home/u/.themes/Adara-Dark".into() },
                SpiceStep::CopyTree { from: "/tmp/x/Adara-Dark".into(), to: "/home/u/.themes/Adara-Dark".into() },
                SpiceStep::WriteLastEdited { meta: "/home/u/.themes/Adara-Dark/metadata.json".into(), ts: 1_780_583_449, create: true },
                SpiceStep::Record {
                    content_id: "spices:theme:Adara-Dark".into(),
                    provider: PROVIDER_ID.into(),
                    web_url: "https://cinnamon-spices.linuxmint.com/themes/view/Adara-Dark".into(),
                },
            ]
        );
        // Без cinnamon/cinnamon.css — не тема Cinnamon.
        let mut names = tr.names.clone();
        names.retain(|n| n != "Adara-Dark/cinnamon/cinnamon.css");
        let snap = snapshot_from_entries(X, &names, &Map::new());
        assert_eq!(check_layout(&snap, X, th, "Adara-Dark", CUR), Err(SpiceRefusal::NoCinnamonCss));
    }

    #[test]
    fn nemo_action_two_roots_and_disabled_on_first_install() {
        let act = type_for_key("action").unwrap();
        let key = "backup-this-file@h3nr1ke";
        let tr = tree(key);
        let s = spice(key, Value::from(37));
        let d = "/home/u/.local/share/nemo/actions";
        let fresh = plan_install(&env(), act, &s, &tr.snap, X, &ctx(false, false)).unwrap();
        assert_eq!(
            no_po(&fresh),
            vec![
                SpiceStep::MakeDirs { path: d.into() },
                SpiceStep::RemoveTree { path: format!("{d}/{key}") },
                SpiceStep::RemoveFile { path: format!("{d}/{key}.nemo_action") },
                SpiceStep::CopyTree { from: format!("/tmp/x/{key}"), to: format!("{d}/{key}") },
                SpiceStep::CopyFile { from: format!("/tmp/x/{key}.nemo_action"), to: format!("{d}/{key}.nemo_action") },
                SpiceStep::WriteLastEdited { meta: format!("{d}/{key}/metadata.json"), ts: 1_780_583_449, create: false },
                SpiceStep::GsettingsListAdd {
                    schema: "org.nemo.plugins".into(),
                    key: "disabled-actions".into(),
                    item: format!("{key}.nemo_action"),
                },
                SpiceStep::Record {
                    content_id: format!("spices:action:{key}"),
                    provider: PROVIDER_ID.into(),
                    web_url: "https://cinnamon-spices.linuxmint.com/actions/view/37".into(),
                },
            ]
        );
        assert!(po_steps(&fresh).iter().any(|(_, mo)| mo == &format!("/home/u/.local/share/locale/pt_BR/LC_MESSAGES/{key}.mo")));
        // Обновление: список выключенных не трогаем; включённость не важна.
        let update = plan_install(&env(), act, &s, &tr.snap, X, &ctx(true, true)).unwrap();
        assert!(!update.iter().any(|s| matches!(s, SpiceStep::GsettingsListAdd { .. } | SpiceStep::ReloadXlet { .. })));
        // Без .nemo_action — отказ.
        let names: Vec<String> = tr.names.iter().filter(|n| !n.ends_with(".nemo_action")).cloned().collect();
        let all: Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/trees.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap();
        let snap = snapshot_from_entries(X, &names, all[key]["texts"].as_object().unwrap());
        assert_eq!(check_layout(&snap, X, act, key, CUR), Err(SpiceRefusal::NoActionFile));
    }

    #[test]
    fn refusals() {
        let a = type_for_key("applet").unwrap();
        // ../ и абсолютные пути.
        for bad in ["../evil", "x/../../evil", "/etc/passwd", "a\\..\\b"] {
            assert_eq!(check_entry_names(&[bad.to_string()]), Err(SpiceRefusal::UnsafePath { entry: bad.into() }), "{bad}");
        }
        assert!(check_entry_names(&["a/./b".into(), "a/b/../c".into()]).is_ok());

        // Каталог в архиве назван не по ключу.
        let tr = tree("1440@jvlianodorneles");
        assert_eq!(check_layout(&tr.snap, X, a, "other@x", CUR), Err(SpiceRefusal::NotZipLayout));
        assert_eq!(check_layout(&tr.snap, X, a, "../x", CUR), Err(SpiceRefusal::InvalidKey));

        // uuid в metadata.json ≠ каталог.
        let mut texts = Map::new();
        texts.insert("1440@jvlianodorneles/metadata.json".into(), Value::from(r#"{"uuid": "1440@someone", "name": "n", "description": "d"}"#));
        let snap = snapshot_from_entries(X, &tr.names, &texts);
        assert_eq!(check_layout(&snap, X, a, "1440@jvlianodorneles", CUR), Err(SpiceRefusal::UuidMismatch { found: "1440@someone".into() }));
        // Нет metadata.json, битый, без name.
        let snap = snapshot_from_entries(X, &tr.names, &Map::new());
        assert_eq!(check_layout(&snap, X, a, "1440@jvlianodorneles", CUR), Err(SpiceRefusal::BadMetadata), "пустой текст не JSON");
        let names: Vec<String> = tr.names.iter().filter(|n| !n.ends_with("metadata.json")).cloned().collect();
        let snap = snapshot_from_entries(X, &names, &Map::new());
        assert_eq!(check_layout(&snap, X, a, "1440@jvlianodorneles", CUR), Err(SpiceRefusal::NoMetadata));
        texts.insert("1440@jvlianodorneles/metadata.json".into(), Value::from(r#"{"uuid": "1440@jvlianodorneles", "description": "d"}"#));
        let snap = snapshot_from_entries(X, &tr.names, &texts);
        assert_eq!(check_layout(&snap, X, a, "1440@jvlianodorneles", CUR), Err(SpiceRefusal::MissingProperty { name: "name".into() }));
        // Несовместимая версия — понятный текст.
        texts.insert(
            "1440@jvlianodorneles/metadata.json".into(),
            Value::from(r#"{"uuid": "1440@jvlianodorneles", "name": "n", "description": "d", "cinnamon-version": ["6.8"]}"#),
        );
        let snap = snapshot_from_entries(X, &tr.names, &texts);
        let err = check_layout(&snap, X, a, "1440@jvlianodorneles", CUR).unwrap_err();
        assert_eq!(err, SpiceRefusal::Incompatible { need: vec!["6.8".into()], current: (6, 6, 9) });
        let msg = err.message("1440");
        assert!(msg.contains("6.8") && msg.contains("6.6.9") && msg.contains("1440"), "{msg}");
        // Каждый отказ даёт непустой текст с именем.
        for r in [
            SpiceRefusal::InvalidKey,
            SpiceRefusal::UnsafePath { entry: "../x".into() },
            SpiceRefusal::NotZipLayout,
            SpiceRefusal::NoMetadata,
            SpiceRefusal::BadMetadata,
            SpiceRefusal::UuidMismatch { found: "f".into() },
            SpiceRefusal::MissingProperty { name: "name".into() },
            SpiceRefusal::Incompatible { need: vec![], current: (6, 6, 9) },
            SpiceRefusal::MissingCinnamonVersion,
            SpiceRefusal::NoMainJs { file: "applet.js".into() },
            SpiceRefusal::NoCinnamonCss,
            SpiceRefusal::NoActionFile,
        ] {
            assert!(r.message("Имя").contains("Имя"), "{r:?}");
        }
    }

    #[test]
    fn live_snapshot_matches_entry_snapshot() {
        let dir = std::env::temp_dir().join(format!("rexon-spices-snap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("u@x/3.8/po")).unwrap();
        std::fs::write(dir.join("u@x/metadata.json"), r#"{"uuid":"u@x","name":"n","description":"d"}"#).unwrap();
        std::fs::write(dir.join("u@x/3.8/applet.js"), "x").unwrap();
        std::fs::write(dir.join("u@x/3.8/po/ru.po"), "").unwrap();
        let snap = live_snapshot(&dir);
        let root = dir.to_string_lossy().into_owned();
        let lay = check_layout(&snap, &root, type_for_key("applet").unwrap(), "u@x", CUR).unwrap();
        assert_eq!(lay.version_subdir.as_deref(), Some("3.8"));
        assert!(lay.po_files.is_empty(), "po ищется только в корне спайса");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn applet_goes_to_the_front_of_the_right_box() {
        let entries: Vec<String> = [
            "panel1:left:0:menu@cinnamon.org",
            "panel1:right:0:systray@cinnamon.org:3",
            "panel1:right:1:calendar@cinnamon.org:7",
            "panel2:right:0:other@x:2",
            "panel1:center:0:clock@x",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (out, next) = applet_enable_front(&entries, "weather@mockturtl", 0);
        assert_eq!(
            out,
            vec![
                "panel1:left:0:menu@cinnamon.org",
                "panel1:right:1:systray@cinnamon.org:3",
                "panel1:right:2:calendar@cinnamon.org:7",
                "panel2:right:0:other@x:2",
                "panel1:center:0:clock@x",
                "panel1:right:0:weather@mockturtl:8",
            ]
        );
        assert_eq!(next, 9, "счётчик 0 при занятом 7 — берём 8");
        let (out, next) = applet_enable_front(&[], "a@b", 12);
        assert_eq!((out, next), (vec!["panel1:right:0:a@b:12".to_string()], 13));
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Исполнитель: скачивание, распаковка, шаги плана
// ═══════════════════════════════════════════════════════════════════════

/// Потолок архива спайса: самый крупный на сайте (замер 17.10.2026) —
/// расширение около 83 МБ.
pub const MAX_ARCHIVE_BYTES: i64 = 120 << 20;
/// Таймаут скачивания архива, секунды (на соединение и каждое чтение).
pub const DOWNLOAD_TIMEOUT_SECS: u64 = 120;

/// Не файловые побочные эффекты исполнителя: настройки gsettings и вызов
/// оболочки по DBus. В бою — [`LiveSys`], в тестах — запись в память.
pub trait SpiceSys {
    /// Список строк ключа; `None` — прочитать не удалось (нет схемы).
    fn list_get(&mut self, schema: &str, key: &str) -> Option<Vec<String>>;
    fn list_set(&mut self, schema: &str, key: &str, items: &[String]) -> Result<(), String>;
    /// Строковый ключ; `None` — прочитать не удалось.
    fn string_get(&mut self, schema: &str, key: &str) -> Option<String>;
    fn reset(&mut self, schema: &str, key: &str) -> Result<(), String>;
    /// `org.Cinnamon.ReloadXlet(uuid, тип)`.
    fn reload_xlet(&mut self, uuid: &str, xlet: &str) -> Result<(), String>;
}

/// Настоящие `gsettings` и `gdbus`.
pub struct LiveSys;

fn run_out(argv: &[&str], timeout: f64) -> Result<String, String> {
    let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
    match crate::splash_exec::run_capture(&argv, timeout) {
        Some(o) if o.rc == 0 => Ok(o.stdout),
        Some(o) => Err(o.stderr.trim().to_string()),
        None => Err(trf("couldn't run %1", &[&argv[0]])),
    }
}

impl SpiceSys for LiveSys {
    fn list_get(&mut self, schema: &str, key: &str) -> Option<Vec<String>> {
        run_out(&["gsettings", "get", schema, key], 10.0).ok().map(|o| crate::de_apply::parse_gsettings_list(&o))
    }
    fn list_set(&mut self, schema: &str, key: &str, items: &[String]) -> Result<(), String> {
        let v = crate::wallpaper_engine::gvariant_string_array(items);
        run_out(&["gsettings", "set", schema, key, &v], 10.0).map(|_| ())
    }
    fn string_get(&mut self, schema: &str, key: &str) -> Option<String> {
        run_out(&["gsettings", "get", schema, key], 10.0).ok().map(|o| crate::de_apply::parse_gsettings_string(&o).unwrap_or_default())
    }
    fn reset(&mut self, schema: &str, key: &str) -> Result<(), String> {
        run_out(&["gsettings", "reset", schema, key], 10.0).map(|_| ())
    }
    fn reload_xlet(&mut self, uuid: &str, xlet: &str) -> Result<(), String> {
        run_out(
            &["gdbus", "call", "--session", "--dest", "org.Cinnamon", "--object-path", "/org/Cinnamon", "--method", "org.Cinnamon.ReloadXlet", uuid, xlet],
            10.0,
        )
        .map(|_| ())
    }
}

/// Исполнитель без gsettings и DBus — для чисто файловых планов.
struct NoSys;

impl SpiceSys for NoSys {
    fn list_get(&mut self, _: &str, _: &str) -> Option<Vec<String>> {
        None
    }
    fn list_set(&mut self, _: &str, _: &str, _: &[String]) -> Result<(), String> {
        Ok(())
    }
    fn string_get(&mut self, _: &str, _: &str) -> Option<String> {
        None
    }
    fn reset(&mut self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn reload_xlet(&mut self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Итог исполнения плана: что не встало, но установку не отменило.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExecReport {
    /// Языки, чей `.po` не собрался.
    pub bad_po: Vec<String>,
    /// Не удалось перезагрузить включённый xlet (оболочка не запущена…).
    pub reload_failed: bool,
}

/// Путь под одним из корней (`home`, каталог настроек, каталог работы):
/// исполнитель ничего не удаляет и не пишет вне них.
fn under(path: &str, roots: &[&str]) -> bool {
    !path.contains("/../")
        && !path.ends_with("/..")
        && roots.iter().any(|r| {
            let r = r.trim_end_matches('/');
            !r.is_empty() && path.starts_with(&format!("{r}/"))
        })
}

fn remove_tree(path: &str) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Скопировать дерево с правами файлов. Ссылки внутри архива не
/// переносятся: спайсу они не нужны, а ссылка наружу из каталога —
/// лазейка.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let md = std::fs::symlink_metadata(e.path())?;
        let dest = to.join(e.file_name());
        if md.is_dir() {
            copy_tree(&e.path(), &dest)?;
            std::fs::set_permissions(&dest, md.permissions())?;
        } else if md.is_file() {
            std::fs::copy(e.path(), &dest)?;
        }
    }
    Ok(())
}

/// `metadata.json` с полем `last-edited` (прочие ключи — в прежнем порядке,
/// отступ 4, как у окна настроек Cinnamon).
pub fn with_last_edited(meta_text: Option<&str>, ts: i64) -> Result<String, String> {
    let mut obj: Map<String, Value> = match meta_text {
        Some(t) => match serde_json::from_str::<Value>(t) {
            Ok(Value::Object(m)) => m,
            _ => return Err("metadata.json".to_string()),
        },
        None => Map::new(),
    };
    obj.insert("last-edited".to_string(), Value::from(ts));
    let mut out = Vec::new();
    let fmt = serde_json::ser::PrettyFormatter::with_indent(b"    ");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, fmt);
    Value::Object(obj).serialize(&mut ser).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Исполнить план. `roots` — где разрешено писать и удалять; `journal` —
/// шаги `Record`/`Forget`. Первая неудача файлового шага останавливает
/// исполнение (`Err` — текст причины); неудачный перевод и перезагрузка
/// xlet — примечания отчёта.
pub fn execute(
    steps: &[SpiceStep],
    roots: &[&str],
    sys: &mut dyn SpiceSys,
    journal: &mut dyn FnMut(&SpiceStep) -> Result<(), String>,
    log: &mut dyn FnMut(&str),
) -> Result<ExecReport, String> {
    let mut report = ExecReport::default();
    let guard = |p: &str| -> Result<(), String> {
        if under(p, roots) {
            Ok(())
        } else {
            Err(trf("refused to touch %1: it's outside the home folder", &[&p]))
        }
    };
    let io = |p: &str, e: std::io::Error| format!("{p}: {e}");
    for step in steps {
        match step {
            SpiceStep::MakeDirs { path } => {
                guard(path)?;
                std::fs::create_dir_all(path).map_err(|e| io(path, e))?;
            }
            SpiceStep::RemoveTree { path } => {
                guard(path)?;
                remove_tree(path).map_err(|e| io(path, e))?;
            }
            SpiceStep::RemoveFile { path } => {
                guard(path)?;
                match std::fs::remove_file(path) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(io(path, e)),
                    _ => {}
                }
            }
            SpiceStep::RemoveDirIfEmpty { path } => {
                guard(path)?;
                let _ = std::fs::remove_dir(path); // не пуст — остаётся
            }
            SpiceStep::CopyTree { from, to } => {
                guard(to)?;
                copy_tree(Path::new(from), Path::new(to)).map_err(|e| io(to, e))?;
            }
            SpiceStep::CopyFile { from, to } => {
                guard(to)?;
                std::fs::copy(from, to).map_err(|e| io(to, e))?;
            }
            SpiceStep::CompilePo { po, mo } => {
                guard(mo)?;
                let lang = Path::new(po).file_name().and_then(|n| n.to_str()).and_then(po_language).unwrap_or("?").to_string();
                let built = std::fs::read(po).map_err(|e| e.to_string()).and_then(|b| po_to_mo(&String::from_utf8_lossy(&b)));
                let written = built.and_then(|bytes| {
                    if let Some(dir) = Path::new(mo).parent() {
                        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                    }
                    std::fs::write(mo, bytes).map_err(|e| e.to_string())
                });
                if let Err(e) = written {
                    log(&format!("перевод {lang} не собран: {e}"));
                    report.bad_po.push(lang);
                }
            }
            SpiceStep::WriteLastEdited { meta, ts, create } => {
                guard(meta)?;
                let text = std::fs::read_to_string(meta).ok();
                if text.is_none() && !create {
                    return Err(trf("%1 is missing", &[meta]));
                }
                let out = with_last_edited(text.as_deref(), *ts).map_err(|e| trf("%1 can't be read", &[&e]))?;
                std::fs::write(meta, out).map_err(|e| io(meta, e))?;
            }
            SpiceStep::GsettingsListAdd { schema, key, item } => {
                let mut list = sys.list_get(schema, key).unwrap_or_default();
                if !list.iter().any(|e| e == item) {
                    list.push(item.clone());
                    sys.list_set(schema, key, &list).map_err(|e| format!("{schema} {key}: {e}"))?;
                }
            }
            SpiceStep::GsettingsListDrop { schema, key, item, field } => {
                // Схемы нет (Cinnamon/Nemo не стоят) — и выключать нечего.
                if let Some(list) = sys.list_get(schema, key) {
                    let kept = list_without(&list, item, *field);
                    if kept.len() != list.len() {
                        sys.list_set(schema, key, &kept).map_err(|e| format!("{schema} {key}: {e}"))?;
                    }
                }
            }
            SpiceStep::GsettingsResetIf { schema, key, value } => {
                if sys.string_get(schema, key).as_deref() == Some(value.as_str()) {
                    sys.reset(schema, key).map_err(|e| format!("{schema} {key}: {e}"))?;
                }
            }
            SpiceStep::ReloadXlet { uuid, xlet } => {
                if let Err(e) = sys.reload_xlet(uuid, xlet) {
                    log(&format!("ReloadXlet {uuid}: {e}"));
                    report.reload_failed = true;
                }
            }
            SpiceStep::Record { .. } | SpiceStep::Forget { .. } => journal(step)?,
        }
    }
    Ok(report)
}

/// Состояние спайса на диске: `None` — не стоит; `Some(отметка)` — стоит,
/// отметка `last-edited` из `metadata.json` (её нет у положенного руками).
/// Действие Nemo стоит, если есть его `.nemo_action`.
pub fn installed_state(env: &SpiceEnv, t: &SpiceType, key: &str) -> Option<Option<i64>> {
    if !valid_key(key) {
        return None;
    }
    let root = install_root(env, t);
    let dir = snap_join(&root, key);
    let present = if t.is_action() {
        Path::new(&snap_join(&root, &format!("{key}.nemo_action"))).is_file()
    } else {
        Path::new(&dir).is_dir()
    };
    if !present {
        return None;
    }
    let meta = std::fs::read_to_string(snap_join(&dir, "metadata.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    Some(meta.as_ref().and_then(installed_last_edited))
}

/// Включён ли xlet (есть в своём списке включённых). Темы и действия — нет:
/// им после обновления перезагружать нечего.
pub fn is_enabled(sys: &mut dyn SpiceSys, t: &SpiceType, key: &str) -> bool {
    use crate::de_customization::{Activate, EntryFmt};
    let Some(Activate::GsettingsList { schema, key: list, entry, .. }) = t.row().map(|r| &r.activate) else {
        return false;
    };
    let field = match entry {
        EntryFmt::CinnamonApplet => Some(3),
        EntryFmt::CinnamonDesklet => Some(0),
        EntryFmt::Uuid => None,
    };
    sys.list_get(schema, list).is_some_and(|l| list_without(&l, key, field).len() != l.len())
}

/// Имена записей zip; `None` — это не zip.
fn zip_names(archive: &Path) -> Option<Vec<String>> {
    let mut zf = zip::ZipArchive::new(std::fs::File::open(archive).ok()?).ok()?;
    (0..zf.len()).map(|i| zf.by_index(i).ok().map(|f| f.name().to_string())).collect()
}

/// Права записей архива (как их хранит zip: `external_attr >> 16`) — на
/// распакованные файлы и каталоги: общий распаковщик их не переносит, а
/// скрипты спайсов (действия Nemo, десклеты) без бита исполнения не
/// запустятся. Ссылки не трогаются; биты сверх `0o777` отбрасываются,
/// владельцу чтение и запись оставляются всегда.
fn apply_zip_modes(archive: &Path, out: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let Some(mut zf) = std::fs::File::open(archive).ok().and_then(|f| zip::ZipArchive::new(f).ok()) else {
        return;
    };
    for i in 0..zf.len() {
        let Ok(e) = zf.by_index(i) else { continue };
        let (Some(mode), Some(rel)) = (e.unix_mode(), e.enclosed_name()) else { continue };
        let path = out.join(rel);
        let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
        if md.is_file() || md.is_dir() {
            let mode = (mode & 0o777) | if md.is_dir() { 0o700 } else { 0o600 };
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode));
        }
    }
}

/// Что поставил план (для журнала): каталог спайса и у действия — файл.
fn planned_paths(steps: &[SpiceStep]) -> Vec<String> {
    steps
        .iter()
        .filter_map(|s| match s {
            SpiceStep::CopyTree { to, .. } | SpiceStep::CopyFile { to, .. } => Some(to.clone()),
            _ => None,
        })
        .collect()
}

/// Фраза итога установки.
pub fn installed_message(t: &SpiceType, name: &str, is_update: bool, report: &ExecReport, reloaded: bool) -> String {
    let mut text = if is_update {
        trf("%1 is updated.", &[&name])
    } else if t.is_theme() {
        trf("%1 is installed. Press “Apply” on its card to use it.", &[&name])
    } else {
        trf("%1 is installed. It isn't turned on yet: press “Enable” on its card.", &[&name])
    };
    if reloaded {
        text.push(' ');
        text.push_str(&if report.reload_failed {
            tr("Cinnamon will pick up the new version after you sign out and back in.")
        } else {
            tr("Cinnamon has already picked up the new version.")
        });
    }
    if !report.bad_po.is_empty() {
        text.push(' ');
        text.push_str(&trf("Translations for these languages weren't installed: %1.", &[&report.bad_po.join(", ")]));
    }
    text
}

/// Поставить уже скачанный архив `archive`: проверка записей, распаковка в
/// `work`, проверка дерева, план, исполнение. `journal` — запись журнала
/// установленного (`Record`, с путями, которые поставил план). Каталог
/// `work` остаётся вызывающему.
#[allow(clippy::too_many_arguments)]
pub fn install_archive(
    env: &SpiceEnv,
    t: &SpiceType,
    s: &Spice,
    name: &str,
    archive: &Path,
    work: &Path,
    cinnamon: Option<(u32, u32, u32)>,
    sys: &mut dyn SpiceSys,
    journal: &mut dyn FnMut(&SpiceStep, &[String]) -> Result<(), String>,
    log: &mut dyn FnMut(&str),
) -> (bool, String) {
    install_archive_in(env, t, s, name, archive, work, cinnamon, None, sys, journal, log)
}

/// [`install_archive`] из раздела GTK или рамок окон (`section`, см.
/// [`PART_SECTIONS`]): тема ставится, только если в самом архиве есть то,
/// что этот раздел применит ([`fits_section`]). Витрина раздела уже
/// отобрана по дереву репозитория, но ставится архив сайта — и проверяется
/// он, а не наш прогноз.
#[allow(clippy::too_many_arguments)]
pub fn install_archive_in(
    env: &SpiceEnv,
    t: &SpiceType,
    s: &Spice,
    name: &str,
    archive: &Path,
    work: &Path,
    cinnamon: Option<(u32, u32, u32)>,
    section: Option<&str>,
    sys: &mut dyn SpiceSys,
    journal: &mut dyn FnMut(&SpiceStep, &[String]) -> Result<(), String>,
    log: &mut dyn FnMut(&str),
) -> (bool, String) {
    let Some(names) = zip_names(archive) else {
        return (false, SpiceRefusal::NotZipLayout.message(name));
    };
    if let Err(r) = check_entry_names(&names) {
        return (false, r.message(name));
    }
    let out = work.join("x");
    let out_s = out.to_string_lossy().into_owned();
    if let Err(e) = crate::appearance_installer::live_safe_extract(&archive.to_string_lossy(), &out_s) {
        return (false, trf("Couldn't unpack the archive of %1: %2", &[&name, &e]));
    }
    apply_zip_modes(archive, &out);
    let snap = live_snapshot(&out);
    if let Some(sec) = section.filter(|_| t.is_theme()) {
        let dir = snap_join(&out_s, &s.key);
        let has = |rel: &str| {
            let p = snap_join(&dir, rel);
            snap.is_file(&p) || snap.is_dir(&p)
        };
        if !fits_section(sec, &has) {
            return (false, part_missing_message(name, sec));
        }
    }
    let is_update = installed_state(env, t, &s.key).is_some();
    let enabled = is_update && is_enabled(sys, t, &s.key);
    let ctx = InstallContext { is_update, enabled, cinnamon };
    let steps = match plan_install(env, t, s, &snap, &out_s, &ctx) {
        Ok(st) => st,
        Err(r) => return (false, r.message(name)),
    };
    let paths = planned_paths(&steps);
    let work_s = work.to_string_lossy().into_owned();
    let roots = [env.home.as_str(), work_s.as_str()];
    log(&trf("Installing %1…", &[&name]));
    match execute(&steps, &roots, sys, &mut |st| journal(st, &paths), log) {
        Ok(report) => (true, installed_message(t, name, is_update, &report, enabled)),
        Err(e) => (false, trf("Couldn't install %1: %2", &[&name, &e])),
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Правка журнала установленного: пишется общий журнал установки,
/// `ocs_installed.json` — его зеркало (шаг 5б слияния установщиков).
fn journal_edit(env: &SpiceEnv, edit: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
    let path = crate::ocs_store::install_log_path_xdg(Some(&env.config), &env.home);
    let mut data = crate::install_journal::store_log_view(&path);
    edit(&mut data);
    crate::install_journal::put_store_log(&path, &data).map_err(|e| e.to_string())
}

/// Записать спайс в журнал установленного (`provider_id` —
/// [`PROVIDER_ID`], адрес — страница сайта).
pub fn journal_record(env: &SpiceEnv, item: &ThemeItem, t: &SpiceType, web_url: &str, paths: &[String]) -> Result<(), String> {
    let entry = crate::ocs_store::InstalledItem {
        content_id: item.id.clone(),
        name: item.name.clone(),
        rexon_type: crate::appearance_installer::kind_to_rexon_type(t.section).to_string(),
        author: item.author.clone(),
        description: item.description.clone(),
        version: item.version.clone(),
        license: item.license.clone(),
        preview_url: item.preview_url.clone(),
        web_url: web_url.to_string(),
        provider_id: PROVIDER_ID.to_string(),
        category_id: t.cat_id(),
        ..Default::default()
    };
    journal_edit(env, |data| {
        crate::ocs_store::record_installed(data, &entry, paths, &crate::engine_update::now_iso(now_secs()));
    })
}

/// Каталог работы под кэшем Rexon (а не во `/tmp`: там память, а архив
/// бывает под сотню мегабайт).
fn make_work_dir() -> Result<std::path::PathBuf, String> {
    let cache = live_cache_path(&TYPES[0]);
    let base = Path::new(&cache).parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&base).map_err(|e| format!("{}: {e}", base.display()))?;
    for n in 0..100u32 {
        let p = base.join(format!("work-{}-{}-{n}", std::process::id(), now_secs()));
        if std::fs::create_dir(&p).is_ok() {
            return Ok(p);
        }
    }
    Err(trf("couldn't create a temporary folder in %1", &[&base.display()]))
}

/// Установить (или обновить) спайс карточки витрины: запись индекса — из
/// кэша, архив — с сайта (только по кнопке), дальше [`install_archive`].
pub fn live_install(item: &ThemeItem, mut log: Box<dyn FnMut(&str) + Send>, progress: Option<Box<dyn FnMut(i64, i64) + Send>>) -> (bool, String) {
    let Some((t, key)) = parse_item_id(&item.id) else {
        return (false, trf("%1 isn't from Cinnamon Spices.", &[&item.name]));
    };
    let name = if item.name.is_empty() { key.to_string() } else { item.name.clone() };
    let Some(s) = load_cache(Path::new(&live_cache_path(t))).and_then(|(_, list)| list.into_iter().find(|s| s.key == key)) else {
        return (false, trf("Couldn't find %1 in the Cinnamon Spices catalog. Refresh the section and try again.", &[&name]));
    };
    let env = SpiceEnv::live();
    let work = match make_work_dir() {
        Ok(w) => w,
        Err(e) => return (false, e),
    };
    let archive = work.join("spice.zip");
    let mut net = crate::net::UreqNet::new();
    let with_progress = progress.is_some();
    if let Some(cb) = progress {
        net = net.with_progress(cb);
    }
    let url = site_url(&s.file);
    let result = if !crate::ocs::http_download(&mut net, &url, &archive.to_string_lossy(), with_progress, DOWNLOAD_TIMEOUT_SECS, MAX_ARCHIVE_BYTES) {
        (false, trf("Couldn't download %1 from Cinnamon Spices. Check your internet connection and try again.", &[&name]))
    } else {
        let web_url = page_url(t, &s.spices_id);
        let mut journal = |step: &SpiceStep, paths: &[String]| match step {
            SpiceStep::Record { .. } => journal_record(&env, item, t, &web_url, paths),
            _ => Ok(()),
        };
        let mut say = |l: &str| log(l);
        let section = part_section_of(item);
        install_archive_in(&env, t, &s, &name, &archive, &work, live_cinnamon_version(), section, &mut LiveSys, &mut journal, &mut say)
    };
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Снять спайс по плану [`plan_uninstall`] через `sys`.
pub fn uninstall_with(env: &SpiceEnv, t: &SpiceType, key: &str, name: &str, sys: &mut dyn SpiceSys, log: &mut dyn FnMut(&str)) -> (bool, String) {
    let locales = live_snapshot(Path::new(&locale_root(env)));
    let plan = match plan_uninstall(env, t, key, &locales) {
        Ok(s) => s,
        Err(r) => return (false, r.message(name)),
    };
    let log_path = crate::ocs_store::install_log_path_xdg(Some(&env.config), &env.home);
    let steps = with_journal_removal(&plan, &log_path, &item_id(t, key));
    let roots = [env.home.as_str(), env.config.as_str()];
    let mut journal = |step: &SpiceStep| match step {
        SpiceStep::Forget { content_id } => journal_edit(env, |d| {
            crate::ocs_store::forget_installed(d, content_id);
        }),
        _ => Ok(()),
    };
    let outcome = execute(&steps, &roots, sys, &mut journal, log);
    match outcome {
        Ok(_) => (true, trf("%1 is removed.", &[&name])),
        Err(e) => (false, trf("Couldn't remove %1: %2", &[&name, &e])),
    }
}

/// План снятия спайса `plan`, в котором файлы, записанные покупкой
/// `content_id` в общем журнале установки (рядом с журналом магазина
/// `log_path`), снимает журнал — только своё (шаг 3г слияния установщиков).
/// Выключение (списки gsettings, сброс темы) и уборка переводов и настроек —
/// шаги плана как были: в журнал они не пишутся. Покупки в журнале нет —
/// план как есть.
fn with_journal_removal(plan: &[SpiceStep], log_path: &str, content_id: &str) -> Vec<SpiceStep> {
    use crate::ext_manager::ExtStep;
    use crate::install_journal as journal;
    let view = journal::store_view(log_path);
    let Some(entry) = view.items.get(&journal::key_of(journal::Source::Store, content_id)) else {
        return plan.to_vec();
    };
    let r = journal::removal_plan(entry, &journal::probe_live);
    let mut ours: Vec<SpiceStep> = r
        .steps
        .into_iter()
        .filter_map(|st| match st {
            ExtStep::RemoveTree { path, .. } => Some(SpiceStep::RemoveTree { path }),
            ExtStep::RemoveFile { path } => Some(SpiceStep::RemoveFile { path }),
            _ => None,
        })
        .collect();
    ours.extend(r.prune.into_iter().map(|path| SpiceStep::RemoveDirIfEmpty { path }));
    let path_of = |st: &SpiceStep| match st {
        SpiceStep::RemoveTree { path } | SpiceStep::RemoveFile { path } => Some(path.clone()),
        _ => None,
    };
    journal::merge_into_plan(plan, &path_of, entry, ours)
}

/// «Удалить» спайса из журнала установленного (`spices:<тип>:<ключ>`).
pub fn live_uninstall(content_id: &str, mut log: Box<dyn FnMut(&str) + Send>) -> (bool, String) {
    let Some((t, key)) = parse_item_id(content_id) else {
        return (false, trf("%1 isn't from Cinnamon Spices.", &[&content_id]));
    };
    let env = SpiceEnv::live();
    let path = crate::ocs_store::install_log_path_xdg(Some(&env.config), &env.home);
    let data = crate::install_journal::store_log_view(&path);
    let name = data.get(content_id).and_then(|r| r.get("name")).and_then(Value::as_str).filter(|n| !n.is_empty()).unwrap_or(key).to_string();
    uninstall_with(&env, t, key, &name, &mut LiveSys, &mut |l| log(l))
}

/// После «Удалить» на плитке с диска (`theme_removal::commit` снял каталог
/// или `.nemo_action`) — убрать остальное, что спайс оставил: переводы,
/// настройки, у действия — его каталог. Только файлы под `env.home` и
/// каталогом настроек; списки gsettings не трогаются (включённое удалить
/// нельзя, журнал правит сам `commit`). Темы и пути не из каталогов
/// спайсов — ничего не делает.
pub fn cleanup_after_removal(env: &SpiceEnv, path: &str) {
    let p = path.trim_end_matches('/');
    let Some((parent, name)) = p.rsplit_once('/') else { return };
    for t in TYPES.iter().filter(|t| !t.is_theme()) {
        if install_root(env, t) != parent {
            continue;
        }
        let key = if t.is_action() {
            match name.strip_suffix(".nemo_action") {
                Some(k) => k,
                None => continue,
            }
        } else {
            name
        };
        let locales = live_snapshot(Path::new(&locale_root(env)));
        let Ok(steps) = plan_uninstall(env, t, key, &locales) else { continue };
        let files: Vec<SpiceStep> = steps
            .into_iter()
            .filter(|s| matches!(s, SpiceStep::RemoveTree { .. } | SpiceStep::RemoveFile { .. } | SpiceStep::RemoveDirIfEmpty { .. }))
            .collect();
        let roots = [env.home.as_str(), env.config.as_str()];
        if let Err(e) = execute(&files, &roots, &mut NoSys, &mut |_| Ok(()), &mut |l| crate::log_warn!("theme", "{l}")) {
            crate::log_warn!("theme", "остатки спайса {key} не сняты: {e}");
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Карточка витрины: строки, кнопка, предупреждение
// ═══════════════════════════════════════════════════════════════════════

/// Вызовы, которые Cinnamon выполняет, остановившись до ответа: спайс с
/// ними может подвешивать оболочку (то же правило, что у окна настроек
/// Cinnamon). Установку это не запрещает — предупреждает.
pub const BLOCKING_CALLS: [&str; 4] = ["spawn_sync", "spawn_command_line_sync", "GTop", "get_file_contents_utf8_sync"];

/// Есть ли в `.js` установленного спайса блокирующие вызовы
/// ([`BLOCKING_CALLS`]). Читается не больше 4 МБ на файл и 2000 файлов;
/// ссылки не разворачиваются.
pub fn scan_for_danger(dir: &Path) -> bool {
    fn walk(dir: &Path, budget: &mut u32) -> bool {
        let Ok(rd) = std::fs::read_dir(dir) else { return false };
        for e in rd.flatten() {
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            let path = e.path();
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            if md.is_dir() {
                if walk(&path, budget) {
                    return true;
                }
            } else if md.is_file() && md.len() <= 4 << 20 && path.extension().is_some_and(|x| x == "js") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if BLOCKING_CALLS.iter().any(|c| text.contains(c)) {
                        return true;
                    }
                }
            }
        }
        false
    }
    let mut budget = 2000;
    walk(dir, &mut budget)
}

/// Каталог установленного спайса для проверки [`scan_for_danger`]; у тем
/// кода нет — `None`.
pub fn code_dir(env: &SpiceEnv, t: &SpiceType, key: &str) -> Option<String> {
    (!t.is_theme() && valid_key(key)).then(|| snap_join(&install_root(env, t), key))
}

/// Строка-предупреждение на карточке.
pub fn danger_line() -> String {
    tr("⚠ May freeze Cinnamon for a moment")
}

/// Подсказка к предупреждению.
pub fn danger_tooltip() -> String {
    tr("This item runs commands and waits for them to finish. While it waits, the whole Cinnamon desktop stops responding — usually for a moment, but a slow command can freeze it for longer.")
}

/// Стат-строка карточки после значка-звезды плитки: «N · обновлено
/// ГГГГ.ММ.ДД» (N — «лайки» сайта; чисел скачиваний у сайта нет).
pub fn stats_line(score: i64, last_edited: i64) -> String {
    let date = version_label(last_edited);
    let n = format!("\u{a0}{}", score.max(0));
    if date.is_empty() {
        n
    } else {
        format!("{n} · {}", trf("updated %1", &[&date]))
    }
}

/// Подсказка к стат-строке.
pub fn stats_tooltip() -> String {
    tr("How many people liked it on the Cinnamon Spices website, and when it was last changed")
}

/// «Лайки» в деталях: «⭐ N».
pub fn detail_stars(score: i64) -> String {
    format!("⭐ {}", score.max(0))
}

/// Кнопка карточки: стоит и на сайте новее — «Обновить», иначе как у
/// любого товара магазина (у спайсов «Применить» на карточке витрины нет:
/// включают его плитка установленного выше и её кнопка).
pub fn card_button(installed: bool, has_update: bool) -> crate::appearance_page::CardButton {
    card_button_for(installed, has_update, false)
}

/// [`card_button`] с «Применить» у поставленного (`can_apply`): так у темы
/// Spices в разделах GTK и рамок окон ([`PART_SECTIONS`]) — сетки
/// установленного со своей кнопкой там может не быть (тема без GTK 3/4 в
/// «Темах GTK» не видна), и включает её сама карточка.
pub fn card_button_for(installed: bool, has_update: bool, can_apply: bool) -> crate::appearance_page::CardButton {
    if installed && has_update {
        return crate::appearance_page::CardButton { text: tr("⬆ Update"), state: "false", enabled: true };
    }
    crate::appearance_page::card_install_button(installed, false, can_apply)
}

/// Главная кнопка деталей — те же состояния, что у [`card_button`].
pub fn detail_button(installed: bool, has_update: bool) -> crate::appearance_page::InstallButton {
    detail_button_for(installed, has_update, false)
}

/// [`detail_button`] с «Применить» у поставленного (см. [`card_button_for`]).
pub fn detail_button_for(installed: bool, has_update: bool, can_apply: bool) -> crate::appearance_page::InstallButton {
    if installed && has_update {
        return crate::appearance_page::InstallButton {
            text: tr("⬆ Update"),
            tooltip: tr("A newer version is on the Cinnamon Spices website. It's installed over this one; its settings are kept."),
            state: "false",
        };
    }
    crate::appearance_page::install_button(installed, false, can_apply)
}

#[cfg(test)]
mod uninstall_tests {
    use super::*;

    fn env() -> SpiceEnv {
        SpiceEnv { home: "/home/u".into(), config: "/home/u/.cfg".into() }
    }

    fn none() -> FsSnapshot {
        FsSnapshot::from_json(&serde_json::json!({}))
    }

    /// Снимок каталога переводов: `ru` и `pt_BR` с `.mo` спайса, `de` — без.
    fn locales(key: &str) -> FsSnapshot {
        let entries = vec![
            format!("ru/LC_MESSAGES/{key}.mo"),
            format!("pt_BR/LC_MESSAGES/{key}.mo"),
            "de/LC_MESSAGES/other@x.mo".to_string(),
        ];
        snapshot_from_entries("/home/u/.local/share/locale", &entries, &Map::new())
    }

    #[test]
    fn update_is_older_mark_only() {
        assert!(has_update(Some(100), 200));
        assert!(!has_update(Some(200), 200));
        assert!(!has_update(Some(300), 200));
        assert!(!has_update(None, 200), "без отметки сравнивать не с чем");
        assert_eq!(installed_last_edited(&serde_json::json!({"last-edited": 1_700_000_000})), Some(1_700_000_000));
        assert_eq!(installed_last_edited(&serde_json::json!({"last-edited": " 42 "})), Some(42));
        assert_eq!(installed_last_edited(&serde_json::json!({"last-edited": 4.2e1})), Some(42));
        assert_eq!(installed_last_edited(&serde_json::json!({"uuid": "a@b"})), None);
        assert_eq!(installed_last_edited(&serde_json::json!({"last-edited": "вчера"})), None);
    }

    #[test]
    fn list_without_matches_exact_field() {
        let applets: Vec<String> = [
            "panel1:left:0:menu@cinnamon.org",
            "panel1:right:0:weather@mockturtl:8",
            "panel1:right:1:!weather@mockturtl:9",
            "panel1:right:2:xweather@mockturtl:10",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            list_without(&applets, "weather@mockturtl", Some(3)),
            vec!["panel1:left:0:menu@cinnamon.org".to_string(), "panel1:right:2:xweather@mockturtl:10".to_string()]
        );
        // Штатная запись без номера экземпляра тоже находится.
        assert_eq!(list_without(&applets, "menu@cinnamon.org", Some(3)).len(), 3);
        let desklets = vec!["a@b:0:100:100".to_string(), "xa@b:1:10:10".to_string()];
        assert_eq!(list_without(&desklets, "a@b", Some(0)), vec!["xa@b:1:10:10".to_string()]);
        let exts = vec!["e@x".to_string(), "e@xy".to_string()];
        assert_eq!(list_without(&exts, "e@x", None), vec!["e@xy".to_string()]);
    }

    #[test]
    fn applet_disabled_then_files_translations_settings_journal() {
        let key = "weather@mockturtl";
        let steps = plan_uninstall(&env(), type_for_key("applet").unwrap(), key, &locales(key)).unwrap();
        let l = "/home/u/.local/share/locale";
        assert_eq!(
            steps,
            vec![
                SpiceStep::GsettingsListDrop { schema: "org.cinnamon".into(), key: "enabled-applets".into(), item: key.into(), field: Some(3) },
                SpiceStep::RemoveTree { path: format!("/home/u/.local/share/cinnamon/applets/{key}") },
                SpiceStep::RemoveFile { path: format!("{l}/pt_BR/LC_MESSAGES/{key}.mo") },
                SpiceStep::RemoveDirIfEmpty { path: format!("{l}/pt_BR/LC_MESSAGES") },
                SpiceStep::RemoveDirIfEmpty { path: format!("{l}/pt_BR") },
                SpiceStep::RemoveFile { path: format!("{l}/ru/LC_MESSAGES/{key}.mo") },
                SpiceStep::RemoveDirIfEmpty { path: format!("{l}/ru/LC_MESSAGES") },
                SpiceStep::RemoveDirIfEmpty { path: format!("{l}/ru") },
                SpiceStep::RemoveTree { path: format!("/home/u/.cfg/cinnamon/spices/{key}") },
                SpiceStep::RemoveTree { path: format!("/home/u/.cinnamon/configs/{key}") },
                SpiceStep::Forget { content_id: format!("spices:applet:{key}") },
            ]
        );
    }

    #[test]
    fn desklet_and_extension_use_their_lists() {
        let empty = none();
        let d = plan_uninstall(&env(), type_for_key("desklet").unwrap(), "AlbumArt3.0@claudiux", &empty).unwrap();
        assert_eq!(
            d[0],
            SpiceStep::GsettingsListDrop {
                schema: "org.cinnamon".into(),
                key: "enabled-desklets".into(),
                item: "AlbumArt3.0@claudiux".into(),
                field: Some(0)
            }
        );
        assert_eq!(d[1], SpiceStep::RemoveTree { path: "/home/u/.local/share/cinnamon/desklets/AlbumArt3.0@claudiux".into() });
        let e = plan_uninstall(&env(), type_for_key("extension").unwrap(), "x@y", &empty).unwrap();
        assert_eq!(e[0], SpiceStep::GsettingsListDrop { schema: "org.cinnamon".into(), key: "enabled-extensions".into(), item: "x@y".into(), field: None });
        assert!(!e.iter().any(|s| matches!(s, SpiceStep::RemoveFile { .. })), "переводов нет — и снимать нечего");
    }

    #[test]
    fn theme_resets_only_if_current_and_keeps_others() {
        let steps = plan_uninstall(&env(), type_for_key("theme").unwrap(), "Adara-Dark", &locales("Adara-Dark")).unwrap();
        assert_eq!(
            steps,
            vec![
                SpiceStep::GsettingsResetIf { schema: "org.cinnamon.theme".into(), key: "name".into(), value: "Adara-Dark".into() },
                SpiceStep::RemoveTree { path: "/home/u/.themes/Adara-Dark".into() },
                SpiceStep::Forget { content_id: "spices:theme:Adara-Dark".into() },
            ]
        );
    }

    #[test]
    fn action_removes_both_roots_and_its_disabled_line() {
        let key = "backup-this-file@h3nr1ke";
        let steps = plan_uninstall(&env(), type_for_key("action").unwrap(), key, &none()).unwrap();
        let d = "/home/u/.local/share/nemo/actions";
        assert_eq!(steps[0], SpiceStep::RemoveFile { path: format!("{d}/{key}.nemo_action") }, "сначала пропадает пункт меню");
        assert_eq!(steps[1], SpiceStep::RemoveTree { path: format!("{d}/{key}") });
        assert_eq!(
            steps[2],
            SpiceStep::GsettingsListDrop { schema: "org.nemo.plugins".into(), key: "disabled-actions".into(), item: format!("{key}.nemo_action"), field: None }
        );
        assert_eq!(steps.last(), Some(&SpiceStep::Forget { content_id: format!("spices:action:{key}") }));
    }

    #[test]
    fn bad_key_refused_before_any_step() {
        for bad in ["", "..", "../x", "a/b"] {
            assert_eq!(plan_uninstall(&env(), type_for_key("applet").unwrap(), bad, &none()), Err(SpiceRefusal::InvalidKey));
        }
    }
}

#[cfg(test)]
mod exec_tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;

    /// gsettings в памяти: `(схема, ключ)` → список или строка.
    #[derive(Default)]
    struct MemSys {
        lists: HashMap<(String, String), Vec<String>>,
        strings: HashMap<(String, String), String>,
        resets: Vec<(String, String)>,
        reloads: Vec<(String, String)>,
    }

    impl SpiceSys for MemSys {
        fn list_get(&mut self, schema: &str, key: &str) -> Option<Vec<String>> {
            Some(self.lists.get(&(schema.into(), key.into())).cloned().unwrap_or_default())
        }
        fn list_set(&mut self, schema: &str, key: &str, items: &[String]) -> Result<(), String> {
            self.lists.insert((schema.into(), key.into()), items.to_vec());
            Ok(())
        }
        fn string_get(&mut self, schema: &str, key: &str) -> Option<String> {
            self.strings.get(&(schema.into(), key.into())).cloned()
        }
        fn reset(&mut self, schema: &str, key: &str) -> Result<(), String> {
            self.strings.remove(&(schema.into(), key.into()));
            self.resets.push((schema.into(), key.into()));
            Ok(())
        }
        fn reload_xlet(&mut self, uuid: &str, xlet: &str) -> Result<(), String> {
            self.reloads.push((uuid.into(), xlet.into()));
            Ok(())
        }
    }

    /// Временный HOME (файлы мелкие).
    struct Home {
        root: std::path::PathBuf,
        env: SpiceEnv,
    }

    impl Home {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("rexon-spices-exec-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("home")).unwrap();
            let home = root.join("home").to_string_lossy().into_owned();
            Home { env: SpiceEnv::at_home(&home), root }
        }
        fn work(&self, n: u32) -> std::path::PathBuf {
            let w = self.root.join(format!("work{n}"));
            std::fs::create_dir_all(&w).unwrap();
            w
        }
        /// Дерево под HOME: «путь права» по алфавиту (как `find -printf '%P %m'`).
        fn tree(&self) -> Vec<String> {
            fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
                use std::os::unix::fs::PermissionsExt;
                for e in std::fs::read_dir(dir).unwrap().flatten() {
                    let md = std::fs::symlink_metadata(e.path()).unwrap();
                    let rel = e.path().strip_prefix(base).unwrap().to_string_lossy().into_owned();
                    out.push(format!("{rel} {:o}", md.permissions().mode() & 0o777));
                    if md.is_dir() {
                        walk(base, &e.path(), out);
                    }
                }
            }
            let mut out = Vec::new();
            let base = Path::new(&self.env.home);
            walk(base, base, &mut out);
            out.sort();
            out
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn trees() -> Value {
        serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/trees.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap()
    }

    /// Собрать zip из фикстуры: имена, права, тексты `metadata.json` и
    /// `.nemo_action`; у `da.po` апплета 1440 — настоящий перевод.
    fn make_zip(archive: &str, dest: &Path) {
        let all = trees();
        let t = &all[archive];
        let mut zw = zip::ZipWriter::new(std::fs::File::create(dest).unwrap());
        for e in t["entries"].as_array().unwrap() {
            let name = e[0].as_str().unwrap();
            let mode = e[1].as_u64().unwrap() as u32;
            let opts = zip::write::SimpleFileOptions::default().unix_permissions(mode);
            if name.ends_with('/') {
                zw.add_directory(name, opts).unwrap();
            } else {
                zw.start_file(name, opts).unwrap();
                let body = if name == "1440@jvlianodorneles/po/da.po" {
                    std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/po/1440-da.po", env!("CARGO_MANIFEST_DIR"))).unwrap()
                } else {
                    t["texts"].get(name).and_then(Value::as_str).unwrap_or("").to_string()
                };
                zw.write_all(body.as_bytes()).unwrap();
            }
        }
        zw.finish().unwrap();
    }

    fn spice(key: &str, id: Value, ts: i64) -> Spice {
        Spice { key: key.into(), spices_id: id, last_edited: ts, name: key.into(), ..Default::default() }
    }

    type Journal = Vec<(String, Vec<String>)>;

    fn install(h: &Home, n: u32, archive: &str, kind: &str, s: &Spice, sys: &mut MemSys, journal: &mut Journal) -> (bool, String) {
        let work = h.work(n);
        let zip = work.join("a.zip");
        make_zip(archive, &zip);
        let t = type_for_key(kind).unwrap();
        let mut j = |st: &SpiceStep, paths: &[String]| {
            if let SpiceStep::Record { content_id, .. } = st {
                journal.push((content_id.clone(), paths.to_vec()));
            }
            Ok::<(), String>(())
        };
        install_archive(&h.env, t, s, &s.name, &zip, &work, Some((6, 6, 9)), sys, &mut j, &mut |_| {})
    }

    #[test]
    fn applet_install_tree_metadata_translations_update_and_removal() {
        let h = Home::new("applet");
        let mut sys = MemSys::default();
        let mut journal = Vec::new();
        let key = "1440@jvlianodorneles";
        let (ok, msg) = install(&h, 0, key, "applet", &spice(key, Value::from(427), 1_700_000_000), &mut sys, &mut journal);
        assert!(ok, "{msg}");
        // Дерево: каждая запись архива — с его правами.
        let tree = h.tree();
        let all = trees();
        for e in all[key]["entries"].as_array().unwrap() {
            let name = e[0].as_str().unwrap().trim_end_matches('/');
            let want = format!(".local/share/cinnamon/applets/{name} {:o}", e[1].as_u64().unwrap());
            assert!(tree.contains(&want), "нет {want}: {tree:#?}");
        }
        assert!(tree.iter().any(|l| l.starts_with(&format!(".local/share/locale/da/LC_MESSAGES/{key}.mo "))), "{tree:#?}");
        // last-edited дописан в конец, прежние ключи — в прежнем порядке, отступ 4.
        let dir = format!(".local/share/cinnamon/applets/{key}");
        let meta = std::fs::read_to_string(format!("{}/{dir}/metadata.json", h.env.home)).unwrap();
        let v: Value = serde_json::from_str(&meta).unwrap();
        assert_eq!(installed_last_edited(&v), Some(1_700_000_000));
        let orig: Value = serde_json::from_str(all[key]["texts"][format!("{key}/metadata.json")].as_str().unwrap()).unwrap();
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        let orig_keys: Vec<&String> = orig.as_object().unwrap().keys().collect();
        assert_eq!(&keys[..orig_keys.len()], &orig_keys[..]);
        assert_eq!(keys.last().map(|k| k.as_str()), Some("last-edited"));
        assert!(meta.contains("\n    \""), "отступ 4: {meta}");
        assert_eq!(journal, vec![(format!("spices:applet:{key}"), vec![format!("{}/{dir}", h.env.home)])]);
        assert_eq!(installed_state(&h.env, type_for_key("applet").unwrap(), key), Some(Some(1_700_000_000)));
        assert!(sys.reloads.is_empty(), "свежий апплет не включён — перезагружать нечего");

        // Обновление включённого: настройки целы, ReloadXlet позван.
        let cfg = format!("{}/cinnamon/spices/{key}", h.env.config);
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(format!("{cfg}/1.json"), "{}").unwrap();
        sys.lists.insert(("org.cinnamon".into(), "enabled-applets".into()), vec![format!("panel1:right:0:{key}:3")]);
        let (ok, msg) = install(&h, 1, key, "applet", &spice(key, Value::from(427), 1_800_000_000), &mut sys, &mut journal);
        assert!(ok, "{msg}");
        assert_eq!(sys.reloads, vec![(key.to_string(), "APPLET".to_string())]);
        assert!(Path::new(&format!("{cfg}/1.json")).is_file(), "настройки пережили обновление");
        assert_eq!(installed_state(&h.env, type_for_key("applet").unwrap(), key), Some(Some(1_800_000_000)));

        // Удаление: выключен, каталог, перевод и настройки сняты.
        let (ok, msg) = uninstall_with(&h.env, type_for_key("applet").unwrap(), key, key, &mut sys, &mut |_| {});
        assert!(ok, "{msg}");
        assert!(sys.lists[&("org.cinnamon".to_string(), "enabled-applets".to_string())].is_empty());
        let left = h.tree();
        assert!(!left.iter().any(|l| l.contains(key)), "{left:#?}");
        assert!(!left.iter().any(|l| l.starts_with(".local/share/locale/da")), "пустой каталог языка снят: {left:#?}");
    }

    #[test]
    fn action_two_roots_disabled_then_removed() {
        let h = Home::new("action");
        let mut sys = MemSys::default();
        let mut journal = Vec::new();
        let key = "backup-this-file@h3nr1ke";
        let (ok, msg) = install(&h, 0, key, "action", &spice(key, Value::from(37), 1_700_000_000), &mut sys, &mut journal);
        assert!(ok, "{msg}");
        let d = ".local/share/nemo/actions";
        let tree = h.tree();
        assert!(tree.iter().any(|l| l.starts_with(&format!("{d}/{key}.nemo_action "))), "{tree:#?}");
        assert!(tree.iter().any(|l| l.starts_with(&format!("{d}/{key}/metadata.json "))), "{tree:#?}");
        assert!(tree.iter().any(|l| l.starts_with(&format!("{d}/{key}/")) && l.ends_with(".sh 755")), "скрипт исполняемый: {tree:#?}");
        let disabled = ("org.nemo.plugins".to_string(), "disabled-actions".to_string());
        assert_eq!(sys.lists[&disabled], vec![format!("{key}.nemo_action")], "свежее действие выключено");
        assert_eq!(journal[0].1.len(), 2, "журнал помнит каталог и файл: {journal:?}");
        // Человек включил; обновление список не трогает.
        sys.lists.insert(disabled.clone(), Vec::new());
        let (ok, _) = install(&h, 1, key, "action", &spice(key, Value::from(37), 1_800_000_000), &mut sys, &mut journal);
        assert!(ok);
        assert!(sys.lists[&disabled].is_empty());
        let (ok, msg) = uninstall_with(&h.env, type_for_key("action").unwrap(), key, key, &mut sys, &mut |_| {});
        assert!(ok, "{msg}");
        assert!(!h.tree().iter().any(|l| l.contains(key)), "{:#?}", h.tree());
    }

    /// Шаг 3г: покупка записана в журнале магазина — каталог и файл действия
    /// снимает общий журнал установки, выключение и уборка переводов и
    /// настроек — план, как прежде; общий журнал сверен и записи нет.
    /// На месте каталога спайса — ссылка человека: снимается сама ссылка,
    /// то, на что она ведёт, цело (как и прежде).
    #[test]
    fn uninstall_goes_by_install_journal_when_recorded() {
        let h = Home::new("by-journal");
        let mut sys = MemSys::default();
        let mut journal = Vec::new();
        let key = "backup-this-file@h3nr1ke";
        let t = type_for_key("action").unwrap();
        let (ok, msg) = install(&h, 0, key, "action", &spice(key, Value::from(37), 1_700_000_000), &mut sys, &mut journal);
        assert!(ok, "{msg}");
        let record = |paths: &[String]| {
            let log = crate::ocs_store::install_log_path_xdg(Some(&h.env.config), &h.env.home);
            std::fs::create_dir_all(Path::new(&log).parent().unwrap()).unwrap();
            let card = serde_json::json!({ item_id(t, key): {"name": key, "rexon_type": "nemo-actions", "paths": paths} });
            std::fs::write(&log, card.to_string()).unwrap();
            log
        };
        let log = record(&journal[0].1);
        let rexon = Path::new(&log).parent().unwrap().to_path_buf();
        let cfg = format!("{}/cinnamon/spices/{key}", h.env.config);
        std::fs::create_dir_all(&cfg).unwrap();
        let (ok, msg) = uninstall_with(&h.env, t, key, key, &mut sys, &mut |_| {});
        assert!(ok, "{msg}");
        assert!(!h.tree().iter().any(|l| l.contains(key)), "{:#?}", h.tree());
        assert!(crate::ocs_store::parse_installed_log(std::fs::read_to_string(&log).ok().as_deref()).is_empty());
        let general = crate::install_journal::parse(std::fs::read_to_string(crate::install_journal::journal_path(&rexon)).ok().as_deref())
            .expect("общий журнал сверен");
        assert!(general.items.is_empty(), "{general:#?}");

        // Чужое на месте каталога: человек поставил туда ссылку на своё.
        // Журнал магазина вида путей не хранит, сверка смотрит по диску.
        let (ok, msg) = install(&h, 1, key, "action", &spice(key, Value::from(37), 1_700_000_000), &mut sys, &mut journal);
        assert!(ok, "{msg}");
        let dir = format!("{}/{key}", install_root(&h.env, t));
        record(&journal[1].1);
        std::fs::remove_dir_all(&dir).unwrap();
        let mine = h.root.join("mine");
        std::fs::create_dir_all(&mine).unwrap();
        std::os::unix::fs::symlink(&mine, &dir).unwrap();
        // Сверка видит ссылку: в журнале теперь ссылка — и снимается сама
        // ссылка, а не то, на что она ведёт.
        let (ok, msg) = uninstall_with(&h.env, t, key, key, &mut sys, &mut |_| {});
        assert!(ok, "{msg}");
        assert!(mine.is_dir(), "снято то, на что ведёт ссылка");
        assert!(std::fs::symlink_metadata(&dir).is_err());
    }

    #[test]
    fn theme_desklet_extension_multiversion_install_where_cinnamon_looks() {
        let h = Home::new("misc");
        let mut sys = MemSys::default();
        let mut journal = Vec::new();
        for (n, (archive, kind, dir)) in [
            ("Adara-Dark", "theme", ".themes/Adara-Dark"),
            ("AlbumArt3.0@claudiux", "desklet", ".local/share/cinnamon/desklets/AlbumArt3.0@claudiux"),
            ("ArrangementOfButtonsOnWindowTitleBar@claudiux", "extension", ".local/share/cinnamon/extensions/ArrangementOfButtonsOnWindowTitleBar@claudiux"),
            ("weather@mockturtl", "applet", ".local/share/cinnamon/applets/weather@mockturtl"),
        ]
        .into_iter()
        .enumerate()
        {
            let (ok, msg) = install(&h, n as u32, archive, kind, &spice(archive, Value::from(1), 1_700_000_000), &mut sys, &mut journal);
            assert!(ok, "{archive}: {msg}");
            let meta: Value = serde_json::from_str(&std::fs::read_to_string(format!("{}/{dir}/metadata.json", h.env.home)).unwrap()).unwrap();
            assert_eq!(installed_last_edited(&meta), Some(1_700_000_000), "{archive}");
        }
        // Права скриптов десклета сохранились (0755 в архиве).
        let tree = h.tree();
        assert!(tree.iter().any(|l| l.contains("AlbumArt3.0@claudiux/scripts/") && l.ends_with(" 755")), "{tree:#?}");
        // Тема выбрана — удаление сбрасывает её к умолчанию.
        sys.strings.insert(("org.cinnamon.theme".into(), "name".into()), "Adara-Dark".into());
        let (ok, _) = uninstall_with(&h.env, type_for_key("theme").unwrap(), "Adara-Dark", "Adara Dark", &mut sys, &mut |_| {});
        assert!(ok);
        assert_eq!(sys.resets, vec![("org.cinnamon.theme".to_string(), "name".to_string())]);
        assert!(!Path::new(&format!("{}/.themes/Adara-Dark", h.env.home)).exists());
    }

    #[test]
    fn refusals_leave_home_untouched() {
        let h = Home::new("refuse");
        let mut sys = MemSys::default();
        let mut journal = Vec::new();
        // Ключ индекса не совпадает с каталогом в архиве.
        let (ok, msg) = install(&h, 0, "1440@jvlianodorneles", "applet", &spice("other@x", Value::from(1), 1), &mut sys, &mut journal);
        assert!(!ok && msg.contains("other@x"), "{msg}");
        // Не zip (страница WAF вместо архива).
        let work = h.work(1);
        std::fs::write(work.join("a.zip"), "<html>WAF</html>").unwrap();
        let (ok, _) = install_archive(
            &h.env,
            type_for_key("applet").unwrap(),
            &spice("a@b", Value::Null, 1),
            "a@b",
            &work.join("a.zip"),
            &work,
            None,
            &mut sys,
            &mut |_, _| Ok(()),
            &mut |_| {},
        );
        assert!(!ok);
        assert!(h.tree().is_empty(), "{:#?}", h.tree());
        assert!(journal.is_empty());
    }

    #[test]
    fn executor_refuses_paths_outside_roots() {
        let mut sys = MemSys::default();
        for p in ["/etc/x", "/home/u/../etc", "/home/u/a/../../etc", "/home/uu/x", "/home/u"] {
            let steps = [SpiceStep::RemoveTree { path: p.into() }];
            let err = execute(&steps, &["/home/u"], &mut sys, &mut |_| Ok(()), &mut |_| {}).unwrap_err();
            assert!(err.contains(p), "{p}: {err}");
        }
    }

    #[test]
    fn last_edited_keeps_order_and_creates_for_themes() {
        let out = with_last_edited(Some(r#"{"uuid":"a@b","name":"Ä","last-edited":1,"z":2}"#), 5).unwrap();
        assert_eq!(out, "{\n    \"uuid\": \"a@b\",\n    \"name\": \"Ä\",\n    \"last-edited\": 5,\n    \"z\": 2\n}");
        assert_eq!(with_last_edited(None, 7).unwrap(), "{\n    \"last-edited\": 7\n}");
        assert!(with_last_edited(Some("[1]"), 7).is_err());
    }

    #[test]
    fn cleanup_after_tile_removal_takes_translations_settings_and_action_dir() {
        let h = Home::new("cleanup");
        let home = h.env.home.clone();
        let key = "a@b";
        for p in [
            format!("{home}/.local/share/locale/ru/LC_MESSAGES/{key}.mo"),
            format!("{home}/.local/share/locale/ru/LC_MESSAGES/keep.mo"),
            format!("{}/cinnamon/spices/{key}/1.json", h.env.config),
            format!("{home}/.local/share/nemo/actions/{key}/run.sh"),
        ] {
            std::fs::create_dir_all(Path::new(&p).parent().unwrap()).unwrap();
            std::fs::write(&p, "").unwrap();
        }
        // Плитка снята: `.nemo_action` уже нет.
        cleanup_after_removal(&h.env, &format!("{home}/.local/share/nemo/actions/{key}.nemo_action"));
        let tree = h.tree();
        assert!(!tree.iter().any(|l| l.contains(key)), "{tree:#?}");
        assert!(tree.iter().any(|l| l.contains("keep.mo")), "чужой перевод на месте: {tree:#?}");
        // Путь не из каталога спайсов — ничего.
        cleanup_after_removal(&h.env, &format!("{home}/.local/share/locale"));
        assert!(h.tree().iter().any(|l| l.contains("keep.mo")));
    }
}

#[cfg(test)]
mod card_tests {
    use super::*;

    #[test]
    fn danger_scan_finds_blocking_calls_in_js_only() {
        let dir = std::env::temp_dir().join(format!("rexon-spices-danger-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("3.8")).unwrap();
        std::fs::write(dir.join("applet.js"), "const x = 1;").unwrap();
        std::fs::write(dir.join("README.md"), "GLib.spawn_command_line_sync").unwrap();
        assert!(!scan_for_danger(&dir), "README не код");
        std::fs::write(dir.join("3.8/applet.js"), "let [ok, out] = GLib.spawn_command_line_sync('ls');").unwrap();
        assert!(scan_for_danger(&dir), "подкаталог версии тоже проверяется");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!scan_for_danger(Path::new("/nonexistent/rexon")));
        let env = SpiceEnv::at_home("/home/u");
        assert_eq!(code_dir(&env, type_for_key("theme").unwrap(), "Adara-Dark"), None, "у темы кода нет");
        assert_eq!(
            code_dir(&env, type_for_key("applet").unwrap(), "a@b").as_deref(),
            Some("/home/u/.local/share/cinnamon/applets/a@b")
        );
        assert_eq!(code_dir(&env, type_for_key("applet").unwrap(), "../x"), None);
    }

    #[test]
    fn card_lines_and_buttons() {
        let line = stats_line(42, 1_780_583_449);
        assert!(line.contains("42") && line.contains("2026.06.04") && !line.contains('★'), "{line}");
        assert_eq!(stats_line(-3, 0), "\u{a0}0");
        assert_ne!(card_button(true, true).text, crate::appearance_page::card_install_button(true, false, false).text);
        assert_eq!(card_button(true, false).text, crate::appearance_page::card_install_button(true, false, false).text);
        assert_eq!(card_button(false, true).text, crate::appearance_page::card_install_button(false, false, false).text, "не стоит — «Установить», а не «Обновить»");
        assert_ne!(detail_button(true, true).text, crate::appearance_page::install_button(true, false, false).text);
        assert!(!detail_button(true, true).tooltip.is_empty());
        assert_eq!(detail_stars(7), "⭐ 7");
        assert!(!danger_line().is_empty() && !danger_tooltip().is_empty());
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  Темы Spices в других окружениях: разделы GTK и рамок окон
// ═══════════════════════════════════════════════════════════════════════
//
// Тема Spices — это тема оболочки Cinnamon (`cinnamon/cinnamon.css`), но
// почти половина из них несёт в том же каталоге и тему приложений
// (`gtk-3.0`, `gtk-4.0`, `gtk-2.0`), и рамки окон (`metacity-1` — Marco и
// Metacity, `xfwm4`). Замер дерева репозитория тем 24.10.2026: из 108 тем
// 50 с таблицей GTK 3/4, 45 с рамками Metacity (ещё у одной каталог есть,
// а файла темы в нём нет), 16 с рамками xfwm4; 56 — только Cinnamon.
//
// Вне Cinnamon такая тема показывается в разделах «Темы GTK», «Рамки окон
// (XFCE)», «Рамки окон (MATE)» и «Тема GNOME Shell» (у четырёх тем есть и
// `gnome-shell`) — и только та, у которой есть часть этого раздела: показать тему без неё значило бы поставить то, что здесь не
// применится. Состав темы в индексе сайта не указан; его даёт дерево
// репозитория тем на GitHub — ОДИН запрос на все темы, раз в сутки,
// условный (`ETag` → 304). Ставится всё равно архив сайта, и при установке
// проверяется он сам ([`install_archive_in`]).

/// Разделы, где темы Spices показываются по своему составу: ключи разделов
/// «Оформления» (таблица — `categories`).
pub const PART_SECTIONS: &[&str] = &["gtk", "xfwm4", "marco", "gnomeshell"];

/// Дерево репозитория тем (все пути одним ответом). Внутри
/// `<ключ>/files/<ключ>/…` — ровно то, что сайт кладёт в архив `<ключ>/…`.
pub const THEME_TREE_URL: &str =
    "https://api.github.com/repos/linuxmint/cinnamon-spices-themes/git/trees/master?recursive=1";
/// Потолок дерева после распаковки (сейчас ~9 МБ).
pub const MAX_TREE_BYTES: u64 = 64 << 20;
/// Версия файла кэша состава тем.
pub const PARTS_CACHE_VERSION: u32 = 1;

/// Раздел ли это, где темы Spices показываются по составу.
pub fn is_part_section(section: &str) -> bool {
    PART_SECTIONS.contains(&section)
}

/// Раздел карточки, если она в разделе GTK или рамок окон
/// ([`PART_SECTIONS`]) — по номеру раздела в `category_id` (его ставит
/// [`section_theme_item`]).
pub fn part_section_of(item: &ThemeItem) -> Option<&'static str> {
    if !parse_item_id(&item.id).is_some_and(|(t, _)| t.is_theme()) {
        return None;
    }
    let key = crate::categories::category_by_id(Path::new(""), item.category_id)?.key;
    PART_SECTIONS.iter().copied().find(|s| *s == key)
}

/// Подкаталог части раздела — из матрицы (`Payload::ThemeSubdir`), не
/// второй копией: `xfwm4` у XFCE, `metacity-1` у MATE, `gnome-shell` у GNOME.
fn part_subdir(section: &str) -> Option<&'static str> {
    let kind = crate::categories::category_kind(section)?;
    match crate::de_customization::row(kind)?.payload {
        crate::de_customization::Payload::ThemeSubdir { sub } => Some(sub),
        _ => None,
    }
}

/// Файлы, без которых часть не прочтут: xfwm4 читает `themerc`, Marco и
/// Metacity — `metacity-theme-<N>.xml` (Marco знает форматы 1–3), оболочка
/// GNOME — `gnome-shell.css`.
fn part_markers(sub: &str) -> &'static [&'static str] {
    match sub {
        "xfwm4" => &["themerc"],
        "metacity-1" => &["metacity-theme-3.xml", "metacity-theme-2.xml", "metacity-theme-1.xml"],
        "gnome-shell" => &["gnome-shell.css"],
        _ => &[],
    }
}

/// Есть ли в теме то, что применит раздел `section`. `has(путь)` — есть ли
/// файл или каталог по пути от корня темы (`gtk-3.0/gtk.css`).
///
/// Тема приложений — то же правило, что у сетки «Тем GTK»
/// (`installed_scan::GTK_PARTS` с `gtk.css`): что витрина показала, то после
/// установки сетка и увидит. Рамки и оболочка GNOME — подкаталог раздела из
/// матрицы с файлом темы внутри: пустой `metacity-1` (бывает, замер выше)
/// Marco не прочтёт.
pub fn fits_section(section: &str, has: &dyn Fn(&str) -> bool) -> bool {
    if section == "gtk" {
        return crate::installed_scan::GTK_PARTS.iter().any(|p| has(&format!("{p}/gtk.css")));
    }
    if !is_part_section(section) {
        return false;
    }
    part_subdir(section).is_some_and(|sub| part_markers(sub).iter().any(|m| has(&format!("{sub}/{m}"))))
}

/// Состав темы для карточки: какие части в ней есть. Имена — названия
/// технологий, без перевода.
pub fn parts_summary(has: &dyn Fn(&str) -> bool) -> Vec<&'static str> {
    let any = |paths: &[&str]| paths.iter().any(|p| has(p));
    let mut out = Vec::new();
    if has("cinnamon/cinnamon.css") {
        out.push("Cinnamon");
    }
    if has("gtk-2.0/gtkrc") {
        out.push("GTK 2");
    }
    if has("gtk-3.0/gtk.css") {
        out.push("GTK 3");
    }
    if has("gtk-4.0/gtk.css") {
        out.push("GTK 4");
    }
    if any(&["metacity-1/metacity-theme-3.xml", "metacity-1/metacity-theme-2.xml", "metacity-1/metacity-theme-1.xml"]) {
        out.push("Metacity/Marco");
    }
    if has("xfwm4/themerc") {
        out.push("xfwm4");
    }
    if has("openbox-3/themerc") {
        out.push("Openbox");
    }
    if has("gnome-shell/gnome-shell.css") {
        out.push("GNOME Shell");
    }
    if has("plank") {
        out.push("Plank");
    }
    out
}

/// Состав всех тем: ключ темы → пути от корня темы на глубину до двух
/// (`gtk-3.0`, `gtk-3.0/gtk.css`), по алфавиту.
pub type ThemeParts = std::collections::BTreeMap<String, Vec<String>>;

/// `has` для [`fits_section`] по составу одной темы.
pub fn has_in(paths: &[String]) -> impl Fn(&str) -> bool + '_ {
    move |p: &str| paths.binary_search_by(|x| x.as_str().cmp(p)).is_ok()
}

/// Разобрать дерево репозитория тем ([`THEME_TREE_URL`]). Берутся только
/// пути `<ключ>/files/<ключ>/…` с годным ключом; обрезанное дерево
/// (`truncated`) — ошибка: половинный состав спрятал бы темы молча.
pub fn parse_theme_tree(body: &str) -> Result<ThemeParts, String> {
    #[derive(Deserialize)]
    struct Entry {
        path: String,
    }
    #[derive(Deserialize)]
    struct Tree {
        #[serde(default)]
        truncated: bool,
        tree: Vec<Entry>,
    }
    let tree: Tree = serde_json::from_str(body).map_err(|e| format!("дерево тем не разобрано: {e}"))?;
    if tree.truncated {
        return Err("дерево тем пришло обрезанным".to_string());
    }
    let mut out = ThemeParts::new();
    for e in &tree.tree {
        let mut it = e.path.splitn(4, '/');
        let (Some(k), Some("files"), Some(k2), Some(rest)) = (it.next(), it.next(), it.next(), it.next()) else {
            continue;
        };
        if k != k2 || !valid_key(k) || rest.is_empty() || rest.split('/').count() > 2 {
            continue;
        }
        out.entry(k.to_string()).or_default().push(rest.to_string());
    }
    for v in out.values_mut() {
        v.sort();
        v.dedup();
    }
    Ok(out)
}

/// Файл кэша состава тем: сам состав (не дерево — оно в десятки раз
/// больше) и валидаторы условного запроса.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PartsCache {
    pub version: u32,
    pub fetched: i64,
    pub failed: i64,
    pub etag: String,
    pub last_modified: String,
    pub parts: ThemeParts,
}

/// Путь кэша состава: `$XDG_CACHE_HOME/rexon/cinnamon-spices/theme-parts.json`.
pub fn parts_cache_path(xdg_cache_home: Option<&str>, home: &str) -> String {
    format!("{}/rexon/cinnamon-spices/theme-parts.json", crate::utils::cache_home(xdg_cache_home, home))
}

/// Итог [`refresh_parts`].
#[derive(Debug, Clone, PartialEq)]
pub struct PartsRefresh {
    pub parts: ThemeParts,
    pub source: RefreshSource,
    /// Что сказать человеку; пусто — всё в порядке.
    pub note: String,
}

/// Состав тем узнать не удалось.
pub fn parts_offline_message(have_saved: bool) -> String {
    if have_saved {
        tr("Couldn't check which Cinnamon Spices themes fit this section — the saved list is shown.")
    } else {
        tr("Couldn't find out which Cinnamon Spices themes fit this section (the list of theme contents comes from GitHub). Check your internet connection and try again later.")
    }
}

/// Состав тем для витрины — тем же порядком, что индекс ([`refresh`]):
/// свежий кэш (моложе суток) без сети; иначе (или по `force`) условный
/// запрос; 304 продлевает, 200 заменяет; сбой — прежний кэш с пометкой и
/// повтор не раньше [`FAIL_BACKOFF_SECS`].
pub fn refresh_parts(net: &mut dyn ConditionalNet, path: &Path, force: bool, now: i64) -> PartsRefresh {
    let saved: PartsCache = std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<PartsCache>(&t).ok())
        .filter(|c| c.version == PARTS_CACHE_VERSION)
        .unwrap_or(PartsCache { version: PARTS_CACHE_VERSION, ..Default::default() });
    let have = !saved.parts.is_empty();
    let fresh = have && saved.fetched > 0 && now >= saved.fetched && now - saved.fetched < CACHE_TTL_SECS;
    let backing_off = saved.failed > 0 && now >= saved.failed && now - saved.failed < FAIL_BACKOFF_SECS;
    let write = |c: &PartsCache| -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string(c).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    };
    if !force && (fresh || backing_off) {
        let (source, note) = if fresh { (RefreshSource::Cache, String::new()) } else { (RefreshSource::Stale, parts_offline_message(have)) };
        return PartsRefresh { parts: saved.parts, source, note };
    }
    let (etag, lm) = if have { (saved.etag.as_str(), saved.last_modified.as_str()) } else { ("", "") };
    let reply = net.conditional_get(THEME_TREE_URL, user_agent(), etag, lm, INDEX_TIMEOUT_SECS);
    let fail = |saved: PartsCache| {
        let have = !saved.parts.is_empty();
        let _ = write(&PartsCache { failed: now, ..saved.clone() });
        PartsRefresh { parts: saved.parts, source: RefreshSource::Stale, note: parts_offline_message(have) }
    };
    match reply {
        Ok(CondReply::NotModified) if have => {
            let _ = write(&PartsCache { fetched: now, failed: 0, ..saved.clone() });
            PartsRefresh { parts: saved.parts, source: RefreshSource::NotModified, note: String::new() }
        }
        Ok(CondReply::Body { body, etag, last_modified }) => match parse_theme_tree(&String::from_utf8_lossy(&body)) {
            Ok(parts) if !parts.is_empty() => {
                let c = PartsCache { version: PARTS_CACHE_VERSION, fetched: now, failed: 0, etag, last_modified, parts };
                let _ = write(&c);
                PartsRefresh { parts: c.parts, source: RefreshSource::Downloaded, note: String::new() }
            }
            // Мусор, обрезанное или пустое дерево кэш не затирают.
            _ => fail(saved),
        },
        Ok(CondReply::NotModified) | Err(_) => fail(saved),
    }
}

/// [`refresh_parts`] на живой машине: сжатый ответ GitHub, кэш этого
/// процесса.
pub fn live_refresh_parts(force: bool) -> PartsRefresh {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    let path = parts_cache_path(std::env::var("XDG_CACHE_HOME").ok().as_deref(), &home);
    refresh_parts(&mut UreqConditional::gzip_up_to(MAX_TREE_BYTES), Path::new(&path), force, now_secs())
}

/// Темы индекса, которые раздел `section` применит: по составу `parts`.
/// Темы, чей состав неизвестен (нет в дереве), не показываются — показать
/// и не суметь применить хуже, чем не показать.
pub fn themes_for_section(list: &[Spice], parts: &ThemeParts, section: &str) -> Vec<Spice> {
    list.iter()
        .filter(|s| parts.get(&s.key).is_some_and(|p| fits_section(section, &has_in(p))))
        .cloned()
        .collect()
}

/// Окружения, где рамки окон рисуются по теме GTK (Mutter и его потомки:
/// GNOME, Budgie, Pantheon), — там тема из «Тем GTK» меняет и рамки.
fn borders_follow_gtk(de: &str) -> bool {
    use crate::de_customization::{de_from_id, De};
    matches!(de_from_id(de), Some(De::Gnome | De::Budgie | De::Pantheon))
}

/// Честная оговорка карточки: что из темы применится здесь. В Cinnamon —
/// ничего не оговаривается (там тема включается целиком в своём разделе).
pub fn part_note(section: &str, de: &str) -> Option<String> {
    use crate::de_customization::{de_from_id, De};
    if de_from_id(de) == Some(De::Cinnamon) || !is_part_section(section) {
        return None;
    }
    Some(if section == "gnomeshell" {
        tr("The Cinnamon shell theme only works in Cinnamon; here the theme's look for the GNOME Shell will be applied.")
    } else if section != "gtk" {
        tr("The Cinnamon shell theme only works in Cinnamon; here the theme's window borders will be applied.")
    } else if borders_follow_gtk(de) {
        tr("The Cinnamon shell theme only works in Cinnamon; here the theme for applications and window borders will be applied.")
    } else {
        tr("The Cinnamon shell theme only works in Cinnamon; here the theme for applications (GTK) will be applied.")
    })
}

/// Отказ установки: в архиве нет части раздела (витрина ошиблась —
/// репозиторий и архив сайта разошлись).
pub fn part_missing_message(name: &str, section: &str) -> String {
    let part = match section {
        "gtk" => "GTK 3/GTK 4",
        "xfwm4" => "xfwm4",
        "gnomeshell" => "GNOME Shell",
        _ => "Metacity/Marco",
    };
    trf("%1 has no %2 part, so there's nothing to apply here — it wasn't installed.", &[&name, &part])
}

/// Карточка темы Spices в разделе `section` ([`PART_SECTIONS`]) окружения
/// `de`: номер раздела в `category_id` — им «Применить» найдёт свой путь
/// (тема GTK, рамки xfwm4, рамки Marco, оболочка GNOME — общий механизм
/// применения), в
/// описании сверху — оговорка [`part_note`] и состав темы.
pub fn section_theme_item(s: &Spice, lang: &str, section: &str, de: &str, parts: &[String]) -> ThemeItem {
    let t = type_for_key("theme").expect("тип тем есть в таблице");
    let mut it = theme_item(t, s, lang);
    if let Some(c) = crate::categories::categories(Path::new("")).into_iter().find(|c| c.key == section) {
        it.category_id = c.id;
    }
    let summary = parts_summary(&has_in(parts));
    let mut head: Vec<String> = part_note(section, de).into_iter().collect();
    if !summary.is_empty() {
        head.push(trf("Included in the theme: %1.", &[&summary.join(", ")]));
    }
    if !head.is_empty() {
        let body = std::mem::take(&mut it.description);
        it.description = if body.trim().is_empty() { head.join("\n") } else { format!("{}\n\n{body}", head.join("\n")) };
    }
    it
}

/// Нажатие карточки темы Spices в разделе GTK или рамок: тема стоит и на
/// сайте не новее — применить её (имя каталога темы); `None` — поставить
/// (или обновить). Решение по диску и сохранённому индексу.
pub fn part_apply_name(env: &SpiceEnv, item: &ThemeItem, index: &[Spice]) -> Option<String> {
    part_section_of(item)?;
    let (t, key) = parse_item_id(&item.id)?;
    let mark = installed_state(env, t, key)?;
    let newer = index.iter().find(|s| s.key == key).is_some_and(|s| has_update(mark, s.last_edited));
    (!newer).then(|| key.to_string())
}

/// [`part_apply_name`] на живой машине.
pub fn live_part_apply_name(item: &ThemeItem) -> Option<String> {
    part_section_of(item)?;
    let t = type_for_key("theme")?;
    let index = load_cache(Path::new(&live_cache_path(t))).map(|(_, l)| l).unwrap_or_default();
    part_apply_name(&SpiceEnv::live(), item, &index)
}

/// Подсказка чипа «Cinnamon Spices» в разделе GTK или рамок окон.
pub fn parts_chip_tooltip(section: &str) -> String {
    if section == "gtk" {
        tr("Themes from the Cinnamon Spices website that include a theme for applications (GTK 3 or GTK 4)")
    } else if section == "gnomeshell" {
        tr("Themes from the Cinnamon Spices website that include a GNOME Shell theme")
    } else {
        tr("Themes from the Cinnamon Spices website that include window borders for this desktop")
    }
}

#[cfg(test)]
mod part_tests {
    use super::*;
    use crate::net::NetError;
    use std::io::Write;

    /// Урезанный настоящий ответ GitHub (дерево репозитория тем, 24.10.2026):
    /// Winterbloom — Cinnamon + GTK 3; Orchidea — Cinnamon + Metacity;
    /// Aune — GTK 3/4, xfwm4 и `metacity-1` без файла темы; Adara-Dark —
    /// только Cinnamon; CBlack — GTK 2/3, Metacity, xfwm4. Плюс чужие пути:
    /// `.github`, каталог с другим ключом внутри, ключ с пробелом.
    fn tree_body() -> String {
        std::fs::read_to_string(format!("{}/tests/data/cinnamon_spices/theme_tree.json", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    fn parts() -> ThemeParts {
        parse_theme_tree(&tree_body()).unwrap()
    }

    fn spice(key: &str, ts: i64) -> Spice {
        Spice { key: key.into(), name: key.into(), description: "Desc".into(), last_edited: ts, file: format!("/files/themes/{key}.zip"), ..Default::default() }
    }

    fn keys(list: &[Spice]) -> Vec<&str> {
        list.iter().map(|s| s.key.as_str()).collect()
    }

    #[test]
    fn tree_gives_parts_of_each_theme_and_nothing_else() {
        let p = parts();
        assert_eq!(p.keys().map(String::as_str).collect::<Vec<_>>(), ["Adara-Dark", "Aune", "CBlack", "Orchidea", "Winterbloom"]);
        let w = &p["Winterbloom"];
        assert!(w.contains(&"gtk-3.0".to_string()) && w.contains(&"gtk-3.0/gtk.css".to_string()), "{w:?}");
        assert!(w.windows(2).all(|x| x[0] < x[1]), "по алфавиту, без повторов");
        assert!(!p["CBlack"].iter().any(|x| x.split('/').count() > 2), "глубже двух уровней не берётся");
        // Обрезанное дерево — ошибка, а не половина тем; мусор — тоже.
        let cut = tree_body().replacen("\"truncated\": false", "\"truncated\": true", 1);
        assert!(cut.contains("\"truncated\": true"));
        assert!(parse_theme_tree(&cut).is_err());
        assert!(parse_theme_tree("<html>rate limit</html>").is_err());
    }

    #[test]
    fn section_shows_only_themes_with_its_part() {
        let p = parts();
        let index: Vec<Spice> = ["Adara-Dark", "Aune", "CBlack", "Orchidea", "Winterbloom", "NotInTree"].iter().map(|k| spice(k, 1)).collect();
        assert_eq!(keys(&themes_for_section(&index, &p, "gtk")), ["Aune", "CBlack", "Winterbloom"]);
        assert_eq!(keys(&themes_for_section(&index, &p, "xfwm4")), ["Aune", "CBlack"]);
        // У Aune каталог metacity-1 есть, а файла темы в нём нет: Marco его
        // не прочтёт — в «Рамках окон (MATE)» её нет.
        assert_eq!(keys(&themes_for_section(&index, &p, "marco")), ["CBlack", "Orchidea"]);
        // Чужие разделы и сам раздел Cinnamon — не по составу.
        assert!(themes_for_section(&index, &p, "cinnamontheme").is_empty());
        assert!(themes_for_section(&index, &p, "icons").is_empty());
        // Оболочки GNOME ни у одной из пяти нет; у темы с ней — есть.
        assert!(themes_for_section(&index, &p, "gnomeshell").is_empty());
        let shell = vec!["gnome-shell".to_string(), "gnome-shell/gnome-shell.css".to_string()];
        assert!(fits_section("gnomeshell", &has_in(&shell)) && !fits_section("gtk", &has_in(&shell)));
        assert!(!fits_section("gnomeshell", &has_in(&shell[..1])), "пустой каталог — не тема оболочки");
        // Подкаталоги частей — из матрицы, а не второй копией.
        assert_eq!(part_subdir("xfwm4"), Some("xfwm4"));
        assert_eq!(part_subdir("marco"), Some("metacity-1"));
        assert_eq!(part_subdir("gnomeshell"), Some("gnome-shell"));
    }

    #[test]
    fn parts_summary_names_what_is_inside() {
        let p = parts();
        assert_eq!(parts_summary(&has_in(&p["CBlack"])), ["Cinnamon", "GTK 2", "GTK 3", "Metacity/Marco", "xfwm4"]);
        assert_eq!(parts_summary(&has_in(&p["Adara-Dark"])), ["Cinnamon"]);
        assert_eq!(parts_summary(&has_in(&p["Aune"])), ["Cinnamon", "GTK 3", "GTK 4", "xfwm4"]);
    }

    #[test]
    fn note_tells_honestly_what_applies_here() {
        assert_eq!(part_note("gtk", "cinnamon"), None, "в Cinnamon оговорка не нужна");
        assert_eq!(part_note("cinnamontheme", "mate"), None);
        let apps = part_note("gtk", "kde").unwrap();
        let apps_borders = part_note("gtk", "gnome").unwrap();
        let borders = part_note("marco", "mate").unwrap();
        assert_ne!(apps, apps_borders);
        assert_ne!(apps, borders);
        assert_eq!(part_note("gtk", "budgie").as_deref(), Some(apps_borders.as_str()), "Budgie: рамки — по теме GTK");
        assert_eq!(part_note("gtk", "xfce").as_deref(), Some(apps.as_str()), "у XFCE рамки — свой раздел");
        assert_eq!(part_note("xfwm4", "xfce").as_deref(), Some(borders.as_str()));
        let shell = part_note("gnomeshell", "gnome").unwrap();
        assert!(shell != borders && shell != apps_borders);
    }

    #[test]
    fn section_card_goes_to_its_section_and_says_what_applies() {
        let p = parts();
        let s = spice("CBlack", 1_700_000_000);
        for (section, id) in [("gtk", 135), ("xfwm4", 906), ("marco", 907)] {
            let it = section_theme_item(&s, "ru_RU", section, "mate", &p["CBlack"]);
            assert_eq!(it.category_id, id, "{section}");
            assert_eq!(it.id, "spices:theme:CBlack", "{section}: та же запись журнала и то же удаление");
            assert_eq!(part_section_of(&it), Some(section));
            let note = part_note(section, "mate").unwrap();
            assert!(it.description.starts_with(&note), "{}", it.description);
            assert!(it.description.contains("GTK 3, Metacity/Marco") && it.description.ends_with("\n\nDesc"), "{}", it.description);
        }
        // В Cinnamon оговорки нет, состав остаётся.
        let it = section_theme_item(&s, "ru_RU", "gtk", "cinnamon", &p["CBlack"]);
        assert!(it.description.contains("Cinnamon, GTK 2") && !it.description.contains(&part_note("gtk", "kde").unwrap()));
        // Карточка раздела Cinnamon по составу не ходит.
        let t = type_for_key("theme").unwrap();
        assert_eq!(part_section_of(&theme_item(t, &s, "ru_RU")), None);
        assert_eq!(part_section_of(&theme_item(type_for_key("applet").unwrap(), &spice("a@b", 1), "ru_RU")), None);
    }

    #[test]
    fn buttons_offer_apply_only_where_the_card_applies() {
        use crate::appearance_page::{card_install_button, install_button};
        assert_eq!(card_button_for(true, false, true).text, card_install_button(true, false, true).text, "стоит — «Применить»");
        assert_eq!(card_button_for(true, false, false).text, card_button(true, false).text);
        assert_eq!(card_button_for(true, true, true).text, card_button(true, true).text, "новее на сайте — «Обновить»");
        assert_eq!(card_button_for(false, false, true).text, card_install_button(false, false, false).text);
        assert_eq!(detail_button_for(true, false, true).text, install_button(true, false, true).text);
        assert_eq!(detail_button_for(true, true, true).text, detail_button(true, true).text);
    }

    /// Записанные ответы по порядку.
    struct Replay(Vec<Result<CondReply, NetError>>, Vec<(String, String)>);

    impl ConditionalNet for Replay {
        fn conditional_get(&mut self, url: &str, _ua: &str, etag: &str, _lm: &str, _t: u64) -> Result<CondReply, NetError> {
            self.1.push((url.into(), etag.into()));
            assert!(!self.0.is_empty(), "лишний запрос");
            self.0.remove(0)
        }
    }

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-spices-parts-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("theme-parts.json")
    }

    fn body200(etag: &str) -> Result<CondReply, NetError> {
        Ok(CondReply::Body { body: tree_body().into_bytes(), etag: etag.into(), last_modified: String::new() })
    }

    const T0: i64 = 1_792_000_000;

    #[test]
    fn parts_cache_day_304_failure_and_garbage() {
        assert_eq!(parts_cache_path(None, "/home/u"), "/home/u/.cache/rexon/cinnamon-spices/theme-parts.json");
        let p = tmp("flow");
        let mut net = Replay(vec![body200("W/\"a\"")], Vec::new());
        let r = refresh_parts(&mut net, &p, false, T0);
        assert_eq!((r.source, r.parts.len(), r.note.as_str()), (RefreshSource::Downloaded, 5, ""));
        assert_eq!(net.1, [(THEME_TREE_URL.to_string(), String::new())]);
        let saved = std::fs::read_to_string(&p).unwrap();
        assert!(saved.len() < 20_000 && !saved.contains("\"tree\""), "в кэше состав, а не дерево");
        // Сутки — без сети.
        let mut quiet = Replay(Vec::new(), Vec::new());
        assert_eq!(refresh_parts(&mut quiet, &p, false, T0 + 3600).source, RefreshSource::Cache);
        // Прошли сутки — условный запрос с ETag, 304 продлевает.
        let mut net = Replay(vec![Ok(CondReply::NotModified)], Vec::new());
        let r = refresh_parts(&mut net, &p, false, T0 + CACHE_TTL_SECS + 1);
        assert_eq!((r.source, r.parts.len()), (RefreshSource::NotModified, 5));
        assert_eq!(net.1[0].1, "W/\"a\"");
        // «Обновить»: ответ — мусор (страница лимита) и обрезанное дерево —
        // прежний состав остаётся, с пометкой; повтор — не сразу.
        let cut = tree_body().replacen("\"truncated\": false", "\"truncated\": true", 1).into_bytes();
        for answer in [
            Ok(CondReply::Body { body: b"{\"message\":\"API rate limit exceeded\"}".to_vec(), etag: String::new(), last_modified: String::new() }),
            Ok(CondReply::Body { body: cut, etag: String::new(), last_modified: String::new() }),
            Err(NetError::Status { code: 403, reason: "rate limit exceeded".into() }),
        ] {
            let mut net = Replay(vec![answer], Vec::new());
            let r = refresh_parts(&mut net, &p, true, T0 + 2 * CACHE_TTL_SECS);
            assert_eq!((r.source, r.parts.len()), (RefreshSource::Stale, 5));
            assert_eq!(r.note, parts_offline_message(true));
        }
        let mut quiet = Replay(Vec::new(), Vec::new());
        let r = refresh_parts(&mut quiet, &p, false, T0 + 2 * CACHE_TTL_SECS + 60);
        assert_eq!((r.source, r.parts.len()), (RefreshSource::Stale, 5), "после сбоя не стучаться сразу");
        // Совсем без кэша и без сети — пусто и объяснение.
        let q = tmp("offline");
        let mut net = Replay(vec![Err(NetError::Transport { detail: "dns".into() })], Vec::new());
        let r = refresh_parts(&mut net, &q, false, T0);
        assert!(r.parts.is_empty() && r.note == parts_offline_message(false));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
        let _ = std::fs::remove_dir_all(q.parent().unwrap());
    }

    /// Временный HOME.
    struct Home(std::path::PathBuf, SpiceEnv);

    impl Home {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!("rexon-spices-partinst-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("home")).unwrap();
            let env = SpiceEnv::at_home(&root.join("home").to_string_lossy());
            Home(root, env)
        }
    }

    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct NoGsettings;

    impl SpiceSys for NoGsettings {
        fn list_get(&mut self, _: &str, _: &str) -> Option<Vec<String>> {
            Some(Vec::new())
        }
        fn list_set(&mut self, _: &str, _: &str, _: &[String]) -> Result<(), String> {
            panic!("тема списков gsettings не трогает")
        }
        fn string_get(&mut self, _: &str, _: &str) -> Option<String> {
            None
        }
        fn reset(&mut self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn reload_xlet(&mut self, _: &str, _: &str) -> Result<(), String> {
            panic!("тема — не xlet")
        }
    }

    /// Архив темы: `<ключ>/` с Cinnamon и перечисленными файлами.
    fn theme_zip(dest: &Path, key: &str, extra: &[&str]) {
        let mut zw = zip::ZipWriter::new(std::fs::File::create(dest).unwrap());
        let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o644);
        for f in ["cinnamon/cinnamon.css"].iter().chain(extra) {
            zw.start_file(format!("{key}/{f}"), opts).unwrap();
            zw.write_all(b"/* */").unwrap();
        }
        zw.finish().unwrap();
    }

    fn install_in(h: &Home, n: u32, key: &str, extra: &[&str], section: Option<&str>) -> (bool, String) {
        let work = h.0.join(format!("work{n}"));
        std::fs::create_dir_all(&work).unwrap();
        let zip = work.join("a.zip");
        theme_zip(&zip, key, extra);
        let t = type_for_key("theme").unwrap();
        let mut journal = |_: &SpiceStep, _: &[String]| Ok::<(), String>(());
        install_archive_in(&h.1, t, &spice(key, 1_700_000_000), key, &zip, &work, None, section, &mut NoGsettings, &mut journal, &mut |_| {})
    }

    #[test]
    fn install_from_gtk_or_borders_checks_the_archive_itself() {
        let h = Home::new("inst");
        let themes = format!("{}/.themes", h.1.home);
        // Тема с GTK 3 из «Тем GTK» — ставится в ~/.themes целиком.
        let (ok, msg) = install_in(&h, 0, "Winter", &["gtk-3.0/gtk.css", "index.theme"], Some("gtk"));
        assert!(ok, "{msg}");
        assert!(Path::new(&format!("{themes}/Winter/gtk-3.0/gtk.css")).is_file());
        assert!(Path::new(&format!("{themes}/Winter/cinnamon/cinnamon.css")).is_file(), "тема ставится целиком");
        // Архив разошёлся с деревом: GTK нет — отказ, на диске ничего.
        let (ok, msg) = install_in(&h, 1, "OnlyShell", &["metacity-1/metacity-theme-3.xml"], Some("gtk"));
        assert!(!ok && msg == part_missing_message("OnlyShell", "gtk"), "{msg}");
        assert!(!Path::new(&format!("{themes}/OnlyShell")).exists());
        // Та же тема в «Рамках окон (MATE)» ставится, в XFCE — нет.
        assert!(install_in(&h, 2, "OnlyShell", &["metacity-1/metacity-theme-3.xml"], Some("marco")).0);
        let (ok, msg) = install_in(&h, 3, "Borderless", &["metacity-1/thumbnail.png", "gtk-2.0/gtkrc"], Some("xfwm4"));
        assert!(!ok && msg == part_missing_message("Borderless", "xfwm4"), "{msg}");
        // Из раздела Cinnamon состав не проверяется — как прежде.
        assert!(install_in(&h, 4, "Borderless", &[], None).0);
    }

    /// Живой запрос к GitHub сжатым ответом (сеть; запускать руками:
    /// `--ignored live_tree_from_github`). Второй запрос — условный, 304.
    #[test]
    #[ignore]
    fn live_tree_from_github() {
        let p = tmp("live");
        let mut net = UreqConditional::gzip_up_to(MAX_TREE_BYTES);
        let r = refresh_parts(&mut net, &p, true, T0);
        assert_eq!(r.source, RefreshSource::Downloaded, "{}", r.note);
        let gtk = r.parts.values().filter(|v| fits_section("gtk", &has_in(v))).count();
        let marco = r.parts.values().filter(|v| fits_section("marco", &has_in(v))).count();
        let xfwm = r.parts.values().filter(|v| fits_section("xfwm4", &has_in(v))).count();
        eprintln!("тем {}, GTK {gtk}, Marco {marco}, xfwm4 {xfwm}", r.parts.len());
        assert!(r.parts.len() > 50 && gtk > 20 && marco > 20 && xfwm > 5);
        let again = refresh_parts(&mut net, &p, true, T0 + 10);
        assert_eq!(again.source, RefreshSource::NotModified);
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn press_applies_installed_theme_and_updates_stale_one() {
        let h = Home::new("press");
        let s = spice("Winter", 1_700_000_000);
        let card = section_theme_item(&s, "ru_RU", "gtk", "kde", &["gtk-3.0".into(), "gtk-3.0/gtk.css".into()]);
        assert_eq!(part_apply_name(&h.1, &card, &[s.clone()]), None, "не стоит — поставить");
        assert!(install_in(&h, 0, "Winter", &["gtk-3.0/gtk.css"], Some("gtk")).0);
        assert_eq!(part_apply_name(&h.1, &card, &[s.clone()]).as_deref(), Some("Winter"), "стоит — применить");
        let newer = Spice { last_edited: s.last_edited + 10, ..s.clone() };
        assert_eq!(part_apply_name(&h.1, &card, &[newer]), None, "на сайте новее — обновить");
        // Карточка раздела Cinnamon так не применяется: там включает плитка
        // установленного.
        let cin = theme_item(type_for_key("theme").unwrap(), &s, "ru_RU");
        assert_eq!(part_apply_name(&h.1, &cin, &[s]), None);
    }
}
