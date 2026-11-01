//! Golden-сверка порта `rexon/appearance/translator.py` (модуль
//! `rexon_core::translator`).
//!
//! Эталон снят с ЖИВОГО `translator.py` (одноразовым
//! генератором, read-only; в дерево не входит):
//!   * `segments` — вывод `translate_html` при детерминированном `translate`
//!     (обёртка сегмента в ‹…›), монипатчены `_ensure_argos`/`_ensure_pair`;
//!   * `hashes`   — `hashlib.sha1(f"{V}:{src}:{tgt}:{text}").hexdigest()`;
//!   * `locales`  — `(_loc.split("_")[0] or "en").lower()`;
//!   * `looks`    — `_looks_like_html`;
//!   * `probes`   — вычисление `probe` из `translate_text`;
//!   * `detect`   — `detect_lang` без langdetect (эвристика по скрипту);
//!   * `plans`    — решение `translate_text` при монипатченных наблюдениях
//!     (`_argos_available`/`detect_lang`/`_ensure_pair`).
//!
//! ⚠️ Сам перевод (argos), SQLite-кеш и langdetect НЕ портированы (side effect,
//! недетерминированы) — проверяется ЧИСТАЯ логика: сегментация, ключ кеша,
//! маппинг локали, детект-эвристика и решение.
//!
//! Набор доснят кейсами на КИРИЛЛИЦЕ,
//! смешанных RU/EN строках, эмодзи, числовых/именованных сущностях и пустом /
//! только-пробельном входе — интерфейс русскоязычный, а сегментация и ключ кеша
//! работают по символам и UTF-8-байтам соответственно. Минимальное покрытие
//! кириллицей проверяется самими тестами, чтобы набор нельзя было «усохнуть».
//! ⚠️ `plans` снимаются с ПУБЛИЧНОЙ `translate_text`, а не с её хвоста
//! `_py_translate_text_tail`: ранние выходы «пусто / одни пробелы» живут в
//! `translate_text`, и порт `plan_translate_text` включает их тоже.

use rexon_core::translator::{
    detect_lang_heuristic, looks_like_html, plan_translate_text, probe_text, render_segments,
    segment_html, translation_cache_key, translation_cache_key_versioned, user_lang_from_locale,
    TranslatePlan, TRANSLATOR_VERSION,
};
use serde_json::Value;

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/translator_cases.json")).unwrap()
}

/// Сегментация HTML + реассемблинг с той же детерминированной «переводилкой»
/// (обёртка ‹…›) даёт байт-в-байт вывод Python `translate_html`.
#[test]
fn html_segmentation_matches_python() {
    let g = golden();
    let cases = g["segments"].as_array().unwrap();
    assert!(cases.len() >= 50, "мало HTML-кейсов");
    for c in cases {
        let input = c["input"].as_str().unwrap();
        let want = c["output"].as_str().unwrap();
        let segs = segment_html(input);
        let got = render_segments(&segs, |s| format!("\u{2039}{s}\u{203a}"));
        assert_eq!(got, want, "segment_html для {input:?}");
    }
    // Перевод названий/описаний оформления — русскоязычный UI, поэтому эталон
    // ОБЯЗАН содержать кириллицу (сегментация режет по символам, не по байтам),
    // смешанные строки и пустой/пробельный вход.
    assert!(is_cyrillic_covered(cases, "input") >= 10, "мало кириллических кейсов");
    assert!(
        cases.iter().any(|c| {
            let s = c["input"].as_str().unwrap();
            has_cyrillic(s) && s.chars().any(|ch| ch.is_ascii_alphabetic())
        }),
        "нет смешанных латиница+кириллица кейсов"
    );
    assert!(
        cases.iter().any(|c| c["input"].as_str().unwrap().is_empty()),
        "нет кейса пустого входа"
    );
    assert!(
        cases
            .iter()
            .any(|c| { let s = c["input"].as_str().unwrap(); !s.is_empty() && s.trim().is_empty() }),
        "нет кейса из одних пробелов"
    );
}

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| ('\u{0400}'..='\u{04ff}').contains(&c))
}

/// Сколько кейсов содержат кириллицу в поле `field`.
fn is_cyrillic_covered(cases: &[Value], field: &str) -> usize {
    cases
        .iter()
        .filter(|c| c[field].as_str().map(has_cyrillic).unwrap_or(false))
        .count()
}

/// src_hash: SHA-1 от `версия:src:tgt:текст` совпадает с hashlib.
///
/// ⚠️ Сверяется МЕХАНИЗМ ключа (порядок полей, UTF-8, SHA-1), а не номер
/// версии: эталон снят при `translator_version` из самого эталона, и версия
/// берётся ОТТУДА. Текущая константа движка ушла вперёд (смена argos →
/// fxtranslate), переснимать из-за этого эталон нечего — см.
/// [`fresh_key_does_not_match_the_old_engine`].
#[test]
fn cache_key_matches_python() {
    let g = golden();
    let golden_version = g["translator_version"].as_u64().unwrap() as u32;
    let hashes = g["hashes"].as_array().unwrap();
    // Ключ = SHA-1 от UTF-8 байтов: кириллица/эмодзи должны быть в эталоне,
    // иначе расхождение по кодировке прошло бы незамеченным.
    assert!(is_cyrillic_covered(hashes, "text") >= 4, "мало кириллицы в ключах кеша");
    for c in hashes {
        let src = c["src"].as_str().unwrap();
        let tgt = c["tgt"].as_str().unwrap();
        let text = c["text"].as_str().unwrap();
        assert_eq!(
            translation_cache_key_versioned(golden_version, src, tgt, text),
            c["hash"].as_str().unwrap(),
            "src_hash для ({src},{tgt},{text:?})"
        );
    }
}

