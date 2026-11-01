//! Автоперевод описаний тем — порт ЧИСТОЙ логики из
//! `rexon/appearance/translator.py`.
//!
//! Портирована ДЕТЕРМИНИРОВАННАЯ часть: маппинг локали пользователя в целевой
//! язык ([`user_lang_from_locale`]), ключ кеша переводов `src_hash`
//! ([`translation_cache_key`] — SHA-1 от `версия:src:tgt:текст`), признак
//! HTML-разметки ([`looks_like_html`]), сегментация HTML по текстовым узлам с
//! сохранением тегов/сущностей дословно ([`segment_html`] — побайтовый порт
//! `_HTMLTextTranslator`/`html.parser.HTMLParser` при `convert_charrefs=False`),
//! дешёвая эвристика языка ([`detect_lang_heuristic`]), выделение видимого
//! текста для детекта ([`probe_text`]) и РЕШЕНИЕ «переводить или вернуть
//! оригинал» ([`plan_translate_text`]).
//!
//! Side-effect'ы СЮДА НЕ переносятся и остаются на Python: сам вызов
//! `argostranslate` (перевод текстового сегмента), SQLite-кеш переводов
//! (`get_cache`/`get_translation`/`put_translation` в `appearance/cache.py`),
//! опциональный `langdetect` (недетерминирован без seed=0, но seed фиксирует
//! лишь его внутренний ГПСЧ — сам движок сюда не тащим) и загрузка моделей
//! (`_ensure_pair`, сеть). Ядро отдаёт СЕГМЕНТЫ, КЛЮЧ и РЕШЕНИЕ; наблюдения
//! (доступен ли argos, что детектнул язык, собирается ли пара src→tgt)
//! передаются параметрами.
//!
//! ⚠️ СВЕРХ ПОРТА (в Python этого нет): фильтр «проза или техника» —
//! [`is_prose_segment`], [`CodeEnvStack`], [`translatable_flags`] и
//! [`render_segments_filtered`]. Python звал движок на каждый текстовый узел, и
//! замер на живых описаниях мастерской показал, что так любой движок ломает
//! технику (`Gnome` → «Гном», `sudo apt-get install …` → «sudo apt-get
//! установить …»). Фильтр решает ДО вызова, кого движку отдавать; сверка —
//! `translator_prose.rs`, паритетное поведение он не задевает. Имя технологии
//! ВНУТРИ переводимой фразы фильтром не спасти (`Sweet Kvantum theme` — проза),
//! поэтому рядом лежит подмена: [`mask_tech_names`] уводит имя под заполнитель
//! `@0@`, [`unmask_tech_names`] возвращает его дословно, а не сошлись
//! заполнители — перевод сегмента не берётся вовсе. Тем же приёмом прячутся
//! имена, которых в нашем списке быть не может, — [`mask_names`] принимает их
//! от вызывающего (название открытого товара, имя автора).
//!
//! ⚠️ СВЕРХ ТОГО (приёмка 24.09): решение «проза или техника» умеет считаться
//! ПО ПРЕДЛОЖЕНИЯМ ([`translatable_flags_by_sentence`]) — узел, где проза стоит
//! рядом со ссылкой, больше не отвергается целиком, — а рядом с ситами
//! заполнителей появилось третье: [`invented_latin_word`] ловит латинское
//! слово, которого в источнике не было («tooltips» → «toolbep»), то есть порчу
//! без всяких имён.
//!
//! Сверка с Python — golden `translator_parity.rs`: эталон снят с живого
//! `translator.py` (segmentation через `translate_html` с монипатченным
//! детерминированным `translate`, `src_hash` из `hashlib.sha1`, `_user_lang`,
//! `_looks_like_html`, `detect_lang` без langdetect, `translate_text` с
//! монипатченными наблюдениями).
//!
//! ФАКТЫ ПОВЕДЕНИЯ Python, воспроизведённые дословно (это НЕ баги — стандартная
//! нормализация `html.parser`, воспроизводим для паритета, см. отчёт):
//!   * закрывающие теги приводятся к нижнему регистру (`</DIV>` → `</div>`),
//!     тогда как открывающий тег отдаётся ВЕРБАТИМ (`get_starttag_text`);
//!   * charref/entityref без завершающей `;` дополняются `;`
//!     (`&#169 ` → `&#169;` + пробел);
//!   * узлы-объявления, которые подкласс `_HTMLTextTranslator` НЕ обрабатывает
//!     (`<!DOCTYPE …>`, `<?pi?>`, `<![CDATA[…]]>`), ВЫПАДАЮТ из вывода — как в
//!     оригинале (эти обработчики у него не переопределены). В описаниях OCS/тем
//!     такие узлы не встречаются.

use serde::Serialize;

/// Версия логики перевода — подмешивается в ключ кеша `src_hash`. Бамп на 1
/// инвалидирует все старые записи (порт `TRANSLATOR_VERSION`).
///
/// `4` — СМЕНА ДВИЖКА (argos → fxtranslate). Бамп здесь не косметика: тексты,
/// переведённые прежним движком, лежат в таблице `translations` под ключами
/// версии `3`, и читать их как свои НЕЛЬЗЯ — качество другое. Новая версия
/// разводит ключи, старые записи просто не находятся и уходят по TTL
/// ([`crate::appearance_cache::TRANSLATION_TTL`]).
///
/// `5` — ТЕХНИКА ВНУТРИ ПРОЗЫ (03.10): фраза с путём, ссылкой или именем файла
/// больше не отвергается целиком, а переводится с техникой под заполнителем.
/// Без бампа сегмент, переведённый прежним правилом наполовину (английская
/// фраза с путём посреди русского абзаца), жил бы в кеше до конца TTL.
///
/// `6` — СЛОВАРЬ ПОПРАВОК ТЕРМИНОВ (03.10, [`TERM_FIXES`]): «расстегните
/// файл» → «распакуйте файл», «скачать скачать» → «скачать». Без бампа старые
/// «расстегните» жили бы в кеше до конца TTL.
pub const TRANSLATOR_VERSION: u32 = 6;

// ── src_hash (ключ кеша переводов) ─────────────────────────────────────────

/// Ключ кеша перевода — `sha1(f"{VERSION}:{src}:{tgt}:{text}").hexdigest()`.
/// Побитовый порт вычисления `h` в `translate()`/`translate_html`-хвосте.
pub fn translation_cache_key(src: &str, tgt: &str, text: &str) -> String {
    translation_cache_key_versioned(TRANSLATOR_VERSION, src, tgt, text)
}

/// Тот же ключ, но с ЯВНОЙ версией движка.
///
/// ⚠️ Нужен сверке: golden снят при версии `3`, и проверять она обязана
/// МЕХАНИЗМ (порядок полей, кодировка, SHA-1), а не текущее значение
/// константы, — иначе любой бамп версии требовал бы переснять эталон.
/// Живой код зовёт [`translation_cache_key`], который сам подставляет
/// [`TRANSLATOR_VERSION`].
pub fn translation_cache_key_versioned(version: u32, src: &str, tgt: &str, text: &str) -> String {
    let payload = format!("{version}:{src}:{tgt}:{text}");
    sha1_hex(payload.as_bytes())
}

/// SHA-1 → hex-строка (нижний регистр), как `hashlib.sha1(...).hexdigest()`.
///
/// Виден внутри крейта: тем же хешем `ocs::preview_cache_name` называет файлы
/// кеша превью — второй реализации SHA-1 в ядре быть не должно.
pub(crate) fn sha1_hex(msg: &[u8]) -> String {
    let mut h: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];
    let ml = (msg.len() as u64).wrapping_mul(8);

    // Паддинг: 0x80, затем 0x00 до длины ≡ 56 (mod 64), затем 8 байт длины BE.
    let mut data = msg.to_vec();
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&ml.to_be_bytes());

    for chunk in data.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in w.iter_mut().enumerate().take(16) {
            let b = i * 4;
            *word = u32::from_be_bytes([chunk[b], chunk[b + 1], chunk[b + 2], chunk[b + 3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = String::with_capacity(40);
    for word in h {
        out.push_str(&format!("{word:08x}"));
    }
    out
}

// ── язык интерфейса (целевой) ──────────────────────────────────────────────

/// Целевой язык из строки локали — порт `(_loc.split("_")[0] or "en").lower()`
/// из `_user_lang`. Получение самой локали (`locale.getlocale`) — side effect,
/// остаётся на Python; сюда приходит уже строка (может быть пустой).
pub fn user_lang_from_locale(locale_str: &str) -> String {
    let head = locale_str.split('_').next().unwrap_or("");
    let base = if head.is_empty() { "en" } else { head };
    base.to_lowercase()
}

/// Язык, НА КОТОРЫЙ переводятся описания и комментарии тем, — язык
/// интерфейса Rexon (`i18n::current_language`: настройка человека, иначе
/// локаль системы, иначе английский).
///
/// Именно интерфейса, а не локали системы: выбрал человек английский Rexon на
/// русской системе — описания рядом с английскими кнопками тоже должны быть
/// английскими, а не русскими. Отдаёт код вида `ru` или `pt_BR`;
/// [`user_lang_from_locale`] сводит его к языку модели.
pub fn interface_locale() -> String {
    crate::i18n::current_language()
}

/// Строка локали покупателя из окружения — в том порядке, в каком её читает
/// сама система локалей: `LC_ALL`, `LC_MESSAGES`, `LANG`. Пусто — локали нет.
///
/// ⚠️ Для перевода описаний НЕ годится — там язык интерфейса
/// ([`interface_locale`]); это язык системы.
///
/// Тот самый `locale.getlocale`, который у Python оставался side effect'ом
/// вызывающего. Живёт здесь, рядом с разбором: цикл по этим трём переменным
/// уже лежал копией в [`crate::distro_compat::Lang::from_env`], и третья копия
/// (перевод описаний) сделала бы расхождение вопросом времени (правило #12).
pub fn locale_from_env() -> String {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                return value;
            }
        }
    }
    String::new()
}

// ── признак HTML и probe-текст ─────────────────────────────────────────────

// `_HTML_TAG_RE = <[a-zA-Z/!][^>]*>|&[a-zA-Z#0-9]+;`.
// Возвращает первое вхождение (start, end) в срезе char'ов, начиная с `from`.
fn html_tag_find(data: &[char], from: usize) -> Option<(usize, usize)> {
    let n = data.len();
    let mut p = from;
    while p < n {
        let c = data[p];
        if c == '<' && p + 1 < n && (data[p + 1].is_ascii_alphabetic() || matches!(data[p + 1], '/' | '!')) {
            // <[a-zA-Z/!][^>]*>
            let mut q = p + 2;
            while q < n && data[q] != '>' {
                q += 1;
            }
            if q < n {
                return Some((p, q + 1));
            }
            // нет закрывающего '>' — этот '<' не матч, ищем дальше.
        } else if c == '&' {
            // &[a-zA-Z#0-9]+;
            let mut q = p + 1;
            while q < n && (data[q].is_ascii_alphanumeric() || data[q] == '#') {
                q += 1;
            }
            if q > p + 1 && q < n && data[q] == ';' {
                return Some((p, q + 1));
            }
        }
        p += 1;
    }
    None
}

/// Есть ли в тексте HTML-разметка — порт `_looks_like_html`.
pub fn looks_like_html(text: &str) -> bool {
    let data: Vec<char> = text.chars().collect();
    html_tag_find(&data, 0).is_some()
}

/// Видимый текст для детекта языка — порт вычисления `probe`:
/// `re.sub(r"\s+", " ", html.unescape(_HTML_TAG_RE.sub(" ", text))).strip()`.
///
/// ⚠️ `html.unescape` реализован для числовых ссылок полностью и для
/// РАСПРОСТРАНЁННЫХ именованных сущностей; так как `_HTML_TAG_RE` уже вырезал
/// все корректные `&…;` в пробел ДО unescape, до него доходят лишь остатки
/// (обычно голые `&`). Результат используется ТОЛЬКО как вход детектора языка,
/// на ключ кеша и на сохраняемую разметку не влияет.
pub fn probe_text(text: &str) -> String {
    let data: Vec<char> = text.chars().collect();
    // _HTML_TAG_RE.sub(" ", text): каждое вхождение → один пробел.
    let mut stripped_tags = String::new();
    let mut i = 0;
    while i < data.len() {
        if let Some((s, e)) = html_tag_find(&data, i) {
            if s == i {
                stripped_tags.push(' ');
                i = e;
                continue;
            }
            // до следующего матча — как есть.
            for &c in &data[i..s] {
                stripped_tags.push(c);
            }
            stripped_tags.push(' ');
            i = e;
        } else {
            for &c in &data[i..] {
                stripped_tags.push(c);
            }
            break;
        }
    }
    let unescaped = html_unescape(&stripped_tags);
    collapse_ws(&unescaped)
}

// `re.sub(r"\s+", " ", s).strip()` — схлопнуть последовательности пробельных в
// один пробел, обрезать края. `\s` в Python = [ \t\n\r\f\v] (+ Unicode при
// re.UNICODE, дефолт для str). Используем char::is_whitespace (Unicode WS).
fn collapse_ws(s: &str) -> String {
    let mut out = String::new();
    let mut in_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            in_ws = true;
        } else {
            if in_ws && !out.is_empty() {
                out.push(' ');
            }
            in_ws = false;
            out.push(c);
        }
    }
    out
}

// ── детект языка (эвристика) ───────────────────────────────────────────────

/// Дешёвая эвристика языка по скрипту — порт ветки `detect_lang` БЕЗ langdetect
/// (строки с проверкой пустоты и подсчётом кириллицы/латиницы). Когда langdetect
/// установлен, Python берёт его результат — это недетерминированный side effect,
/// который в ядро не портирован; вызывающий передаёт итог детекта параметром в
/// [`plan_translate_text`].
pub fn detect_lang_heuristic(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    // _CYRILLIC = [Ѐ-ӿ] (U+0400..U+04FF); _LATIN = [A-Za-z].
    let cyr = text.chars().filter(|&c| ('\u{0400}'..='\u{04ff}').contains(&c)).count();
    let lat = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    if cyr > 0 && cyr >= lat {
        return Some("ru".to_string());
    }
    if lat > 0 {
        return Some("en".to_string());
    }
    None
}

// ── решение «переводить или нет» ───────────────────────────────────────────

/// План перевода текста — результат [`plan_translate_text`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TranslatePlan {
    /// Не переводить — вернуть оригинал, `was_translated=false` (пусто/пробелы,
    /// уже на целевом языке, argos недоступен).
    Skip,
    /// Переводить: язык-источник и путь (HTML по сегментам или обычный текст).
    Translate { src: String, tgt: String, is_html: bool },
}

