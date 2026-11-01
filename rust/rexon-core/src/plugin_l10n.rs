//! Подписи плагинов демона waywallen: каталог автора плагина поверх нашего
//! русского слоя.
//!
//! # Зачем
//!
//! С waywallen 0.3.7 плагины-источники отдают подписи не голой строкой, а
//! парой `PluginMessage{plugin_id, msgid}` (см. `control.proto`), а сам
//! перевод демон присылает отдельно — каталогами `.po` на запрос
//! `plugin_translation_list`. Вендорный QML прогоняет КАЖДУЮ подпись через
//! `I18n.tr()`, то есть через этот механизм.
//!
//! У нас перевод подписей каталогов уже есть — [`crate::remote_l10n`],
//! русские таблицы Мастерской Steam и Wallhaven. Если просто взять
//! апстримовский путь, наш перевод пропадёт: у плагина
//! `org.waywallen.open-wallpaper-engine` (Мастерская Steam) русского
//! каталога НЕТ вовсе — замерено на живом демоне 0.3.7: каталоги есть у
//! `image`, `video`, `wallhaven` (ru + zh-CN), а у open-wallpaper-engine
//! только zh-CN. Именно там живёт основная масса наших подписей.
//!
//! # Порядок разрешения подписи
//!
//! 1. **каталог плагина** — перевод автора плагина: он обновляется вместе с
//!    плагином и достаётся всем пользователям waywallen, а не только нам;
//! 2. **наш [`crate::remote_l10n`]** — заполняет дыры;
//! 3. **сам `msgid`** — если не нашлось нигде.
//!
//! ⚠️ Пункт 2 приходит с ДВУХ сторон, и обе — один и тот же наш слой:
//!
//!   * `fallback` — подпись, которую [`crate::remote_l10n::translate_source`]
//!     уже посчитал ПО КЛЮЧУ (`trend_day` → «Популярные (Сегодня)»,
//!     фильтр `mature` → «18+ (для взрослых)», источник `wallpaper_engine` →
//!     «Мастерская Steam»). Ключа в `PluginMessage` нет вовсе, поэтому
//!     сюда его приносит сам мост — полем `rexonFallback` в той же карте;
//!   * [`crate::remote_l10n::text`] — поиск по английской строке
//!     (`msgid`), которым закрыты кнопки и строки состояния плагинов.
//!
//! Без первой половины русский пропал бы у всех сортировок и заголовков
//! фильтров разом: наши таблицы для них ключевые, а не текстовые — поймано
//! приёмкой `scripts/test_we_plugin_translations.py` на живом демоне.
//!
//! Обратный порядок означал бы, что мы молча перебиваем перевод автора
//! плагина своим — этого делать нельзя. По мере появления апстримовских
//! каталогов наш слой сам собой ужимается, а подписи не мигают.
//!
//! ⚠️ `msgid` — это английская исходная строка, а не ключ (проверено на
//! живом демоне: `labelText.msgid == "Log in to Wallhaven"`). Поэтому
//! таблицы `remote_l10n`, где ключ — тоже английская строка, ложатся сюда
//! без единой новой карты (правило #12: чиним общий движок, а не заводим
//! второй словарь).
//!
//! # Чего здесь нет
//!
//! Ввода-вывода: разбор `.po` и поиск — чистые функции над байтами и
//! строками. Сеть (запрос `plugin_translation_list`), Qt-свойства и сигналы
//! — за мостом `rexon-gui/src/plugin_translations.rs`.

use std::collections::BTreeMap;

/// Разобранный каталог одного плагина на одном языке: `msgid` → `msgstr`.
pub type Messages = BTreeMap<String, String>;

/// Каталоги всех плагинов: `plugin_id` → `locale` → сообщения.
pub type Catalogs = BTreeMap<String, BTreeMap<String, Messages>>;

// ============================================================================
// Разбор .po
// ============================================================================

/// Снять кавычки и раскрыть экранирование одной строки `.po`.
///
/// Возвращает `None`, если в строке нет закавыченного куска: такие строки
/// (комментарии, `msgid_plural` без кавычек, мусор) просто пропускаются.
fn unquote(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let body = &line[start + 1..];
    let end = {
        // Закрывающая кавычка — последняя неэкранированная.
        let bytes = body.as_bytes();
        let mut found = None;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'"' => {
                    found = Some(i);
                    i += 1;
                }
                _ => i += 1,
            }
        }
        found?
    };
    let raw = &body[..end];

    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            // Незнакомая пара остаётся как есть: портить чужой текст хуже,
            // чем показать лишний слэш.
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    Some(out)
}

