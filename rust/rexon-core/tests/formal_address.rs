//! Guard of the formal "вы" address in the core's texts.
//!
//! The product is sold to an ordinary buyer, and it talks to them the same way
//! as Steam and Windows do — with the formal "вы" (owner's decision of 11.09.2026). Before that
//! strings were written in a mix: the crash window said "Помоги разработчику —
//! отправь отчёт", the neighbouring tab said "Выберите рабочий стол". The inconsistency
//! is visible to the buyer at once and reads as carelessness.
//!
//! A one-off fix is not enough here: strings are added every day, and the next "ты" form
//! will arrive with the next tab. So the check reads the core's SOURCES
//! (`rexon-core/src/**.rs`), pulls string literals out of them and goes red on
//! forbidden forms: the pronouns "ты", 2nd person singular verbs and the
//! "ты" imperative mood.
//!
//! LIMITS (deliberate, so the check does not cry wolf):
//!   * ONLY string literals are read — comments and names in the code
//!     do not concern the check: the rule is about addressing a person, not about the
//!     language of the source;
//!   * the core is the only place where the product's labels live, so
//!     it is enough: Python and QML hold no letters of their own (the exception is the
//!     «Appearance» golden);
//!   * the list of imperative forms is EXPLICIT. Russian has no general rule
//!     "a verb ending in -и" («помощи», «двери», «строки» are not commands), and an attempt
//!     to guess the morphology would give false positives on every other
//!     word. A new form is caught by adding the word to [`IMPERATIVES`];
//!   * 2nd person singular verbs are caught by ending (-ешь/-ёшь/-ишь, including
//!     with -ся), and nouns with the same ending are listed in
//!     [`NOT_VERBS`].

mod source_literals;

use std::path::Path;

use source_literals::{collect_rs, string_literals};

/// Second person singular pronouns in all cases.
const PRONOUNS: &[&str] = &[
    "ты", "тебя", "тебе", "тобой", "тобою", "твой", "твоя", "твоё", "твое", "твои", "твоих",
    "твоим", "твоими", "твоем", "твоём", "твою", "твоего", "твоему", "твоей", "твоих",
];

/// The "ты" imperative mood. Extended as new strings appear —
/// the check exists precisely so that a form does not slip through silently.
const IMPERATIVES: &[&str] = &[
    "активируй", "введи", "верни", "вернись", "включи", "войди", "возьми", "вставь", "выбери",
    "выбирай", "выдели", "выключи", "выполни", "выйди", "глянь", "дай", "дождись", "жди", "жми",
    "загляни", "загрузи", "задай", "закрой", "залогинься", "запомни", "запусти", "запускай",
    "зайди", "измени", "используй", "исправь", "кликни", "нажми", "нажимай", "настрой", "оставь",
    "отключи", "отмени", "отметь", "отправь", "открой", "открывай", "очисти", "перезагрузи",
    "перезайди", "перезапусти", "переименуй", "перенеси", "переключи", "переключись", "перетащи",
    "перетяни",
    "подключи", "подожди", "подтверди", "покажи", "положи", "помоги", "помни", "поставь",
    "поменяй", "попробуй", "посмотри", "прими", "приложи", "примени", "пришли", "прокрути",
    "проверь", "проверяй", "продолжи", "прочитай", "прочти", "разверни", "разреши", "распакуй",
    "расскажи", "реши", "сбрось", "сверни", "сделай", "скачай", "скопируй", "смотри", "сними",
    "сообщи", "сохрани", "спроси", "ставь", "убедись", "убери", "удали", "удаляй", "укажи",
    "установи", "устанавливай", "учти", "щёлкни", "щелкни", "ищи", "напиши", "найди", "оцени",
    "перейди", "переустанови", "пересобери", "поддержи", "поделись", "подпишись", "получи",
    "обнови", "опиши", "повтори", "добавь", "создай", "выставь", "купи", "передай", "назови",
];

/// Words that end like a 2nd person singular verb but are not one.
/// Short ones («лишь», «тишь») are also cut off by length — see [`forbidden`].
const NOT_VERBS: &[&str] = &["брешь", "плешь", "лишь", "тишь", "глушь"];

/// Strings whose "ты" form is forgiven deliberately. Empty — and let it stay
/// empty: every entry here means that somewhere the buyer is addressed with
/// "ты". Format: the exact text of the literal.
const ALLOWED: &[&str] = &[];