/// Решение `translate_text` ДО фактического перевода — порт управляющей логики
/// (строки «пусто→оригинал», «уже на целевом→оригинал», «argos нет→оригинал»,
/// выбор src с откатом на `en`, выбор HTML/обычного пути). Сам перевод и
/// финальное сравнение `result.strip() != text.strip()` (определяющее
/// `was_translated`) выполняет вызывающий — это side effect.
///
/// Наблюдения (параметры):
///   * `target` — уже вычисленный целевой язык (`target or _user_lang()`);
///   * `detected_src` — результат `detect_lang(probe)` (None или код языка);
///   * `engine_available` — доступен ли движок перевода (`_argos_available()`
///     у прежнего движка; имя параметра нейтральное — движок меняется);
///   * `src_pair_ensurable` — `_ensure_pair(src, tgt)` для КАНДИДАТА src
///     (`detected_src` либо `"en"`), т.е. собирается ли путь перевода.
pub fn plan_translate_text(
    text: &str,
    target: &str,
    detected_src: Option<&str>,
    engine_available: bool,
    src_pair_ensurable: bool,
) -> TranslatePlan {
    // if not text or not text.strip(): return text, False
    if text.trim().is_empty() {
        return TranslatePlan::Skip;
    }
    // src = detect_lang(probe); if src and src == tgt: return text, False
    // (пустая строка в Python «falsy» → не срабатывает).
    let src_detected = detected_src.filter(|s| !s.is_empty());
    if let Some(s) = src_detected {
        if s == target {
            return TranslatePlan::Skip;
        }
    }
    // if not _argos_available(): return text, False
    if !engine_available {
        return TranslatePlan::Skip;
    }
    // src = src or "en"; if not _ensure_pair(src, tgt): src = "en"
    let mut src = src_detected.unwrap_or("en").to_string();
    if !src_pair_ensurable {
        src = "en".to_string();
    }
    let is_html = looks_like_html(text);
    TranslatePlan::Translate { src, tgt: target.to_string(), is_html }
}

// ── сегментация HTML ───────────────────────────────────────────────────────

/// Сегмент разобранного HTML — порт вывода `_HTMLTextTranslator`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Segment {
    /// Дословный фрагмент (тег/сущность/комментарий/пробельный узел) — НЕ
    /// переводится, вставляется как есть.
    Literal { text: String },
    /// Переводимый текстовый узел: ведущие/замыкающие пробелы вынесены наружу
    /// (argos их «съедает»), переводится только `stripped`. Реассемблинг:
    /// `lead + (translate(stripped) || stripped) + trail`.
    Text { lead: String, stripped: String, trail: String },
}

/// Разбить HTML на сегменты (порт `_HTMLTextTranslator.feed`+`close`).
/// Переводимый текст — в [`Segment::Text`], всё остальное — [`Segment::Literal`]
/// дословно. Ничего не переводит: перевод текстовых узлов — дело вызывающего
/// (argos, side effect). Сборку строки см. [`render_segments`].
pub fn segment_html(text: &str) -> Vec<Segment> {
    let mut seg = Segmenter::new(text);
    seg.goahead(false);
    seg.goahead(true);
    seg.out
}

/// Собрать строку из сегментов, переводя текстовые узлы колбэком `translate`.
/// Порт реассемблинга `"".join(p.out)` вместе с `_translate_segment`:
/// `f"{lead}{res or stripped}{trail}"` (пустой перевод → берётся оригинал).
pub fn render_segments(segments: &[Segment], translate: impl Fn(&str) -> String) -> String {
    let mut out = String::new();
    for s in segments {
        match s {
            Segment::Literal { text } => out.push_str(text),
            Segment::Text { lead, stripped, trail } => {
                let res = translate(stripped);
                out.push_str(lead);
                if res.is_empty() {
                    out.push_str(stripped);
                } else {
                    out.push_str(&res);
                }
                out.push_str(trail);
            }
        }
    }
    out
}

// ── фильтр «проза или техника» ─────────────────────────────────────────────
//
// ⚠️ ЭТОГО В PYTHON НЕТ — новое поведение, не паритет. Python звал движок на
// КАЖДЫЙ текстовый узел, и замер на живых описаниях мастерской показал, чем
// это кончается у ЛЮБОГО движка: `Gnome` → «Гном», `Unity` → «Единство»,
// `Cinnamon` → «Корица», `sudo apt-get install gtk2-engines-murrine` →
// «sudo apt-get установить gtk2-двигатели-муррин» (примеры дословно из
// `mt-bench/samples.md`, образец 10 «Qogir theme»). Домашнее правило: имена
// технологий и стандартов остаются латиницей, переводим только человеческую
// прозу.
//
// Фильтр — ЧИСТЫЕ функции и НЕ трогает [`segment_html`]/[`render_segments`]
// (они побайтово сверены с Python, сверка `translator_parity.rs`). Решение
// «звать движок или нет» принимается ДО вызова: сегмент, признанный техникой,
// просто не попадает движку, а пустой ответ колбэка [`render_segments`]
// означает «оставить оригинал» — то есть отказ переводить уже выражается в
// существующем интерфейсе, дописывать его не пришлось.
//
// ⚠️ Здесь решается судьба ЦЕЛОГО сегмента, не слов внутри него: «Sweet
// Kvantum theme» для фильтра — проза (три слова, не все имена продуктов).
// Имя ВНУТРИ фразы защищает подмена — [`mask_tech_names`] и
// [`unmask_tech_names`] ниже, по тому же списку имён.

/// Головы командных строк: сегмент, начинающийся с такого слова, — команда,
/// а не проза. `yum` в списке не для красоты: именно `yum install
/// gtk-murrine-engine gtk2-engines` стоит в живом описании Qogir.
const COMMAND_HEADS: &[&str] = &[
    "sudo", "apt", "apt-get", "pacman", "yay", "paru", "dnf", "yum", "zypper", "flatpak", "snap",
    "git", "cd", "chmod", "chown", "systemctl", "makepkg", "cmake", "make",
    // Головы файловых и установочных команд описаний оформления (`cp -r Foo
    // ~/.themes`, `tar -xf …`, `kpackagetool6 -i …`). Пока правило 4 отвергало
    // любую строку с путём, они ловились им; теперь путь внутри прозы
    // переводится (см. [`technical_spans`]), и командная строка обязана
    // узнаваться по голове сама.
    "cp", "mv", "rm", "ln", "mkdir", "tar", "wget", "curl", "sh", "bash", "kpackagetool5",
    "kpackagetool6", "plasmapkg2", "lookandfeeltool", "gsettings", "dconf", "kwriteconfig5",
    "kwriteconfig6", "xdg-open", "sed", "meson", "ninja", "pip", "npm", "cargo", "dpkg", "rpm",
    "emerge", "xbps-install", "nix-env", "eopkg",
];

/// Имена технологий и продуктов — ОДИН список на два дела, и это не удобство,
/// а условие правильности: [`is_product_name_token`] решает судьбу ЦЕЛОГО
/// сегмента (`Gnome`, `2 Unity`, `6 Cinnamon`, `9 Kde Plasma` — подписи строк
/// из живых описаний, любой движок делает из них «Гном», «Единство»,
/// «Корицу»), а [`mask_tech_names`] прячет то же имя ВНУТРИ фразы. Разъедься
/// эти два списка — и «Sweet Kvantum theme» защищалось бы не так, как
/// «9 Kde Plasma» (правило #12).
///
/// ⚠️ Написание — КАНОНИЧЕСКОЕ (`GNOME`, а не `gnome`): по нему сверяется
/// регистр найденного в тексте слова ([`name_casing_matches`]). Фильтру
/// сегмента регистр по-прежнему безразличен.
const PRODUCT_NAMES: &[&str] = &[
    "GNOME", "KDE", "Plasma", "Qt", "GTK", "Kvantum", "Papirus", "Dolphin", "Konsole", "Latte",
    "SDDM", "Plymouth", "XFCE", "Cinnamon", "Mate", "Unity",
    // Остаток живого списка «Supported desktops» из описания Qogir: `3 Budgie`
    // любой движок превращает в «3 Баджи», а `8 Lxde`/`4 Pantheon` того же
    // класса — подписи строк, а не проза.
    "Budgie", "LXDE", "Pantheon",
    // Имена, которые встречаются только ВНУТРИ фразы живых описаний, — для
    // фильтра сегмента они ничего не меняют (рядом с ними всегда есть проза),
    // а прятать их от движка надо: «Firefox theme», «Nautilus headerbar»,
    // «Nordic Aurorae theme», «GTK2 engine Murrine 0.98.1.1 or later».
    "Aurorae", "Firefox", "Nautilus", "Murrine",
    // Палитра Nord: в описании товара «Nordic KDE» стоит «the awesome Nord
    // color palette», и движок делает из неё «Норд цветовая палитра».
    "Nord",
];

/// Описательные слова НАЗВАНИЯ ТОВАРА: по такому слову одному имя не узнаётся.
///
/// ⭐ ЗАЧЕМ. Название товара узнаётся не только целой цепочкой слов, но и
/// отдельными значащими словами (см. [`guarded_name_variants`]), — иначе товар
/// «Nordic KDE» не защищал бы своё же `Nordic Plasma theme`. Но брать ЛЮБОЕ
/// слово названия нельзя: из «Sweet Mars LAF Plasma 6» слово `laf` охраняло бы
/// каждое `LAF` в прозе, а из «Dark Glass Theme» — слово `theme`, и тогда под
/// заполнителем оказалась бы вся проза описания.
///
/// Список лежит рядом с [`PRODUCT_NAMES`] и ровно по той же причине: одно
/// место на все решения об именах (правило #12).
const NAME_FILLER_WORDS: &[&str] = &[
    "theme", "themes", "icons", "icon", "color", "colors", "colour", "colours", "scheme",
    "schemes", "global", "laf", "pack", "edition", "for", "and", "the", "with", "dark", "light",
];

/// Теги «кодового окружения»: внутри них не переводится НИЧЕГО, даже если
/// текст похож на прозу — автор явно сказал, что это код.
const CODE_ENV_TAGS: &[&str] = &["code", "pre", "kbd", "samp", "tt"];

/// Расширения файлов: токен, кончающийся на такое, — путь, не слово.
///
/// Архивы и файлы тем (`*.tar.gz`, `index.theme`, `Nordic.colors`,
/// `Sweet.kvconfig`) добавлены 03.10 вместе с маскировкой техники внутри прозы:
/// такое имя файла теперь не отвергает фразу, а уходит под заполнитель и
/// возвращается дословно.
const FILE_EXTENSIONS: &[&str] = &[
    ".sh", ".conf", ".desktop", ".py", ".qml", ".json", ".gz", ".xz", ".zst", ".bz2", ".zip",
    ".7z", ".tar", ".theme", ".colors", ".kvconfig", ".svg", ".png", ".jpg", ".ini", ".txt",
    ".css", ".xml", ".md",
];

/// Обрезать по краям всё, что не буква и не цифра (`«Features:»` → `Features`,
/// `(Elementary` → `Elementary`). `+`/`#` тоже уходят, поэтому `GTK+`
/// опознаётся как имя продукта `gtk`.
fn trim_token_punct(tok: &str) -> &str {
    tok.trim_matches(|c: char| !c.is_alphanumeric())
}

/// Версия или разрешение экрана: `1.2.3`, `5.8`, `0.98.1.1`, `v0.1`,
/// `3840x2160`. Такой токен — не слово: он не мешает соседней прозе
/// (`Murrine 0.98.1.1 or later` переводим), но сам по себе прозой не является.
pub fn is_version_or_resolution_token(tok: &str) -> bool {
    let t = trim_token_punct(tok);
    if t.is_empty() {
        return false;
    }
    // Разрешение: <цифры>x<цифры> (или `×`).
    let mut split = t.splitn(2, ['x', 'X', '×']);
    if let (Some(a), Some(b)) = (split.next(), split.next()) {
        if !a.is_empty()
            && !b.is_empty()
            && a.chars().all(|c| c.is_ascii_digit())
            && b.chars().all(|c| c.is_ascii_digit())
        {
            return true;
        }
    }
    // Версия: необязательная `v`, затем не меньше двух числовых частей.
    let body = t.strip_prefix('v').or_else(|| t.strip_prefix('V')).unwrap_or(t);
    let parts: Vec<&str> = body.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Токен «техника по форме»: в нём есть `/`, `\`, `::`, `--`, `~/`, `$`, `@`
/// либо он кончается расширением файла.
///
/// ⚠️ Проверяется только на ЗНАЧИМЫХ токенах (в которых есть буква) —
/// намеренно. `-->` в живой фразе «browsing the related collection -->
/// Aritim-Light Wallpapers Collection» (образец 12) содержит `--`, но это
/// стрелка в прозе, а не ключ командной строки; ключ всегда прилеплен к
/// буквам (`--user`).
pub fn is_technical_token(tok: &str) -> bool {
    if tok.contains('/')
        || tok.contains('\\')
        || tok.contains("::")
        || tok.contains("--")
        || tok.contains('$')
        || tok.contains('@')
    {
        return true;
    }
    let lower = tok.trim_end_matches(|c: char| ",;:!?)\"'»".contains(c)).to_lowercase();
    let base = lower.trim_end_matches('.');
    FILE_EXTENSIONS.iter().any(|ext| lower.ends_with(ext) || base.ends_with(ext))
}

/// Имя продукта целиком (без учёта регистра, края от пунктуации обрезаются).
///
/// Регистр здесь безразличен намеренно: решается судьба ЦЕЛОГО сегмента, и
/// подпись строки `9 kde plasma` — такая же подпись, как `9 Kde Plasma`.
/// Регистр важен только там, где имя вынимается ИЗ фразы
/// ([`name_casing_matches`]).
pub fn is_product_name_token(tok: &str) -> bool {
    let t = trim_token_punct(tok);
    !t.is_empty() && PRODUCT_NAMES.iter().any(|n| n.eq_ignore_ascii_case(t))
}

/// «Человеческое слово»: одни буквы (после обрезки краевой пунктуации) и их не
/// меньше трёх. Нужно ради ЗАГОЛОВКОВ описаний — `Features:`, `Installation:`,
/// `Note:`, `Folders`, `WALLPAPERS` — это проза из одного слова, и отсекать её
/// правилом «всего один токен» нельзя.
fn is_plain_word(tok: &str) -> bool {
    let t = trim_token_punct(tok);
    t.chars().count() >= 3 && t.chars().all(|c| c.is_alphabetic())
}

/// Ключ командной строки: `-r`, `-S`, `--user` (один-два дефиса, затем буква
/// или цифра). Одинокое тире и стрелка `-->` ключом не считаются.
///
/// Дефисом считается и неразрывный `‑` (U+2011) с роднёй: живое описание
/// «Sugar Candy for SDDM» пишет команду `sddm‑greeter ‑‑test‑mode` именно им.
fn is_command_flag(tok: &str) -> bool {
    let is_dash = |c: char| matches!(c, '-' | '\u{2010}' | '\u{2011}' | '\u{2212}');
    let body = tok.trim_start_matches(is_dash);
    let dashes = tok[..tok.len() - body.len()].chars().count();
    (1..=2).contains(&dashes) && body.chars().next().is_some_and(char::is_alphanumeric)
}

/// Командная строка ВНУТРИ фразы: голова команды в нижнем регистре, за
/// которой стоит ключ (`Copy: cp -r bigsur ~/.local/share/sounds/`, `Use the
/// following command: kpackagetool6 -i Alba.P6.tar.xz` — обе строки живые, KDE
/// Store). Такую фразу движку не отдаём целиком: показать ему `cp -r bigsur`
/// без заполнителя — значит рискнуть «бигсуром».
fn has_inline_command(tokens: &[&str]) -> bool {
    tokens.windows(2).any(|pair| {
        let head = pair[0].trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
        COMMAND_HEADS.contains(&head) && is_command_flag(pair[1])
    })
}

/// Пара обычных слов через косую черту — `and/or`, `light/dark`, `on/off`.
/// Это проза, а не путь: обе части — строчные буквы, без точек и цифр.
/// ⚠️ Части с заглавной (`Fedora/RedHat`, `Plasma/KDE`) сюда НЕ попадают —
/// это имена, и им место под заполнителем, а не в руках движка.
fn is_plain_slash_pair(core: &str) -> bool {
    let parts: Vec<&str> = core.split('/').collect();
    parts.len() >= 2
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_alphabetic() && c.is_lowercase()))
}

