//! Golden check of the `rexon/appearance/translator.py` port (module
//! `rexon_core::translator`).
//!
//! The reference was captured from the LIVE `translator.py` (by a one-off
//! generator, read-only; not part of the tree):
//!   * `segments` — output of `translate_html` with a deterministic `translate`
//!     (segment wrapped in ‹…›), `_ensure_argos`/`_ensure_pair` monkeypatched;
//!   * `hashes`   — `hashlib.sha1(f"{V}:{src}:{tgt}:{text}").hexdigest()`;
//!   * `locales`  — `(_loc.split("_")[0] or "en").lower()`;
//!   * `looks`    — `_looks_like_html`;
//!   * `probes`   — computing `probe` from `translate_text`;
//!   * `detect`   — `detect_lang` without langdetect (script heuristic);
//!   * `plans`    — the `translate_text` decision with monkeypatched
//!     observations (`_argos_available`/`detect_lang`/`_ensure_pair`).
//!
//! ⚠️ The translation itself (argos), the SQLite cache and langdetect are NOT
//! ported (side effects, non-deterministic) — the PURE logic is checked:
//! segmentation, cache key, locale mapping, detection heuristic and decision.
//!
//! The set was extended with cases in CYRILLIC, mixed RU/EN strings, emoji,
//! numeric/named entities and empty / whitespace-only input — the interface is
//! Russian-speaking, and segmentation and the cache key work on characters and
//! UTF-8 bytes respectively. Minimal Cyrillic coverage is checked by the tests
//! themselves, so the set cannot «shrink».
//! ⚠️ `plans` are captured from the PUBLIC `translate_text`, not from its tail
//! `_py_translate_text_tail`: the early exits «empty / only spaces» live in
//! `translate_text`, and the port `plan_translate_text` includes them too.

use rexon_core::translator::{
    detect_lang_heuristic, looks_like_html, plan_translate_text, probe_text, render_segments,
    segment_html, translation_cache_key, translation_cache_key_versioned, user_lang_from_locale,
    TranslatePlan, TRANSLATOR_VERSION,
};
use serde_json::Value;

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/translator_cases.json")).unwrap()
}

/// HTML segmentation + reassembly with the same deterministic «translator»
/// (‹…› wrapper) gives byte-for-byte the output of Python `translate_html`.
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
    // Translating look names/descriptions — a Russian-speaking UI, so the
    // reference MUST contain Cyrillic (segmentation cuts by characters, not
    // bytes), mixed strings and empty/whitespace input.
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

/// How many cases contain Cyrillic in the field `field`.
fn is_cyrillic_covered(cases: &[Value], field: &str) -> usize {
    cases
        .iter()
        .filter(|c| c[field].as_str().map(has_cyrillic).unwrap_or(false))
        .count()
}

/// src_hash: SHA-1 of `version:src:tgt:text` matches hashlib.
///
/// ⚠️ The key MECHANISM is checked (field order, UTF-8, SHA-1), not the version
/// number: the reference was captured with the `translator_version` from the
/// reference itself, and the version is taken FROM THERE. The current engine
/// constant has moved on (argos → fxtranslate), there is no reason to
/// recapture the reference for that — see
/// [`fresh_key_does_not_match_the_old_engine`].
#[test]
fn cache_key_matches_python() {
    let g = golden();
    let golden_version = g["translator_version"].as_u64().unwrap() as u32;
    let hashes = g["hashes"].as_array().unwrap();
    // Key = SHA-1 of the UTF-8 bytes: Cyrillic/emoji must be in the reference,
    // otherwise an encoding mismatch would go unnoticed.
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

/// Entries of the FORMER engine are not picked up: on the same text today's
/// key DIFFERS from the reference one captured with the argos version.
///
/// This is the proof of cache incompatibility: the live code will look up the
/// translation by the new `src_hash`, rows of version `3` in the
/// `translations` table stay unreachable and expire by TTL.
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

/// Mapping the user locale to the target language.
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

/// HTML markup flag.
#[test]
fn looks_like_html_matches_python() {
    for c in golden()["looks"].as_array().unwrap() {
        let input = c["input"].as_str().unwrap();
        assert_eq!(looks_like_html(input), c["html"].as_bool().unwrap(), "looks для {input:?}");
    }
}

/// probe text for language detection (tag strip + unescape + collapsing spaces).
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

/// Language detection heuristic (without langdetect).
#[test]
fn detect_lang_heuristic_matches_python() {
    for c in golden()["detect"].as_array().unwrap() {
        let input = c["input"].as_str().unwrap();
        let want = c["lang"].as_str(); // Option: None → JSON null
        let got = detect_lang_heuristic(input);
        assert_eq!(got.as_deref(), want, "detect для {input:?}");
    }
}

/// The translate_text decision (skip / translate + src + is_html).
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
        // The reference field is still called `argos_available` (captured from the
        // former engine); the core parameter is the neutral `engine_available`.
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
