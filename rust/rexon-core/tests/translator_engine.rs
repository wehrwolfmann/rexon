//! Check of live translation: the whole [`rexon_core::translator_engine`] glue —
//! from the locale to the finished string.
//!
//! The engine is a STAND-IN with a call counter, while the database is real, in a temporary
//! directory. This checks what cannot be seen otherwise:
//!   * a second run of the same text makes ZERO engine calls, that is the cache
//!     is really written and read (and not "seems to work");
//!   * technical segments go to the output VERBATIM — the engine does not see them;
//!   * HTML tags are intact, the translation lands only in text nodes;
//!   * a block for the engine is no longer than `MAX_BLOCK_SENTENCES`;
//!   * the lowercase first letter the engine answers with in 88.6 % of cases
//!     is raised by the glue.
//!
//! Live translation on the real model is an `#[ignore]` check at the end of the file (following
//! `fxtranslate_vendor.rs`): the 43 MB model is not put into the tree.

use std::sync::Mutex;

use rexon_core::appearance_cache::LiveCache;
use rexon_core::translator_engine as te;
use rexon_core::translator_engine::{TranslateEngine, MAX_BLOCK_SENTENCES};

// ============================================================================
// Stand
// ============================================================================

/// A temporary home: its own cache database for each check.
struct TempHome(String);

impl TempHome {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("rexon-translate-{tag}-{nanos}"));
        std::fs::create_dir_all(&path).expect("временный дом");
        TempHome(path.to_string_lossy().into_owned())
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A stand-in engine: counts calls and remembers every block.
///
/// It translates in a deliberately recognizable way and ON PURPOSE with a lowercase letter — exactly as
/// the real engine answers (measured: 88.6 % of lines). So a capital letter in
/// the output can only come from the glue.
#[derive(Default)]
struct FakeEngine {
    pair: Option<(String, String)>,
    blocks: Mutex<Vec<Vec<String>>>,
    /// What the engine does with name placeholders in the answer (corruption — like the live one).
    spoil: Spoil,
}

/// Corruption of a placeholder in the engine's answer. All three forms are real, seen on
/// the live Firefox model while choosing the placeholder form (stand 21.09).
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Spoil {
    /// Placeholders intact.
    #[default]
    None,
    /// `@0@` eaten entirely.
    Drop,
    /// `@0@` came twice (the engine multiplied the name).
    Double,
    /// A stub `@` remained of `@0@` («Nordic @ Kvantum тема»).
    Crumb,
}

impl FakeEngine {
    /// An engine that can do exactly `en`→`ru`.
    fn enru() -> Self {
        FakeEngine {
            pair: Some(("en".to_string(), "ru".to_string())),
            blocks: Mutex::default(),
            spoil: Spoil::None,
        }
    }

    /// The same engine, but corrupting the name placeholder in the answer.
    fn spoiling(spoil: Spoil) -> Self {
        FakeEngine { spoil, ..FakeEngine::enru() }
    }

    /// An engine that does not exist (the model turned out to be missing from the package).
    fn missing() -> Self {
        FakeEngine::default()
    }

    /// How many times the engine was called.
    fn calls(&self) -> usize {
        self.blocks.lock().unwrap().len()
    }

    /// All sentences the engine saw.
    fn seen(&self) -> Vec<String> {
        self.blocks.lock().unwrap().iter().flatten().cloned().collect()
    }

    /// Block lengths in order.
    fn block_sizes(&self) -> Vec<usize> {
        self.blocks.lock().unwrap().iter().map(Vec::len).collect()
    }
}

impl TranslateEngine for FakeEngine {
    fn available(&self, src: &str, tgt: &str) -> bool {
        self.pair.as_ref().is_some_and(|(s, t)| s == src && t == tgt)
    }

    fn translate_block(&self, src: &str, tgt: &str, block: &[&str]) -> Vec<String> {
        assert_eq!((src, tgt), ("en", "ru"), "движок позвали не на своё направление");
        assert!(
            block.len() <= MAX_BLOCK_SENTENCES,
            "блок из {} предложений — больше предела {MAX_BLOCK_SENTENCES}",
            block.len()
        );
        self.blocks
            .lock()
            .unwrap()
            .push(block.iter().map(|s| s.to_string()).collect());
        // With a lowercase letter — like the real engine.
        block
            .iter()
            .map(|s| {
                let mut answer = format!("перевод: {s}");
                // Names get "translated" — otherwise the stand-in engine would be more honest than
                // the live one (see MANGLED_NAMES).
                for (name, ru) in MANGLED_NAMES.iter().chain(MANGLED_TECH).chain(MANGLED_TERMS) {
                    answer = answer.replace(name, ru);
                }
                match self.spoil {
                    Spoil::None => answer,
                    Spoil::Drop => answer.replace("@0@", ""),
                    Spoil::Double => answer.replace("@0@", "@0@ и @0@"),
                    Spoil::Crumb => answer.replace("@0@", "@"),
                }
            })
            .collect()
    }
}

/// Names the stand-in engine "translates" — exactly those the live model
/// really inflects and translates (measurements in `live_names_…` below).
///
/// ⚠️ Without this the stand-in engine would be MORE HONEST than the real one: it returns
/// the sentence verbatim, so a name in the first (bare) pass would always "survive",
/// and the path under placeholders — the second pass — would not be checked
/// at all. The order matters: a long name before a short one ("KDE Plasma" before
/// "Plasma", "Nordic" before "Nord").
const MANGLED_NAMES: &[(&str, &str)] = &[
    ("KDE Plasma", "кде плазма"),
    ("Kvantum", "квантская"),
    ("Cinnamon", "корица"),
    ("Aurorae", "аврора"),
    ("Plasma", "плазма"),
    ("Nordic", "скандинавская"),
    ("Nord", "север"),
];

/// Technical bits the stand-in engine "translates" in the bare pass — for the same
/// reason as [`MANGLED_NAMES`]: otherwise a path in the first pass would always survive,
/// and the path under a placeholder would not be checked.
const MANGLED_TECH: &[(&str, &str)] =
    &[("~/.themes", "~/.темы"), ("/usr/share/sounds", "/usr/доля/звуки")];

/// Terms the stand-in engine "translates" as crookedly as the live
/// Firefox model (measured 03.10: "Just unzip the file…" → «Просто расстегните
/// файл…») — so that the correction dictionary is checked along the whole path, not only
/// as a pure function.
const MANGLED_TERMS: &[(&str, &str)] = &[("Unzip", "расстегните")];

/// A workshop description that is live in composition: prose, a heading, a command in `<pre>`,
/// a line with a path, a desktop list and a link.
const DESCRIPTION: &str = concat!(
    "<p>This global theme brings a warm, muted palette to the whole desktop. ",
    "It ships matching Plasma, Aurorae and Kvantum styles.</p>",
    "<p>Installation:</p>",
    "<pre>sudo apt-get install gtk2-engines-murrine</pre>",
    "<p>Copy the folder to ~/.themes and pick it in Settings.</p>",
    "<p>Preview: https://example.invalid/shot.png</p>"
);

// ============================================================================
// Checks
// ============================================================================

/// ⭐ THE MAIN THING: a second run of the same text does NOT call the engine even once, and the output is
/// byte-for-byte the same. This is the proof that the cache works for writing,
/// and the key is computed identically in both runs.
#[test]
fn the_second_run_of_the_same_text_never_calls_the_engine() {
    let home = TempHome::new("cache");

    let first_engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let first = te::translate_live(DESCRIPTION, "ru_RU.UTF-8", &first_engine, &mut cache);
    assert!(first.was_translated, "перевод обязан был случиться");
    assert!(first_engine.calls() >= 1, "движок не позвали ни разу");

    // Another database connection, another counter — as when an item is opened a second time.
    let second_engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let second = te::translate_live(DESCRIPTION, "ru_RU.UTF-8", &second_engine, &mut cache);
    assert_eq!(
        second_engine.calls(),
        0,
        "движок позвали второй раз — кеш не сработал: {:?}",
        second_engine.seen()
    );
    assert_eq!(second.text, first.text, "из кеша пришёл другой текст");
    assert!(second.was_translated, "перевод из кеша — тоже перевод");
}

/// The engine gets only prose, and AS SENTENCES at that. It does not see
/// technical bits at all, and technology names go under placeholders in a SECOND pass — and
/// only for the sentences in which the engine lost the name.
#[test]
fn the_engine_only_ever_sees_prose_sentences() {
    let home = TempHome::new("prose");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    te::translate_live(DESCRIPTION, "ru_RU.UTF-8", &engine, &mut cache);

    let seen = engine.seen();
    assert_eq!(
        seen,
        vec![
            // Pass 1 — sentences BARE, as the person wrote them: a placeholder
            // itself throws off the decoder, and there is no point spending it needlessly.
            "This global theme brings a warm, muted palette to the whole desktop.".to_string(),
            "It ships matching Plasma, Aurorae and Kvantum styles.".to_string(),
            "Installation:".to_string(),
            // Since 03.10 also phrases with a path and a link inside (the command in `<pre>`
            // is still not visible to the engine at all).
            "Copy the folder to ~/.themes and pick it in Settings.".to_string(),
            "Preview: https://example.invalid/shot.png".to_string(),
            // Pass 2 — only the sentences where a name or a path got lost.
            // `Plasma, Aurorae` is a chain of names separated by a comma, hence under ONE
            // placeholder; `Kvantum` after the word `and` — under its own.
            "It ships matching @0@ and @1@ styles.".to_string(),
            "Copy the folder to @0@ and pick it in Settings.".to_string(),
        ],
        "движку досталось не то"
    );
    // The first pass is ONE block for all five sentences (the batch path, not
    // one by one), the second is a block of the two damaged ones.
    assert_eq!(engine.block_sizes(), vec![5, 2]);
}

/// ⭐ THE MAIN THING ABOUT NAMES: the engine lost (multiplied, nibbled) a placeholder — the
/// ORIGINAL of the segment goes to the output, not the corrupted translation, and such a
/// segment is not written to the cache (the next attempt asks the engine again).
///
/// The corruption is faked by the stand-in engine on purpose: on the live model it happens
/// rarely and not on demand, and relying on "it did not happen today" is not allowed.
#[test]
fn a_lost_placeholder_returns_the_original_segment() {
    // A paragraph of TWO sentences: the second one with a name. One corrupted sentence
    // cancels the whole segment — otherwise we would get a mush of Russian and English.
    let source = concat!(
        "<p>This theme is calm and quiet. ",
        "A Kvantum theme heavily based on Fluent Design elements.</p>"
    );
    for spoil in [Spoil::Drop, Spoil::Double, Spoil::Crumb] {
        let home = TempHome::new("lost");
        let engine = FakeEngine::spoiling(spoil);
        let mut cache = LiveCache::open(&home.0);
        let out = te::translate_live(source, "ru_RU.UTF-8", &engine, &mut cache);
        assert!(engine.calls() >= 1, "движок не позвали — сверка ни о чём");
        assert_eq!(out.text, source, "в вывод пошла каша вместо оригинала");
        assert!(!out.was_translated, "испорченный перевод объявлен переводом");
        assert!(
            !out.text.contains('@'),
            "заполнитель уехал покупателю в лицо: {}",
            out.text
        );

        // Not written to the cache: the second run calls the engine again.
        let again = FakeEngine::spoiling(spoil);
        let mut cache = LiveCache::open(&home.0);
        let second = te::translate_live(source, "ru_RU.UTF-8", &again, &mut cache);
        assert!(again.calls() >= 1, "порча попала в кеш");
        assert_eq!(second.text, source);
    }
}