/// Технические куски ВНУТРИ строки — то, что движку показывать нельзя, но и
/// отвергать из-за него всю фразу незачем: (начало, конец) в байтах, по
/// порядку, без пересечений.
///
/// ⭐ ЗАЧЕМ (дефект владельца 03.10, KDE Store, «ZorinOS 6 Sound Theme»): «Just
/// unzip the file and put in the /usr/share/sounds folder to add the to your
/// desktop.» оставалась английской целиком, потому что прежнее правило 4
/// [`is_prose_segment`] отвергало строку за ОДИН путь в ней. Теперь путь уходит
/// под заполнитель тем же приёмом, что имена ([`mask_names`]), остальное
/// переводится, и путь возвращается дословно.
///
/// Что считается куском техники:
///   * всё между парой обратных кавычек вместе с ними (`` `make install` ``);
///   * токен между пробелами, технический по форме ([`is_technical_token`]:
///     путь, ссылка, `$HOME`, `--user`, `index.theme`, `*.tar.gz`). Краевая
///     пунктуация фразы («(», «.», «,», «:», кавычки) в кусок НЕ входит:
///     «… /usr/share/sounds.» прячет путь без точки;
///   * в токене должна быть буква (стрелка `-->` — не техника) и не должно
///     быть `@` (почта отвергает строку целиком, см. [`NAME_PLACEHOLDER_MARK`]);
///   * пара обычных слов через косую черту (`and/or`) — проза, не кусок.
pub fn technical_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    // Обратные кавычки — парами.
    let ticks: Vec<usize> = text.match_indices('`').map(|(i, _)| i).collect();
    for pair in ticks.chunks_exact(2) {
        spans.push((pair[0], pair[1] + 1));
    }
    let in_ticks = |s: usize, e: usize| spans.iter().any(|(a, b)| s < *b && e > *a);
    let mut tokens: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                tokens.push((s, i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        tokens.push((s, text.len()));
    }
    let mut found: Vec<(usize, usize)> = Vec::new();
    for (s, e) in tokens {
        if in_ticks(s, e) {
            continue;
        }
        let tok = &text[s..e];
        let lead = tok.len() - tok.trim_start_matches(|c: char| "([{\"'«“‘<".contains(c)).len();
        let core_end = tok.trim_end_matches(|c: char| ".,;:!?)]}\"'»”’>".contains(c)).len();
        if core_end <= lead {
            continue;
        }
        let core = &tok[lead..core_end];
        if core.contains(NAME_PLACEHOLDER_MARK)
            || !core.chars().any(char::is_alphabetic)
            || is_version_or_resolution_token(core)
            || is_plain_slash_pair(core)
            || !is_technical_token(core)
        {
            continue;
        }
        found.push((s + lead, s + core_end));
    }
    spans.extend(found);
    spans.sort();
    spans
}

/// Проза ли сегмент, то есть звать ли на него движок перевода.
///
/// Порядок правил важен, поэтому он зафиксирован тестами
/// (`translator_prose.rs`, строки — из живых описаний мастерской):
///  1. нет ни одной буквы (`____`, `-----`, `1.2.3`) — не проза;
///  2. сегмент начинается с головы команды в НИЖНЕМ регистре и короче девяти
///     токенов (`sudo apt-get install …`, `pacman -S …`) — не проза. Регистр и
///     длина здесь не придирка, а защита прозы: «Make sure to …» начинается с
///     `Make` с большой буквы и командой не является;
///  3. значимых слов не осталось (были одни числа/пунктуация) — не проза;
///  4. техника ВНУТРИ строки ([`technical_spans`]: путь, ссылка, имя файла,
///     переменная, ключ, кусок в обратных кавычках). До 03.10 любой такой токен
///     отвергал строку целиком, и «Just unzip the file and put in the
///     /usr/share/sounds folder…» оставалась английской (дефект владельца,
///     KDE Store, «ZorinOS 6 Sound Theme»). Теперь техника уходит под
///     заполнитель ([`mask_names`]) и возвращается дословно, а судьба строки
///     решается по тому, что ОСТАЛОСЬ:
///     * `@` в значимом слове (почта) — не проза, как и прежде: на `@` держится
///       проверка потери заполнителя;
///     * строка НАЧИНАЕТСЯ с техники (`~/.config/Kvantum`, `./install.sh`,
///       `` `make install` ``) — не проза; кроме списка имён через косую черту
///       (`Fedora/RedHat distros:`);
///     * первое слово строчное, а рядом ключ (`kvantummanager --set Nordic`,
///       `cp -r Foo ~/.themes`), либо внутри стоит голова команды с ключом
///       (`Copy: cp -r bigsur ~/.local/share/sounds/`) — командная строка, не
///       проза;
///     * иначе техника вырезается, и остаток проверяется правилами 1-8 заново
///       («Copy the folder to and pick it in Settings.» — проза);
///  5. ВСЕ значимые слова — имена продуктов (`Gnome`, `2 Unity`, `9 Kde
///     Plasma`) — не проза;
///  6. значимое слово одно: проза, только если это человеческое слово (пункт
///     про заголовки выше);
///  7. не-букв среди непробельных символов больше трети — не проза;
///  8. слов длиной от двух букв меньше двух — не проза.
pub fn is_prose_segment(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || !trimmed.chars().any(|c| c.is_alphabetic()) {
        return false;
    }
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    // Значимые токены — те, в которых есть буква и которые не версия/разрешение.
    let words: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|t| t.chars().any(|c| c.is_alphabetic()) && !is_version_or_resolution_token(t))
        .collect();
    // (2) голова команды.
    if tokens.len() <= 8 {
        if let Some(first) = words.first() {
            let head = first.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
            if head.chars().all(|c| !c.is_uppercase()) && COMMAND_HEADS.contains(&head) {
                return false;
            }
        }
    }
    // (3) одни числа и пунктуация.
    if words.is_empty() {
        return false;
    }
    // (4) техника внутри строки — под заполнитель, судьбу решает остаток.
    if words.iter().any(|w| w.contains(NAME_PLACEHOLDER_MARK)) {
        return false;
    }
    let spans = technical_spans(trimmed);
    if !spans.is_empty() {
        // Начало-техника. Исключение — список имён через косую черту
        // (`Fedora/RedHat distros:`, живой заголовок описания Qogir): это
        // подпись, а не путь, и её остаток переводится («Дистрибутивы …:»).
        let first_letter = trimmed.find(|c: char| c.is_alphabetic()).unwrap_or(0);
        let (lead_start, lead_end) = spans[0];
        let lead_is_names = trimmed[lead_start..lead_end]
            .split('/')
            .all(|p| !p.is_empty() && p.chars().all(char::is_alphabetic));
        if lead_start <= first_letter && !lead_is_names {
            return false;
        }
        let first_is_lowercase = words.first().is_some_and(|w| !w.chars().any(char::is_uppercase));
        if (first_is_lowercase && tokens.iter().any(|t| is_command_flag(t)))
            || has_inline_command(&tokens)
        {
            return false;
        }
        let mut rest = String::with_capacity(trimmed.len());
        let mut copied = 0usize;
        for (start, end) in &spans {
            rest.push_str(&trimmed[copied..*start]);
            rest.push(' ');
            copied = *end;
        }
        rest.push_str(&trimmed[copied..]);
        return is_prose_segment(&rest);
    }
    // (5) весь сегмент — имена продуктов.
    if words.iter().all(|w| is_product_name_token(w)) {
        return false;
    }
    // (6) одно значимое слово — прозой считаем только человеческое слово.
    if words.len() == 1 {
        return is_plain_word(words[0]);
    }
    // (7) доля не-букв.
    let dense: Vec<char> = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    let letters = dense.iter().filter(|c| c.is_alphabetic()).count();
    if (dense.len() - letters) * 3 > dense.len() {
        return false;
    }
    // (8) меньше двух слов длиной от двух букв.
    let long_words = words
        .iter()
        .filter(|w| trim_token_punct(w).chars().filter(|c| c.is_alphabetic()).count() >= 2)
        .count();
    long_words >= 2
}

// ── Защита имён технологий ВНУТРИ фразы ────────────────────────────────────
//
// Фильтр выше решает судьбу ЦЕЛОГО сегмента, и этого мало: «Sweet Kvantum
// theme» для него проза (три слова, и не все они имена), а живой движок
// отвечает «сладкий Kvantum тема» на неё и «квантская тема» на «A Kvantum
// theme heavily based on Microsoft's Fluent Design elements» — имя внутри
// фразы он склоняет и переводит. Дом требует обратного: имена технологий и
// стандартов остаются латиницей.
//
// Лечение — подмена: перед движком имя уходит под заполнитель, после движка
// возвращается на место дословно. Функции ЧИСТЫЕ ([`mask_tech_names`] /
// [`unmask_tech_names`]), в текст движка они не заглядывают, а договор
// [`crate::translator_engine::TranslateEngine`] не меняют вовсе — подмена
// живёт СНАРУЖИ вызова.

/// Знак-метка заполнителя.
///
/// `@` выбран не на вкус: это единственный знак, который НЕ МОЖЕТ прийти в
/// переводе от движка сам. Во-первых, сегмент со значимым токеном, содержащим
/// `@`, фильтр уже признал техникой ([`is_technical_token`], правило 4) — до
/// движка такой текст не доходит; во-вторых, `@` в исходнике всё равно
/// выключает подмену целиком (см. [`mask_tech_names`]). Значит `@`, оставшийся
/// в ответе после подстановки имён, — ВСЕГДА след порчи, и это делает проверку
/// потери надёжной, а не приблизительной.
pub const NAME_PLACEHOLDER_MARK: char = '@';

/// Заполнитель на месте имени с номером `index`: `@0@`, `@1@`, …
///
/// Форма выбрана ЗАМЕРОМ на живой модели Firefox `en→ru`, а не догадкой
/// (стенд `zz_probe`, 21.09; в дереве остался его итог — сверка
/// `live_names_survive_the_real_engine` в `translator_engine.rs`). Сравнивались
/// двадцать форм на семи живых фразах; итог:
///   * `@0@` — цела во всех; хвостовая метка ещё и спасает от склейки номеров
///     (`@1@` не является началом `@10@`);
///   * `@@0@@` — рвётся в 6 фразах из 7 («сладкая @@@@@ тема»): движок множит
///     и глотает повторённый знак;
///   * `【0】`, `⟦0⟧` — распадаются на байты `<0xE2><0x9F><0xA6>` (знака нет в
///     словаре модели), `$0` — становится «1 доллар», `<0>`/`{{0}}`/`Vv0vv` —
///     дублируются и дописываются лишними буквами.
/// Форма без метки на конце (`@0`, `@x0`) тоже держится, но номера тогда
/// склеиваются с соседними цифрами живого текста («@1 3» ↔ «@13»).
pub fn name_placeholder(index: usize) -> String {
    format!("{NAME_PLACEHOLDER_MARK}{index}{NAME_PLACEHOLDER_MARK}")
}

/// Текст с заполнителями вместо имён и сами имена по порядку номеров:
/// `names[0]` — то, что стояло в `@0@`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MaskedNames {
    pub text: String,
    pub names: Vec<String>,
}

/// Совпадает ли найденное в тексте слово с каноническим именем из
/// [`PRODUCT_NAMES`] ПО РЕГИСТРУ.
///
/// Считаются своими три записи имени: как в списке (`Kvantum`, `GNOME`),
/// ВСЕМИ ЗАГЛАВНЫМИ (`KVANTUM`) и С Заглавной (`Gnome`, `Kde`, `Xfce` — именно
/// так имена написаны в живых описаниях мастерской). Полностью строчная запись
/// именем НЕ считается, и это главное здесь: в списке лежат `Mate`, `Unity`,
/// `Cinnamon`, `Plasma` — обычные английские слова, и во фразе «works with your
/// mate» подменять `mate` нечем.
pub fn name_casing_matches(canonical: &str, found: &str) -> bool {
    if canonical == found || found == canonical.to_uppercase() {
        return true;
    }
    let lower = canonical.to_lowercase();
    let mut capitalized = String::with_capacity(lower.len());
    for (i, c) in lower.chars().enumerate() {
        if i == 0 {
            capitalized.extend(c.to_uppercase());
        } else {
            capitalized.push(c);
        }
    }
    found == capitalized
}

/// Имя-АББРЕВИАТУРА в [`PRODUCT_NAMES`]: записано одними ЗАГЛАВНЫМИ (`KDE`,
/// `GTK`, `SDDM`, `XFCE`, `LXDE`, `GNOME`).
///
/// ⭐ ЗАЧЕМ. Такое имя остаётся именем в ЛЮБОМ регистре, и это лечит ПОТЕРЮ
/// СЛОВА, снятую живьём 23.09 на модели Firefox `en→ru`: «A colorful theme
/// inspired by sweet kde theme.» (товар «Iridescent») она отдаёт как
/// «красочная тема, вдохновленная сладкой темой.» — слова `kde` в переводе нет
/// вовсе. Тот же замер показывает, что виновата запись строчными: `sweet KDE
/// theme` и `sweet Kde theme` доходят целыми, заполнитель — тоже
/// («вдохновленная сладкой @0@ темой»), а одинокое `kde is nice.` движок и
/// вовсе переводит как «Кейд».
///
/// ⚠️ Послабление только для НАШЕГО списка и только для аббревиатур: `kde`,
/// `gtk`, `sddm`, `xfce`, `lxde` английскими словами не бывают ни в каком
/// регистре. Имена, которые ЯВЛЯЮТСЯ словами (`Mate`, `Unity`, `Plasma`,
/// `Cinnamon`), в списке записаны С Заглавной, под это правило не попадают и
/// живут по строгому [`name_casing_matches`] — «works with your mate»
/// по-прежнему переводится. Названия ТОВАРОВ от вызывающего послабления не
/// получают вовсе: подпись «SWEET» заглавными не должна съедать `sweet` в
/// прозе.
///
/// Цена правила ровно одна и она мелкая: `GNOME` в списке тоже заглавными, а
/// строчное `gnome` — английское слово («садовый гном»). Теперь такая фраза
/// останется с латинским `gnome` вместо «гнома». В магазине оформления
/// строчное `gnome` — почти всегда рабочий стол, а латинское слово в прозе
/// честнее пропавшего.
fn is_acronym_name(canonical: &str) -> bool {
    canonical.chars().count() >= MIN_GUARDED_NAME_CHARS
        && canonical.chars().any(|c| c.is_alphabetic())
        && canonical.chars().all(|c| !c.is_alphabetic() || c.is_uppercase())
}