/// Записи ПРЕЖНЕГО движка не подхватываются: на одном и том же тексте
/// сегодняшний ключ ОТЛИЧАЕТСЯ от эталонного, снятого при версии argos.
///
/// Это и есть доказательство несовместимости кеша: искать перевод живой код
/// будет по новому `src_hash`, строки версии `3` в таблице `translations`
/// остаются недосягаемыми и уходят по TTL.
#[test]
fn fresh_key_does_not_match_the_old_engine() {
    let g = golden();
    let golden_version = g["translator_version"].as_u64().unwrap() as u32;
    assert!(
        TRANSLATOR_VERSION > golden_version,
        "смена движка обязана поднять TRANSLATOR_VERSION выше версии эталона \
         ({TRANSLATOR_VERSION} против {golden_version})"
    );
    let mut checked = 0usize;
    for c in g["hashes"].as_array().unwrap() {
        let src = c["src"].as_str().unwrap();
        let tgt = c["tgt"].as_str().unwrap();
        let text = c["text"].as_str().unwrap();
        let old_key = c["hash"].as_str().unwrap();
        assert_eq!(
            translation_cache_key_versioned(golden_version, src, tgt, text),
            old_key,
            "ключ старой версии обязан воспроизводиться"
        );
        assert_ne!(
            translation_cache_key(src, tgt, text),
            old_key,
            "запись старого движка нашлась бы по новому ключу ({src},{tgt},{text:?})"
        );
        checked += 1;
    }
    assert!(checked >= 4, "мало кейсов для доказательства");
}

/// Маппинг локали пользователя в целевой язык.
#[test]
fn user_lang_matches_python() {
    for c in golden()["locales"].as_array().unwrap() {
        let loc = c["locale"].as_str().unwrap();
        assert_eq!(
            user_lang_from_locale(loc),
            c["lang"].as_str().unwrap(),
            "user_lang для {loc:?}"
        );
    }
}

/// Признак HTML-разметки.
#[test]
fn looks_like_html_matches_python() {
    for c in golden()["looks"].as_array().unwrap() {
        let input = c["input"].as_str().unwrap();
        assert_eq!(looks_like_html(input), c["html"].as_bool().unwrap(), "looks для {input:?}");
    }
}

/// probe-текст для детекта языка (tag-strip + unescape + схлопывание пробелов).
#[test]
fn probe_text_matches_python() {
    let g = golden();
    let cases = g["probes"].as_array().unwrap();
    for c in cases {
        let input = c["input"].as_str().unwrap();
        assert_eq!(probe_text(input), c["probe"].as_str().unwrap(), "probe для {input:?}");
    }
    assert!(is_cyrillic_covered(cases, "input") >= 4, "мало кириллицы в probe-кейсах");
}

/// Эвристика детекта языка (без langdetect).
#[test]
fn detect_lang_heuristic_matches_python() {
    for c in golden()["detect"].as_array().unwrap() {
        let input = c["input"].as_str().unwrap();
        let want = c["lang"].as_str(); // Option: None → JSON null
        let got = detect_lang_heuristic(input);
        assert_eq!(got.as_deref(), want, "detect для {input:?}");
    }
}

/// Решение translate_text (skip / translate + src + is_html).
#[test]
fn plan_translate_text_matches_python() {
    let g = golden();
    assert!(
        is_cyrillic_covered(g["plans"].as_array().unwrap(), "text") >= 5,
        "мало кириллицы в кейсах решения"
    );
    for c in g["plans"].as_array().unwrap() {
        let text = c["text"].as_str().unwrap();
        let target = c["target"].as_str().unwrap();
        let detected = c["detected_src"].as_str(); // None → null
        // Поле эталона так и зовётся `argos_available` (снято с прежнего
        // движка); параметр ядра — нейтральный `engine_available`.
        let engine = c["argos_available"].as_bool().unwrap();
        let pair = c["src_pair_ensurable"].as_bool().unwrap();
        let got = plan_translate_text(text, target, detected, engine, pair);

        let want = &c["plan"];
        match want["action"].as_str().unwrap() {
            "skip" => assert_eq!(got, TranslatePlan::Skip, "plan skip для {text:?}"),
            "translate" => {
                let want_plan = TranslatePlan::Translate {
                    src: want["src"].as_str().unwrap().to_string(),
                    tgt: want["tgt"].as_str().unwrap().to_string(),
                    is_html: want["is_html"].as_bool().unwrap(),
                };
                assert_eq!(got, want_plan, "plan translate для {text:?}");
            }
            other => panic!("неизвестное действие {other}"),
        }
    }
}