/// A technology name stays in Latin letters along the whole path too: the engine translated the name —
/// the sentence is asked again under placeholders, and the name returns to the output
/// verbatim.
#[test]
fn names_come_back_verbatim_through_the_whole_path() {
    let home = TempHome::new("names");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let out = te::translate_live(
        "<p>Sweet Kvantum theme based on Cinnamon and KDE Plasma.</p>",
        "ru_RU.UTF-8",
        &engine,
        &mut cache,
    );
    assert!(out.was_translated);
    for name in ["Kvantum", "Cinnamon", "KDE Plasma"] {
        assert!(out.text.contains(name), "имя {name} потерялось: {}", out.text);
    }
    assert!(!out.text.contains('@'), "заполнитель остался в выводе: {}", out.text);
    // The translation is taken from the SECOND pass: the names went under placeholders, and
    // it is its answer that went to the output.
    assert!(
        engine.seen().iter().any(|s| s.contains("@0@")),
        "второго прохода не было — имя спасать было нечем: {:?}",
        engine.seen()
    );
    // ⚠️ The name is shown to the engine bare (pass 1) — but its answer is not trusted:
    // the translated name is not in the output in any form.
    for mangled in ["квантская", "корица", "кде плазма"] {
        assert!(
            !out.text.contains(mangled),
            "в вывод пошёл ответ, где имя переведено («{mangled}»): {}",
            out.text
        );
    }
}

/// ⭐ Acceptance defect of 24.09 (D4b): in a node where prose stands next to a link,
/// the PROSE is translated, and the link stays verbatim — previously the whole node silently stayed
/// in English.
///
/// The live case: maxkaspar's comment on "Awesome Widgets" — "hddtemp shows
/// only the first digit. here is a screenshot http://…". The engine
/// gets ONLY the first sentence: the link is not shown to it at all.
///
/// Since 03.10 the second phrase is translated too — the link in it goes under a placeholder
/// only if the bare answer corrupted it; here the engine did not touch it, and it is
/// taken from the first pass verbatim.
#[test]
fn prose_next_to_a_link_is_translated_and_the_link_is_left_alone() {
    let source = concat!(
        "<p>hddtemp shows only the first digit. ",
        "here is a screenshot http://i.imgur.com/abc.png</p>"
    );
    let home = TempHome::new("link");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let out = te::translate_live(source, "ru_RU.UTF-8", &engine, &mut cache);

    assert!(out.was_translated, "перевод не случился — вкладка не покажет «оригинал»");
    assert_eq!(
        engine.seen(),
        vec![
            "hddtemp shows only the first digit.".to_string(),
            "here is a screenshot http://i.imgur.com/abc.png".to_string(),
        ],
        "движку досталось не то"
    );
    assert!(
        out.text.contains("перевод: hddtemp shows only the first digit."),
        "проза не переведена: {}",
        out.text
    );
    assert!(
        out.text.contains("перевод: here is a screenshot http://i.imgur.com/abc.png"),
        "фраза со ссылкой не переведена или ссылку тронули: {}",
        out.text
    );
}

/// Technical segments go to the output VERBATIM, and the tags are intact. A path and a link
/// INSIDE a phrase (since 03.10) — verbatim too, and the phrase around them is translated.
#[test]
fn technical_segments_and_tags_survive_verbatim() {
    let home = TempHome::new("verbatim");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let out = te::translate_live(DESCRIPTION, "ru_RU.UTF-8", &engine, &mut cache).text;

    for verbatim in [
        // Nothing is translated inside `<pre>`.
        "<pre>sudo apt-get install gtk2-engines-murrine</pre>",
        // A path inside a phrase: the engine's bare answer corrupted it («~/.темы»),
        // the second pass answer was taken, the path came back byte for byte.
        "<p>Перевод: Copy the folder to ~/.themes and pick it in Settings.</p>",
        // A link inside a phrase is intact.
        "<p>Перевод: Preview: https://example.invalid/shot.png</p>",
    ] {
        assert!(out.contains(verbatim), "пропал дословный кусок «{verbatim}»\n{out}");
    }
    assert!(!out.contains("~/.темы"), "в вывод пошёл испорченный путь\n{out}");
    // The markup is in place: as many tags as there were.
    for tag in ["<p>", "</p>", "<pre>", "</pre>"] {
        assert_eq!(
            out.matches(tag).count(),
            DESCRIPTION.matches(tag).count(),
            "число тегов «{tag}» изменилось\n{out}"
        );
    }
    // The translation landed in text nodes, not over the markup.
    assert!(out.starts_with("<p>Перевод: This global theme"), "начало вывода:\n{out}");
}

/// ⭐ The lowercase first letter the engine answers with is raised by the glue —
/// and raised according to the SOURCE.
#[test]
fn the_engine_lowercase_first_letter_is_lifted() {
    let home = TempHome::new("case");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let out = te::translate_live(
        "<p>This theme is warm. <b>Bold</b> and also readable.</p>",
        "ru_RU.UTF-8",
        &engine,
        &mut cache,
    )
    .text;

    // The engine answered «перевод: …» in lowercase; the output has «Перевод: …».
    assert!(out.contains("<p>Перевод: This theme is warm."), "не поднята заглавная:\n{out}");
    // The node "and also readable." continues the phrase after `<b>`, and in the source
    // it starts with a lowercase letter: it must not be raised.
    assert!(out.contains("перевод: and also readable."), "поднято лишнее:\n{out}");
}

/// A block is no longer than the limit: a long description gets several of them.
#[test]
fn a_long_description_is_split_into_blocks() {
    let home = TempHome::new("blocks");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);

    // Twenty DIFFERENT sentences in one paragraph (identical ones would merge into
    // one unique text).
    let mut text = String::from("<p>");
    for i in 0..20 {
        text.push_str(&format!("The theme number {i} repaints the whole desktop. "));
    }
    text.push_str("</p>");
    te::translate_live(&text, "ru_RU.UTF-8", &engine, &mut cache);

    assert_eq!(engine.block_sizes(), vec![MAX_BLOCK_SENTENCES, 20 - MAX_BLOCK_SENTENCES]);
}

/// The model from the PACKAGE is found by the same layout used to look for the vendor
/// QML of the wallpapers tab: `<data directory>/translate/<pair>` (in the developer
/// tree the data directory is `data/`). The files are made real (empty)
/// in a temporary directory — the search probe is live.
#[test]
fn a_shipped_model_is_found_inside_the_package() {
    let root = TempHome::new("package");
    let dir = format!("{}/{}/translate/enru", root.0, rexon_core::installer::DEV_TREE_DATA_DIR);
    std::fs::create_dir_all(&dir).expect("каталог модели");
    for name in ["model.enru.intgemm.alphas.bin", "vocab.enru.spm"] {
        std::fs::write(format!("{dir}/{name}"), b"").expect("файл модели");
    }
    let on_disk = |p: &str| std::path::Path::new(p).is_file();

    let found = te::pick_model(None, &root.0, "/нет-такого-дома", "en", "ru", &on_disk)
        .expect("модель в пакете обязана найтись");
    assert_eq!(found.model, format!("{dir}/model.enru.intgemm.alphas.bin"));
    assert_eq!(found.src_vocab, found.trg_vocab, "у пары общий словарь");

    // There is no other direction in the package — and that is not an error but "no translation".
    assert!(te::pick_model(None, &root.0, "/нет-такого-дома", "en", "de", &on_disk).is_none());
}

/// No model — no translation, and no error: the original and `was_translated = false`.
#[test]
fn without_a_model_the_original_comes_back_untouched() {
    let home = TempHome::new("nomodel");
    let engine = FakeEngine::missing();
    let mut cache = LiveCache::open(&home.0);
    let out = te::translate_live(DESCRIPTION, "ru_RU.UTF-8", &engine, &mut cache);

    assert_eq!(out.text, DESCRIPTION);
    assert!(!out.was_translated);
    assert_eq!(engine.calls(), 0, "движка нет, а его позвали");
}

/// The text is already in the user's language — the engine is not called (the core's decision).
#[test]
fn text_already_in_the_target_language_is_left_alone() {
    let home = TempHome::new("same");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let russian = "<p>Тёплая глобальная тема для всего рабочего стола.</p>";
    let out = te::translate_live(russian, "ru_RU.UTF-8", &engine, &mut cache);

    assert_eq!(out.text, russian);
    assert!(!out.was_translated);
    assert_eq!(engine.calls(), 0);
}

/// An empty description — an empty answer without a single call.
#[test]
fn empty_text_is_not_translated() {
    let home = TempHome::new("empty");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let out = te::translate_live("   ", "ru_RU.UTF-8", &engine, &mut cache);

    assert_eq!(out.text, "   ");
    assert!(!out.was_translated);
    assert_eq!(engine.calls(), 0);
}

// ============================================================================
// A batch of many short texts (a comments page)
// ============================================================================

/// ⭐ THE MAIN THING about the batch: twenty short comments go to the engine
/// in FULL blocks, not one by one.
///
/// This is the very reason [`te::translate_live_batch`] exists:
/// twenty separate `translate_live` calls would give twenty calls of one
/// sentence each — that is, the engine's batch path idling.
#[test]
fn a_page_of_short_comments_goes_to_the_engine_in_full_blocks() {
    let home = TempHome::new("comments");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);

    // Twenty DIFFERENT comments of one sentence each — as on a live
    // workshop page.
    let texts: Vec<String> = (0..20)
        .map(|i| format!("Thanks, this theme number {i} looks great on my desktop."))
        .collect();
    let asked: Vec<&str> = texts.iter().map(String::as_str).collect();
    let done = te::translate_live_batch(&asked, "ru_RU.UTF-8", &engine, &mut cache);

    assert_eq!(done.len(), asked.len(), "ответ обязан быть по тексту на текст");
    assert!(done.iter().all(|d| d.was_translated), "перевелись не все: {done:?}");
    assert_eq!(
        engine.block_sizes(),
        vec![MAX_BLOCK_SENTENCES, 20 - MAX_BLOCK_SENTENCES],
        "блоки набрались не поперёк текстов",
    );
    // The answer order is the question order: a translation did not slide onto another
    // comment.
    assert!(done[7].text.contains("theme number 7"), "съехал порядок: {:?}", done[7].text);
}

/// A Russian comment among English ones does not go to the engine at all and comes
/// back with `was_translated = false` — by this sign the tab does not draw
/// a "show original" link above it.
#[test]
fn a_russian_comment_in_the_batch_is_left_alone() {
    let home = TempHome::new("mixed");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);

    let russian = "Спасибо, тема отличная, поставил сразу.";
    let asked = vec!["Thanks, works like a charm.", russian, "   "];
    let done = te::translate_live_batch(&asked, "ru_RU.UTF-8", &engine, &mut cache);

    assert!(done[0].was_translated, "английский комментарий не перевёлся");
    assert_eq!(done[1].text, russian, "русский комментарий переписали");
    assert!(!done[1].was_translated, "у русского зажглась бы кнопка «оригинал»");
    assert!(!done[2].was_translated, "пустой комментарий «перевёлся»");
    let seen = engine.seen();
    assert!(
        !seen.iter().any(|s| s.contains("Спасибо")),
        "русский текст показали движку: {seen:?}",
    );
}