/// Слово целиком — имя технологии (с учётом регистра, а для аббревиатур
/// списка — в любом, см. [`is_acronym_name`]).
fn is_masked_name(word: &str) -> bool {
    PRODUCT_NAMES.iter().any(|n| {
        name_casing_matches(n, word) || (is_acronym_name(n) && word.eq_ignore_ascii_case(n))
    })
}

/// Границы слов: максимальные пробеги букв и цифр. Именно они дают «границу
/// слова», из-за которой `kde` внутри `kdeglobals` не имя: пробег там —
/// `kdeglobals` целиком, и он ни с чем не сравнится.
fn word_runs(text: &str) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_alphanumeric() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            runs.push((s, i));
        }
    }
    if let Some(s) = start {
        runs.push((s, text.len()));
    }
    runs
}

/// Клей между двумя именами: они прячутся под ОДИН заполнитель, если между
/// ними только пробелы, запятая или косая черта.
///
/// Это не украшение, а живучесть, и она замерена: «Nordic Kvantum theme»
/// двумя заполнителями подряд движок ломает («Nordic @ Kvantum тема» — знак
/// остался, номер съеден), а список «Gnome, Unity, Budgie, Pantheon, XFCE,
/// Cinnamon and Mate» из семи заполнителей он размножает (Budgie и XFCE
/// приходят дважды). Одним заполнителем на всю цепочку обе фразы проходят
/// целыми — а содержимое цепочки возвращается в текст дословно, так что
/// запятые внутри неё остаются авторскими.
fn is_name_glue(sep: &str) -> bool {
    matches!(sep.trim(), "" | "," | "/")
}

/// Клей ВНУТРИ имени, переданного вызывающим: `Aritim-Light` в описании
/// пишется через дефис, `NT Legacy` — через пробел, а `Sweet_Mars` — через
/// подчёркивание, и это одно и то же имя. Поэтому внутренний шов имени
/// шире, чем клей между ДВУМЯ разными именами ([`is_name_glue`]): тот решает
/// судьбу чужих друг другу слов, а этот — шов заранее известного имени.
fn is_guarded_gap(sep: &str) -> bool {
    matches!(sep.trim(), "" | "," | "/" | "-" | "–" | "—" | "_")
}

/// Сколько знаков должно быть в имени от вызывающего, чтобы его вообще
/// прятать.
///
/// ⛔ Имя из одного-двух знаков не прячется никогда. Товар «A» или «UI» дал бы
/// заполнитель на каждое такое слово описания, а цена ошибки здесь
/// несимметрична: лишний заполнитель — это либо порча фразы, либо (по второму
/// ситу) отказ от перевода ВСЕГО сегмента. Пустое имя — тот же случай.
const MIN_GUARDED_NAME_CHARS: usize = 3;

/// Слова охраняемого имени по порядку, либо `None` — имя прятать не надо
/// (пустое, из одной пунктуации или короче [`MIN_GUARDED_NAME_CHARS`]).
fn guarded_name_words(name: &str) -> Option<Vec<String>> {
    let runs = word_runs(name);
    let letters: usize = runs.iter().map(|(s, e)| name[*s..*e].chars().count()).sum();
    if runs.is_empty() || letters < MIN_GUARDED_NAME_CHARS {
        return None;
    }
    Some(runs.iter().map(|(s, e)| name[*s..*e].to_string()).collect())
}

/// Значащее ли слово названия: годится ли оно на то, чтобы узнать товар ОДНИМ
/// собой.
///
/// Не годятся: описательные слова ([`NAME_FILLER_WORDS`]), числа («6» из
/// «Sweet Mars LAF Plasma 6») и куски короче [`MIN_GUARDED_NAME_CHARS`] («NT»
/// из «NT Legacy» — по нему одному товар не узнаётся, зато целая цепочка
/// «NT Legacy» ловится как и прежде).
fn is_meaningful_name_word(word: &str) -> bool {
    if word.chars().count() < MIN_GUARDED_NAME_CHARS {
        return false;
    }
    if word.chars().all(|c| c.is_numeric()) {
        return false;
    }
    let lower = word.to_lowercase();
    !NAME_FILLER_WORDS.contains(&lower.as_str())
}

/// Во что превращается название товара для охраны: цепочки слов, которые
/// считаются этим товаром, от длинной к короткой.
///
/// ⭐ ЗАЧЕМ. Раньше охранялась ТОЛЬКО целая цепочка слов названия, и этого
/// хватало ровно на однословные имена. Живая приёмка 23.09 показала цену:
/// товар «Nordic KDE» (цепочка `["Nordic","KDE"]`) в своём же описании имел
/// «Nordic Plasma theme» — цепочка не совпала, и движок отдавал «Скандинавская
/// Plasma тема»; товар «Sweet Mars LAF Plasma 6» так же терял `Sweet-Mars
/// Kvantum theme` («Сладкий марс Kvantum тема»).
///
/// Поэтому из названия берётся:
///   1. целая цепочка всех слов — как и раньше, она пробуется первой;
///   2. группы ПОДРЯД идущих значащих слов («Sweet Mars» из «Sweet Mars LAF
///      Plasma 6») — они ловят название, написанное через шов
///      (`Sweet-Mars`), одним заполнителем, а не двумя подряд, которые живой
///      движок ломает;
///   3. каждое значащее слово по отдельности («Nordic», «Sweet», «Mars»).
///
/// ⚠️ Регистр здесь не ослабляется: цепочка из ОДНОГО слова (а любое слово
/// пункта 3 — такая цепочка) требует совпадения регистра по
/// [`name_casing_matches`], иначе товар «Dark» съел бы слово `dark` в прозе.
/// Правило живёт в [`guarded_name_at`] и общее на оба списка.
fn guarded_name_variants(name: &str) -> Vec<Vec<String>> {
    let Some(words) = guarded_name_words(name) else {
        return Vec::new();
    };
    let mut out: Vec<Vec<String>> = vec![words.clone()];
    let mut group: Vec<String> = Vec::new();
    let mut groups: Vec<Vec<String>> = Vec::new();
    for word in &words {
        if is_meaningful_name_word(word) {
            group.push(word.clone());
        } else if !group.is_empty() {
            groups.push(std::mem::take(&mut group));
        }
    }
    if !group.is_empty() {
        groups.push(group);
    }
    for group in groups {
        if group.len() > 1 {
            out.push(group.clone());
        }
        out.extend(group.into_iter().map(|w| vec![w]));
    }
    let mut seen: Vec<Vec<String>> = Vec::with_capacity(out.len());
    out.retain(|words| {
        let fresh = !seen.contains(words);
        if fresh {
            seen.push(words.clone());
        }
        fresh
    });
    out
}

/// Охраняемое имя, начинающееся ПРОБЕГОМ `k`: сколько пробегов оно занимает
/// (`None` — с этого места ни одно имя не начинается).
///
/// Имена вызывающего пробуются раньше [`PRODUCT_NAMES`] и от самого длинного,
/// иначе «NT Legacy» проиграло бы своему же первому слову.
///
/// ⚠️ Правило регистра разное, и это главное решение здесь:
///   * имя из НЕСКОЛЬКИХ слов сверяется БЕЗ учёта регистра. Совпасть с целой
///     цепочкой слов случайно нельзя, так что «sweet kvantum» в тексте — это
///     тот же товар, что «Sweet Kvantum» в его названии;
///   * имя из ОДНОГО слова требует, чтобы регистр совпал по
///     [`name_casing_matches`] (как в названии, ВСЕМИ ЗАГЛАВНЫМИ или С
///     Заглавной; полностью строчная запись — не имя). Названия сплошь бывают
///     обычными словами — «Dark», «Sweet», «Nordic», «Layan», — и «a dark
///     theme» переводить надо («тёмная тема»), а «Dark» как имя товара —
///     нельзя. Это ровно то правило, по которому уже живут `Mate`, `Unity`,
///     `Cinnamon` из общего списка (правило #12: одно правило на оба списка).
fn guarded_name_at(
    text: &str,
    runs: &[(usize, usize)],
    k: usize,
    guarded: &[Vec<String>],
) -> Option<usize> {
    for words in guarded {
        if k + words.len() > runs.len() {
            continue;
        }
        let matched = words.iter().enumerate().all(|(i, word)| {
            let (s, e) = runs[k + i];
            let found = &text[s..e];
            let same = if words.len() == 1 {
                name_casing_matches(word, found)
            } else {
                found.to_lowercase() == word.to_lowercase()
            };
            same && (i == 0 || is_guarded_gap(&text[runs[k + i - 1].1..s]))
        });
        if matched {
            return Some(words.len());
        }
    }
    let (start, end) = runs[k];
    is_masked_name(&text[start..end]).then_some(1)
}

/// Спрятать имена технологий под заполнители перед вызовом движка.
///
/// С 03.10 под заполнитель уходит и ТЕХНИКА внутри фразы — путь, ссылка, имя
/// файла, переменная, `код` ([`technical_spans`]); возвращается она тем же
/// [`unmask_tech_names`] дословно, байт в байт.
///
/// ⚠️ Если в исходнике УЖЕ есть `@`, подмена не делается вовсе (имена
/// остаются как есть): иначе заполнитель было бы не отличить от авторского
/// знака, и проверка потери перестала бы что-либо доказывать. Такой текст
/// переводится как до этой правки.
pub fn mask_tech_names(text: &str) -> MaskedNames {
    mask_names(text, &[])
}