/// Какую часть записи мы сейчас набираем.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    None,
    Id,
    Str,
}

/// Разобрать каталог `.po` в карту `msgid` → `msgstr`.
///
/// Понимает продолжения строк, экранирование и `msgstr[0]`. Пропускает
/// заголовок (запись с пустым `msgid`), пустые переводы и `#`-комментарии.
/// Битый файл не ошибка: разбирается всё, что разобралось, — иначе одна
/// кривая запись съела бы весь каталог плагина.
pub fn parse_po(po: &[u8]) -> Messages {
    let text = String::from_utf8_lossy(po);
    let mut out = Messages::new();
    let mut id = String::new();
    let mut msg = String::new();
    let mut field = Field::None;

    // Записать набранную пару и начать следующую.
    fn flush(out: &mut Messages, id: &mut String, msg: &mut String) {
        if !id.is_empty() && !msg.is_empty() {
            out.insert(std::mem::take(id), std::mem::take(msg));
        } else {
            id.clear();
            msg.clear();
        }
    }

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgid_plural") {
            // Множественные формы демон не шлёт; продолжения такой записи
            // не должны попасть в msgid.
            let _ = rest;
            field = Field::None;
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgid") {
            flush(&mut out, &mut id, &mut msg);
            field = Field::Id;
            if let Some(part) = unquote(rest) {
                id.push_str(&part);
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgstr") {
            // `msgstr[0]` — первая множественная форма, годится как перевод;
            // остальные формы пропускаем.
            if rest.starts_with('[') && !rest.starts_with("[0]") {
                field = Field::None;
                continue;
            }
            field = Field::Str;
            if let Some(part) = unquote(rest) {
                msg.push_str(&part);
            }
            continue;
        }
        if line.starts_with("msgctxt") {
            // Контекст мы не различаем: у демона его нет.
            field = Field::None;
            continue;
        }
        if line.starts_with('"') {
            // Продолжение предыдущего поля.
            if let Some(part) = unquote(line) {
                match field {
                    Field::Id => id.push_str(&part),
                    Field::Str => msg.push_str(&part),
                    Field::None => {}
                }
            }
        }
    }
    flush(&mut out, &mut id, &mut msg);
    out
}

// ============================================================================
// Локаль
// ============================================================================

/// Языковые метки от самой точной к самой общей: `ru_RU.UTF-8` →
/// `["ru-RU", "ru"]`.
///
/// Демон помечает каталоги как `ru` и `zh-CN` (через дефис), поэтому
/// подчёркивание нормализуется в дефис. Пустая локаль даёт пустой список —
/// звать каталог не по чему.
pub fn locale_candidates(locale: &str) -> Vec<String> {
    let cleaned = locale
        .split(['.', '@'])
        .next()
        .unwrap_or("")
        .replace('_', "-");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return Vec::new();
    }
    let mut out = vec![cleaned.to_string()];
    if let Some((lang, _region)) = cleaned.split_once('-') {
        if !lang.is_empty() {
            out.push(lang.to_string());
        }
    }
    out
}

// ============================================================================
// Разрешение подписи
// ============================================================================

/// Подпись из каталога плагина — пункт 1 порядка. `None` — в каталоге нет.
pub fn catalog_lookup(
    catalogs: &Catalogs,
    plugin_id: &str,
    msgid: &str,
    locale: &str,
) -> Option<String> {
    if plugin_id.is_empty() || msgid.is_empty() {
        return None;
    }
    let by_locale = catalogs.get(plugin_id)?;
    for tag in locale_candidates(locale) {
        if let Some(found) = by_locale.get(&tag).and_then(|m| m.get(msgid)) {
            return Some(found.clone());
        }
    }
    None
}

/// Подпись для покупателя: каталог плагина → наш `remote_l10n` → сам `msgid`.
///
/// `fallback` — наша подпись, посчитанная по ключу (см. шапку модуля);
/// пустая строка означает «ключевого перевода нет».
///
/// Пустой `msgid` возвращается пустым: в протоколе это «подписи нет», и
/// подставлять туда что-либо нельзя — вендорный QML сам берёт запасной
/// текст (`I18n.tr(x) || modelData.name`).
pub fn resolve(
    catalogs: &Catalogs,
    plugin_id: &str,
    msgid: &str,
    fallback: &str,
    locale: &str,
) -> String {
    resolve_with_arguments(catalogs, plugin_id, msgid, fallback, locale, &[])
}