// ============================================================================
// The model in the package
// ============================================================================

/// The model ships IN THE PACKAGE, and it is put exactly where and under the names the core
/// looks for it.
///
/// A drifted path or name is the quietest failure possible: the engine
/// will say "no model", there will be no translation, and nothing will crash. So the path and
/// the names are taken FROM THE CORE rather than retyped into the check by hand.
#[test]
fn model_ships_where_core_looks_for_it() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // The PKGBUILD downloads the model, scripts/package-build installs it.
    let pkgbuild = std::fs::read_to_string(root.join("packaging/aur/PKGBUILD")).unwrap()
        + &std::fs::read_to_string(root.join("scripts/package-build")).unwrap();

    // The direction directory: `<data>/translate/<pair>`.
    let dir = format!("$data/{}/$mt_pair", te::MODEL_DIR_REL);
    assert!(pkgbuild.contains(&dir), "пакет кладёт модель не в «{dir}»");
    assert!(pkgbuild.contains("mt_pair=enru"), "в PKGBUILD нет направления en→ru");

    // The file names are those `model_files_in` looks for; in the PKGBUILD the pair is moved
    // into a variable, so the name pattern is checked.
    let want = te::model_files_in("", "en", "ru", &|_: &str| true).expect("имена файлов модели");
    for path in [want.model, want.src_vocab] {
        let name = path.rsplit('/').next().unwrap().to_string();
        let tmpl = name.replace(&te::model_pair("en", "ru"), "${mt_pair}");
        assert!(pkgbuild.contains(&tmpl), "пакет не кладёт файл модели {name}");
    }

    // The archives are verified by makepkg (sha256sums); gzip checks the unpacked
    // data with its own CRC, so the package gets exactly the measured edition.
    let sums = pkgbuild.split("sha256sums=(").nth(1).expect("sha256sums in PKGBUILD");
    assert!(
        !sums.trim_start().starts_with(')') && pkgbuild.contains("model.enru.intgemm.alphas.bin.gz::"),
        "PKGBUILD не сверяет архив модели"
    );
    assert!(
        pkgbuild.contains("LICENSE.translations-model-MPL-2.0"),
        "пакет не ставит лицензию модели (MPL-2.0)"
    );
    // There must be no promise of Python translation in the package: the Rust window calls
    // its own engine, not argostranslate.
    assert!(
        !pkgbuild.contains("argostranslate"),
        "в PKGBUILD осталось обещание перевода, которого окно не использует"
    );

    // A manual install (not as a package) copies the whole data directory, and the model
    // must come along with it — its names do not fall under the skip list.
    for name in [
        "translate",
        &te::model_pair("en", "ru"),
        "model.enru.intgemm.alphas.bin",
        "vocab.enru.spm",
    ] {
        assert!(
            !rexon_core::installer_exec::payload_skip(name),
            "ручная установка пропустит «{name}» — модель не доедет"
        );
    }
}

/// ⭐ The ITEM TITLE passed by the caller comes to the output VERBATIM — with the same
/// technique as technology names: if the engine translated it in the first pass, the
/// sentence goes through the second pass under a placeholder.
///
/// Live material: "Nordic Kvantum theme" was returned by the engine as «Скандинавская
/// Kvantum тема», "Sweet Kvantum theme" as «Сладкая Kvantum тема» (runs of
/// 21.09). The core has no list of theme titles, but the window knows the title.
#[test]
fn the_item_name_from_the_caller_comes_back_verbatim() {
    for (name, source) in [
        ("Nordic", "<p>Nordic Kvantum theme for the whole desktop.</p>"),
        ("Sweet Kvantum", "<p>Sweet Kvantum theme for the whole desktop.</p>"),
        ("NT Legacy", "<p>NT Legacy brings the old look back to the desktop.</p>"),
    ] {
        let home = TempHome::new("item-name");
        let engine = FakeEngine::enru();
        let mut cache = LiveCache::open(&home.0);
        let out = te::translate_live_guarded(source, "ru_RU.UTF-8", &engine, &mut cache, &[name]);
        assert!(out.was_translated, "перевод не случился на {source:?}");
        assert!(out.text.contains(name), "название {name:?} потерялось: {}", out.text);
        assert!(!out.text.contains('@'), "заполнитель уехал в вывод: {}", out.text);
        // The translation of the title did not get into the output: "Nordic" did not become
        // «скандинавской», and "Sweet Kvantum" did not become «сладкой квантской».
        for mangled in ["скандинавская", "квантская"] {
            assert!(
                !out.text.contains(mangled),
                "название переведено («{mangled}»): {}",
                out.text
            );
        }
    }
}

/// The author's name is hidden on a par with the title, and empty and very short names in
/// the list are dropped by the core itself — the caller has nothing to check.
#[test]
fn the_caller_may_pass_several_names_and_empty_ones() {
    let home = TempHome::new("author");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let out = te::translate_live_guarded(
        "<p>Sweet Kvantum theme made by Rtlewis for every desktop.</p>",
        "ru_RU.UTF-8",
        &engine,
        &mut cache,
        &["Sweet Kvantum", "Rtlewis", "", "A"],
    );
    assert!(out.was_translated);
    for name in ["Sweet Kvantum", "Rtlewis"] {
        assert!(out.text.contains(name), "имя {name} потерялось: {}", out.text);
    }
    assert!(!out.text.contains('@'), "заполнитель остался в выводе: {}", out.text);
}

/// The old contract is intact: [`te::translate_live`] without guarded names works exactly
/// as before the fix — the window needs to change nothing where it has no title.
#[test]
fn the_call_without_guarded_names_behaves_as_before() {
    let source = "<p>Nordic Kvantum theme for the whole desktop.</p>";
    let home = TempHome::new("plain");
    let plain_engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let plain = te::translate_live(source, "ru_RU.UTF-8", &plain_engine, &mut cache);

    let home = TempHome::new("empty-list");
    let empty_engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let empty = te::translate_live_guarded(source, "ru_RU.UTF-8", &empty_engine, &mut cache, &[]);

    assert_eq!(plain.text, empty.text, "пустой список имён изменил перевод");
    assert_eq!(plain_engine.seen(), empty_engine.seen(), "пустой список изменил вопрос движку");
    // Without guarding, the title goes to the engine — exactly what the fix cures.
    assert!(
        plain_engine.seen().iter().any(|s| s.contains("Nordic")),
        "сверка ни о чём: движок не видел названия и без охраны"
    );
}

/// Placeholder corruption on a guarded title is judged just as strictly: the segment
/// is returned as the ORIGINAL and not written to the cache.
#[test]
fn a_spoiled_item_name_returns_the_original_segment() {
    let source = "<p>This theme is calm and quiet. Nordic brings the Nord palette.</p>";
    for spoil in [Spoil::Drop, Spoil::Double, Spoil::Crumb] {
        let home = TempHome::new("item-lost");
        let engine = FakeEngine::spoiling(spoil);
        let mut cache = LiveCache::open(&home.0);
        let out =
            te::translate_live_guarded(source, "ru_RU.UTF-8", &engine, &mut cache, &["Nordic"]);
        assert!(engine.calls() >= 1, "движок не позвали — сверка ни о чём");
        assert_eq!(out.text, source, "в вывод пошла каша вместо оригинала");
        assert!(!out.was_translated, "испорченный перевод объявлен переводом");

        let again = FakeEngine::spoiling(spoil);
        let mut cache = LiveCache::open(&home.0);
        let second =
            te::translate_live_guarded(source, "ru_RU.UTF-8", &again, &mut cache, &["Nordic"]);
        assert!(again.calls() >= 1, "порча попала в кеш");
        assert_eq!(second.text, source);
    }
}

// ============================================================================
// Live check on the real model
// ============================================================================

/// The model directory for the live check: the stand one by default (the model is not put
/// into the tree, its place is the package).
/// ⭐ Owner's defect 03.10 (KDE Store, "ZorinOS 6 Sound Theme"): a phrase with
/// a path inside is translated, the path comes back byte for byte — even when the engine's bare
/// answer corrupted the path (the second pass, the path under a placeholder).
#[test]
fn a_sentence_with_a_path_inside_is_translated_and_the_path_is_kept() {
    let source = concat!(
        "<p>Sound theme from ZorinOS 6. ",
        "Just unzip the file and put in the /usr/share/sounds folder to add the to your desktop.</p>"
    );
    let home = TempHome::new("path-inside");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let out = te::translate_live(source, "ru_RU.UTF-8", &engine, &mut cache);

    assert!(out.was_translated);
    assert!(
        out.text.contains(
            "Перевод: Just unzip the file and put in the /usr/share/sounds folder to add the to your desktop."
        ),
        "фраза с путём не переведена или путь испорчен: {}",
        out.text
    );
    assert!(!out.text.contains("звуки"), "в вывод пошёл испорченный путь: {}", out.text);
    assert!(
        engine.seen().iter().any(|s| s.contains("the @0@ folder")),
        "второго прохода с путём под заполнителем не было: {:?}",
        engine.seen()
    );
}

/// The engine lost the path mark (ate, multiplied, nibbled it) — the output gets the
/// ORIGINAL, not a translation with a hole where the path was.
#[test]
fn a_lost_path_placeholder_returns_the_original() {
    let source = "<p>Just unzip the file and put in the /usr/share/sounds folder.</p>";
    for spoil in [Spoil::Drop, Spoil::Double, Spoil::Crumb] {
        let home = TempHome::new("path-lost");
        let engine = FakeEngine::spoiling(spoil);
        let mut cache = LiveCache::open(&home.0);
        let out = te::translate_live(source, "ru_RU.UTF-8", &engine, &mut cache);
        assert!(engine.calls() >= 2, "второго прохода не было — сверка ни о чём");
        assert_eq!(out.text, source, "в вывод пошла порча вместо оригинала");
    }
}

/// ⛔ A command line and a line consisting of one path still do not reach the engine.
#[test]
fn pure_commands_never_reach_the_engine() {
    let source = concat!(
        "<p>cp -r Nordic ~/.themes</p>",
        "<p>sudo pacman -S kvantum</p>",
        "<p>/usr/share/sounds</p>",
        "<p>./install.sh --user</p>"
    );
    let home = TempHome::new("commands");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let out = te::translate_live(source, "ru_RU.UTF-8", &engine, &mut cache);
    assert_eq!(engine.calls(), 0, "движку показали команду: {:?}", engine.seen());
    assert_eq!(out.text, source);
}

/// The term correction dictionary sits on the engine path: «расстегните» from `unzip`
/// reaches the customer as «распакуйте», technical bits and case intact.
#[test]
fn a_term_slip_of_the_engine_is_fixed_on_the_way_out() {
    let home = TempHome::new("term-fix");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let source = "Unzip the file into ~/.themes now.";
    let out = te::translate_live(source, "ru_RU.UTF-8", &engine, &mut cache);
    assert!(out.text.contains("распакуйте"), "термин не поправлен: {}", out.text);
    assert!(!out.text.contains("расстегните"), "термин не поправлен: {}", out.text);
    assert!(out.text.contains("~/.themes"), "техника потеряна: {}", out.text);
    assert!(out.text.starts_with('П'), "регистр первой буквы: {}", out.text);
}

/// The stand model directory is in the home directory of whoever runs the check.
fn stand_model_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/file-verify/mt-bench/models/firefox-enru")
}