/// То же, что [`mask_tech_names`], плюс имена, которые попросил спрятать
/// ВЫЗЫВАЮЩИЙ: название самого товара, имя автора — всё, чего в списке
/// [`PRODUCT_NAMES`] быть не может.
///
/// ⭐ ЗАЧЕМ. Списка названий тем нет и быть не может (мастерская пополняется
/// каждый день), но название открытого товара окно знает — оно приходит рядом
/// с описанием. Без этого живой движок отвечал «Скандинавская Kvantum тема» на
/// «Nordic Kvantum theme» и «Сладкая Kvantum тема» на «Sweet Kvantum theme»:
/// имя технологии мы уже защитили, а НАЗВАНИЕ ТОВАРА он переводил как прозу.
///
/// Название узнаётся не только целой цепочкой своих слов, но и значащими
/// словами по отдельности ([`guarded_name_variants`]): у товара «Nordic KDE»
/// строка описания «Nordic Plasma theme» целой цепочки не содержит, а защищать
/// её надо.
///
/// Приём тот же и сита те же: имя уходит под заполнитель, [`unmask_tech_names`]
/// возвращает его ДОСЛОВНО (ровно тем написанием, что стояло в исходном
/// тексте, а не тем, что пришло от вызывающего), а не сошлись заполнители —
/// сегмент отдаётся оригиналом.
pub fn mask_names(text: &str, guarded: &[&str]) -> MaskedNames {
    if text.contains(NAME_PLACEHOLDER_MARK) {
        return MaskedNames { text: text.to_string(), names: Vec::new() };
    }
    // Длинное имя пробуется раньше короткого ещё и внутри ОДНОГО названия:
    // целая цепочка «Sweet Mars LAF Plasma 6» — раньше группы «Sweet Mars», а
    // та — раньше одинокого «Sweet». Сортировка устойчивая, так что порядок
    // [`guarded_name_variants`] внутри одной длины сохраняется.
    let mut extra: Vec<Vec<String>> =
        guarded.iter().flat_map(|n| guarded_name_variants(n)).collect();
    extra.sort_by(|a, b| b.len().cmp(&a.len()));

    // Техника внутри фразы (путь, ссылка, файл, переменная, `код`) — под
    // заполнитель наравне с именами ([`technical_spans`]). Имя ВНУТРИ куска
    // техники (`kde` в `~/.local/share/kde`) отдельно не прячется: кусок
    // уходит целиком.
    let tech = technical_spans(text);
    let overlaps_tech = |s: usize, e: usize| tech.iter().any(|(a, b)| s < *b && e > *a);
    let mut spans: Vec<(usize, usize)> = tech.clone();

    let runs = word_runs(text);
    let mut k = 0usize;
    while k < runs.len() {
        if overlaps_tech(runs[k].0, runs[k].1) {
            k += 1;
            continue;
        }
        let Some(taken) = guarded_name_at(text, &runs, k, &extra) else {
            k += 1;
            continue;
        };
        let (start, end) = (runs[k].0, runs[k + taken - 1].1);
        if overlaps_tech(start, end) {
            k += 1;
            continue;
        }
        spans.push((start, end));
        k += taken;
    }
    spans.sort();

    // Цепочка подряд (имена и техника через пробел, запятую или косую черту)
    // — под ОДИН заполнитель: два заполнителя подряд живой движок ломает.
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        if let Some(last) = merged.last_mut() {
            if start >= last.1 && is_name_glue(&text[last.1..start]) {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    let mut out = String::with_capacity(text.len());
    let mut names: Vec<String> = Vec::new();
    let mut copied = 0usize;
    for (start, end) in merged {
        out.push_str(&text[copied..start]);
        out.push_str(&name_placeholder(names.len()));
        names.push(text[start..end].to_string());
        copied = end;
    }
    out.push_str(&text[copied..]);
    MaskedNames { text: out, names }
}

/// Вернуть имена на место в переводе. `None` — заполнители не сошлись, перевод
/// брать НЕЛЬЗЯ.
///
/// Потеря ловится двумя ситами, и вместе они закрывают все виды порчи,
/// увиденные на живой модели:
///   1. каждый заполнитель обязан встретиться РОВНО ОДИН раз — это и потеря
///      (движок съел `@2@`), и размножение (`Budgie` дважды);
///   2. после подстановки в строке не должно остаться ни одного `@` — это
///      огрызки («Nordic @ Kvantum тема», `@@1@@@`) и выдуманные номера,
///      которых мы не давали.
/// Имён не было — сверять нечего, перевод берётся как есть (`@` в нём мог
/// прийти только из исходника, см. [`mask_tech_names`]).
///
/// ⭐ ПЕРЕД ситами лечится ОДИН вид размножения — СЛИПШИЙСЯ повтор
/// ([`collapse_adjacent_repeat`]): `@1@ @1@` подряд. Приёмка 24.09, комментарий
/// к товару «Nordic KDE»: «Can we expect soon the @0@ Bluish theme of @1@ for
/// @2@ too?» живая модель отдала как «…@0@ Bluish тему @1@ @1@ для @2@ тоже?»,
/// сито 1 отвергало перевод, и покупатель видел английский комментарий целиком
/// (вместе с «Very nice.», которое перевелось прекрасно). Слипшийся повтор
/// однозначен: имя стояло в источнике ОДИН раз, значит вторая копия подряд —
/// заикание декодировщика, и убрать её можно, не угадывая. Повтор в РАЗНЫХ
/// местах строки по-прежнему отказ: там вторая копия может быть настоящим
/// вторым упоминанием, и выбирать между ними мы не вправе.
pub fn unmask_tech_names(translated: &str, names: &[String]) -> Option<String> {
    if names.is_empty() {
        return Some(translated.to_string());
    }
    let mut out = translated.to_string();
    for (index, name) in names.iter().enumerate() {
        let placeholder = name_placeholder(index);
        out = collapse_adjacent_repeat(&out, &placeholder);
        if out.matches(&placeholder).count() != 1 {
            return None;
        }
        out = out.replace(&placeholder, name);
    }
    if out.contains(NAME_PLACEHOLDER_MARK) {
        return None;
    }
    Some(out)
}

/// Свернуть СЛИПШИЕСЯ повторы заполнителя: `@1@ @1@` → `@1@`.
///
/// Разделять копии разрешено только пробелами — ничего другого между ними при
/// заикании декодировщика не появлялось, а любая буква или знак между
/// заполнителями означают, что это два РАЗНЫХ места перевода, и сворачивать их
/// нельзя.
fn collapse_adjacent_repeat(text: &str, placeholder: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(placeholder) {
        let after = at + placeholder.len();
        out.push_str(&rest[..after]);
        rest = &rest[after..];
        loop {
            let gap = rest.len() - rest.trim_start_matches([' ', '\u{a0}', '\t']).len();
            match rest[gap..].strip_prefix(placeholder) {
                Some(tail) => rest = tail,
                None => break,
            }
        }
    }
    out.push_str(rest);
    out
}

/// Латинское слово из перевода, которого в источнике НЕ БЫЛО, — след порчи
/// движка. `None` — латиница в переводе вся своя.
///
/// ⭐ ЗАЧЕМ. Приёмка 24.09, товар «Awesome Widgets» (arcanis): строку
/// `graphical item support - tooltips, bars` живая модель отдала как «служба
/// графических элементов - toolbep, bars». `toolbep` — НЕ смесь алфавитов
/// (проверено по кодам: все семь букв ASCII, `U+0074 U+006F U+006F U+006C
/// U+0062 U+0065 U+0070`), а слово, ВЫДУМАННОЕ подсловным декодировщиком из
/// `tooltips`. Ни фильтр прозы, ни сита заполнителей такого не видят: имён в
/// строке нет вовсе.
///
/// Поймать это можно ровно одним способом — сравнением с источником. Латиница
/// в переводе `en→ru` приходит ТОЛЬКО из источника (имена, версии, ключи,
/// пути), своей латиницы у русского перевода нет. Значит латинское слово,
/// которого в источнике не найти, — порча, и предложение с ним брать нельзя.
///
/// Сравнение без учёта регистра и ПО ВХОЖДЕНИЮ: `Kvantum` из `Kvantum's` своё,
/// `GTK` из `GTK2` своё, `bars` из `bars` своё, а `toolbep` — чужое. Слова
/// короче двух букв не судятся (одинокая `x` в «3 x 4» и тому подобное).
pub fn invented_latin_word(source: &str, translated: &str) -> Option<String> {
    let lower_source = source.to_lowercase();
    let mut word = String::new();
    for c in translated.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphabetic() {
            word.push(c);
            continue;
        }
        if word.len() >= 2 && !lower_source.contains(&word.to_lowercase()) {
            return Some(word);
        }
        word.clear();
    }
    None
}

// ── Словарь поправок терминов (после движка) ──────────────────────────────

/// Одна поправка: НЕВЕРНАЯ словоформа движка → ВЕРНАЯ в той же самой
/// грамматической форме. Сравнение без учёта регистра и `ё`/`е`; регистр
/// первой буквы найденного слова переносится на замену.
#[derive(Debug, Clone, Copy)]
pub enum TermForm {
    /// Слово целиком — в любом месте фразы.
    Word(&'static str, &'static str),
    /// Повелительное, совпадающее по написанию с будущим временем
    /// (`загрузите`: «загрузите архив» и «если вы загрузите архив»). Меняется,
    /// только если среди трёх слов перед ним нет «вы» — иначе это будущее, и у
    /// замены была бы другая форма («скачаете», а не «скачайте»).
    Imperative(&'static str, &'static str),
    /// Существительное, род которого у замены другой (`извлечения` ср. р. →
    /// `распаковки` ж. р.). Меняется ТОЛЬКО сразу после предлога: тогда при
    /// нём нет ни прилагательного, ни сказуемого, которые пришлось бы
    /// согласовывать заново («после извлечения» → «после распаковки»), а
    /// «извлечение не сработало» остаётся как есть.
    AfterPreposition(&'static str, &'static str),
    /// Основа причастия: основа + полное окончание (`ый`, `ая`, `ого`, …,
    /// [`PARTICIPLE_ENDINGS`]) → новая основа + то же окончание
    /// («извлечённого» → «распакованного»). Окончание переносится как есть,
    /// значит род, число и падеж сохраняются без всякого склонения.
    ParticipleStem(&'static str, &'static str),
    /// Несколько слов подряд (через пробелы) → готовая замена.
    Phrase(&'static str, &'static str),
}

/// Полные окончания причастий и прилагательных — все, какие бывают у
/// `-нный`. Короткие формы (`извлечён`, `извлечена`) в словаре записаны
/// отдельными словами.
const PARTICIPLE_ENDINGS: &[&str] = &[
    "ый", "ий", "ая", "ое", "ые", "ого", "ой", "ую", "ых", "ым", "ыми", "ом", "ому",
];

/// Предлоги, после которых существительное [`TermForm::AfterPreposition`]
/// стоит без согласуемого слова.
const PREPOSITIONS: &[&str] = &[
    "после", "для", "до", "без", "при", "к", "ко", "перед", "от", "с", "со", "по", "из", "в",
    "во", "о", "об", "на", "через",
];

/// Группа поправок одного термина.
///
/// ⭐ ПРАВКА ТОЛЬКО ПО ПАРЕ: группа срабатывает, если в ИСХОДНОМ предложении
/// есть один из её английских терминов ([`TermFix::source_words`], сравнение
/// по началу слова: `unzip` ловит и `unzipping`) и проходит
/// [`TermFix::gate`]. «Расстегните», пришедшее не от `unzip`, словарь не
/// трогает — может, речь правда о застёжке.
#[derive(Debug, Clone, Copy)]
pub struct TermFix {
    /// Язык источника и перевода (`en` → `ru`). Словарь `en` → `de` встаёт
    /// сюда же своими группами, механизм общий.
    pub src: &'static str,
    pub tgt: &'static str,
    /// Начала английских слов, при которых группа включается.
    pub source_words: &'static [&'static str],
    /// Дополнительное условие по исходному предложению (в нижнем регистре).
    pub gate: fn(&str) -> bool,
    /// Пары «неверно → верно».
    pub forms: &'static [TermForm],
    /// Свернуть слипшийся повтор ВЕРНОЙ формы: «скачать скачать» → «скачать»
    /// (живая модель на «Download bigsur-sound-theme.tar.gz from Files»).
    pub collapse_repeats: bool,
}

fn always(_: &str) -> bool {
    true
}

/// `extract` значит «распаковать», только когда речь об архиве.
///
/// Живые описания KDE Store знают и другое `extract`: «automatically extract
/// and apply dominant icon color», «All sounds have been extracted from MacOS
/// Monterey», «icons extracted from the beta of Uos» — там «извлечь» верно.
/// Поэтому нужен признак архива (архив, zip/tar, файл, папка, пакет, путь,
/// download) и НЕ нужен `from` после `extract` (извлекают ИЗ чего-то, а
/// распаковывают архив).
fn extract_means_unpack(source: &str) -> bool {
    let Some(at) = source.find("extract") else { return false };
    // «…directly from extracted source files» — извлечённые ИЗ чего-то.
    if source[..at].trim_end().ends_with("from") {
        return false;
    }
    // `from` после `extract` — «извлечь ИЗ», если между ними не архив:
    // «extracted from MacOS Monterey», «extract the chrome folder from one of
    // the IE themes» — извлечь; «extract "maliceandspitekv.zip" from this
    // page» — распаковать (предмет — сам архив).
    let after = &source[at..];
    let words: Vec<&str> = after.split_whitespace().collect();
    if let Some(from) = words.iter().position(|w| *w == "from") {
        const ARCHIVE: &[&str] = &[
            "archive", "zip", "tar", ".gz", ".xz", ".7z", ".rar", ".bz2", ".zst", "tgz",
            "compressed", "package",
        ];
        let object = &words[..from];
        if !object.iter().any(|w| ARCHIVE.iter().any(|a| w.contains(a))) {
            return false;
        }
    }
    if source.contains("color") || source.contains("colour") {
        return false;
    }
    const MARKERS: &[&str] = &[
        "archive", "zip", "tar", "compressed", "package", "file", "folder", "download", "~/",
        "/usr/", "/home", ".7z", ".rar",
    ];
    MARKERS.iter().any(|m| source.contains(m))
}

/// `download` → «скачать», только если в предложении нет ДРУГОГО `load`
/// (`upload`, `load`, `reload`): там «загрузить» может быть законным словом
/// для них. И нет имени окна KDE «Download New …» («The Download New Global
/// Themes program…»): у KDE ru это «Загрузить новые …», и переделывать чужое
/// название окна в «скачать» нельзя.
fn download_alone(source: &str) -> bool {
    let other_load = source
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.contains("load") && !w.starts_with("download"));
    !other_load && !source.contains("download new")
}

/// «Drag and drop» в любом написании, а не любое `drag…` (`dragon`).
fn drag_and_drop(source: &str) -> bool {
    ["drag and drop", "drag & drop", "drag-and-drop", "drag'n'drop", "drag n drop"]
        .iter()
        .any(|p| source.contains(p))
}

/// «Files» — вкладка страницы товара KDE Store («Download … from Files»,
/// «from the Files tab»), а не слово «файлы».
fn files_tab(source: &str) -> bool {
    source.contains("from files") || source.contains("files tab") || source.contains("files(")
}

/// Формы «распаковать» вместо того, что живая модель Firefox en→ru делает из
/// `unzip`/`unpack`/`extract` («расстегните», «извлеките», «вытащить»,
/// «разбейте», «распечатайте» и выдуманные «извайте», «изящите»).
///
/// ИСТОЧНИК ТЕРМИНА: KDE ru — Ark («Extract» → «Распаковать», «Extract here»
/// → «Распаковать в эту папку», «Extraction failed…» → «Не удалось
/// распаковать…»), kcm и Discover; наш po/ru.po («Couldn't unpack the
/// archive: %1» → «Не удалось распаковать архив: %1», «unpacking the theme» →
/// «распаковки темы»).
const UNPACK_FORMS: &[TermForm] = &[
    // извлечь / извлекать (перфектив → перфектив, имперфектив → имперфектив)
    TermForm::Word("извлеките", "распакуйте"),
    TermForm::Word("извлеки", "распакуй"),
    TermForm::Word("извлечь", "распаковать"),
    TermForm::Word("извлечет", "распакует"),
    TermForm::Word("извлекут", "распакуют"),
    TermForm::Word("извлекайте", "распаковывайте"),
    TermForm::Word("извлекай", "распаковывай"),
    TermForm::Word("извлекать", "распаковывать"),
    TermForm::Word("извлекает", "распаковывает"),
    TermForm::Word("извлекают", "распаковывают"),
    TermForm::Word("извлекая", "распаковывая"),
    TermForm::Word("извлекши", "распаковав"),
    TermForm::Word("извлечен", "распакован"),
    TermForm::Word("извлечена", "распакована"),
    TermForm::Word("извлечено", "распаковано"),
    TermForm::Word("извлечены", "распакованы"),
    TermForm::ParticipleStem("извлеченн", "распакованн"),
    TermForm::AfterPreposition("извлечения", "распаковки"),
    TermForm::AfterPreposition("извлечению", "распаковке"),
    TermForm::AfterPreposition("извлечении", "распаковке"),
    TermForm::AfterPreposition("извлечением", "распаковкой"),
    // расстегнуть (unzip)
    TermForm::Word("расстегните", "распакуйте"),
    TermForm::Word("расстегни", "распакуй"),
    TermForm::Word("расстегнуть", "распаковать"),
    TermForm::Word("расстегнув", "распаковав"),
    TermForm::Word("расстегивайте", "распаковывайте"),
    TermForm::Word("расстегивать", "распаковывать"),
    TermForm::ParticipleStem("расстегнут", "распакованн"),
    TermForm::AfterPreposition("расстегивания", "распаковки"),
    // вытащить, выкрутить (extract)
    TermForm::Word("вытащите", "распакуйте"),
    TermForm::Word("вытащи", "распакуй"),
    TermForm::Word("вытащить", "распаковать"),
    TermForm::Word("выкрутите", "распакуйте"),
    TermForm::Word("выкрутить", "распаковать"),
    // разбить (unpack), распечатать (unzip)
    TermForm::Word("разбейте", "распакуйте"),
    TermForm::Word("распечатайте", "распакуйте"),
    TermForm::Word("распечатать", "распаковать"),
    // Слова, которых в русском нет, — выдумки декодировщика, все повелительные.
    TermForm::Word("извайте", "распакуйте"),
    TermForm::Word("изящите", "распакуйте"),
    TermForm::Word("извахните", "распакуйте"),
    TermForm::Word("извлечайте", "распаковывайте"),
];

/// Формы «скачать» вместо «загрузить» и выдуманного «скашляйте».
///
/// ИСТОЧНИК ТЕРМИНА: наш po/ru.po — кнопка «Download» → «Скачать», «Couldn't
/// download the archive» → «Не удалось скачать архив», «📥 Download Only» →
/// «📥 Только скачать». (KDE в kcm пишет «Загрузить новые …» — у Rexon своё
/// слово, и описание темы говорит с человеком тем же словом, что кнопка.)
/// Существительные («загрузка», «загрузки») не трогаются: «скачивание» —
/// другого рода.
const DOWNLOAD_FORMS: &[TermForm] = &[
    TermForm::Imperative("загрузите", "скачайте"),
    TermForm::Word("загрузи", "скачай"),
    TermForm::Word("загрузить", "скачать"),
    TermForm::Word("загружайте", "скачивайте"),
    TermForm::Word("загружай", "скачивай"),
    TermForm::Word("загружать", "скачивать"),
    TermForm::Word("загружает", "скачивает"),
    TermForm::Word("загружают", "скачивают"),
    TermForm::Word("загружая", "скачивая"),
    TermForm::Word("загрузив", "скачав"),
    TermForm::Word("загрузил", "скачал"),
    TermForm::Word("загрузила", "скачала"),
    TermForm::Word("загрузило", "скачало"),
    TermForm::Word("загрузили", "скачали"),
    TermForm::Word("загрузит", "скачает"),
    TermForm::Word("загрузят", "скачают"),
    TermForm::Word("загружен", "скачан"),
    TermForm::Word("загружена", "скачана"),
    TermForm::Word("загружено", "скачано"),
    TermForm::Word("загружены", "скачаны"),
    TermForm::ParticipleStem("загруженн", "скачанн"),
    TermForm::ParticipleStem("загружаем", "скачиваем"),
    // Выдумки декодировщика («Download the archive and unpack it.» →
    // «скашляйте архив…»).
    TermForm::Word("скашляйте", "скачайте"),
    TermForm::Word("скашлять", "скачать"),
];

