//! Live auto-translation of descriptions: assembling the translation core from
//! ready parts into ONE door for the window — [`translate_live`] (and
//! [`translate_live_batch`], the same door for several texts at once).
//!
//! The parts were ready, but there was no translation: [`crate::translator`]
//! gives the decision («translate or not»), HTML segmentation, the cache key
//! and the «prose or technical» filter; [`crate::appearance_cache::LiveCache`]
//! can read and write translations; the vendored `fxtranslate` can translate.
//! Here they are glued together, and one function faces outward: give a text
//! and a locale — get a translation. The tab does not need to know about
//! sentence blocks, cache keys, or where the model lies.
//!
//! ⚠️ What IS here besides the glue (Python did not have this):
//!   * **one engine per process** ([`LiveEngine`], `OnceLock<Mutex<…>>`). The
//!     model is 43 MB of mapped file and ~70 MB of prepared matrices; a second
//!     instance for a second open item would silently double that;
//!   * **a block for the engine is no more than [`MAX_BLOCK_SENTENCES`]
//!     sentences**. The batch path's encoder pads the WHOLE block to its
//!     longest line, so «let us translate all hundred sentences of the page at
//!     once» is not a speed-up but waste: one long line makes all the short
//!     ones more expensive;
//!   * **a block is filled ACROSS texts** ([`translate_live_batch`]). A
//!     comments page is twenty texts of one or two phrases each, and one by
//!     one they would give twenty almost empty blocks, i.e. the engine's batch
//!     path running idle;
//!   * **guarded names from the caller** ([`translate_live_guarded`]). The
//!     engine translates a theme name («Nordic Kvantum theme» →
//!     «Скандинавская Kvantum тема»), and the core has no list of names and
//!     cannot have one — but the window knows the item name and passes it
//!     next to the description;
//!   * **name substitution is spent only when needed** — two passes in
//!     [`translate_misses`]: first the sentence goes to the engine bare, and
//!     only if a name got lost in the answer is it asked again under
//!     placeholders. A placeholder itself confuses the decoder (measured
//!     24.09: `@0@` instead of `KDE` turned «Темы» into «Теммы»), so it is
//!     cheaper not to spend it;
//!   * **we raise the case of the first letter ourselves**
//!     ([`lift_first_letter`]). Measured 20.09 on FLORES-200: 88.6 % of lines
//!     (897 of 1012) arrive starting with a lowercase letter, and this is
//!     cured neither by the multiplication core nor by our vendor patch — the
//!     patch fixes CORRUPTION of the first word, while the case stays our job;
//!   * **the cache is per SEGMENT**, not per whole text. Workshop descriptions
//!     repeat each other paragraph by paragraph («Installation:», «Copy the
//!     folder to ~/.themes», lists of environments), and the segment key makes
//!     the second showing of a page free.
//!
//! The model does NOT lie in the tree (43 MB) — its place is in the delivery,
//! the [`MODEL_DIR_REL`] directory inside the `rexon/` package. No model — the
//! engine is honestly unavailable, [`translate_live`] returns the original and
//! `was_translated = false`; there are no errors in the customer's face then,
//! translation is a convenience, not a condition for the tab to work.
//!
//! Checks — `rust/rexon-core/tests/translator_engine.rs`: a fake engine with a
//! call counter on a temporary database (a second run of the same text — zero
//! calls) and an `#[ignore]` check of live translation on the real model.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::appearance_cache::LiveCache;
use crate::translator::{
    detect_lang_heuristic, drop_invented_repeats, fold_name_links, invented_latin_word,
    is_prose_segment, is_soft_guarded_name, mask_names, mask_only, plan_translate_text,
    probe_text, render_folded, segment_html, split_sentence_edges, translatable_flags_with_links,
    translation_cache_key, unmask_tech_names, user_lang_from_locale, MaskedNames, NameLinkRun,
    Segment, TranslatePlan,
};

// ============================================================================
// Engine: the trait
// ============================================================================

/// Translation engine — exactly what the glue needs from it.
///
/// The shape of the trait answers two facts, not taste:
///
///   1. **the language pair is part of the availability question.** Firefox
///      models are one per direction (`en→ru` is a separate file), so «the
///      engine is available» makes no sense without a pair: `en→ru` may be in
///      the delivery, `en→de` not. This is also the original's
///      `_ensure_pair`, whose observation [`plan_translate_text`] expects;
///   2. **translation is batched.** The unit of work is a BLOCK of sentences,
///      because that is what the engine was measured on (the encoder
///      processes the block at once). Give the trait one line — and the batch
///      path would become unreachable, and the difference is measurable: 217
///      sentences/s in blocks vs ~12 one by one.
///
/// Splitting text into sentences, batching, curing case and going to the cache
/// are NOT assigned to the engine: the glue does that ([`translate_live`]),
/// the same way for the live engine and for the fake one in checks.
pub trait TranslateEngine {
    /// Whether `src`→`tgt` translation is ready right now: the model is in
    /// place and loaded. False is not an error but «there will be no translation».
    fn available(&self, src: &str, tgt: &str) -> bool;

    /// Translate a block of sentences in one go.
    ///
    /// Contract: the length and order of the answer EQUAL the input; an empty
    /// string in place of a sentence means «could not, take the original». An
    /// answer of a different length the glue counts as a refusal for the
    /// whole block.
    fn translate_block(&self, src: &str, tgt: &str, block: &[&str]) -> Vec<String>;
}