/// Live en→ru translation of a paragraph: the real model, the real cache, the whole glue.
///
/// `#[ignore]` on purpose — a downloaded model (43 MB) is needed. Run:
///
/// ```sh
/// cargo test -p rexon-core --release --test translator_engine \
///     -- --ignored --nocapture live_paragraph
/// ```
///
/// The model directory is `REXON_TRANSLATE_MODEL_DIR` (the same variable
/// the vendor watchdog uses).
#[test]
#[ignore = "needs the downloaded Firefox en→ru model (REXON_TRANSLATE_MODEL_DIR), not kept in the tree"]
fn live_paragraph_is_translated_by_the_real_engine() {
    if std::env::var_os(te::MODEL_DIR_ENV).is_none() {
        std::env::set_var(te::MODEL_DIR_ENV, stand_model_dir());
    }
    let dir = std::env::var(te::MODEL_DIR_ENV).unwrap_or_default();
    let files = te::model_files_in(&dir, "en", "ru", &|p: &str| {
        std::path::Path::new(p).is_file()
    });
    assert!(files.is_some(), "модели нет в «{dir}» — задайте {}", te::MODEL_DIR_ENV);

    let home = TempHome::new("live");
    let engine = te::LiveEngine::new();
    assert!(engine.available("en", "ru"), "модель не загрузилась из «{dir}»");

    let source = concat!(
        "<p>This global theme brings a warm, muted palette to the whole desktop. ",
        "It ships matching Plasma, Aurorae and Kvantum styles.</p>",
        "<p>Installation:</p>",
        "<pre>sudo apt-get install gtk2-engines-murrine</pre>",
        "<p>Copy the folder to ~/.themes and pick it in Settings.</p>"
    );

    let mut cache = LiveCache::open(&home.0);
    let started = std::time::Instant::now();
    let cold = te::translate_live(source, "ru_RU.UTF-8", &engine, &mut cache);
    let cold_secs = started.elapsed().as_secs_f64();

    // The second run comes from the cache, the engine is silent.
    let mut cache = LiveCache::open(&home.0);
    let started = std::time::Instant::now();
    let warm = te::translate_live(source, "ru_RU.UTF-8", &engine, &mut cache);
    let warm_secs = started.elapsed().as_secs_f64();

    println!("ядро умножения   : {}", fxtranslate::gemm::backend());
    println!("источник         : {source}");
    println!("перевод ({cold_secs:.3} с) : {}", cold.text);
    println!("из кеша ({warm_secs:.3} с) : {}", warm.text);

    assert!(cold.was_translated, "перевод не случился");
    assert_eq!(warm.text, cold.text, "из кеша пришёл другой текст");
    assert!(
        cold.text.chars().any(|c| ('а'..='я').contains(&c)),
        "в переводе нет русских букв: {}",
        cold.text
    );
    // Technical bits intact, markup intact.
    assert!(
        cold.text.contains("<pre>sudo apt-get install gtk2-engines-murrine</pre>"),
        "команду тронули: {}",
        cold.text
    );
    assert!(cold.text.contains("~/.themes"), "путь тронули: {}", cold.text);
    assert_eq!(
        cold.text.matches("<p>").count(),
        source.matches("<p>").count(),
        "теги поехали: {}",
        cold.text
    );
    // The first letter is a capital (case fix on the engine's live answer).
    // ⚠️ Taking "the first letter of the output" is wrong: it would be the `p` of `<p>`. The letter
    // is looked for in the FIRST text node, that is after the first closed tag.
    let first_node = cold.text.trim_start_matches(|c: char| c != '>').trim_start_matches('>');
    let first_letter = first_node.chars().find(|c| c.is_alphabetic()).unwrap_or(' ');
    assert!(first_letter.is_uppercase(), "первая буква строчная: {}", cold.text);
}

/// Live check of NAME PROTECTION on the real model: the same phrase is first
/// handed to the engine bare (what it does with the name by itself), then goes the full path
/// through [`te::translate_live`] (the name under a placeholder).
///
/// Run — as for the check above:
///
/// ```sh
/// REXON_TRANSLATE_MODEL_DIR=$HOME/file-verify/mt-bench/models/firefox-enru \
///     cargo test -p rexon-core --release --test translator_engine \
///     -- --ignored --nocapture live_names
/// ```
///
/// Measurement 21.09 on the Firefox `en→ru` model (printed by the run):
///   * bare engine `A Kvantum theme heavily based on Microsoft's Fluent
///     Design elements.` → «квантская тема в значительной степени основана на
///     элементах Microsoft Fluent Design.» — the name translated and inflected;
///   * full path → «Kvantum тема, в значительной степени основанная на
///     элементах Microsoft Fluent Design.» — the name in Latin letters;
///   * "Copy the Papirus folder and enable it in Dolphin." bare engine —
///     «спровочивайте папирусную папирус …», full path — «Скройте папку
///     Papirus и включите ее в Dolphin.».
///
/// Measurement 23.09 (A LOST WORD, the item "Iridescent"): "A colorful theme inspired
/// by sweet kde theme." bare engine — «красочная тема, вдохновленная сладкой
/// темой.» (the word `kde` is NOT in the translation), full path — «Красочная тема,
/// вдохновленная сладкой kde темой.».
#[test]
#[ignore = "needs the downloaded Firefox en→ru model (REXON_TRANSLATE_MODEL_DIR), not kept in the tree"]
fn live_names_survive_the_real_engine() {
    if std::env::var_os(te::MODEL_DIR_ENV).is_none() {
        std::env::set_var(te::MODEL_DIR_ENV, stand_model_dir());
    }
    let engine = te::LiveEngine::new();
    assert!(engine.available("en", "ru"), "модель не загрузилась");

    // Live phrases: sample 9 ("Fluent Dark"), sample 5 (Sweet-Mars),
    // sample 6 (Nordic KDE) and a compound case with three names.
    let phrases = [
        "A Kvantum theme heavily based on Microsoft's Fluent Design elements.",
        "Sweet Kvantum theme",
        "Nordic Kvantum theme",
        "This theme is based on Cinnamon and works with GNOME and KDE Plasma.",
        "Copy the Papirus folder and enable it in Dolphin.",
        // ⭐ A LOST WORD (the item "Iridescent", acceptance 23.09): the lowercase `kde`
        // was dropped by the model from the translation entirely — «красочная тема,
        // вдохновленная сладкой темой.». Abbreviations from the list are protected in
        // any case, and the word returns to its place.
        "A colorful theme inspired by sweet kde theme.",
    ];

    // BEFORE: the engine on a bare phrase, without substitution.
    let bare = engine.translate_block("en", "ru", &phrases);
    // AFTER: the core's full path (prose filter + name substitution + cache).
    let home = TempHome::new("live-names");
    let mut cache = LiveCache::open(&home.0);
    let guarded: Vec<String> = phrases
        .iter()
        .map(|p| te::translate_live(p, "ru_RU.UTF-8", &engine, &mut cache).text)
        .collect();

    for ((source, before), after) in phrases.iter().zip(bare.iter()).zip(guarded.iter()) {
        println!("источник : {source}");
        println!("  до     : {before}");
        println!("  после  : {after}");
        assert!(!after.contains('@'), "заполнитель уехал в вывод: {after}");
        // Every name from the phrase stands in the output verbatim.
        for name in ["Kvantum", "Cinnamon", "GNOME", "KDE Plasma", "Papirus", "Dolphin", "kde"] {
            if source.contains(name) {
                assert!(after.contains(name), "имя {name} потерялось: {after}");
            }
        }
    }
    // At least one phrase is really translated (and not returned as the original).
    assert!(
        guarded.iter().zip(phrases.iter()).any(|(a, s)| a != s
            && a.chars().any(|c| ('а'..='я').contains(&c))),
        "ни одна фраза не переведена: {guarded:?}"
    );
}

/// Live check of TECHNICAL BITS INSIDE PROSE on the real model (owner's defect
/// 03.10, "ZorinOS 6 Sound Theme"): phrases from live KDE Store descriptions (OCS API,
/// categories "Sounds" 316, "Cursors" 107, "Icons" 132, "GTK" 135, "SDDM" 101,
/// "Kvantum" 123, "Global" 722) — a path, a file name, `code` inside prose.
///
/// Run — as for the checks above:
///
/// ```sh
/// cargo test -p rexon-core --release --test translator_engine \
///     -- --ignored --nocapture live_technique
/// ```
#[test]
#[ignore = "needs the downloaded Firefox en→ru model (REXON_TRANSLATE_MODEL_DIR), not kept in the tree"]
fn live_technique_inside_prose_survives_the_real_engine() {
    if std::env::var_os(te::MODEL_DIR_ENV).is_none() {
        std::env::set_var(te::MODEL_DIR_ENV, stand_model_dir());
    }
    let engine = te::LiveEngine::new();
    assert!(engine.available("en", "ru"), "модель не загрузилась");

    let prose = [
        "Just unzip the file and put in the /usr/share/sounds folder to add the to your desktop.",
        "Enter 'Enchanted' as the folder name of your sound theme which you copied to /usr/share/sounds directory.",
        "Installation: Open the ZIP file and extract the folder \"MacOS Sounds\" to ~/.local/share/sounds",
        "Download it to a folder in your /home partition, or, if you want to use it system-wide, /usr/share/sounds is the place.",
        "Extract the 11X.tar.xz file and copy the 11X folder;",
        "Download bigsur-sound-theme.tar.gz from Files",
        "Extract the archive into ~/.local/share/sounds.",
        "Unpack into /usr/share/icons/ and it should become available in your mouse settings manager.",
        "Download the `master.zip` package and extract it.",
        "Move all the folders inside the extracted archive to ~/.themes.",
        "To change the inherit icons, you can change the index.theme file in the icons directory of each theme.",
        "Most common ones are /usr/lib/sddm/themes and /usr/share/sddm/themes.",
        "To install place sound theme in /usr/share/sounds folder.",
        "Unzip the file and use root to move to the /usr/share/themes folder",
        "After installing Kvantum Manager, download and extract \"maliceandspitekv.zip\" from this page",
    ];
    let commands = [
        "Copy: cp -r bigsur ~/.local/share/sounds/",
        "sudo cp -r Quintom_Ink /usr/share/icons",
        "Use the following command: kpackagetool6 -i Alba.P6.tar.xz",
    ];

    let bare = engine.translate_block("en", "ru", &prose);
    let home = TempHome::new("live-technique");
    let mut cache = LiveCache::open(&home.0);
    let mut translated = 0usize;
    for (source, before) in prose.iter().zip(bare.iter()) {
        let after = te::translate_live(source, "ru_RU.UTF-8", &engine, &mut cache).text;
        println!("источник : {source}");
        println!("  до     : {before}");
        println!("  после  : {after}");
        assert!(!after.contains('@'), "заполнитель уехал в вывод: {after}");
        for (a, b) in rexon_core::translator::technical_spans(source) {
            let piece = &source[a..b];
            assert!(after.contains(piece), "техника «{piece}» не дословно: {after}");
        }
        if after != *source && after.chars().any(|c| ('а'..='я').contains(&c)) {
            translated += 1;
        }
    }
    for source in commands {
        let after = te::translate_live(source, "ru_RU.UTF-8", &engine, &mut cache).text;
        println!("команда  : {source}\n  после  : {after}");
        assert_eq!(after, source, "команду тронули");
    }
    println!("переведено фраз: {translated} из {}", prose.len());
    assert!(translated * 3 >= prose.len() * 2, "переведено меньше двух третей: {translated}");
}