/// Словарь поправок терминов перевода описаний — ОДНО место.
///
/// ⭐ ЗАЧЕМ (дефект владельца 03.10, KDE Store): «Just unzip the file…» живая
/// модель Firefox en→ru переводит «Просто расстегните файл…», «Download
/// bigsur-sound-theme.tar.gz from Files» — «Скачать скачать … с Файлы»,
/// «extract» — «извлеките», «вытащить», «изящите».
///
/// ⚠️ ПОЧЕМУ ПОСЛЕ ДВИЖКА, А НЕ ДО. Подмена до движка проверена на той же
/// модели и ненадёжна: замена `unzip` на `unpack` в источнике даёт то
/// «распаковать его», то «Разбейте в /usr/share/icons/» (`Unpack into …`), а
/// заполнитель на месте глагола сам сбивает декодировщик (см. два прохода в
/// `translator_engine`). Правка после движка по паре «исходный термин →
/// неверная форма → верная в ТОЙ ЖЕ форме» не угадывает ни вида, ни падежа:
/// форму выбрал движок, словарь меняет только лексему.
///
/// ⛔ НЕ ИЗОБРЕТАТЬ ФОРМ. Каждая пара — одна грамматическая форма в другую;
/// где у замены другой род (существительные), правка идёт только сразу после
/// предлога ([`TermForm::AfterPreposition`]); где повелительное совпадает с
/// будущим — только без «вы» рядом ([`TermForm::Imperative`]). Форма, которой
/// здесь нет, остаётся как есть.
///
/// Добавить язык (`de`): новые группы с `tgt: "de"` — «entzippen» →
/// «entpacken» и т. п. — источник терминов тот же: KDE de (Ark: «Extract» →
/// «Entpacken») и наш po/de.po.
pub const TERM_FIXES: &[TermFix] = &[
    // unzip/unpack/untar/decompress — всегда «распаковать».
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["unzip", "unpack", "untar", "decompress", "uncompress", "unrar"],
        gate: always,
        forms: UNPACK_FORMS,
        collapse_repeats: false,
    },
    // extract — «распаковать» только про архив ([`extract_means_unpack`]).
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["extract"],
        gate: extract_means_unpack,
        forms: UNPACK_FORMS,
        collapse_repeats: false,
    },
    // download — «скачать» (кнопка Rexon «Скачать»).
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["download"],
        gate: download_alone,
        forms: DOWNLOAD_FORMS,
        collapse_repeats: true,
    },
    // «from Files» — вкладка KDE Store; движок оставляет «с Файлы»
    // (именительный после предлога).
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["files"],
        gate: files_tab,
        forms: &[
            TermForm::Phrase("с файлы", "со вкладки «Файлы»"),
            TermForm::Phrase("из файлы", "со вкладки «Файлы»"),
            TermForm::Phrase("со вкладки файлы", "со вкладки «Файлы»"),
        ],
        collapse_repeats: false,
    },
    // drag and drop — «перетащите» (KDE ru, kcm_kwindecoration: «Drag buttons
    // between here and the titlebar» → «Перетаскивайте кнопки…»). Движок
    // дописывает к нему «и высадите».
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["drag"],
        gate: drag_and_drop,
        forms: &[
            TermForm::Phrase("перетащите и высадите", "перетащите"),
            TermForm::Phrase("перетащите и бросьте", "перетащите"),
            TermForm::Phrase("перетащите и отпустите", "перетащите"),
            TermForm::Phrase("перетащить и высадить", "перетащить"),
            TermForm::Phrase("перетащить и бросить", "перетащить"),
            TermForm::Phrase("перетащить и отпустить", "перетащить"),
            TermForm::Phrase("перетаскивайте и бросайте", "перетаскивайте"),
        ],
        collapse_repeats: false,
    },
    // icon pack — «набор значков» (KDE ru, kcm_icons: «Installing icon
    // themes…» → «Установка наборов значков...»). «Пакет» и «набор» —
    // одного рода и склонения, падеж переносится как есть.
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["icon"],
        gate: |s| s.contains("icon pack") || s.contains("icons pack") || s.contains("iconpack"),
        forms: &[
            TermForm::Phrase("пакет значков", "набор значков"),
            TermForm::Phrase("пакета значков", "набора значков"),
            TermForm::Phrase("пакету значков", "набору значков"),
            TermForm::Phrase("пакетом значков", "набором значков"),
            TermForm::Phrase("пакете значков", "наборе значков"),
            TermForm::Phrase("пакеты значков", "наборы значков"),
            TermForm::Phrase("пакетов значков", "наборов значков"),
            TermForm::Phrase("пакет иконок", "набор значков"),
            TermForm::Phrase("пакета иконок", "набора значков"),
            TermForm::Phrase("пакеты иконок", "наборы значков"),
            TermForm::Phrase("пакетов иконок", "наборов значков"),
        ],
        collapse_repeats: false,
    },
    // Installation — «Установка» (KDE ru, Discover: «Install» → «Установка»,
    // kcm_icons «Installing…» → «Установка…»). Движок берёт «Инсталляция».
    // Обе — ж. р. первого склонения, но «инсталляции» неоднозначно по падежу,
    // поэтому только однозначные формы.
    TermFix {
        src: "en",
        tgt: "ru",
        source_words: &["install"],
        gate: always,
        forms: &[
            TermForm::Word("инсталляция", "установка"),
            TermForm::Word("инсталляцию", "установку"),
            TermForm::Word("инсталляцией", "установкой"),
        ],
        collapse_repeats: false,
    },
];

/// Слово в нижнем регистре и с `ё` → `е` — так словарь и сравнивает.
fn term_key(word: &str) -> String {
    word.to_lowercase().replace('ё', "е")
}

/// Замена с регистром первой буквы найденного (`Извлеките` → `Распакуйте`).
fn with_case_of(found: &str, replacement: &str) -> String {
    let upper = found.chars().next().is_some_and(char::is_uppercase);
    if !upper {
        return replacement.to_string();
    }
    let mut chars = replacement.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Слова строки: (начало, конец) в байтах, буквы подряд.
fn letter_runs(text: &str) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_alphabetic() {
            if start.is_none() {
                start = Some(i);
            }
        } else if let Some(s) = start.take() {
            runs.push((s, i));
        }
    }
    if let Some(s) = start {
        runs.push((s, text.len()));
    }
    runs
}

/// Между словами только пробелы — значит это слова одной фразы подряд.
fn only_spaces(gap: &str) -> bool {
    gap.chars().all(|c| c == ' ' || c == '\u{a0}')
}

/// Попробовать одну форму с `k`-го слова: сколько слов съедено и замена.
fn match_term_form(
    text: &str,
    runs: &[(usize, usize)],
    k: usize,
    form: &TermForm,
) -> Option<(usize, String)> {
    let word = &text[runs[k].0..runs[k].1];
    let key = term_key(word);
    let prev_words = |n: usize| -> Vec<String> {
        runs[..k].iter().rev().take(n).map(|(s, e)| term_key(&text[*s..*e])).collect()
    };
    match *form {
        TermForm::Word(wrong, right) => {
            (key == term_key(wrong)).then(|| (1, with_case_of(word, right)))
        }
        TermForm::Imperative(wrong, right) => (key == term_key(wrong)
            && !prev_words(3).iter().any(|w| w == "вы"))
        .then(|| (1, with_case_of(word, right))),
        TermForm::AfterPreposition(wrong, right) => {
            let after_prep = k > 0
                && only_spaces(&text[runs[k - 1].1..runs[k].0])
                && PREPOSITIONS.contains(&prev_words(1)[0].as_str());
            (key == term_key(wrong) && after_prep).then(|| (1, with_case_of(word, right)))
        }
        TermForm::ParticipleStem(wrong, right) => {
            let ending = key.strip_prefix(&term_key(wrong))?;
            PARTICIPLE_ENDINGS
                .contains(&ending)
                .then(|| (1, with_case_of(word, &format!("{right}{ending}"))))
        }
        TermForm::Phrase(wrong, right) => {
            let parts: Vec<String> = wrong.split(' ').map(term_key).collect();
            if k + parts.len() > runs.len() {
                return None;
            }
            for (j, part) in parts.iter().enumerate() {
                let (s, e) = runs[k + j];
                if term_key(&text[s..e]) != *part {
                    return None;
                }
                if j > 0 && !only_spaces(&text[runs[k + j - 1].1..s]) {
                    return None;
                }
            }
            Some((parts.len(), with_case_of(word, right)))
        }
    }
}

/// Верные формы группы — то, что сворачивается при слипшемся повторе.
fn right_forms(fix: &TermFix) -> Vec<String> {
    fix.forms
        .iter()
        .filter_map(|f| match *f {
            TermForm::Word(_, r) | TermForm::Imperative(_, r) => Some(term_key(r)),
            _ => None,
        })
        .collect()
}

/// Применить одну группу к переводу.
fn apply_term_fix(fix: &TermFix, translated: &str) -> String {
    let runs = letter_runs(translated);
    let mut out = String::with_capacity(translated.len());
    let mut copied = 0usize;
    let mut k = 0usize;
    while k < runs.len() {
        let found = fix.forms.iter().find_map(|f| match_term_form(translated, &runs, k, f));
        match found {
            Some((taken, replacement)) => {
                out.push_str(&translated[copied..runs[k].0]);
                out.push_str(&replacement);
                copied = runs[k + taken - 1].1;
                k += taken;
            }
            None => k += 1,
        }
    }
    out.push_str(&translated[copied..]);
    if !fix.collapse_repeats {
        return out;
    }
    // Слипшийся повтор верной формы: «скачать скачать» → «скачать».
    let rights = right_forms(fix);
    let runs = letter_runs(&out);
    let mut collapsed = String::with_capacity(out.len());
    let mut copied = 0usize;
    for pair in runs.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let first = term_key(&out[a.0..a.1]);
        if b.0 >= copied
            && only_spaces(&out[a.1..b.0])
            && first == term_key(&out[b.0..b.1])
            && rights.contains(&first)
        {
            collapsed.push_str(&out[copied..a.1]);
            copied = b.1;
        }
    }
    collapsed.push_str(&out[copied..]);
    collapsed
}

/// Поправить термины перевода по словарю [`TERM_FIXES`].
///
/// `source` — исходное предложение (английское), `translated` — ответ
/// движка для него. Группа срабатывает, только если её английский термин есть
/// в `source` и проходит её условие; иначе перевод не трогается. Пара языков
/// без групп (пока это всё, кроме `en` → `ru`) возвращается как есть.
pub fn apply_term_fixes(src: &str, tgt: &str, source: &str, translated: &str) -> String {
    let lower = source.to_lowercase();
    let words: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).collect();
    let mut out = translated.to_string();
    for fix in TERM_FIXES.iter().filter(|f| f.src == src && f.tgt == tgt) {
        let has_term =
            fix.source_words.iter().any(|t| words.iter().any(|w| w.starts_with(t)));
        if has_term && (fix.gate)(&lower) {
            out = apply_term_fix(fix, &out);
        }
    }
    out
}

/// Тег кодового окружения (`code`, `pre`, `kbd`, `samp`, `tt`), без учёта
/// регистра.
pub fn is_code_env_tag(name: &str) -> bool {
    let lower = name.to_lowercase();
    CODE_ENV_TAGS.contains(&lower.as_str())
}

/// Разобрать литерал как тег: имя, закрывающий ли, самозакрывающийся ли.
/// Литералом [`segment_html`] отдаёт не только теги (ещё сущности, комментарии
/// и пробельные узлы) — для них возвращается `None`.
fn parse_literal_tag(lit: &str) -> Option<(String, bool, bool)> {
    let body = lit.strip_prefix('<')?;
    let (closing, rest) = match body.strip_prefix('/') {
        Some(r) => (true, r),
        None => (false, body),
    };
    if !rest.chars().next()?.is_ascii_alphabetic() {
        return None;
    }
    let name: String =
        rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    let self_closing = lit.trim_end().trim_end_matches('>').trim_end().ends_with('/');
    Some((name, closing, self_closing))
}

/// Стек кодового окружения: скармливаем ему литералы по порядку и спрашиваем,
/// внутри кода мы или нет. Внутри `code`/`pre`/`kbd`/`samp`/`tt` не переводится
/// ничего.
#[derive(Debug, Clone, Default)]
pub struct CodeEnvStack {
    stack: Vec<String>,
}

impl CodeEnvStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Учесть литерал (тег). Не-теги игнорируются.
    pub fn feed_literal(&mut self, literal: &str) {
        let Some((name, closing, self_closing)) = parse_literal_tag(literal) else {
            return;
        };
        if !is_code_env_tag(&name) {
            return;
        }
        if closing {
            if let Some(pos) = self.stack.iter().rposition(|n| *n == name) {
                self.stack.truncate(pos);
            }
        } else if !self_closing {
            self.stack.push(name);
        }
    }

    /// Мы внутри кодового окружения?
    pub fn in_code(&self) -> bool {
        !self.stack.is_empty()
    }
}

/// Для каждого сегмента — звать ли движок. `Literal` всегда `false`; `Text` —
/// только если он не внутри кодового окружения и признан прозой ЦЕЛИКОМ.
pub fn translatable_flags(segments: &[Segment]) -> Vec<bool> {
    flags_with(segments, &|text| is_prose_segment(text))
}

/// То же, но проза решается ещё и ПО ПРЕДЛОЖЕНИЯМ: сегмент годится, если проза
/// он сам ЛИБО хоть одно его предложение.
///
/// ⭐ ЗАЧЕМ. Приёмка 24.09, комментарий maxkaspar к «Awesome Widgets»: «hddtemp
/// shows only the first digit. here is a screenshot http://…». Первая фраза —
/// чистая проза, вторая содержит ссылку, и правило 4 [`is_prose_segment`]
/// («хоть одно значимое слово технично по форме») отвергало ВЕСЬ узел — человек
/// видел английский комментарий без всякого объяснения. Правило само верное:
/// ломать ссылку нельзя. Неверна была ЕДИНИЦА решения — целый текстовый узел,
/// хотя движку текст всё равно уезжает предложениями
/// ([`crate::translator_engine`]). Теперь узел пропускается, когда в нём есть
/// проза, а судьбу каждого предложения решает то же самое правило: ссылки,
/// пути и команды движку по-прежнему не достаются и остаются в выводе дословно.
/// (С 03.10 правило 4 уже не отвергает фразу за ссылку внутри — ссылка уходит
/// под заполнитель, см. [`technical_spans`]; единица решения — по-прежнему
/// предложение.)
///
/// Границы предложений даёт ВЫЗЫВАЮЩИЙ — тем же разбивателем, каким потом
/// кормится движок. Своего второго разбивателя здесь не заводится: две правды о
/// границах предложений разошлись бы молча.
pub fn translatable_flags_by_sentence(
    segments: &[Segment],
    sentences: &dyn Fn(&str) -> Vec<String>,
) -> Vec<bool> {
    flags_with(segments, &|text| {
        is_prose_segment(text) || sentences(text).iter().any(|s| is_prose_segment(s))
    })
}