/// То же, что [`resolve`], но с подстановками `%1`, `%2`… из протокола.
///
/// waywallen 0.3.8 добавил `PluginMessage.arguments`: плагин отдаёт шаблон
/// («Signed in as %1») и отдельно значения, чтобы переводчику доставался
/// шаблон, а не склеенная строка. Подстановка идёт ПОСЛЕ перевода — иначе
/// в каталоге искалась бы строка с уже подставленным именем.
pub fn resolve_with_arguments(
    catalogs: &Catalogs,
    plugin_id: &str,
    msgid: &str,
    fallback: &str,
    locale: &str,
    arguments: &[String],
) -> String {
    if msgid.is_empty() {
        // Подписи нет вовсе — запасной текст выбирает вендорный QML.
        return String::new();
    }
    let text = if let Some(found) = catalog_lookup(catalogs, plugin_id, msgid, locale) {
        found
    } else if !fallback.is_empty() {
        fallback.to_string()
    } else {
        crate::remote_l10n::text(msgid).to_string()
    };
    format_message(&text, arguments)
}

/// Подставить `arguments` вместо `%1`, `%2`… .
///
/// Номер — любое число подряд идущих цифр после `%`; вне диапазона и без
/// цифр место остаётся как было (плагин мог написать «100%»). Пустой список
/// значений не меняет строку вовсе.
pub fn format_message(text: &str, arguments: &[String]) -> String {
    if arguments.is_empty() {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut end = i + 1;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
        let digits: String = chars[i + 1..end].iter().collect();
        match digits.parse::<usize>() {
            Ok(n) if n >= 1 && n <= arguments.len() => out.push_str(&arguments[n - 1]),
            _ => out.extend(&chars[i..end]),
        }
        i = end;
    }
    out
}