/// Live check of ITEM TITLE PROTECTION on the real model: the same phrase first
/// through the bare engine (what it does with the title by itself), then the full path through
/// [`te::translate_live_guarded`] with the title guarded.
///
/// Run — as for the checks above:
///
/// ```sh
/// REXON_TRANSLATE_MODEL_DIR=$HOME/file-verify/mt-bench/models/firefox-enru \
///     cargo test -p rexon-core --release --test translator_engine \
///     -- --ignored --nocapture live_item_name
/// ```
///
/// Measurement 21.09 on the Firefox `en→ru` model, VERBATIM as the run prints it
/// (on the left the item title, then "before" — the bare engine, "after" — the full path):
///   * `Sweet Kvantum theme` (Sweet): before — «сладкий Kvantum тема», after —
///     «Sweet Kvantum тема»;
///
/// ⚠️ A CAVEAT about short captions (measurement 24.09, after switching to two
/// passes): `Nordic Kvantum theme` and `Nordic Plasma theme` are now taken from the
/// FIRST, bare pass — the name in the answer is intact — and together with it the output
/// gets «Также», which the engine appends to a caption without a verb («Также
/// тема Nordic Kvantum»). Previously the substitution happened to clean up this word, giving
/// «Nordic Kvantum тема». We tolerate it deliberately: the same bare pass also cures
/// «Теммы» from the 24.09 acceptance (`live_acceptance_defects_of_24_09_are_fixed`), and
/// we have nothing to choose between the two answers with — both are formally intact.
///   * `Sweet Mars is a dark global theme with red accents.` (Sweet Mars):
///     before — «сладкий Марс - темная глобальная тема с красными акцентами.»,
///     after — «Sweet Mars - это темная глобальная тема с красными акцентами.»;
///   * `Nordic Kvantum theme based on the Nord palette.` (Nordic): before — «также
///     тема Nordic Kvantum основана на палитре Севера.», after — «Nordic
///     Kvantum тема на основе Nord палитры.»;
///   * `NT Legacy is a global theme for Plasma.` (NT Legacy): before — «NT Legacy
///     является глобальной темой для плазмы.», after — «NT Legacy является
///     глобальной темой для Plasma.»;
///   * `Install Layan and enjoy the rounded look of the desktop.` (Layan):
///     before — «установите Layan и наслаждайтесь округлым видом рабочего стола.»,
///     after — «Установите Layan и наслаждайтесь округлым видом рабочего
///     стола.»;
///
/// Measurement 23.09, COMPOUND titles (verbatim as the run prints it):
///   * `Nordic is a dark theme created using the awesome Nord color palette.`
///     (Nordic KDE): before — «и скандинавская - это темная тема, созданная с
///     использованием удивительной цветовой палитры Nord.», after — «Nordic -
///     это темная тема, созданная с использованием потрясающей Nord цветовой
///     палитры.»;
///   * `Nordic Plasma theme`: before — «также тема Nordic Plasma», after — «Также
///     тема Nordic Plasma» (see the caveat about short captions below);
///   * `Nordic color scheme`: before — «скандинавская цветовая схема», after —
///     «Nordic цветовая схема»;
///   * `Sweet-Mars Kvantum theme` (Sweet Mars LAF Plasma 6): before — «сладкий
///     марс квантум тема», after — «Sweet-Mars Kvantum тема»;
///   * `Sweet Plasma theme`: before — «сладкая плазменная тема», after — «Sweet
///     Plasma тема»;
///   * `Sweet wallpaper`: before — «сладкие обои», after — «Sweet обои».
///
///   * ⛔ the flip side for an item whose name is a dictionary word: `A dark and
///     quiet theme for everyone` (Dark) — before «темная и тихая тема для всех»,
///     after «Темная и тихая тема для всех»: the lowercase `dark` does NOT count as a title
///     and is translated, as it should be.
#[test]
#[ignore = "needs the downloaded Firefox en→ru model (REXON_TRANSLATE_MODEL_DIR), not kept in the tree"]
fn live_item_name_survives_the_real_engine() {
    if std::env::var_os(te::MODEL_DIR_ENV).is_none() {
        std::env::set_var(te::MODEL_DIR_ENV, stand_model_dir());
    }
    let engine = te::LiveEngine::new();
    assert!(engine.available("en", "ru"), "модель не загрузилась");

    // The titles are live (samples 5, 6 and workshop item captions), the last
    // line is a single-word dictionary title.
    // The third column is the word that MUST stay in Latin letters, or `None`
    // for the opposite case (a lowercase dictionary title is translated).
    let cases = [
        ("Nordic", "Nordic Kvantum theme", Some("Nordic")),
        ("Sweet", "Sweet Kvantum theme", Some("Sweet")),
        ("NT Legacy", "NT Legacy is a global theme for Plasma.", Some("NT Legacy")),
        ("Layan", "Install Layan and enjoy the rounded look of the desktop.", Some("Layan")),
        ("Sweet Mars", "Sweet Mars is a dark global theme with red accents.", Some("Sweet Mars")),
        ("Nordic", "Nordic Kvantum theme based on the Nord palette.", Some("Nordic")),
        ("Dark", "A dark and quiet theme for everyone", None),
        // ⭐ Acceptance 23.09, COMPOUND titles: lines verbatim from the descriptions
        // of the live items "Nordic KDE" (eliverlara) and "Sweet Mars LAF Plasma 6".
        // None of them contains the whole chain of title words.
        (
            "Nordic KDE",
            "Nordic is a dark theme created using the awesome Nord color palette.",
            Some("Nordic"),
        ),
        ("Nordic KDE", "Nordic Plasma theme", Some("Nordic Plasma")),
        ("Nordic KDE", "Nordic Kvantum theme", Some("Nordic Kvantum")),
        ("Nordic KDE", "Nordic color scheme", Some("Nordic")),
        ("Sweet Mars LAF Plasma 6", "Sweet-Mars Kvantum theme", Some("Sweet-Mars Kvantum")),
        ("Sweet Mars LAF Plasma 6", "Sweet Plasma theme", Some("Sweet Plasma")),
        ("Sweet Mars LAF Plasma 6", "Sweet wallpaper", Some("Sweet")),
    ];

    // BEFORE: the engine on a bare phrase, without any substitution. A block of ONE phrase — the
    // same block size the full path asks it with, otherwise "before" and "after"
    // would be compared under different conditions (block neighbors change the answer).
    let bare: Vec<String> = cases
        .iter()
        .map(|(_, text, _)| {
            engine.translate_block("en", "ru", &[text]).pop().unwrap_or_default()
        })
        .collect();

    let home = TempHome::new("live-item-name");
    let mut cache = LiveCache::open(&home.0);
    for ((name, source, must_survive), before) in cases.iter().zip(bare.iter()) {
        let after =
            te::translate_live_guarded(source, "ru_RU.UTF-8", &engine, &mut cache, &[name]).text;
        println!("название : {name}");
        println!("источник : {source}");
        println!("  до     : {before}");
        println!("  после  : {after}");
        assert!(!after.contains('@'), "заполнитель уехал в вывод: {after}");
        if let Some(word) = must_survive {
            assert!(after.contains(word), "название {word:?} потерялось: {after}");
            assert!(
                after.chars().any(|c| ('а'..='я').contains(&c)),
                "фраза не переведена вовсе: {after}"
            );
        } else {
            // A dictionary title in lowercase is an ordinary word, and it is
            // TRANSLATED (otherwise the rule would eat prose).
            assert!(!after.contains("dark"), "слово `dark` осталось латиницей: {after}");
        }
    }
}

/// ⭐ FOUR DEFECTS OF THE 24.09 LIVE ACCEPTANCE on the real model: verbatim lines
/// of workshop items and comments by which the customer saw "the translation
/// did not arrive".
///
/// Run — as for the checks above:
///
/// ```sh
/// REXON_TRANSLATE_MODEL_DIR=/usr/lib/rexon/rexon/translate/enru \
///     cargo test -p rexon-core --release --test translator_engine \
///     -- --ignored --nocapture live_acceptance_defects
/// ```
///
/// What was found (measurement 24.09, verbatim as the run prints it):
///   * **D4a, viz67's comment on "Nordic KDE"** — "Very nice. / Can we expect
///     soon the Nordic Bluish theme of Gnome for KDE too?". The culprit was NOT
///     the prose filter (it considers both phrases prose) but placeholder corruption:
///     to "Can we expect soon the @0@ Bluish theme of @1@ for @2@ too?" the model
///     answers «…@0@ Bluish тему @1@ @1@ для @2@ тоже?» — `@1@` as a glued
///     repeat, the sieve rejected the translation, and the WHOLE comment stayed
///     in English, including "Very nice.". BEFORE: "Very nice.\nCan we expect soon
///     the Nordic Bluish theme of Gnome for KDE too?" (translated=false);
///     AFTER: «Очень мило.\nМожем ли мы ожидать в ближайшее время Nordic Bluish
///     тему Gnome для KDE тоже?».
///   * **D4b, maxkaspar's comment on "Awesome Widgets"** — rule 4 of the filter
///     ("a significant word is technical in form") judged the WHOLE node by the link in
///     the second sentence. BEFORE: "hddtemp shows only the first digit. here is a
///     screenshot http://…" (translated=false); AFTER: «интрампература
///     показывает только первую цифру. here is a screenshot http://…» — the prose
///     is translated, the link verbatim. ⛔ `hddtemp` → «интрампература» is
///     a REMAINING model defect: this word is not in the name list and cannot be,
///     and the bare engine mangles it the same way («темп показывает только
///     первую цифру.» on the whole comment).
///   * **D1, the shared paragraph of the author of "Nordic KDE" and "Sweet Mars"** — «Теммы» with
///     an extra letter was made by OUR placeholder, not the model: "becoming a supporter
///     for the KDE Themes and Extensions section" through the bare engine — «стать
///     сторонником раздела KDE «Темы и расширения»» (correct!), the same phrase with
///     `@0@` in place of `KDE` — «стать сторонником раздела @0@ «Теммы и
///     расширения»». AFTER (substitution is spent only where the name gets lost):
///     «стать сторонником раздела KDE «Темы и расширения»».
///   * **D2, a description line of "Awesome Widgets"** — "graphical item support -
///     tooltips, bars" is returned by the model as «служба графических элементов -
///     toolbep, bars». There are no placeholders in the line at all (it has no names), and
///     `toolbep` is not a mix of alphabets but a Latin word made up by the model
///     (codes: `U+0074 U+006F U+006F U+006C U+0062 U+0065 U+0070`). We cannot
///     cure it, but it is caught by comparing with the source: AFTER, the line
///     stays the original rather than a mush.
///
/// ⛔ **D3 stays entirely with the engine**: "Nordic is a dark theme created using
/// the awesome <a …>Nord</a> color palette." — `Nord` is inside a link,
/// so there are three nodes, and the tail " color palette." is translated by the model on its own as
/// «цветовая палитра цветов» (the same tautology from the bare engine on the same
/// fragment). Our placeholders have nothing to do with it — there are no names in the tail.
#[test]
#[ignore = "needs the downloaded Firefox en→ru model (REXON_TRANSLATE_MODEL_DIR), not kept in the tree"]
fn live_acceptance_defects_of_24_09_are_fixed() {
    if std::env::var_os(te::MODEL_DIR_ENV).is_none() {
        std::env::set_var(te::MODEL_DIR_ENV, stand_model_dir());
    }
    let engine = te::LiveEngine::new();
    assert!(engine.available("en", "ru"), "модель не загрузилась");

    // (item, author, text) — verbatim from the acceptance.
    let nordic = ("Nordic KDE", "eliverlara");
    let widgets = ("Awesome Widgets", "arcanis");
    let cases = [
        (
            nordic,
            "Very nice.\nCan we expect soon the Nordic Bluish theme of Gnome for KDE too?",
        ),
        (
            widgets,
            "hddtemp shows only the first digit. here is a screenshot http://i.imgur.com/abc.png",
        ),
        (nordic, " becoming a supporter for the  KDE Themes and Extensions section "),
        (widgets, " graphical item support - tooltips, bars"),
    ];

    let home = TempHome::new("live-defects");
    let mut cache = LiveCache::open(&home.0);
    let mut out: Vec<String> = Vec::new();
    for ((name, author), source) in cases.iter() {
        let bare = engine.translate_block("en", "ru", &[source]).pop().unwrap_or_default();
        let done =
            te::translate_live_guarded(source, "ru_RU.UTF-8", &engine, &mut cache, &[name, author]);
        println!("источник     : {source:?}");
        println!("  голый      : {bare:?}");
        println!("  полный путь: переведено={} {:?}", done.was_translated, done.text);
        assert!(!done.text.contains('@'), "заполнитель уехал в вывод: {}", done.text);
        out.push(done.text);
    }

    // D4a: BOTH sentences of the comment are translated, names in Latin letters.
    let comment = &out[0];
    assert!(comment.starts_with("Очень"), "первая фраза осталась английской: {comment}");
    assert!(!comment.contains("Can we expect"), "вторая фраза осталась английской: {comment}");
    for name in ["Nordic", "Gnome", "KDE"] {
        assert!(comment.contains(name), "имя {name} потерялось: {comment}");
    }

    // D4b: the prose is translated, the link verbatim.
    let screenshot = &out[1];
    assert!(
        screenshot.contains("http://i.imgur.com/abc.png"),
        "ссылку тронули: {screenshot}"
    );
    assert!(
        screenshot.chars().any(|c| ('а'..='я').contains(&c)),
        "проза рядом со ссылкой так и не переведена: {screenshot}"
    );

    // D1: no extra letter in «Темы», the name in Latin letters.
    let supporter = &out[2];
    assert!(!supporter.contains("Теммы"), "лишняя буква на месте: {supporter}");
    assert!(supporter.contains("Темы"), "раздел назван не по-русски: {supporter}");
    assert!(supporter.contains("KDE"), "имя KDE потерялось: {supporter}");

    // D2: the engine's made-up word did not get into the output — the line stayed the original.
    let tooltips = &out[3];
    assert!(!tooltips.contains("toolbep"), "каша движка уехала покупателю: {tooltips}");
    assert_eq!(
        tooltips.trim(),
        "graphical item support - tooltips, bars",
        "ожидался честный оригинал: {tooltips}"
    );
}