// Общая часть обоих наборов признаков: кодовое окружение плюс заданное правило
// «проза ли».
fn flags_with(segments: &[Segment], is_prose: &dyn Fn(&str) -> bool) -> Vec<bool> {
    let mut env = CodeEnvStack::new();
    let mut flags = Vec::with_capacity(segments.len());
    for s in segments {
        match s {
            Segment::Literal { text } => {
                env.feed_literal(text);
                flags.push(false);
            }
            Segment::Text { stripped, .. } => {
                flags.push(!env.in_code() && is_prose(stripped));
            }
        }
    }
    flags
}

/// [`render_segments`] с фильтром: движок зовётся только на сегментах-прозе,
/// остальные текстовые узлы остаются оригиналом (колбэку отдаётся пустая
/// строка — ровно то, что [`render_segments`] понимает как «оставить как было»,
/// поэтому сам реассемблинг переписывать не пришлось).
pub fn render_segments_filtered(
    segments: &[Segment],
    translate: impl Fn(&str) -> String,
) -> String {
    render_segments_with_flags(segments, &translatable_flags(segments), translate)
}

/// То же, но признаки СЧИТАНЫ ЗАРАНЕЕ: сборщик обязан пользоваться ровно теми
/// признаками, по которым отбирал текст для движка
/// ([`translatable_flags_by_sentence`]), иначе переведённый узел остался бы в
/// выводе оригиналом.
pub fn render_segments_with_flags(
    segments: &[Segment],
    flags: &[bool],
    translate: impl Fn(&str) -> String,
) -> String {
    let next = std::cell::Cell::new(0usize);
    render_segments(segments, |s| {
        // render_segments зовёт колбэк ровно по одному разу на Segment::Text
        // и строго по порядку — индекс текстового узла восстанавливается счётчиком.
        let idx = next.get();
        next.set(idx + 1);
        let allow = segments
            .iter()
            .enumerate()
            .filter(|(_, seg)| matches!(seg, Segment::Text { .. }))
            .nth(idx)
            .map(|(i, _)| flags[i])
            .unwrap_or(false);
        if allow {
            translate(s)
        } else {
            String::new()
        }
    })
}

// Пробельные символы `[\t\n\r\f ]` из регэкспов html.parser (ASCII-набор).
fn is_html_ws(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | '\u{0c}' | ' ')
}

/// Порт `html.parser.HTMLParser` при `convert_charrefs=False`, суженный до
/// обработчиков, которые переопределяет `_HTMLTextTranslator`.
struct Segmenter {
    data: Vec<char>,
    n: usize,
    i: usize,
    out: Vec<Segment>,
    // RAWTEXT/RCDATA: если внутри script/style/… — имя элемента и «escapable».
    cdata_elem: Option<String>,
    escapable: bool,
    support_cdata: bool,
}

const CDATA_CONTENT_ELEMENTS: &[&str] = &["script", "style", "xmp", "iframe", "noembed", "noframes"];
const RCDATA_CONTENT_ELEMENTS: &[&str] = &["textarea", "title"];

impl Segmenter {
    fn new(text: &str) -> Self {
        let data: Vec<char> = text.chars().collect();
        let n = data.len();
        Segmenter { data, n, i: 0, out: Vec::new(), cdata_elem: None, escapable: true, support_cdata: true }
    }

    fn slice(&self, a: usize, b: usize) -> String {
        self.data[a..b].iter().collect()
    }

    fn starts_with(&self, pat: &str, at: usize) -> bool {
        let pc: Vec<char> = pat.chars().collect();
        if at + pc.len() > self.n {
            return false;
        }
        self.data[at..at + pc.len()] == pc[..]
    }

    // rawdata[i:i+len].lower() == pat (pat нижним регистром).
    fn starts_with_ci(&self, pat: &str, at: usize) -> bool {
        let pc: Vec<char> = pat.chars().collect();
        if at + pc.len() > self.n {
            return false;
        }
        self.data[at..at + pc.len()]
            .iter()
            .zip(&pc)
            .all(|(a, b)| a.to_ascii_lowercase() == *b)
    }

    // ── обработчики (соответствуют переопределениям _HTMLTextTranslator) ────
    fn handle_data(&mut self, s: &str) {
        let chars: Vec<char> = s.chars().collect();
        // stripped = data.strip(); if not stripped: return data (Literal).
        let first = chars.iter().position(|c| !c.is_whitespace());
        match first {
            None => self.out.push(Segment::Literal { text: s.to_string() }),
            Some(f) => {
                let last = chars.iter().rposition(|c| !c.is_whitespace()).unwrap();
                let lead: String = chars[..f].iter().collect();
                let stripped: String = chars[f..=last].iter().collect();
                let trail: String = chars[last + 1..].iter().collect();
                self.out.push(Segment::Text { lead, stripped, trail });
            }
        }
    }

    fn handle_starttag_text(&mut self, start: usize, end: usize) {
        // get_starttag_text() — дословный текст тега (для starttag и startendtag).
        self.out.push(Segment::Literal { text: self.slice(start, end) });
    }

    fn handle_endtag(&mut self, tag: &str) {
        self.out.push(Segment::Literal { text: format!("</{tag}>") });
    }

    fn handle_entityref(&mut self, name: &str) {
        self.out.push(Segment::Literal { text: format!("&{name};") });
    }

    fn handle_charref(&mut self, name: &str) {
        self.out.push(Segment::Literal { text: format!("&#{name};") });
    }

    fn handle_comment(&mut self, data: &str) {
        self.out.push(Segment::Literal { text: format!("<!--{data}-->") });
    }
    // handle_decl / handle_pi / unknown_decl — подкласс НЕ переопределяет →
    // узлы <!DOCTYPE>/<?..?>/<![CDATA[..]]> выпадают (ничего не добавляем).

    fn set_cdata_mode(&mut self, elem: &str, escapable: bool) {
        self.cdata_elem = Some(elem.to_ascii_lowercase());
        self.escapable = escapable;
    }

    fn clear_cdata_mode(&mut self) {
        self.cdata_elem = None;
        self.escapable = true;
    }

    // Найти следующий «интересный» символ от `from` (аналог interesting.search).
    fn interesting_search(&self, from: usize) -> Option<usize> {
        match &self.cdata_elem {
            None => (from..self.n).find(|&p| matches!(self.data[p], '&' | '<')),
            Some(elem) => {
                if elem == "plaintext" {
                    return None; // interesting = \z
                }
                (from..self.n).find(|&p| {
                    (self.escapable && self.data[p] == '&') || self.matches_close_tag(p, elem)
                })
            }
        }
    }

    // </elem(?=[\t\n\r\f />]) без учёта регистра.
    fn matches_close_tag(&self, p: usize, elem: &str) -> bool {
        if self.data[p] != '<' || p + 1 >= self.n || self.data[p + 1] != '/' {
            return false;
        }
        let ec: Vec<char> = elem.chars().collect();
        let start = p + 2;
        if start + ec.len() > self.n {
            return false;
        }
        for (k, e) in ec.iter().enumerate() {
            if self.data[start + k].to_ascii_lowercase() != *e {
                return false;
            }
        }
        let after = start + ec.len();
        after < self.n && (is_html_ws(self.data[after]) || matches!(self.data[after], '/' | '>'))
    }

    fn goahead(&mut self, end: bool) {
        while self.i < self.n {
            let i = self.i;
            let j = match self.interesting_search(i) {
                Some(m) => m,
                None => {
                    if self.cdata_elem.is_some() {
                        break;
                    }
                    self.n
                }
            };
            if i < j {
                let d = self.slice(i, j);
                self.handle_data(&d);
            }
            self.i = j;
            if self.i == self.n {
                break;
            }
            let i = self.i;
            if self.data[i] == '<' {
                let mut k: isize;
                if i + 1 < self.n && self.data[i + 1].is_ascii_alphabetic() {
                    k = self.parse_starttag(i);
                } else if self.starts_with("</", i) {
                    k = self.parse_endtag(i);
                } else if self.starts_with("<!--", i) {
                    k = self.parse_comment(i);
                } else if self.starts_with("<?", i) {
                    k = self.parse_pi(i);
                } else if self.starts_with("<!", i) {
                    k = self.parse_html_declaration(i);
                } else if i + 1 < self.n || end {
                    self.handle_data("<");
                    k = (i + 1) as isize;
                } else {
                    break;
                }
                if k < 0 {
                    if !end {
                        break;
                    }
                    self.recover_incomplete(i);
                    k = self.n as isize;
                }
                self.i = k as usize;
            } else if self.starts_with("&#", i) {
                if let Some(mend) = self.charref_match(i) {
                    let name = self.slice(i + 2, mend - 1);
                    self.handle_charref(&name);
                    let k = if self.data[mend - 1] != ';' { mend - 1 } else { mend };
                    self.i = k;
                    continue;
                }
                if self.incomplete_charref_match(i) {
                    if end {
                        let name = self.slice(i + 2, self.n);
                        self.handle_charref(&name);
                        self.i = self.n;
                        break;
                    }
                    break;
                } else if i + 3 < self.n {
                    self.handle_data("&#");
                    self.i = i + 2;
                } else {
                    break;
                }
            } else if self.data[i] == '&' {
                if let Some((g1_end, mend)) = self.entityref_match(i) {
                    let name = self.slice(i + 1, g1_end);
                    self.handle_entityref(&name);
                    let k = if self.data[mend - 1] != ';' { mend - 1 } else { mend };
                    self.i = k;
                    continue;
                }
                if self.incomplete_amp_match(i) {
                    if end {
                        let name = self.slice(i + 1, self.n);
                        self.handle_entityref(&name);
                        self.i = self.n;
                        break;
                    }
                    break;
                } else if i + 1 < self.n {
                    self.handle_data("&");
                    self.i = i + 1;
                } else {
                    break;
                }
            } else {
                // interesting.search() «соврал» — недостижимо.
                break;
            }
        }
        if end && self.i < self.n {
            let d = self.slice(self.i, self.n);
            self.handle_data(&d);
            self.i = self.n;
        }
    }

    // Восстановление при неполном «<…» на истинном EOF (goahead end=true).
    fn recover_incomplete(&mut self, i: usize) {
        let n = self.n;
        if i + 1 < n && self.data[i + 1].is_ascii_alphabetic() {
            // starttagopen — pass.
        } else if self.starts_with("</", i) {
            if i + 2 == n {
                self.handle_data("</");
            } else if i + 2 < n && self.data[i + 2].is_ascii_alphabetic() {
                // endtagopen — pass.
            } else {
                let d = self.slice(i + 2, n);
                self.handle_comment(&d);
            }
        } else if self.starts_with("<!--", i) {
            let mut j = n;
            for suffix in ["--!", "--", "-"] {
                let sc: Vec<char> = suffix.chars().collect();
                if n >= i + 4 + sc.len() && self.data[n - sc.len()..n] == sc[..] {
                    j -= sc.len();
                    break;
                }
            }
            let d = self.slice(i + 4, j);
            self.handle_comment(&d);
        } else if self.starts_with("<![CDATA[", i) && self.support_cdata {
            // unknown_decl — pass.
        } else if self.starts_with_ci("<!doctype", i) {
            // handle_decl — pass.
        } else if self.starts_with("<!", i) {
            let d = self.slice(i + 2, n);
            self.handle_comment(&d);
        } else if self.starts_with("<?", i) {
            // handle_pi — pass.
        }
    }

    // charref = &#(?:[0-9]+|[xX][0-9a-fA-F]+)[^0-9a-fA-F]. Возвращает индекс
    // ПОСЛЕ завершающего символа (match.end()) либо None.
    fn charref_match(&self, i: usize) -> Option<usize> {
        if !self.starts_with("&#", i) {
            return None;
        }
        let mut p = i + 2;
        let hex = p < self.n && matches!(self.data[p], 'x' | 'X');
        if hex {
            p += 1;
            let start = p;
            while p < self.n && self.data[p].is_ascii_hexdigit() {
                p += 1;
            }
            if p == start {
                return None; // нужен хотя бы один hex
            }
        } else {
            let start = p;
            while p < self.n && self.data[p].is_ascii_digit() {
                p += 1;
            }
            if p == start {
                return None; // нужна хотя бы одна цифра
            }
        }
        // завершающий [^0-9a-fA-F]
        if p < self.n && !self.data[p].is_ascii_hexdigit() {
            Some(p + 1)
        } else {
            None
        }
    }

    // incomplete_charref = &#(?:[0-9]|[xX][0-9a-fA-F]).
    fn incomplete_charref_match(&self, i: usize) -> bool {
        if !self.starts_with("&#", i) {
            return false;
        }
        let p = i + 2;
        if p < self.n && self.data[p].is_ascii_digit() {
            return true;
        }
        if p < self.n && matches!(self.data[p], 'x' | 'X') {
            return p + 1 < self.n && self.data[p + 1].is_ascii_hexdigit();
        }
        false
    }

    // entityref = &([a-zA-Z][-.a-zA-Z0-9]*)[^a-zA-Z0-9]. Возвращает
    // (конец group1, match.end()) либо None.
    fn entityref_match(&self, i: usize) -> Option<(usize, usize)> {
        if self.data.get(i) != Some(&'&') {
            return None;
        }
        let mut p = i + 1;
        if p >= self.n || !self.data[p].is_ascii_alphabetic() {
            return None;
        }
        p += 1;
        while p < self.n && (self.data[p].is_ascii_alphanumeric() || matches!(self.data[p], '-' | '.')) {
            p += 1;
        }
        let g1_end = p;
        // завершающий [^a-zA-Z0-9]
        if p < self.n && !self.data[p].is_ascii_alphanumeric() {
            Some((g1_end, p + 1))
        } else {
            None
        }
    }

    // incomplete = &[a-zA-Z#].
    fn incomplete_amp_match(&self, i: usize) -> bool {
        self.data.get(i) == Some(&'&')
            && i + 1 < self.n
            && (self.data[i + 1].is_ascii_alphabetic() || self.data[i + 1] == '#')
    }

    // ── разбор тегов ────────────────────────────────────────────────────────
    fn parse_starttag(&mut self, i: usize) -> isize {
        let endpos = self.locatetagend(i + 1);
        if self.data[endpos - 1] != '>' {
            return -1;
        }
        // tagfind_tolerant: имя тега + завершающие ws/'/'(не перед '>').
        let (tag, mut k) = self.tagfind(i + 1);
        while k < endpos {
            match self.attrfind(k) {
                Some(mend) => k = mend,
                None => break,
            }
        }
        // end = rawdata[k:endpos].strip()
        let end_str: String = self.slice(k, endpos);
        let end_str = end_str.trim();
        if end_str != ">" && end_str != "/>" {
            let d = self.slice(i, endpos);
            self.handle_data(&d);
            return endpos as isize;
        }
        if end_str == "/>" {
            self.handle_starttag_text(i, endpos); // handle_startendtag → тот же verbatim
        } else {
            self.handle_starttag_text(i, endpos);
            if CDATA_CONTENT_ELEMENTS.contains(&tag.as_str()) || tag == "plaintext" {
                self.set_cdata_mode(&tag, false);
            } else if RCDATA_CONTENT_ELEMENTS.contains(&tag.as_str()) {
                self.set_cdata_mode(&tag, true);
            }
        }
        endpos as isize
    }