/// How many sentences to give the engine at a time.
///
/// ⚠️ Not «the more the better». The batch encoder pads the block to its
/// longest line, i.e. one long sentence is paid for by all the short ones.
/// The 20.09 measurement ran block 8 (1012 FLORES-200 sentences in 4.67 s on
/// four threads); the working limit is 8–16, beyond that only padding grows.
/// We take the upper bound: look descriptions are short and of similar length.
pub const MAX_BLOCK_SENTENCES: usize = 16;

/// How many data threads to give the engine's batch path.
///
/// Not `0` (= all the machine's cores) on purpose: translation runs while the
/// person is looking at the tab, and there is no reason to take the whole CPU
/// for it — a page of twenty descriptions is translated in about a second even
/// on one. Four is the number the throughput was measured with.
const ENGINE_THREADS: usize = 4;

// ============================================================================
// Where the model lies
// ============================================================================

/// Environment variable with the model directory — the path for DEVELOPMENT
/// (the model is not put into the tree, and everyone has their own bench copy).
pub const MODEL_DIR_ENV: &str = "REXON_TRANSLATE_MODEL_DIR";

/// Directory of translation models inside the `rexon/` data package; inside it
/// — a directory per direction (`translate/enru`).
///
/// The owner decided to put the model IN THE PACKAGE, so it is looked up in the
/// same place and in the same order as the wallpaper tab's vendor QML and
/// `themes.json` — [`crate::installer::package_data_dir_candidates`]
/// (developer tree → distribution package → system install → home install).
/// No search order of its own is set up here: a second truth about where the
/// product lies has already once cost us QML plugins that were not found.
pub const MODEL_DIR_REL: &str = "translate";

/// Files of one direction's model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFiles {
    /// Weights (`model.enru.intgemm.alphas.bin`).
    pub model: String,
    /// Source vocabulary (`vocab.enru.spm`).
    pub src_vocab: String,
    /// Target vocabulary: for pairs with a shared vocabulary — the same file.
    pub trg_vocab: String,
}

/// Direction name in Firefox file names: `en` + `ru` = `enru`.
pub fn model_pair(src: &str, tgt: &str) -> String {
    format!("{}{}", src.to_lowercase(), tgt.to_lowercase())
}

/// Directories where a direction's model may lie, in decreasing preference.
///
/// `env_dir` — the [`MODEL_DIR_ENV`] value (a directory with the model files
/// FLAT, as the bench provides it); `repo_root` — the developer tree root
/// ([`crate::cli::repo_root`]), `home` — the home directory.
pub fn model_dir_candidates(
    env_dir: Option<&str>,
    repo_root: &str,
    home: &str,
    src: &str,
    tgt: &str,
) -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();
    if let Some(dir) = env_dir {
        let dir = dir.trim().trim_end_matches('/');
        if !dir.is_empty() {
            dirs.push(dir.to_string());
        }
    }
    let pair = model_pair(src, tgt);
    for pkg in crate::installer::package_data_dir_candidates(repo_root, home) {
        dirs.push(format!("{pkg}/{MODEL_DIR_REL}/{pair}"));
    }
    dirs.dedup();
    dirs
}

/// Model files in ONE directory, or `None` if they are not there.
///
/// The layout is flat — the same as a downloaded Firefox model:
/// `model.<pair>.intgemm.alphas.bin` and `vocab.<pair>.spm`. Pairs with
/// separate vocabularies (CJK) have `srcvocab.…`/`trgvocab.…` instead of the
/// shared `vocab.…`; supported, although the delivery so far has only `en→ru`
/// with a shared vocabulary.
///
/// ⛔ The shortlist (`lex.*.bin`) is deliberately not looked for: measured
/// 20.09 — minus 0.4 chrF++ and plus 2.8 MB of delivery for a speed-up that
/// is not visible on dozens of short descriptions.
///
/// The file system probe comes as a callback: the core does not touch the
/// disk itself.
pub fn model_files_in(
    dir: &str,
    src: &str,
    tgt: &str,
    is_file: &dyn Fn(&str) -> bool,
) -> Option<ModelFiles> {
    let dir = dir.trim_end_matches('/');
    let pair = model_pair(src, tgt);
    let model = format!("{dir}/model.{pair}.intgemm.alphas.bin");
    if !is_file(&model) {
        return None;
    }
    let shared = format!("{dir}/vocab.{pair}.spm");
    if is_file(&shared) {
        return Some(ModelFiles { model, src_vocab: shared.clone(), trg_vocab: shared });
    }
    let src_vocab = format!("{dir}/srcvocab.{pair}.spm");
    let trg_vocab = format!("{dir}/trgvocab.{pair}.spm");
    if is_file(&src_vocab) && is_file(&trg_vocab) {
        return Some(ModelFiles { model, src_vocab, trg_vocab });
    }
    None
}

/// The first found model of a direction from [`model_dir_candidates`].
pub fn pick_model(
    env_dir: Option<&str>,
    repo_root: &str,
    home: &str,
    src: &str,
    tgt: &str,
    is_file: &dyn Fn(&str) -> bool,
) -> Option<ModelFiles> {
    model_dir_candidates(env_dir, repo_root, home, src, tgt)
        .into_iter()
        .find_map(|dir| model_files_in(&dir, src, tgt, is_file))
}

/// A direction's model on a LIVE machine: the environment + tree/delivery.
pub fn pick_model_live(src: &str, tgt: &str) -> Option<ModelFiles> {
    let env_dir = std::env::var(MODEL_DIR_ENV).unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    pick_model(
        Some(env_dir.as_str()),
        &crate::cli::repo_root(),
        &home,
        src,
        tgt,
        &|path: &str| std::path::Path::new(path).is_file(),
    )
}