/// Live check of the TERM CORRECTION DICTIONARY on the real model: 18 phrases
/// of KDE Store descriptions with unzip/unpack/extract/download (OCS cache, 03.10) and two
/// control ones where `extract` means «извлечь».
///
/// Run — as for the checks above:
///
/// ```sh
/// REXON_TRANSLATE_MODEL_DIR=$HOME/file-verify/mt-bench/models/firefox-enru \
///     cargo test -p rexon-core --test translator_engine \
///     -- --ignored --nocapture live_term_fixes
/// ```
///
/// What it guards: (1) the output has no form left that the dictionary
/// can correct (the output is a fixed point of [`apply_term_fixes`]);
/// (2) «распак…» is present in all archive phrases where the engine gave a verb at all;
/// (3) the dictionary did not touch the control phrases — «извлеч…» is in place; (4) technical bits
/// are intact, there are no placeholders in the output.
#[test]
#[ignore = "needs the downloaded Firefox en→ru model (REXON_TRANSLATE_MODEL_DIR), not kept in the tree"]
fn live_term_fixes_survive_the_real_engine() {
    use rexon_core::translator::{apply_term_fixes, technical_spans};
    if std::env::var_os(te::MODEL_DIR_ENV).is_none() {
        std::env::set_var(te::MODEL_DIR_ENV, stand_model_dir());
    }
    let engine = te::LiveEngine::new();
    assert!(engine.available("en", "ru"), "модель не загрузилась");

    let archive = [
        "Just unzip the file and put in the /usr/share/sounds folder to add the to your desktop.",
        "Unzip the file and use root to move to the /usr/share/themes folder",
        "Download bigsur-sound-theme.tar.gz from Files",
        "Installation: Open the ZIP file and extract the folder \"MacOS Sounds\" to ~/.local/share/sounds",
        "Extract the archive into ~/.local/share/sounds.",
        "After installing Kvantum Manager, download and extract \"maliceandspitekv.zip\" from this page",
        "Download the `master.zip` package and extract it.",
        "Move all the folders inside the extracted archive to ~/.themes.",
        "Unpack into /usr/share/icons/ and it should become available in your mouse settings manager.",
        "To install download the zip file and extract it and put the contents in",
        "Download and extract the .tar.gz file",
        "Just extract it to ~/.local/share/plasma/look-and-feel/",
        "Download the tar archive from this site and extract the contents to the theme directory of SDDM",
        "Download the archive, extract it, run ./install.sh, and then add the oneOff widget to your Plasma panel.",
        "Then extract your downloaded file into",
        "After extracting the folder, you can apply the desktop theme via KDE System Settings by going to",
        "To install, just extract the archive and copy each folder to ~/.kde/share/apps/aurorae/themes/",
        "Extract the 11X.tar.xz file and copy the 11X folder;",
    ];
    let control = [
        "All sounds have been extracted from MacOS Monterey and converted to ogg using ffmpeg.",
        "~Dynamic Accent Colors: Automatically extracts and applies the dominant color from your wallpaper to UI elements.",
    ];

    let home = TempHome::new("live-terms");
    let mut cache = LiveCache::open(&home.0);
    let mut unpacked = 0usize;
    let mut asked_unpack = 0usize;
    for source in archive.iter().chain(control.iter()) {
        let bare = engine.translate_block("en", "ru", &[source]);
        let after = te::translate_live(source, "ru_RU.UTF-8", &engine, &mut cache).text;
        println!("источник : {source}");
        println!("  движок : {}", bare.first().map(String::as_str).unwrap_or(""));
        println!("  после  : {after}");
        assert!(!after.contains('@'), "заполнитель уехал в вывод: {after}");
        for (a, b) in technical_spans(source) {
            let piece = &source[a..b];
            assert!(after.contains(piece), "техника «{piece}» не дословно: {after}");
        }
        assert_eq!(
            apply_term_fixes("en", "ru", source, &after),
            after,
            "в выводе осталась форма, которую словарь умеет поправить"
        );
        let lower = after.to_lowercase();
        let about_unpacking = ["unzip", "unpack", "extract"]
            .iter()
            .any(|w| source.to_lowercase().contains(w));
        if archive.contains(source) && about_unpacking {
            asked_unpack += 1;
            if lower.contains("распак") {
                unpacked += 1;
            }
        }
        if control.contains(source) {
            assert!(lower.contains("извлек") || lower.contains("извлеч"), "контроль тронут: {after}");
        }
    }
    println!("«распак…» в {unpacked} из {asked_unpack} фраз про архив");
    assert!(unpacked * 4 >= asked_unpack * 3, "«распаковать» меньше чем в трёх четвертях");
}

// ============================================================================
// Theme description 2380824 (KDE Store, 06.11): quality rules on live pieces
// ============================================================================
//
// The card "Windows XP Professional - Luna Blue and Classic Suite" showed
// the whole class of defects at once: «и и», «Базцитовый Linux», «разногласия» instead of
// Discord, «исследователь», «Лофон», «Лоток системы», ✨ → 👍 and a paragraph
// that stayed entirely in English. Below is a check for each rule; the engine's
// answers are VERBATIM those the live Firefox en→ru model gave on 06.11.

/// The item title and the author — as the window passes them.
const XP_NAME: &str = "Windows XP Professional - Luna Blue and Classic Suite";
const XP_AUTHOR: &str = "ruissoftware";

/// A "scripted" engine: answers a known sentence with the recorded live
/// answer, the rest as [`FakeEngine`] does («перевод: …»).
struct ScriptedEngine {
    answer: Box<dyn Fn(&str) -> Option<String>>,
    seen: Mutex<Vec<String>>,
}