    fn parse_endtag(&mut self, i: usize) -> isize {
        // fast check: есть ли '>' после i+2
        if (i + 2..self.n).all(|p| self.data[p] != '>') {
            return -1;
        }
        // endtagopen = </ + letter
        if !(i + 2 < self.n && self.data[i + 2].is_ascii_alphabetic()) {
            if i + 2 < self.n && self.data[i + 2] == '>' {
                return (i + 3) as isize; // </> игнорируется
            }
            return self.parse_bogus_comment(i);
        }
        let endpos = self.locatetagend(i + 2);
        if self.data[endpos - 1] != '>' {
            return -1;
        }
        let (tag, _) = self.tagfind(i + 2);
        self.handle_endtag(&tag);
        self.clear_cdata_mode();
        endpos as isize
    }

    fn parse_bogus_comment(&mut self, i: usize) -> isize {
        match (i + 2..self.n).find(|&p| self.data[p] == '>') {
            None => -1,
            Some(pos) => {
                let d = self.slice(i + 2, pos);
                self.handle_comment(&d);
                (pos + 1) as isize
            }
        }
    }

    fn parse_comment(&mut self, i: usize) -> isize {
        // commentclose = --!?>  (search от i+4); иначе commentabruptclose = -?> (match).
        let mut found: Option<(usize, usize)> = None;
        let mut p = i + 4;
        while p < self.n {
            if self.data[p] == '-' && p + 1 < self.n && self.data[p + 1] == '-' {
                if p + 2 < self.n && self.data[p + 2] == '>' {
                    found = Some((p, p + 3));
                    break;
                }
                if p + 3 < self.n && self.data[p + 2] == '!' && self.data[p + 3] == '>' {
                    found = Some((p, p + 4));
                    break;
                }
            }
            p += 1;
        }
        if found.is_none() {
            // commentabruptclose.match at i+4: -?>
            let q = i + 4;
            if q < self.n && self.data[q] == '>' {
                found = Some((q, q + 1));
            } else if q + 1 < self.n && self.data[q] == '-' && self.data[q + 1] == '>' {
                found = Some((q, q + 2));
            }
        }
        match found {
            None => -1,
            Some((start, endp)) => {
                let d = self.slice(i + 4, start);
                self.handle_comment(&d);
                endp as isize
            }
        }
    }

    fn parse_pi(&mut self, i: usize) -> isize {
        match (i + 2..self.n).find(|&p| self.data[p] == '>') {
            None => -1,
            // handle_pi — не переопределён, ничего не добавляем; return end.
            Some(pos) => (pos + 1) as isize,
        }
    }

    fn parse_html_declaration(&mut self, i: usize) -> isize {
        if self.starts_with("<!--", i) {
            return self.parse_comment(i);
        }
        if self.starts_with("<![CDATA[", i) && self.support_cdata {
            // find ]]> от i+9
            let target = [']', ']', '>'];
            let mut p = i + 9;
            while p + 3 <= self.n {
                if self.data[p..p + 3] == target {
                    // unknown_decl — не переопределён, ничего не добавляем.
                    return (p + 3) as isize;
                }
                p += 1;
            }
            return -1;
        }
        if self.starts_with_ci("<!doctype", i) {
            match (i + 9..self.n).find(|&p| self.data[p] == '>') {
                None => -1,
                // handle_decl — не переопределён, ничего не добавляем.
                Some(gtpos) => (gtpos + 1) as isize,
            }
        } else {
            self.parse_bogus_comment(i)
        }
    }

    // ── регэксп-примитивы разбора тега ─────────────────────────────────────
    // tagfind_tolerant = ([a-zA-Z][^\t\n\r\f />]*)(?:[\t\n\r\f ]|/(?!>))*
    // Возвращает (имя в НИЖНЕМ регистре, конец матча).
    fn tagfind(&self, from: usize) -> (String, usize) {
        let mut p = from;
        // [a-zA-Z] — гарантирован вызывающим; далее [^\t\n\r\f />]*.
        let name_start = p;
        p += 1;
        while p < self.n && !is_html_ws(self.data[p]) && !matches!(self.data[p], '/' | '>') {
            p += 1;
        }
        let name: String = self.data[name_start..p].iter().collect::<String>().to_lowercase();
        p = self.skip_tag_trailing(p);
        (name, p)
    }

    // (?:[\t\n\r\f ]|/(?!>))* — пробел ИЛИ '/' не перед '>'.
    fn skip_tag_trailing(&self, mut p: usize) -> usize {
        while p < self.n {
            let c = self.data[p];
            // пробел ИЛИ '/' не непосредственно перед '>'.
            if is_html_ws(c) || (c == '/' && !(p + 1 < self.n && self.data[p + 1] == '>')) {
                p += 1;
            } else {
                break;
            }
        }
        p
    }

    // attrfind_tolerant.match(k) → конец матча (одно имя-атрибут[=значение]) или None.
    fn attrfind(&self, k: usize) -> Option<usize> {
        // (?<=['"\t\n\r\f /]) — символ перед именем атрибута.
        if k == 0 {
            return None;
        }
        let prev = self.data[k - 1];
        if !(prev == '\'' || prev == '"' || is_html_ws(prev) || prev == '/') {
            return None;
        }
        // имя: [^\t\n\r\f />] затем [^\t\n\r\f /=>]*
        let mut p = k;
        if p >= self.n || is_html_ws(self.data[p]) || matches!(self.data[p], '/' | '>') {
            return None;
        }
        p += 1;
        while p < self.n && !is_html_ws(self.data[p]) && !matches!(self.data[p], '/' | '=' | '>') {
            p += 1;
        }
        // необязательный индикатор значения: [ws]*=[ws]*(value)
        let save = p;
        let mut q = p;
        while q < self.n && is_html_ws(self.data[q]) {
            q += 1;
        }
        if q < self.n && self.data[q] == '=' {
            q += 1;
            while q < self.n && is_html_ws(self.data[q]) {
                q += 1;
            }
            match self.match_attr_value(q) {
                Some(vend) => p = vend,
                None => p = save, // значение не собралось → индикатор не матчится
            }
        } else {
            p = save;
        }
        // завершающие (?:[\t\n\r\f ]|/(?!>))*
        p = self.skip_tag_trailing(p);
        Some(p)
    }

    // '[^']*' | "[^"]*" | (?!['"])[^>\t\n\r\f ]*
    fn match_attr_value(&self, q: usize) -> Option<usize> {
        if q >= self.n {
            // (?!['"])[^…]* при пустом хвосте матчит пусто.
            return Some(q);
        }
        match self.data[q] {
            '\'' => {
                let mut p = q + 1;
                while p < self.n && self.data[p] != '\'' {
                    p += 1;
                }
                if p < self.n {
                    Some(p + 1)
                } else {
                    None // нет закрывающей кавычки — альтернатива не матчит
                }
            }
            '"' => {
                let mut p = q + 1;
                while p < self.n && self.data[p] != '"' {
                    p += 1;
                }
                if p < self.n {
                    Some(p + 1)
                } else {
                    None
                }
            }
            _ => {
                // (?!['"]) уже проверено (не кавычка); [^>\t\n\r\f ]*
                let mut p = q;
                while p < self.n && !is_html_ws(self.data[p]) && self.data[p] != '>' {
                    p += 1;
                }
                Some(p)
            }
        }
    }

    // locatetagend.match(from) → конец матча (жадный, всегда матчит).
    fn locatetagend(&self, from: usize) -> usize {
        let mut p = from;
        // [a-zA-Z]
        if p < self.n && self.data[p].is_ascii_alphabetic() {
            p += 1;
        }
        // [^\t\n\r\f />]*
        while p < self.n && !is_html_ws(self.data[p]) && !matches!(self.data[p], '/' | '>') {
            p += 1;
        }
        // [\t\n\r\f /]*
        while p < self.n && (is_html_ws(self.data[p]) || self.data[p] == '/') {
            p += 1;
        }
        // (?: (?<=['"\t\n\r\f /]) attrname (=value)? [\t\n\r\f /]* )*
        loop {
            if p == 0 {
                break;
            }
            let prev = self.data[p - 1];
            if !(prev == '\'' || prev == '"' || is_html_ws(prev) || prev == '/') {
                break;
            }
            if p >= self.n || is_html_ws(self.data[p]) || matches!(self.data[p], '/' | '>') {
                break;
            }
            // attrname
            p += 1;
            while p < self.n && !is_html_ws(self.data[p]) && !matches!(self.data[p], '/' | '=' | '>') {
                p += 1;
            }
            // (=value)?
            let save = p;
            let mut q = p;
            while q < self.n && is_html_ws(self.data[q]) {
                q += 1;
            }
            if q < self.n && self.data[q] == '=' {
                q += 1;
                while q < self.n && is_html_ws(self.data[q]) {
                    q += 1;
                }
                match self.match_attr_value(q) {
                    Some(vend) => p = vend,
                    None => p = save,
                }
            } else {
                p = save;
            }
            // [\t\n\r\f /]*
            while p < self.n && (is_html_ws(self.data[p]) || self.data[p] == '/') {
                p += 1;
            }
        }
        // >?
        if p < self.n && self.data[p] == '>' {
            p += 1;
        }
        p
    }
}

// ── html.unescape (для probe_text) ─────────────────────────────────────────
//
// Числовые ссылки — полностью; именованные — распространённый набор. Так как
// _HTML_TAG_RE вырезает корректные `&…;` ДО unescape, влияние ограничено.
///
/// Публичная: вкладка «Оформление» разбирает этим же текст комментариев
/// магазина. Второго разборщика сущностей в ядре заводить нельзя (правило
/// #12) — разойдись они, и один и тот же комментарий читался бы по-разному
/// в детали товара и в детекторе языка.
pub fn html_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '&' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // &(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[a-zA-Z][a-zA-Z0-9]*;?)
        if i + 1 < chars.len() && chars[i + 1] == '#' {
            let mut p = i + 2;
            let hex = p < chars.len() && matches!(chars[p], 'x' | 'X');
            if hex {
                p += 1;
            }
            let start = p;
            while p < chars.len()
                && ((hex && chars[p].is_ascii_hexdigit()) || (!hex && chars[p].is_ascii_digit()))
            {
                p += 1;
            }
            if p > start {
                let num: String = chars[start..p].iter().collect();
                let cp = if hex {
                    u32::from_str_radix(&num, 16).ok()
                } else {
                    num.parse::<u32>().ok()
                };
                if p < chars.len() && chars[p] == ';' {
                    p += 1;
                }
                match cp.and_then(char_from_html_codepoint) {
                    Some(c) => out.push(c),
                    None => out.push('\u{fffd}'),
                }
                i = p;
                continue;
            }
        } else if i + 1 < chars.len() && chars[i + 1].is_ascii_alphabetic() {
            let mut p = i + 1;
            while p < chars.len() && chars[p].is_ascii_alphanumeric() {
                p += 1;
            }
            let mut name: String = chars[i + 1..p].iter().collect();
            let mut consumed = p;
            if p < chars.len() && chars[p] == ';' {
                name.push(';');
                consumed = p + 1;
            }
            if let Some(rep) = named_entity(&name) {
                out.push_str(rep);
                i = consumed;
                continue;
            }
            // без ';' попробуем и вариант с ';'
            if !name.ends_with(';') {
                if let Some(rep) = named_entity(&format!("{name};")) {
                    out.push_str(rep);
                    i = consumed;
                    continue;
                }
            }
        }
        out.push('&');
        i += 1;
    }
    out
}

/// Порт `html.escape(text)` (quote=True): `& < > " '` → сущности.
///
/// ⚠️ Апостроф Python отдаёт как `&#x27;`, а не `&#39;` — вид записи здесь
/// значим, потому что вывод сверяется с оригиналом побайтно.
pub fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => out.push(ch),
        }
    }
    out
}

// Спецправила HTML для C1 и запрещённых кодовых точек (подмножество html5).
fn char_from_html_codepoint(cp: u32) -> Option<char> {
    // Windows-1252 карта для 0x80..0x9F, как в html._invalid_charrefs.
    const C1: [(u32, char); 27] = [
        (0x80, '\u{20ac}'), (0x82, '\u{201a}'), (0x83, '\u{0192}'), (0x84, '\u{201e}'),
        (0x85, '\u{2026}'), (0x86, '\u{2020}'), (0x87, '\u{2021}'), (0x88, '\u{02c6}'),
        (0x89, '\u{2030}'), (0x8a, '\u{0160}'), (0x8b, '\u{2039}'), (0x8c, '\u{0152}'),
        (0x8e, '\u{017d}'), (0x91, '\u{2018}'), (0x92, '\u{2019}'), (0x93, '\u{201c}'),
        (0x94, '\u{201d}'), (0x95, '\u{2022}'), (0x96, '\u{2013}'), (0x97, '\u{2014}'),
        (0x98, '\u{02dc}'), (0x99, '\u{2122}'), (0x9a, '\u{0161}'), (0x9b, '\u{203a}'),
        (0x9c, '\u{0153}'), (0x9e, '\u{017e}'), (0x9f, '\u{0178}'),
    ];
    if cp == 0 {
        return Some('\u{fffd}');
    }
    if let Some(&(_, c)) = C1.iter().find(|&&(k, _)| k == cp) {
        return Some(c);
    }
    char::from_u32(cp)
}

// Распространённые именованные сущности (ключ включает ';', плюс несколько
// «legacy» без ';', как в html5-словаре CPython).
fn named_entity(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp;" | "amp" => "&",
        "lt;" | "lt" => "<",
        "gt;" | "gt" => ">",
        "quot;" | "quot" => "\"",
        "apos;" => "'",
        "nbsp;" | "nbsp" => "\u{a0}",
        "copy;" | "copy" => "\u{a9}",
        "reg;" | "reg" => "\u{ae}",
        "trade;" => "\u{2122}",
        "deg;" | "deg" => "\u{b0}",
        "plusmn;" | "plusmn" => "\u{b1}",
        "middot;" | "middot" => "\u{b7}",
        "bull;" => "\u{2022}",
        "hellip;" => "\u{2026}",
        "mdash;" => "\u{2014}",
        "ndash;" => "\u{2013}",
        "lsquo;" => "\u{2018}",
        "rsquo;" => "\u{2019}",
        "ldquo;" => "\u{201c}",
        "rdquo;" => "\u{201d}",
        "laquo;" | "laquo" => "\u{ab}",
        "raquo;" | "raquo" => "\u{bb}",
        "times;" | "times" => "\u{d7}",
        "divide;" | "divide" => "\u{f7}",
        "euro;" => "\u{20ac}",
        "pound;" | "pound" => "\u{a3}",
        "cent;" | "cent" => "\u{a2}",
        "sect;" | "sect" => "\u{a7}",
        "para;" | "para" => "\u{b6}",
        "eacute;" | "eacute" => "\u{e9}",
        "egrave;" | "egrave" => "\u{e8}",
        "agrave;" | "agrave" => "\u{e0}",
        "uuml;" | "uuml" => "\u{fc}",
        "ouml;" | "ouml" => "\u{f6}",
        "auml;" | "auml" => "\u{e4}",
        "szlig;" | "szlig" => "\u{df}",
        "ntilde;" | "ntilde" => "\u{f1}",
        "ccedil;" | "ccedil" => "\u{e7}",
        _ => return None,
    })
}