/// "ты" forms in SOMEONE ELSE'S speech, which Rexon does not utter but translates.
///
/// The term fix dictionary (`translator.rs`, `TERM_FIXES`) repairs the machine
/// translation of theme descriptions: the theme author wrote "Unzip it", the engine gave «извлеки»
/// — we fix it to «распакуй». Here the address is chosen by the description's author, not
/// by the product: replacing "ты" with "вы" inside their phrase would mix two
/// forms of address in one sentence («распакуйте и положи»). So in the pair
/// "engine form → fixed form" both sides keep the person of the original.
///
/// The forgiveness is narrow: an exact literal (one word) and ONLY in the given file.
/// The same form in any other source or inside a phrase goes red as
/// before; an entry that is no longer in the file goes red too — the list does not
/// get overgrown with dead exceptions.
const FOREIGN_SPEECH: &[(&str, &str)] = &[
    ("src/translator.rs", "распакуй"),
    ("src/translator.rs", "загрузи"),
    ("src/translator.rs", "скачай"),
];

/// One found case.
#[derive(Debug)]
struct Hit {
    file: String,
    line: usize,
    word: String,
    literal: String,
}

#[test]
fn core_texts_use_formal_address() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    assert!(files.len() > 50, "исходники ядра не найдены: {}", src.display());

    let mut hits: Vec<Hit> = Vec::new();
    let mut foreign_seen = vec![false; FOREIGN_SPEECH.len()];
    for file in &files {
        let text = std::fs::read_to_string(file).expect("исходник не читается");
        let name = file
            .strip_prefix(src.parent().unwrap())
            .unwrap_or(file)
            .display()
            .to_string();
        for (line, literal) in string_literals(&text) {
            if ALLOWED.contains(&literal.as_str()) {
                continue;
            }
            if let Some(k) = FOREIGN_SPEECH.iter().position(|(f, l)| *f == name && *l == literal) {
                foreign_seen[k] = true;
                continue;
            }
            for word in cyrillic_words(&literal) {
                if forbidden(&word) {
                    hits.push(Hit {
                        file: name.clone(),
                        line,
                        word,
                        literal: short(&literal),
                    });
                }
            }
        }
    }

    let stale: Vec<_> = FOREIGN_SPEECH
        .iter()
        .zip(&foreign_seen)
        .filter(|(_, seen)| !**seen)
        .map(|(entry, _)| entry)
        .collect();
    assert!(stale.is_empty(), "исключения FOREIGN_SPEECH больше не встречаются — убрать: {stale:?}");

    assert!(
        hits.is_empty(),
        "строки ядра говорят покупателю «ты» — продукт обращается на «вы»:\n{}",
        hits.iter()
            .map(|h| format!("  {}:{}: «{}» в {}", h.file, h.line, h.word, h.literal))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The check checks itself: planted forms must be caught, otherwise
/// the green colour above means nothing.
#[test]
fn guard_catches_informal_forms() {
    for bad in [
        "Нажми кнопку",
        "Если хочешь, открой окно",
        "это твой рабочий стол",
        "ты останешься без сессии",
        "убедись, что демон запущен",
        "не найдёшь — напиши",
    ] {
        assert!(
            cyrillic_words(bad).iter().any(|w| forbidden(w)),
            "сторож не заметил «ты»-форму: {bad}"
        );
    }
    for good in [
        "Нажмите кнопку",
        "это ваш рабочий стол",
        "вы останетесь без сессии",
        "убедитесь, что демон запущен",
        // Words that look like 2nd person singular but are not.
        "брешь в обороне",
        "эта помощь не нужна",
        "мышь и клавиатура",
        "сброс темы ниже — лишь косметика",
    ] {
        assert!(
            !cyrillic_words(good).iter().any(|w| forbidden(w)),
            "сторож покраснел на нормальной строке: {good}"
        );
    }
}

fn forbidden(word: &str) -> bool {
    if NOT_VERBS.contains(&word) {
        return false;
    }
    if PRONOUNS.contains(&word) || IMPERATIVES.contains(&word) {
        return true;
    }
    // 2nd person singular verb: -ешь/-ёшь/-ишь, including reflexive. The length
    // threshold cuts off particles and nouns with the same ending («лишь»,
    // «тишь»): Russian has no such verbs shorter than five letters — the
    // shortest are «идёшь», «ждёшь», «поёшь».
    let stem = word.strip_suffix("ся").unwrap_or(word);
    ["ешь", "ёшь", "ишь"].iter().any(|e| stem.ends_with(e)) && stem.chars().count() > 4
}

/// Cyrillic words (a hyphen inside a word is kept).
fn cyrillic_words(text: &str) -> Vec<String> {
    text.split(|c: char| !(('а'..='я').contains(&c) || ('А'..='Я').contains(&c) || c == 'ё' || c == 'Ё'))
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

fn short(literal: &str) -> String {
    let trimmed: String = literal.chars().take(60).collect();
    if literal.chars().count() > 60 {
        format!("«{trimmed}…»")
    } else {
        format!("«{trimmed}»")
    }
}