impl ScriptedEngine {
    fn new(answer: impl Fn(&str) -> Option<String> + 'static) -> Self {
        ScriptedEngine { answer: Box::new(answer), seen: Mutex::default() }
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

impl TranslateEngine for ScriptedEngine {
    fn available(&self, src: &str, tgt: &str) -> bool {
        (src, tgt) == ("en", "ru")
    }

    fn translate_block(&self, _src: &str, _tgt: &str, block: &[&str]) -> Vec<String> {
        self.seen.lock().unwrap().extend(block.iter().map(|s| s.to_string()));
        block
            .iter()
            .map(|s| (self.answer)(s).unwrap_or_else(|| format!("перевод: {s}")))
            .collect()
    }
}

/// Run a piece of the description with the title and the author guarded, on a fresh database.
fn xp_translate(source: &str, engine: &ScriptedEngine) -> te::LiveTranslation {
    let home = TempHome::new("xp");
    let mut cache = LiveCache::open(&home.0);
    te::translate_live_guarded(source, "ru_RU.UTF-8", engine, &mut cache, &[XP_NAME, XP_AUTHOR])
}

/// (a) Names from the core's tables (distributions, programs) — in Latin letters. The node
/// "Bazzite Linux" consists entirely of names, it does not reach the engine at all; "Discord"
/// inside a phrase was translated by the engine («раздор»), and the phrase was asked again under a
/// placeholder.
#[test]
fn xp_product_names_from_core_tables_stay_latin() {
    let engine = ScriptedEngine::new(|s| {
        if s.contains("Discord") {
            // Bare pass: the model's live answer.
            return Some(
                "светло-голубая область уведомлений Luna, показывающая живые фоновые приложения \
                 (раздор, Steam и т. Д.), контроль громкости."
                    .to_string(),
            );
        }
        s.contains("@0@").then(|| {
            "светло-голубая область уведомлений Luna, показывающая живые фоновые приложения \
             (@0@ и т. Д.), контроль громкости."
                .to_string()
        })
    });
    let out = xp_translate(
        "<p>for <strong>KDE Plasma 6</strong> and <strong>Bazzite Linux</strong></p>\
         <p>Light-blue Luna notification area showing live background apps (Discord, Steam, etc.), \
         volume control.</p>",
        &engine,
    );
    assert!(out.text.contains("<strong>Bazzite Linux</strong>"), "{}", out.text);
    assert!(out.text.contains("(Discord, Steam и т. Д.)"), "{}", out.text);
    assert!(!out.text.contains("раздор"), "{}", out.text);
    assert!(
        !engine.seen().iter().any(|s| s == "Bazzite Linux"),
        "узел из одних имён ушёл движку: {:?}",
        engine.seen()
    );
    // Ordinary lowercase words are still prose: "a steam cloud" is not a name.
    assert!(rexon_core::translator::is_prose_segment("a steam cloud over windows"));
}

/// (b) Picture characters are not shown to the engine: ✨ at an edge is cut off and
/// returned, ✨ inside a phrase goes under a placeholder.
#[test]
fn xp_pictographs_survive_byte_for_byte() {
    let engine = ScriptedEngine::new(|s| match s {
        // Exactly what the model did with the whole line: «### 👍 Особенности:».
        "### ✨ Features:" => Some("### 👍 Особенности:".to_string()),
        "Features:" => Some("особенности:".to_string()),
        _ if s.contains('✨') => Some(s.replace('✨', "👍").replace("Enjoy the", "наслаждайтесь")),
        _ if s.contains("@0@") => Some(s.replace("Enjoy the", "наслаждайтесь").replace(" look", " видом")),
        _ => None,
    });
    let out = xp_translate("<p>### ✨ Features:<br />Enjoy the ✨ look</p>", &engine);
    assert!(out.text.contains("### ✨ Особенности:"), "{}", out.text);
    assert!(out.text.contains("Наслаждайтесь ✨ видом"), "{}", out.text);
    assert!(!out.text.contains('👍'), "знак подменён: {}", out.text);
    assert!(
        !engine.seen().iter().any(|s| s.contains("### ")),
        "край строки ушёл движку: {:?}",
        engine.seen()
    );
}

/// A colon after a bold caption is an edge: the model replaced it with a dash.
#[test]
fn xp_leading_colon_after_bold_label_is_kept() {
    let engine = ScriptedEngine::new(|s| {
        (s == "Choose between the authentic welcome screen and the classic dialog.")
            .then(|| "выбор между аутентичным экраном приветствия и классическим диалогом.".to_string())
    });
    let out = xp_translate(
        "<p><strong>Screen</strong>: Choose between the authentic welcome screen and the classic dialog.</p>",
        &engine,
    );
    assert!(
        out.text.contains("</strong>: Выбор между"),
        "двоеточие потерялось: {}",
        out.text
    );
}

/// (c) What the engine made up beyond the source: «и и» for a single "and", the conjunction «и»
/// at the start of a phrase, a dash at the start. An author's repeat stays.
#[test]
fn xp_invented_repeats_and_leading_marks_are_dropped() {
    let engine = ScriptedEngine::new(|s| match s {
        "and" => Some("и и".to_string()),
        "Full documentation and source code available on GitHub:" => {
            Some("и полная документация и исходный код доступны на GitHub:".to_string())
        }
        "Lock Screen" => Some("- экран замка".to_string()),
        "This is very very nice." => Some("это очень очень мило.".to_string()),
        _ => None,
    });
    let out = xp_translate(
        "<p>for <strong>KDE Plasma 6</strong> and <strong>Bazzite Linux</strong></p>\
         <p>Full documentation and source code available on GitHub: <a href=\"https://x.invalid\">https://x.invalid</a></p>\
         <p><strong>Lock Screen</strong></p><p>This is very very nice.</p>",
        &engine,
    );
    assert!(out.text.contains("</strong> и <strong>"), "«и и» осталось: {}", out.text);
    assert!(out.text.contains("<p>Полная документация и исходный код"), "{}", out.text);
    assert!(out.text.contains("<strong>Экран блокировки</strong>"), "{}", out.text);
    assert!(out.text.contains("Это очень очень мило."), "авторский повтор съеден: {}", out.text);
}

/// Term corrections on lone captions: "Explorer", "Logon", "Windows XP
/// System Tray (", "Classic Start Menu (" — the model's live answers.
#[test]
fn xp_windows_terms_are_fixed() {
    let engine = ScriptedEngine::new(|s| match s {
        "Explorer" => Some("исследователь".to_string()),
        "Logon" => Some("Лофон".to_string()),
        "Windows XP System Tray (" => Some("лоток системы Windows XP (".to_string()),
        "Classic Start Menu (" => Some("Classic Начало Меню (".to_string()),
        _ => None,
    });
    let out = xp_translate(
        "<p><strong>Explorer</strong></p><p><strong>Logon</strong></p>\
         <p><strong>Windows XP System Tray (</strong></p><p><strong>Classic Start Menu (</strong></p>",
        &engine,
    );
    for want in ["Проводник", "Вход в систему", "Системный лоток Windows XP (", "Classic Меню «Пуск» ("] {
        assert!(out.text.contains(want), "нет «{want}»: {}", out.text);
    }
}

/// (d) The paragraph ": Full Windows XP Explorer replica … Classic View …" stayed
/// in English ENTIRELY: the bare translation was good but lost `Classic` (a word
/// from the item title), the second pass hid all four names and the engine
/// multiplied `@0@`. Now the second pass hides only what was lost, and if
/// it fails too, the bare translation is taken: the lost name is "soft".
#[test]
fn xp_paragraph_with_a_lost_title_word_is_not_left_in_english() {
    const CORE: &str = "Full Windows XP Explorer replica with Category View, Classic View, System \
                        Properties, Display Properties, Run dialog, and Windows Task Manager with \
                        green CPU/PF performance graphs.";
    let engine = ScriptedEngine::new(|s| {
        if s == CORE {
            return Some(
                "полная копия проводника Windows XP с видом категории, классическим \
                 представлением, системными свойствами, свойствами отображения, диалогом запуска \
                 и диспетчером задач Windows с зелеными графиками производительности CPU/PF."
                    .to_string(),
            );
        }
        // Second pass: live corruption — the placeholder multiplied.
        s.contains("@0@").then(|| "полная @0@ реплика @0@ с видом категории.".to_string())
    });
    let out = xp_translate(&format!("<p><strong>Explorer</strong>: {CORE}</p>"), &engine);
    assert!(out.was_translated);
    assert!(out.text.contains(": Полная копия проводника Windows XP"), "{}", out.text);
    assert!(!out.text.contains("Full Windows"), "абзац остался английским: {}", out.text);
    // The second pass hid ONE name — the lost `Classic`, not all four.
    let masked: Vec<String> = engine.seen().into_iter().filter(|s| s.contains('@')).collect();
    assert_eq!(masked.len(), 1, "{masked:?}");
    assert!(masked[0].contains("@0@ View") && !masked[0].contains("@1@"), "{masked:?}");
}

/// The strictness is as before for "hard" names: a name from the core's lists that is kept
/// neither bare nor under a placeholder still cancels the segment.
#[test]
fn xp_a_lost_core_name_still_returns_the_original() {
    let engine = ScriptedEngine::new(|s| {
        if s.contains('@') {
            Some("приложения (и т. Д.).".to_string())
        } else {
            Some("приложения (раздор, Steam и т. Д.).".to_string())
        }
    });
    let source = "<p>Background apps (Discord, Steam, etc.).</p>";
    let out = xp_translate(source, &engine);
    assert_eq!(out.text, source);
    assert!(!out.was_translated);
}

/// A cache with old rules is not read: a record of the previous version (`6`) with a bad
/// translation lies under another key, and the person immediately sees the new translation —
/// without cleaning the database by hand.
#[test]
fn xp_translations_cached_under_old_rules_are_not_shown() {
    use rexon_core::translator::{translation_cache_key, translation_cache_key_versioned};
    let home = TempHome::new("xp-old-cache");
    let mut cache = LiveCache::open(&home.0);
    let segment = "Explorer";
    cache.put_translation(&translation_cache_key_versioned(6, "en", "ru", segment), "Исследователь");
    assert_ne!(
        translation_cache_key_versioned(6, "en", "ru", segment),
        translation_cache_key("en", "ru", segment),
        "версия правил не поднята — старые переводы читались бы как свои"
    );
    let engine = ScriptedEngine::new(|s| (s == "Explorer").then(|| "исследователь".to_string()));
    let out = te::translate_live_guarded(
        "<p><strong>Explorer</strong></p>",
        "ru_RU.UTF-8",
        &engine,
        &mut cache,
        &[XP_NAME],
    );
    assert!(out.text.contains("Проводник"), "{}", out.text);
}

/// Pure parts of the rules: phrase edges, names hidden again, "softness".
#[test]
fn xp_rule_helpers() {
    use rexon_core::translator as tr;
    assert_eq!(tr::split_sentence_edges("### ✨ Features:"), ("### ✨ ", "Features:", ""));
    assert_eq!(tr::split_sentence_edges(": Full text."), (": ", "Full text.", ""));
    assert_eq!(tr::split_sentence_edges("Enjoy! 🎉"), ("", "Enjoy!", " 🎉"));
    // A command flag and a bracket are not an edge.
    assert_eq!(tr::split_sentence_edges("-r flag"), ("", "-r flag", ""));
    assert_eq!(tr::split_sentence_edges("(Discord)"), ("", "(Discord)", ""));

    let masked = tr::mask_names("Windows XP with Classic View and CPU/PF", &[XP_NAME]);
    assert_eq!(masked.names, ["Windows", "Classic", "CPU/PF"]);
    let only = tr::mask_only(&masked, &[false, true, false]);
    assert_eq!(only.text, "Windows XP with @0@ View and CPU/PF");
    assert_eq!(only.names, ["Classic"]);

    assert!(tr::is_soft_guarded_name("Classic", &[XP_NAME]));
    assert!(!tr::is_soft_guarded_name("Windows", &[XP_NAME]), "имя из списка ядра");
    assert!(!tr::is_soft_guarded_name("Nordic", &["Nordic"]), "название целиком");
    assert!(!tr::is_soft_guarded_name("CPU/PF", &[XP_NAME]), "техника");

    assert_eq!(tr::drop_invented_repeats("en", "ru", "and", "и и"), "и");
    assert_eq!(tr::drop_invented_repeats("en", "ru", "Both A and B", "и A, и B"), "и A, и B");
    assert!(tr::is_pictograph('✨') && tr::is_pictograph('🚀') && !tr::is_pictograph('ё'));
}


// ============================================================================
// The "Candy icons" description (KDE Store 1305251, eliverlara, 08.11): link names
// and set terms
// ============================================================================
//
// Live check 08.11: «…чтобы соответствовать моему Сладкий тема темы»
// (the link text "Sweet" translated, "icon theme" → «икона темы»), "Note" →
// «Записка», "open an issue" → «открыть вопрос», "pull request" → «вытащить
// запрос». The description is VERBATIM from the KDE Store OCS API.

const CANDY_DESCRIPTION: &str = "<h2>Support Me &hearts;<br />Your support is truly vital to keep this and my other linux projects alive, so it would be great if you consider becoming a supporter on any of the different platforms, any help means a lot<br /><br />- <a href=\"https://github.com/sponsors/eliverlara\">Become a sponsor on github</a><br />- <a href=\"https://ko-fi.com/eliverlara\">Become a sponsor on Ko-fi</a><br />- <a href=\"https://www.paypal.com/paypalme/eliverlara\">Donate using PayPal</a><br /><br />Sweet gradient icon theme originally created to fit my <a href=\"https://www.gnome-look.org/p/1253385/\">Sweet</a> theme<br /><br />If you want you can combine them with any of the options provided in <a href=\"https://www.opendesktop.org/p/1284047/\">sweet-folders</a> as shown in the second screenshot<br /><br /><b>Note</b><br /><br />To request for icons please <a href=\"https://github.com/EliverLara/candy-icons/issues\">open an issue</a> on github, or if you want to contribute open a <a href=\"https://github.com/EliverLara/candy-icons/pulls\">pull request</a></h2>";

const CANDY_NAME: &str = "Candy icons";
const CANDY_AUTHOR: &str = "eliverlara";

/// General requirements for the translation of the "Candy icons" description.
fn assert_candy_is_fixed(text: &str) {
    let lower = text.to_lowercase();
    assert!(
        text.contains("<a href=\"https://www.gnome-look.org/p/1253385/\">Sweet</a>"),
        "ссылка-имя тронута: {text}"
    );
    assert!(
        text.contains("<a href=\"https://www.opendesktop.org/p/1284047/\">sweet-folders</a>"),
        "{text}"
    );
    for bad in ["сладк", "тема темы", "икона", "иконы", "записка", "открыть вопрос", "вытащить"] {
        assert!(!lower.contains(bad), "осталось «{bad}»: {text}");
    }
    for want in ["<b>Примечание</b>", "заявку", "запрос на слияние", "тем"] {
        assert!(text.contains(want), "нет «{want}»: {text}");
    }
}

/// A script on the live model's answers (measurement 08.11, Firefox en→ru): a phrase with
/// a link name goes to the engine AS A WHOLE, not "Sweet" and "theme" separately.
#[test]
fn candy_name_link_is_folded_into_its_sentence_and_kept() {
    let engine = ScriptedEngine::new(|s| {
        Some(
            match s {
                "Sweet gradient icon theme originally created to fit my Sweet theme" => {
                    "сладкий градиент икона темы, первоначально созданный в соответствии с моей \
                     сладкой темой"
                }
                "@0@ gradient icon theme originally created to fit my @1@ theme" => {
                    "@0@ градиент икона темы, первоначально созданный в соответствии с моей темой @1@"
                }
                "Note" => "записка",
                "To request for icons please" => "запросить иконки, пожалуйста",
                "open an issue" => "открыть вопрос",
                "pull request" => "вытащить запрос",
                "Sweet" | "theme" => panic!("кусок фразы со ссылкой ушёл движку один: {s}"),
                _ => return None,
            }
            .to_string(),
        )
    });
    let home = TempHome::new("candy");
    let mut cache = LiveCache::open(&home.0);
    let out = te::translate_live_guarded(
        CANDY_DESCRIPTION,
        "ru_RU.UTF-8",
        &engine,
        &mut cache,
        &[CANDY_NAME, CANDY_AUTHOR],
    );
    assert!(out.was_translated);
    assert_candy_is_fixed(&out.text);
    assert!(out.text.contains("тема значков"), "{}", out.text);
    assert!(out.text.contains("темой <a href"), "{}", out.text);
    assert!(out.text.contains("Запросить значки"), "{}", out.text);
    // Prose links are still translated: "Become a sponsor on github".
    assert!(!out.text.contains(">Become a sponsor on github<"), "{}", out.text);
    // A slug is not glued: neighboring pieces go to the engine as before, the slug itself
    // never does.
    assert!(
        engine.seen().iter().any(|s| s == "as shown in the second screenshot"),
        "{:?}",
        engine.seen()
    );
    assert!(!engine.seen().iter().any(|s| s.contains("sweet-folders")), "{:?}", engine.seen());
}

/// Without a translation the glued phrase comes back as the original markup byte for byte.
#[test]
fn candy_untranslated_folded_sentence_keeps_its_markup() {
    use rexon_core::translator::{fold_name_links, render_folded, segment_html, translatable_flags};
    let html = "<p>Made to fit my <a href=\"https://www.gnome-look.org/p/1253385/\">Sweet</a> theme \
                and <a href=\"https://github.com/EliverLara/Nordic\">Nordic</a>.</p>";
    let (segs, runs) = fold_name_links(segment_html(html));
    let flags = translatable_flags(&segs);
    assert_eq!(render_folded(&segs, &flags, &runs, |_| String::new()), html);
    // Two links in one phrase — one glue, the names in order.
    let run = runs.iter().flatten().next().expect("склейка");
    assert_eq!(run.names(), ["Sweet", "Nordic"]);
    assert_eq!(run.plain(), "Made to fit my Sweet theme and Nordic.");
    // A translation without the name — also the original markup, not a lost link.
    assert_eq!(render_folded(&segs, &flags, &runs, |_| "без имён".to_string()), html);
    let linked = render_folded(&segs, &flags, &runs, |_| "под мою тему Sweet и Nordic.".to_string());
    assert_eq!(
        linked,
        "<p>под мою тему <a href=\"https://www.gnome-look.org/p/1253385/\">Sweet</a> и \
         <a href=\"https://github.com/EliverLara/Nordic\">Nordic</a>.</p>"
    );
    // The link name occurs twice in the phrase: the tag goes onto the same occurrence by count.
    let html = "<p>Sweet icons to fit my <a href=\"https://www.gnome-look.org/p/1253385/\">Sweet</a> theme</p>";
    let (segs, runs) = fold_name_links(segment_html(html));
    let flags = translatable_flags(&segs);
    assert_eq!(
        render_folded(&segs, &flags, &runs, |_| "значки Sweet под мою тему Sweet".to_string()),
        "<p>значки Sweet под мою тему <a href=\"https://www.gnome-look.org/p/1253385/\">Sweet</a></p>"
    );
    // The engine lost one occurrence — the nearest one by position is taken.
    assert_eq!(
        render_folded(&segs, &flags, &runs, |_| "значки под мою тему Sweet".to_string()),
        "<p>значки под мою тему <a href=\"https://www.gnome-look.org/p/1253385/\">Sweet</a></p>"
    );
}

/// What counts as a link name and what as prose.
#[test]
fn candy_name_link_rules() {
    use rexon_core::translator::{is_name_link, is_product_page_url, markdown_name_links, technical_spans};
    assert!(is_name_link("Sweet", "https://www.gnome-look.org/p/1253385/"));
    assert!(is_name_link("sweet-folders", "https://www.opendesktop.org/p/1284047/"));
    assert!(is_name_link("Nordic", "https://github.com/EliverLara/Nordic"));
    assert!(is_name_link("Sweet Dark", "https://store.kde.org/p/1294174"));
    assert!(!is_name_link("open an issue", "https://github.com/EliverLara/candy-icons/issues"));
    assert!(!is_name_link("pull request", "https://github.com/EliverLara/candy-icons/pulls"));
    assert!(!is_name_link("Become a sponsor on github", "https://github.com/sponsors/eliverlara"));
    assert!(!is_name_link("Download", "https://www.gnome-look.org/p/1253385/"));
    assert!(!is_name_link("Here", "https://github.com/EliverLara/Nordic"));
    assert!(!is_name_link("Sweet", "https://www.paypal.com/paypalme/eliverlara"));
    assert!(!is_product_page_url("https://github.com/sponsors/eliverlara"));
    assert!(!is_product_page_url("https://github.com/EliverLara/candy-icons/issues"));
    // Markdown: the name is guarded as a whole, a prose link is not.
    let md = "Made to fit my [Sweet Dark](https://store.kde.org/p/1294174) theme, [open an issue](https://github.com/a/b/issues).";
    let spans = markdown_name_links(md);
    assert_eq!(spans.len(), 1);
    assert_eq!(&md[spans[0].0..spans[0].1], "[Sweet Dark](https://store.kde.org/p/1294174)");
    assert!(technical_spans(md).contains(&spans[0]));
}

/// The dictionary of set terms — on the model's live answers (measurement 08.11).
#[test]
fn candy_stable_terms_are_fixed() {
    use rexon_core::translator::apply_term_fixes;
    let fix = |src: &str, tgt: &str| apply_term_fixes("en", "ru", src, tgt);
    assert_eq!(fix("Note", "записка"), "примечание");
    assert_eq!(fix("Note:", "заметка:"), "примечание:");
    assert_eq!(fix("NOTE", "НОТЕ ПРИМЕЧАНИЕ"), "Примечание");
    assert_eq!(fix("Note: requires Kvantum.", "заметка: требует Kvantum."), "примечание: требует Kvantum.");
    // "Note" not at the start is just a word.
    assert_eq!(fix("Take a note of it.", "сделайте заметку."), "сделайте заметку.");
    assert_eq!(fix("open an issue", "открыть вопрос"), "создать заявку");
    assert_eq!(
        fix("Feel free to open an issue if you find a bug.", "не стесняйтесь открывать проблему, если вы обнаружите ошибку."),
        "не стесняйтесь создавать заявку, если вы обнаружите ошибку."
    );
    assert_eq!(fix("Report bugs on the issue tracker.", "сообщить об ошибках в трекере вопросов."), "сообщить об ошибках в трекере заявок.");
    assert_eq!(fix("pull request", "вытащить запрос"), "запрос на слияние");
    assert_eq!(fix("Pull requests are welcome.", "вытащитные запросы приветствуются."), "запросы на слияние приветствуются.");
    assert_eq!(fix("An icon theme for KDE.", "иконописная тема для KDE."), "тема значков для KDE.");
    assert_eq!(fix("Icon theme", "икона тема"), "тема значков");
    assert_eq!(
        fix("Sweet gradient icon theme", "Sweet тема иконы градиента"),
        "Sweet тема значков градиента"
    );
    assert_eq!(fix("This icon set is based on Papirus.", "этот набор иконок основан на Papirus."), "этот набор значков основан на Papirus.");
    assert_eq!(fix("To request for icons please", "запросить иконки, пожалуйста"), "запросить значки, пожалуйста");
    // «иконки» with a single `icon` may be the genitive case.
    assert_eq!(fix("The icon color", "цвет иконки"), "цвет иконки");
    assert_eq!(fix("A cursor theme for Linux.", "курсорная тема для Linux."), "тема курсоров для Linux.");
    assert_eq!(fix("theme", "тема темы"), "тема");
    assert_eq!(fix("Please star the repository if you like it.", "пожалуйста, отведайте репозиторий, если вам это нравится."), "пожалуйста, отметьте звёздочкой репозиторий, если вам это нравится.");
    assert_eq!(fix("A plasma widget that shows the weather.", "плазменный виджет, который показывает погоду."), "виджет Plasma, который показывает погоду.");
    // Already correct — not touched.
    assert_eq!(fix("A dark GTK theme with Kvantum support.", "темная тема GTK с поддержкой Kvantum."), "темная тема GTK с поддержкой Kvantum.");
}

/// Live check of the whole description on the real model.
#[test]
#[ignore = "needs the downloaded Firefox en→ru model (REXON_TRANSLATE_MODEL_DIR), not kept in the tree"]
fn live_candy_icons_description_is_fixed() {
    if std::env::var_os(te::MODEL_DIR_ENV).is_none() {
        std::env::set_var(te::MODEL_DIR_ENV, stand_model_dir());
    }
    let engine = te::LiveEngine::new();
    assert!(engine.available("en", "ru"), "модель не загрузилась");
    let home = TempHome::new("live-candy");
    let mut cache = LiveCache::open(&home.0);
    let out = te::translate_live_guarded(
        CANDY_DESCRIPTION,
        "ru_RU.UTF-8",
        &engine,
        &mut cache,
        &[CANDY_NAME, CANDY_AUTHOR],
    );
    println!("{}", out.text);
    assert_candy_is_fixed(&out.text);
    assert!(out.text.contains("скриншот"), "фраза у «sweet-folders» обрублена: {}", out.text);
}