// ============================================================================
// Engine: the live implementation on top of fxtranslate
// ============================================================================

/// Work for the model thread: a block of sentences and where to put the answer.
struct Job {
    block: Vec<String>,
    reply: std::sync::mpsc::Sender<Vec<String>>,
}

/// The thread the loaded model lives on.
struct Worker {
    jobs: std::sync::mpsc::Sender<Job>,
}

/// The direction the engine was already brought up for. `worker = None` — it
/// was tried and failed (no model, or the file did not read); the attempt is
/// not repeated for every segment, otherwise we would thrash the disk on every
/// description line.
struct Loaded {
    pair: String,
    worker: Option<Worker>,
}

/// The engine is ONE per process: one loaded model, one thread.
///
/// ⚠️ Why the model lives on a SEPARATE thread and the lock holds only its
/// mailbox. The crate's `Engine` is `Sync` (the crate even checks this
/// statically), but NOT `Send`: inside the prepared matrix lies a raw pointer
/// to memory handed out by a C++ shim. So `Mutex<Engine>` in a static
/// variable is impossible in principle — not because of our caution but by
/// types, and «forcing» it with `unsafe impl Send` would mean vouching for
/// someone else's library without proof. The model is loaded where it lives
/// and never moves; only strings travel outward.
///
/// The lock is still needed, and not for memory's sake: it prevents two tabs
/// from bringing the model up twice (43 MB of mapped file and ~70 MB of
/// prepared matrices) and also prevents the heavy batch pass from sharing the
/// CPU with itself.
static ENGINE: OnceLock<Mutex<Option<Loaded>>> = OnceLock::new();

fn engine_slot() -> &'static Mutex<Option<Loaded>> {
    ENGINE.get_or_init(|| Mutex::new(None))
}

/// Bring up the thread with a direction's model. `None` — no model, or it did not read.
fn spawn_worker(src: &str, tgt: &str) -> Option<Worker> {
    let files = pick_model_live(src, tgt)?;
    let (jobs, inbox) = std::sync::mpsc::channel::<Job>();
    let (ready, loaded) = std::sync::mpsc::channel::<bool>();
    std::thread::Builder::new()
        .name("rexon-translate".to_string())
        .spawn(move || {
            let engine = match fxtranslate::engine::Engine::load_mmapped(
                &files.model,
                &files.src_vocab,
                &files.trg_vocab,
            ) {
                Ok(engine) => engine.with_threads(ENGINE_THREADS),
                // The model is in place but did not read (a truncated file, a
                // foreign version) — the engine is unavailable, the tab works without
                // translation.
                Err(_) => {
                    let _ = ready.send(false);
                    return;
                }
            };
            if ready.send(true).is_err() {
                return;
            }
            drop(ready);
            // The mailbox closed together with the process — the thread leaves by itself.
            while let Ok(job) = inbox.recv() {
                let block: Vec<&str> = job.block.iter().map(String::as_str).collect();
                let _ = job.reply.send(engine.translate_batch(&block));
            }
        })
        .ok()?;
    match loaded.recv() {
        Ok(true) => Some(Worker { jobs }),
        _ => None,
    }
}

/// Give the job `f` the mailbox of the `src`→`tgt` direction's model, or `None`.
///
/// ⚠️ A second direction is NOT brought up in the same process: that is
/// another 43 MB of model and ~70 MB of matrices for a translation nobody
/// asked for (the user's target language is one per session — their locale).
/// Another direction was asked — the answer is «unavailable».
fn with_worker<R>(src: &str, tgt: &str, f: impl FnOnce(&Worker) -> R) -> Option<R> {
    let pair = model_pair(src, tgt);
    let mut slot = engine_slot().lock().ok()?;
    if slot.is_none() {
        let worker = spawn_worker(src, tgt);
        *slot = Some(Loaded { pair: pair.clone(), worker });
    }
    let loaded = slot.as_ref()?;
    if loaded.pair != pair {
        return None;
    }
    loaded.worker.as_ref().map(f)
}

/// The live engine: the vendored `fxtranslate` on the model from the delivery.
///
/// The instance holds nothing — all the weight is in [`ENGINE`]; creating it
/// for every job of the tab is fine.
#[derive(Debug, Clone, Copy, Default)]
pub struct LiveEngine;

impl LiveEngine {
    pub fn new() -> Self {
        LiveEngine
    }
}

impl TranslateEngine for LiveEngine {
    fn available(&self, src: &str, tgt: &str) -> bool {
        with_worker(src, tgt, |_| ()).is_some()
    }

    fn translate_block(&self, src: &str, tgt: &str, block: &[&str]) -> Vec<String> {
        let asked: Vec<String> = block.iter().map(|s| s.to_string()).collect();
        let done = with_worker(src, tgt, |worker| {
            let (reply, answer) = std::sync::mpsc::channel::<Vec<String>>();
            worker.jobs.send(Job { block: asked, reply }).ok()?;
            answer.recv().ok()
        });
        // The thread died or the mailbox is closed — refusal for the whole block (the trait contract).
        done.flatten().unwrap_or_else(|| vec![String::new(); block.len()])
    }
}

// ============================================================================
// Curing the case of the first letter
// ============================================================================