/// Сложить документы ответа демона в каталоги.
///
/// Вход — то, что пришло в `PluginTranslationListResponse.documents`:
/// тройки `(plugin_id, locale, po)`. Разбор здесь один раз на ответ, а не на
/// подпись: `resolve` обязан быть поиском по карте — его зовут на КАЖДУЮ
/// подпись страницы.
pub fn build_catalogs<'a, I>(documents: I) -> Catalogs
where
    I: IntoIterator<Item = (&'a str, &'a str, &'a [u8])>,
{
    let mut out = Catalogs::new();
    for (plugin_id, locale, po) in documents {
        if plugin_id.is_empty() || locale.is_empty() {
            continue;
        }
        let messages = parse_po(po);
        if messages.is_empty() {
            continue;
        }
        out.entry(plugin_id.to_string())
            .or_default()
            .insert(locale.to_string(), messages);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PO: &str = r#"
msgid ""
msgstr "Project-Id-Version: test\nLanguage: ru\n"

#: main.lua:59
msgid "Trending"
msgstr "В тренде"

#: main.lua:60
msgid "Long line"
msgstr ""
"первая часть "
"и вторая"

msgid "Escaped \"quote\""
msgstr "Кавычка \"тут\""

msgid "Empty translation"
msgstr ""
"#;

    #[test]
    fn po_разбирается_с_продолжениями_и_экранированием() {
        let m = parse_po(PO.as_bytes());
        assert_eq!(m.get("Trending").map(String::as_str), Some("В тренде"));
        assert_eq!(
            m.get("Long line").map(String::as_str),
            Some("первая часть и вторая")
        );
        assert_eq!(
            m.get("Escaped \"quote\"").map(String::as_str),
            Some("Кавычка \"тут\"")
        );
        // Заголовок (пустой msgid) и пустой перевод в карту не попадают.
        assert!(!m.contains_key(""));
        assert!(!m.contains_key("Empty translation"));
    }

    #[test]
    fn метки_локали_от_точной_к_общей() {
        assert_eq!(locale_candidates("ru_RU.UTF-8"), vec!["ru-RU", "ru"]);
        assert_eq!(locale_candidates("zh-CN"), vec!["zh-CN", "zh"]);
        assert_eq!(locale_candidates("ru"), vec!["ru"]);
        assert!(locale_candidates("").is_empty());
    }

    /// Пункт 1: перевод автора плагина сильнее нашего.
    #[test]
    fn каталог_плагина_бьёт_наш_слой() {
        // Наш слой говорит на языке интерфейса Rexon; закрепляем исходный
        // (английский), русский закреплён сверкой `remote_l10n_parity`.
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let catalogs = build_catalogs([(
            "org.waywallen.wallhaven",
            "ru",
            format!("msgid \"{API_KEY_HINT}\"\nmsgstr \"Скопируйте ключ\"\n").as_bytes(),
        )]);
        // В наших таблицах для этой строки своя подпись; каталог плагина побеждает.
        assert_eq!(
            resolve(&catalogs, "org.waywallen.wallhaven", API_KEY_HINT, "", "ru_RU"),
            "Скопируйте ключ"
        );
        assert_eq!(crate::remote_l10n::text(API_KEY_HINT), OUR_API_KEY_HINT);
    }

    /// Строка плагина, у которой в наших таблицах СВОЯ подпись (не та же
    /// английская) — на ней видно, что сработал именно наш слой.
    const API_KEY_HINT: &str = "Copy the API key from Wallhaven Account Settings.";
    const OUR_API_KEY_HINT: &str = "The API key is in your Wallhaven account settings.";

    /// Пункт 2: без каталога держит наш слой — это случай Мастерской Steam.
    #[test]
    fn без_каталога_держит_наш_русский() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let catalogs = Catalogs::new();
        assert_eq!(
            resolve(
                &catalogs,
                "org.waywallen.wallhaven",
                API_KEY_HINT,
                "",
                "ru_RU"
            ),
            OUR_API_KEY_HINT
        );
    }

    /// Пункт 2 действует и когда каталог плагина есть, но строки в нём нет.
    #[test]
    fn дыра_в_каталоге_закрывается_нашим_слоем() {
        let catalogs = build_catalogs([(
            "org.waywallen.wallhaven",
            "ru",
            "msgid \"Trending\"\nmsgstr \"В тренде\"\n".as_bytes(),
        )]);
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        assert_eq!(
            resolve(&catalogs, "org.waywallen.wallhaven", API_KEY_HINT, "", "ru"),
            OUR_API_KEY_HINT
        );
    }

    /// Пункт 3: незнакомая строка возвращается как есть, а не пустой.
    #[test]
    fn незнакомое_остаётся_собой() {
        let catalogs = Catalogs::new();
        assert_eq!(
            resolve(&catalogs, "org.waywallen.image", "Brand new label", "", "ru"),
            "Brand new label"
        );
        assert_eq!(resolve(&catalogs, "", "", "", "ru"), "");
    }

    /// Чужой язык не подставляется: у Мастерской есть только zh-CN — и он
    /// НЕ должен всплыть на русской машине.
    #[test]
    fn чужой_язык_не_подставляется() {
        let catalogs = build_catalogs([(
            "org.waywallen.open-wallpaper-engine",
            "zh-CN",
            "msgid \"Content rating\"\nmsgstr \"内容分级\"\n".as_bytes(),
        )]);
        assert_eq!(
            resolve(
                &catalogs,
                "org.waywallen.open-wallpaper-engine",
                "Content rating",
                "",
                "ru_RU"
            ),
            "Content rating"
        );
    }

    /// Ключевой перевод нашего слоя (сортировки, заголовки фильтров) —
    /// это пункт 2, а не пункт 1: каталог плагина его перебивает.
    #[test]
    fn ключевой_перевод_ниже_каталога_но_выше_msgid() {
        let catalogs = build_catalogs([(
            "org.waywallen.wallhaven",
            "ru",
            "msgid \"Trending\"\nmsgstr \"В тренде\"\n".as_bytes(),
        )]);
        // Каталог есть — он и побеждает, наш ключевой перевод уступает.
        assert_eq!(
            resolve(
                &catalogs,
                "org.waywallen.wallhaven",
                "Trending",
                "Наш вариант",
                "ru"
            ),
            "В тренде"
        );
        // Каталога нет — держит ключевой перевод.
        assert_eq!(
            resolve(
                &catalogs,
                "org.waywallen.open-wallpaper-engine",
                "Trending today",
                "Популярные (Сегодня)",
                "ru"
            ),
            "Популярные (Сегодня)"
        );
    }

    /// Битая запись не съедает остальной каталог.
    #[test]
    fn битая_запись_не_рушит_каталог() {
        let po = "msgid \"good\"\nmsgstr \"ок\"\nmsgid broken\nmsgstr also broken\nmsgid \"tail\"\nmsgstr \"хвост\"\n"
            .as_bytes();
        let m = parse_po(po);
        assert_eq!(m.get("good").map(String::as_str), Some("ок"));
        assert_eq!(m.get("tail").map(String::as_str), Some("хвост"));
    }
}