/// Raise the first letter of the translation if it is capital in the source.
///
/// An engine defect reproduced on 88.6 % of FLORES-200 lines: the translation
/// arrives starting with a lowercase letter («поддерживаемые настольные
/// компьютеры: …»). Cured by the SOURCE, not unconditionally: an HTML text
/// node may be a continuation of a phrase after `<b>…</b>` and legitimately
/// start with a lowercase letter.
pub fn lift_first_letter(source: &str, translated: &str) -> String {
    let src_upper = source
        .chars()
        .find(|c| c.is_alphabetic())
        .map(|c| c.is_uppercase())
        .unwrap_or(false);
    if !src_upper {
        return translated.to_string();
    }
    let Some((idx, ch)) = translated.char_indices().find(|(_, c)| c.is_alphabetic()) else {
        return translated.to_string();
    };
    if ch.is_uppercase() {
        return translated.to_string();
    }
    let mut out = String::with_capacity(translated.len() + 1);
    out.push_str(&translated[..idx]);
    out.extend(ch.to_uppercase());
    out.push_str(&translated[idx + ch.len_utf8()..]);
    out
}

// ============================================================================
// Glue: translating one description
// ============================================================================

/// Result of [`translate_live`]: the text (translated or original) and a flag
/// that the translation really happened (port of the original's `was_translated`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTranslation {
    pub text: String,
    pub was_translated: bool,
}

impl LiveTranslation {
    fn untouched(text: &str) -> Self {
        LiveTranslation { text: text.to_string(), was_translated: false }
    }
}

/// Translate a description: from the user's locale to a ready string.
///
/// The order is exactly what Python had, plus our two insertions (the prose
/// filter and the segment cache):
///   1. target language — [`user_lang_from_locale`] from the locale string;
///   2. source language — [`detect_lang_heuristic`] over [`probe_text`]
///      (visible text without markup);
///   3. DECISION — [`plan_translate_text`]: empty, already in the target
///      language or no engine → the original without any work;
///   4. splitting into segments with [`segment_html`] and selecting prose
///      ([`translatable_flags`]) — technical text does not reach the engine;
///   5. cache per segment ([`translation_cache_key`]) — READ before the engine;
///   6. the engine on what was not found in the cache: sentences, blocks of
///      [`MAX_BLOCK_SENTENCES`], curing case;
///   7. WRITE to the cache and assembling the string [`render_segments_filtered`].
///
/// `locale` — the user's locale string (`ru_RU.UTF-8`, `en_US`, empty);
/// getting it is the caller's business, as in the original.
///
/// This is one text of [`translate_live_batch`], and there is deliberately no
/// second translation path in the tree (rule #12): the panel description and
/// the comments page go to the engine by one road.
pub fn translate_live(
    text: &str,
    locale: &str,
    engine: &dyn TranslateEngine,
    cache: &mut LiveCache,
) -> LiveTranslation {
    translate_live_guarded(text, locale, engine, cache, &[])
}

/// The same as [`translate_live`], but the caller adds ITS OWN guarded names:
/// the name of the open item, the author's name — everything that cannot be
/// in the core's list.
///
/// ⭐ WHY. The core protects technology names by its own list, but there was
/// nothing to protect a THEME NAME with — there is no list of names and there
/// cannot be one. The live engine answered «Скандинавская Kvantum тема» to
/// «Nordic Kvantum theme» and «Сладкая Kvantum тема» to «Sweet Kvantum
/// theme». The window KNOWS the item name (it arrives next to the
/// description), so the core takes names from it and hides them with the
/// same placeholder trick — see [`crate::translator::mask_names`].
///
/// The list contract: the core drops empty and very short (1-2 characters)
/// names itself, so the caller has nothing to check — the name and the
/// author's name can be given as is. Case: a multi-word name is recognized in
/// any case, a ONE-word name — only Capitalized or in caps (otherwise the item
/// «Dark» would eat the word `dark` in prose).
pub fn translate_live_guarded(
    text: &str,
    locale: &str,
    engine: &dyn TranslateEngine,
    cache: &mut LiveCache,
    guarded: &[&str],
) -> LiveTranslation {
    translate_live_batch_guarded(&[text], locale, engine, cache, guarded)
        .pop()
        .unwrap_or_else(|| LiveTranslation::untouched(text))
}

/// Translate SEVERAL texts in one trip to the engine. The answer is one text
/// per text, in the same order.
///
/// The order of work is the same as [`translate_live`], but batching is
/// end-to-end: the decision and parsing are computed per text (otherwise a
/// Russian comment among English ones would go to the engine), while sentence
/// blocks are filled ACROSS TEXTS — one go for the whole page.
///
/// ⭐ This is why the function appeared: workshop comments are short (one or
/// two phrases) and there are twenty of them per page. Twenty calls to
/// `translate_live` would give twenty blocks of one or two sentences, i.e. the
/// engine's batch path running idle (217 sentences/s in blocks vs ~12 one by
/// one, measured 20.09). Here the same twenty comments go as two or three full
/// blocks.
///
/// A text that [`plan_translate_text`] said «do not translate» to (empty,
/// already in the customer's language, no engine) is returned unchanged and
/// with `was_translated = false` — that is how the tab knows it need not draw
/// the «show original» link.
pub fn translate_live_batch(
    texts: &[&str],
    locale: &str,
    engine: &dyn TranslateEngine,
    cache: &mut LiveCache,
) -> Vec<LiveTranslation> {
    translate_live_batch_guarded(texts, locale, engine, cache, &[])
}

/// The same as [`translate_live_batch`], with the caller's guarded names
/// ([`translate_live_guarded`]). The list is ONE for the whole batch: a
/// comments page is comments on ONE item, and they share its name.
pub fn translate_live_batch_guarded(
    texts: &[&str],
    locale: &str,
    engine: &dyn TranslateEngine,
    cache: &mut LiveCache,
    guarded: &[&str],
) -> Vec<LiveTranslation> {
    let target = user_lang_from_locale(locale);

    // Pair availability is asked ONCE per pair: for the live engine it is
    // looking for the model on disk and loading it, and there are twenty
    // texts here.
    let mut availability: HashMap<String, bool> = HashMap::new();

    // Parsing and decision — per text.
    let mut prepared: Vec<Option<Prepared>> = Vec::with_capacity(texts.len());
    // Language pairs in order of appearance: usually there is one (`en`→`ru`),
    // but a comments page may well turn out multilingual.
    let mut pairs: Vec<(String, String)> = Vec::new();
    // Unique texts to translate under each pair: descriptions and comments
    // repeat each other, and there is no reason to pay twice for one line.
    let mut wanted: Vec<Vec<String>> = Vec::new();
    // Names from the name links of the whole batch ([`fold_name_links`]).
    let mut link_names: Vec<String> = Vec::new();

    for text in texts {
        let probe = probe_text(text);
        let detected = detect_lang_heuristic(&probe);
        // The source candidate — the same the decision will choose: detected or `en`.
        let candidate = detected.as_deref().filter(|s| !s.is_empty()).unwrap_or("en");
        // The original's `src_pair_ensurable` = «the path for THIS candidate
        // can be assembled»; `engine_available` = «there is at least some
        // path» (the decision falls back to `en`, and if there is no model for
        // it either — there is nothing to translate with).
        let pair_ready = pair_available(engine, &mut availability, candidate, &target);
        let any_ready = pair_ready || pair_available(engine, &mut availability, "en", &target);

        let plan = plan_translate_text(text, &target, detected.as_deref(), any_ready, pair_ready);
        let TranslatePlan::Translate { src, tgt, .. } = plan else {
            prepared.push(None);
            continue;
        };

        // ⚠️ The path is one for HTML and plain text, and this is not a
        // simplification: Python chose the plain path when `looks_like_html`
        // was false, i.e. the text has NEITHER tags NOR entities — and on such
        // text `segment_html` gives exactly one translatable node. Same
        // behaviour, one branch fewer.
        // A name link (`my <a href=…/p/1253385/>Sweet</a> theme`) is glued with
        // neighbouring nodes into one phrase: the engine sees the whole
        // sentence, not «Sweet» and «theme» separately ([`fold_name_links`]).
        let (segments, runs) = fold_name_links(segment_html(text));
        for run in runs.iter().flatten() {
            for name in run.names() {
                if !link_names.iter().any(|n| n == name) {
                    link_names.push(name.to_string());
                }
            }
        }
        // Prose is decided BY SENTENCE with the same boundaries the text later
        // goes to the engine with: the node «hddtemp shows only the first
        // digit. here is a screenshot http://…» is neither given whole nor
        // rejected whole.
        let flags = translatable_flags_with_links(&segments, &sentence_texts);

        let pair = match pairs.iter().position(|(s, t)| *s == src && *t == tgt) {
            Some(at) => at,
            None => {
                pairs.push((src, tgt));
                wanted.push(Vec::new());
                pairs.len() - 1
            }
        };
        for (seg, allow) in segments.iter().zip(flags.iter()) {
            if let (Segment::Text { stripped, .. }, true) = (seg, *allow) {
                if !wanted[pair].contains(stripped) {
                    wanted[pair].push(stripped.clone());
                }
            }
        }
        prepared.push(Some(Prepared { segments, flags, runs, pair }));
    }

    // Names from links are guarded on a par with the item name: «Sweet» in
    // «…to fit my Sweet theme» is the name of a neighbouring theme, not «сладкий».
    let guarded_all: Vec<&str> =
        guarded.iter().copied().chain(link_names.iter().map(String::as_str)).collect();
    let guarded = guarded_all.as_slice();

    // The cache comes BEFORE the engine; the engine on the rest, and the cache WRITE right after it.
    let mut ready: Vec<HashMap<String, String>> = vec![HashMap::new(); pairs.len()];
    for (pair, (src, tgt)) in pairs.iter().enumerate() {
        let mut misses: Vec<&str> = Vec::new();
        for w in &wanted[pair] {
            match cache.get_translation(&translation_cache_key(src, tgt, w)) {
                Some(hit) => {
                    ready[pair].insert(w.clone(), hit);
                }
                None => misses.push(w.as_str()),
            }
        }
        if misses.is_empty() {
            continue;
        }
        // ⚠️ The cache key does NOT contain the guarded names — on purpose. The
        // key is computed from the segment text, and a name is substituted only
        // where it occurs in that text; add the name to the key — and a line
        // shared by all items («Installation:», «Copy the folder to ~/.themes»)
        // would be cached anew for every item, i.e. the segment cache would
        // lose its point.
        for (w, translated) in
            misses.iter().zip(translate_misses(&misses, src, tgt, engine, guarded))
        {
            let Some(translated) = translated else { continue };
            cache.put_translation(&translation_cache_key(src, tgt, w), &translated);
            ready[pair].insert((*w).to_string(), translated);
        }
    }

    let mut out: Vec<LiveTranslation> = Vec::with_capacity(texts.len());
    for (text, prep) in texts.iter().zip(prepared.iter()) {
        let Some(prep) = prep else {
            out.push(LiveTranslation::untouched(text));
            continue;
        };
        let done = &ready[prep.pair];
        let rendered = render_folded(&prep.segments, &prep.flags, &prep.runs, |s| {
            done.get(s).cloned().unwrap_or_default()
        });
        out.push(LiveTranslation {
            was_translated: rendered.trim() != text.trim(),
            text: rendered,
        });
    }
    out
}

/// Whether a translation may follow for this text — the same decision as
/// [`plan_translate_text`], but WITHOUT the engine (the pair is assumed
/// available).
///
/// The window needs it to decide whether to show the «Translating the
/// description…» mark: Russian text for a Russian customer, an empty
/// description — «no» right away, and the mark does not flash even for a
/// moment. The model is not loaded here (for the live engine the availability
/// question means finding and loading 43 MB).
pub fn translation_may_follow(text: &str, locale: &str) -> bool {
    let target = user_lang_from_locale(locale);
    let detected = detect_lang_heuristic(&probe_text(text));
    matches!(
        plan_translate_text(text, &target, detected.as_deref(), true, true),
        TranslatePlan::Translate { .. }
    )
}

/// A text the decision said «translate» to: its segments, the «call the
/// engine or not» flags and the number of the language pair in the batch's
/// shared list.
///
/// The flags are stored rather than recomputed during assembly: computing
/// them twice means risking that selection and assembly diverge, and a
/// translated node would go to the output as the original.
struct Prepared {
    segments: Vec<Segment>,
    flags: Vec<bool>,
    /// Gluings through name links — in the place of their node ([`fold_name_links`]).
    runs: Vec<Option<NameLinkRun>>,
    pair: usize,
}

/// Sentence boundaries of one text — by the splitter the translation crate
/// itself uses. ONE truth about boundaries for the whole module: both prose
/// selection and filling engine blocks go by it.
fn sentence_texts(text: &str) -> Vec<String> {
    use fxtranslate::segment::{BasicSegmenter, Segmenter};
    BasicSegmenter.sentences(text).iter().map(|s| s.of(text).to_string()).collect()
}

/// Whether the `src`→`tgt` pair is ready, remembering what was already asked.
///
/// The memo is not decoration: for the live engine
/// [`TranslateEngine::available`] means finding the model files and loading
/// it, and in a comments page batch the question would be asked anew for each
/// of twenty texts.
fn pair_available(
    engine: &dyn TranslateEngine,
    seen: &mut HashMap<String, bool>,
    src: &str,
    tgt: &str,
) -> bool {
    if let Some(known) = seen.get(&model_pair(src, tgt)) {
        return *known;
    }
    let ready = engine.available(src, tgt);
    seen.insert(model_pair(src, tgt), ready);
    ready
}

/// Translate the texts that were not in the cache. `None` in place of a text —
/// the engine could not (not a single sentence), such a result is not written
/// to the cache: let the next attempt ask the engine again.
///
/// ⭐ TWO PASSES, and this is a measurement, not caution. A name placeholder
/// ITSELF confuses the decoder: «becoming a supporter for the KDE Themes and
/// Extensions section» the live model translates correctly — «стать
/// сторонником раздела KDE «Темы и расширения»», — while the same phrase with
/// `@0@` in place of `KDE` comes back with «Теммы и расширения» (acceptance
/// 24.09, the author's shared paragraph in the items «Nordic KDE» and «Sweet
/// Mars»). So substitution should be spent only where the name really gets
/// lost without it:
///   * **pass 1** — the sentence BARE, as the person wrote it. The answer is
///     taken if every guarded name stands in it VERBATIM;
///   * **pass 2** — only the sentences where the engine translated or inflected
///     a name («Nordic» → «скандинавская»): they go under placeholders, and
///     then the former [`unmask_tech_names`] sieves work. Since 06.11 ONLY the
///     names lost in the first pass go under placeholders;
///   * **pass 3** (without the engine) — if a name did not hold even under a
///     placeholder, but all lost names are «soft» (a word from the item name),
///     the bare answer of the first pass is taken instead of throwing away the
///     whole paragraph.
/// The second trip to the engine is paid only for corrupted sentences, and most
/// phrases of a description stopped seeing placeholders at all.
fn translate_misses(
    misses: &[&str],
    src: &str,
    tgt: &str,
    engine: &dyn TranslateEngine,
    guarded: &[&str],
) -> Vec<Option<String>> {
    use fxtranslate::segment::{BasicSegmenter, Segmenter, Span};

    // ⚠️ Sentences, not whole nodes: the engine's batch path feeds each line
    // to the decoder WHOLE, and a paragraph longer than the model window (127
    // tokens) is silently cut — the decoder emits EOS before the end. The
    // boundaries come from the same splitter the crate itself uses.
    let spans: Vec<Vec<Span>> = misses.iter().map(|t| BasicSegmenter.sentences(t)).collect();

    // A flat list of sentences of all texts — so the block is filled ACROSS
    // nodes: otherwise a one-sentence paragraph would go to the engine alone.
    let mut flat: Vec<&str> = Vec::new();
    for (text, spans) in misses.iter().zip(spans.iter()) {
        for span in spans.iter() {
            flat.push(span.of(text));
        }
    }

    // What not to give the engine at all: a technical sentence (a command, a
    // line of a single path or link) stays the original even in the middle
    // of a translatable paragraph — the same rule that selected the node
    // itself ([`translatable_flags_by_sentence`]). A path or link INSIDE
    // prose (since 03.10) does not reject the sentence: [`mask_names`] hides
    // them on a par with names, and below they are checked and returned the
    // same way.
    let prose: Vec<bool> = flat.iter().map(|s| is_prose_segment(s)).collect();

    // Sentence edges (a list marker, «### ✨ », the colon after a bold
    // label, emoji at the end) are not shown to the engine at all and come
    // back byte for byte ([`split_sentence_edges`]). Everything below is about
    // the middle.
    let edges: Vec<(&str, &str, &str)> = flat.iter().map(|s| split_sentence_edges(s)).collect();
    let core = |i: usize| edges[i].1;
    let wrap = |i: usize, t: String| format!("{}{t}{}", edges[i].0, edges[i].2);

    let mut done: Vec<String> = vec![String::new(); flat.len()];
    // The «placeholders did not match» mark: this sentence's translation is corrupted.
    let mut spoiled: Vec<bool> = vec![false; flat.len()];

    // ── pass 1: bare sentences, without placeholders.
    let bare: Vec<usize> =
        (0..flat.len()).filter(|i| prose[*i] && core(*i).chars().any(char::is_alphabetic)).collect();
    let asked: Vec<&str> = bare.iter().map(|i| core(*i)).collect();
    // ⚠️ Names (technologies — by the core's list, item and author — from the
    // caller) are hidden HERE, i.e. AFTER splitting into sentences, not
    // before. The order matters: the crate itself computes sentence
    // boundaries from the source text, and a substitution done earlier would
    // shift them (and `reassemble` below restores spaces from the ORIGINAL
    // and requires spans exactly from it).
    let mut retry: Vec<(usize, MaskedNames, Vec<bool>, String)> = Vec::new();
    for (i, answer) in bare.iter().zip(translate_blocks(engine, src, tgt, &asked)) {
        let source = core(*i);
        if answer.is_empty() {
            // The engine could not — the output will keep the sentence's original.
            continue;
        }
        let masked = mask_names(source, guarded);
        let lost: Vec<bool> =
            masked.names.iter().map(|n| !name_stands_verbatim(&answer, n)).collect();
        if lost.iter().any(|l| *l) {
            retry.push((*i, masked, lost, answer));
            continue;
        }
        if let Some(text) = take_if_clean(src, tgt, source, &answer) {
            done[*i] = wrap(*i, text);
        }
    }

    // ── pass 2: the same sentences, but under placeholders — ONLY the names
    // the engine lost in the first pass ([`mask_only`]). A placeholder itself
    // confuses the decoder, and with four at once it breaks; with one it does not.
    let hidden: Vec<MaskedNames> =
        retry.iter().map(|(_, masked, lost, _)| mask_only(masked, lost)).collect();
    let asked: Vec<&str> = hidden.iter().map(|m| m.text.as_str()).collect();
    let second = translate_blocks(engine, src, tgt, &asked);
    for (((i, masked, lost, first), only), answer) in retry.iter().zip(hidden.iter()).zip(second) {
        let source = core(*i);
        // Names left bare must survive in the second answer too.
        let restored = (!answer.is_empty())
            .then(|| unmask_tech_names(&answer, &only.names))
            .flatten()
            .filter(|r| masked.names.iter().all(|n| name_stands_verbatim(r, n)));
        if let Some(restored) = restored {
            if let Some(text) = take_if_clean(src, tgt, source, &restored) {
                done[*i] = wrap(*i, text);
            }
            continue;
        }
        // ── pass 3: the name did not hold even under a placeholder. If all
        // lost names are «soft» (a word from the item name, not the name
        // itself, not a name from the core's lists and not technical text,
        // [`is_soft_guarded_name`]), we take the bare translation: a translated
        // word from the name is better than a whole English paragraph.
        // Otherwise — corruption.
        let soft = masked
            .names
            .iter()
            .zip(lost.iter())
            .filter(|(_, l)| **l)
            .all(|(n, _)| is_soft_guarded_name(n, guarded));
        if soft {
            if let Some(text) = take_if_clean(src, tgt, source, first) {
                done[*i] = wrap(*i, text);
                continue;
            }
        }
        // The placeholders did not match — the sentence is empty, and the whole
        // segment is marked corrupted (below).
        spoiled[*i] = true;
    }

    let mut out: Vec<Option<String>> = Vec::with_capacity(misses.len());
    let mut at = 0usize;
    for (text, spans) in misses.iter().zip(spans.iter()) {
        let mine = &done[at..at + spans.len()];
        let broken = spoiled[at..at + spans.len()].iter().any(|b| *b);
        at += spans.len();
        // ⚠️ One corrupted sentence cancels the WHOLE segment, not just itself.
        // Better not to translate than to hand over a mess: a paragraph where
        // one phrase is Russian and the second stayed English looks worse than
        // an honest original, and a translation with a lost name is simply
        // untrue. Such a segment is not written to the cache (`None`), and the
        // next attempt will ask the engine anew.
        if broken || mine.is_empty() || mine.iter().all(|s| s.is_empty()) {
            out.push(None);
            continue;
        }
        // An empty place is the sentence's original; the crate itself restores
        // the spaces between sentences from the source string.
        let pieces: Vec<String> = spans
            .iter()
            .zip(mine.iter())
            .map(|(span, t)| if t.is_empty() { span.of(text).to_string() } else { t.clone() })
            .collect();
        out.push(Some(fxtranslate::segment::reassemble(text, spans, &pieces)));
    }
    out
}

/// Give sentences to the engine in blocks of [`MAX_BLOCK_SENTENCES`]. The
/// answer is one line per sentence and in the same order; an empty line means
/// «the engine could not, take the original».
fn translate_blocks(
    engine: &dyn TranslateEngine,
    src: &str,
    tgt: &str,
    asked: &[&str],
) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(asked.len());
    for block in asked.chunks(MAX_BLOCK_SENTENCES) {
        let mut answer = engine.translate_block(src, tgt, block);
        // An answer of the wrong length — refusal for the whole block (the trait
        // contract). Silently shifting translations onto other sentences would be
        // worse than a refusal.
        if answer.len() != block.len() {
            answer = vec![String::new(); block.len()];
        }
        out.extend(answer.into_iter().map(|t| t.trim().to_string()));
    }
    out
}

/// Whether the name stands in the engine's answer VERBATIM and as a separate word.
///
/// ⚠️ A substring occurrence is not enough, and this is not nitpicking: `Nord`
/// is found inside `Nordic`, so the answer «также тема Nordic Kvantum основана
/// на палитре Севера.» would pass as intact — although the word `Nord` itself
/// is translated in it («Севера»), and that is exactly what we guard. Any
/// non-alphanumeric character or the line edge counts as a boundary.
fn name_stands_verbatim(answer: &str, name: &str) -> bool {
    let mut from = 0usize;
    while let Some(at) = answer[from..].find(name) {
        let start = from + at;
        let end = start + name.len();
        let free_before =
            answer[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let free_after = answer[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if free_before && free_after {
            return true;
        }
        from = start + name.chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// Take a sentence's translation, raising the first letter, or `None` — it
/// must not be taken.
///
/// There is one refusal, and it is about corruption that neither the prose
/// filter nor the placeholder sieves see: an INVENTED Latin word
/// ([`invented_latin_word`]) — «служба графических элементов - toolbep, bars»
/// instead of `tooltips` (acceptance 24.09, «Awesome Widgets»). The refusal
/// applies to ONE sentence: the other phrases of the paragraph stay
/// translated, and the original takes the corrupted one's place.
///
/// Before raising the letter the terms are fixed by the dictionary
/// ([`crate::translator::apply_term_fixes`]: «расстегните» from `unzip` →
/// «распакуйте»). Here and not earlier: the translation already has names and
/// paths returned, and the invented Latin sieve has already been passed.
fn take_if_clean(src: &str, tgt: &str, source: &str, translated: &str) -> Option<String> {
    if invented_latin_word(source, translated).is_some() {
        return None;
    }
    let fixed = crate::translator::apply_term_fixes(src, tgt, source, translated);
    // What the engine invented beyond the source — the repeat «и и», the
    // conjunction «и» at the start ([`drop_invented_repeats`]): after the term
    // fixes (the dictionary has its own collapses) and before raising the
    // letter (the conjunction could be the first word).
    let fixed = drop_invented_repeats(src, tgt, source, &fixed);
    Some(lift_first_letter(source, &fixed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Model search order: the environment first, then the delivery layouts,
    /// exactly the same ones the vendor QML is looked up by (one truth about
    /// where the product lies).
    #[test]
    fn model_dirs_start_with_the_environment_then_follow_the_package() {
        let dirs = model_dir_candidates(Some("/стенд/модель"), "/дерево", "/home/w", "en", "ru");
        assert_eq!(dirs[0], "/стенд/модель");
        let pkg = crate::installer::package_data_dir_candidates("/дерево", "/home/w");
        assert_eq!(dirs.len(), 1 + pkg.len());
        for (candidate, base) in dirs[1..].iter().zip(pkg.iter()) {
            assert_eq!(candidate, &format!("{base}/translate/enru"));
        }
        // An empty variable is not the candidate «» (otherwise we would search from the root).
        let blank = model_dir_candidates(Some("  "), "", "", "en", "ru");
        assert!(
            blank.iter().all(|d| d.starts_with('/')),
            "пустая переменная попала в кандидаты: {blank:?}"
        );
    }

    /// A model is recognized by weights + vocabulary; a shared vocabulary and
    /// separate ones — both Firefox layouts.
    #[test]
    fn a_model_needs_both_weights_and_a_vocabulary() {
        let shared = |p: &str| {
            p == "/m/model.enru.intgemm.alphas.bin" || p == "/m/vocab.enru.spm"
        };
        let files = model_files_in("/m", "en", "ru", &shared).expect("общий словарь");
        assert_eq!(files.src_vocab, files.trg_vocab);
        assert!(files.model.ends_with("model.enru.intgemm.alphas.bin"));

        let split = |p: &str| {
            p == "/m/model.jaen.intgemm.alphas.bin"
                || p == "/m/srcvocab.jaen.spm"
                || p == "/m/trgvocab.jaen.spm"
        };
        let files = model_files_in("/m", "ja", "en", &split).expect("раздельные словари");
        assert_ne!(files.src_vocab, files.trg_vocab);

        // Weights without a vocabulary are not a model.
        let weights_only = |p: &str| p == "/m/model.enru.intgemm.alphas.bin";
        assert!(model_files_in("/m", "en", "ru", &weights_only).is_none());
        // Neither is a vocabulary without weights.
        let vocab_only = |p: &str| p == "/m/vocab.enru.spm";
        assert!(model_files_in("/m", "en", "ru", &vocab_only).is_none());
    }

    /// The case is raised by the SOURCE: capital in the source — raise it,
    /// lowercase — leave it (a continuation of a phrase after a tag).
    #[test]
    fn the_first_letter_follows_the_source() {
        assert_eq!(lift_first_letter("This theme", "эта тема"), "Эта тема");
        assert_eq!(lift_first_letter("This theme", "Эта тема"), "Эта тема");
        assert_eq!(lift_first_letter("and also", "и ещё"), "и ещё");
        // A leading non-letter is kept, the first LETTER is raised.
        assert_eq!(lift_first_letter("«Dark» theme", "«тёмная» тема"), "«Тёмная» тема");
        // No letters at all — the line is not touched.
        assert_eq!(lift_first_letter("Theme", "— 1.2.3 —"), "— 1.2.3 —");
    }
}
