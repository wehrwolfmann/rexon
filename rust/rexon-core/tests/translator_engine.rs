//! Сверка живого перевода: склейка [`rexon_core::translator_engine`] целиком —
//! от локали до готовой строки.
//!
//! Движок ПОДСТАВНОЙ, со счётчиком вызовов, а база — настоящая, во временном
//! каталоге. Так проверяется то, чего иначе не увидеть:
//!   * второй прогон того же текста делает НОЛЬ вызовов движка, то есть кеш
//!     действительно пишется и читается (а не «вроде работает»);
//!   * технические сегменты уходят в вывод ДОСЛОВНО — движок их не видит;
//!   * теги HTML целы, перевод садится только в текстовые узлы;
//!   * блок для движка не длиннее `MAX_BLOCK_SENTENCES`;
//!   * строчная первая буква, которой движок отвечает в 88,6 % случаев,
//!     поднимается склейкой.
//!
//! Живой перевод на настоящей модели — `#[ignore]`-сверка в конце файла (по
//! образцу `fxtranslate_vendor.rs`): модель 43 МБ в дерево не кладётся.

use std::sync::Mutex;

use rexon_core::appearance_cache::LiveCache;
use rexon_core::translator_engine as te;
use rexon_core::translator_engine::{TranslateEngine, MAX_BLOCK_SENTENCES};

// ============================================================================
// Стенд
// ============================================================================

/// Временный дом: своя база кеша на каждую сверку.
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

/// Подставной движок: считает вызовы и помнит каждый блок.
///
/// Переводит нарочито узнаваемо и НАРОЧНО со строчной буквы — ровно так, как
/// отвечает настоящий движок (замер: 88,6 % строк). Значит заглавная буква в
/// выводе может прийти только от склейки.
#[derive(Default)]
struct FakeEngine {
    pair: Option<(String, String)>,
    blocks: Mutex<Vec<Vec<String>>>,
    /// Что движок делает с заполнителями имён в ответе (порча — как живая).
    spoil: Spoil,
}

/// Порча заполнителя в ответе движка. Все три формы — настоящие, увиденные на
/// живой модели Firefox при подборе формы заполнителя (стенд 21.09).
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Spoil {
    /// Заполнители целы.
    #[default]
    None,
    /// `@0@` съеден целиком.
    Drop,
    /// `@0@` пришёл дважды (движок размножил имя).
    Double,
    /// От `@0@` остался огрызок `@` («Nordic @ Kvantum тема»).
    Crumb,
}

impl FakeEngine {
    /// Движок, который умеет ровно `en`→`ru`.
    fn enru() -> Self {
        FakeEngine {
            pair: Some(("en".to_string(), "ru".to_string())),
            blocks: Mutex::default(),
            spoil: Spoil::None,
        }
    }

    /// Тот же движок, но портящий заполнитель имени в ответе.
    fn spoiling(spoil: Spoil) -> Self {
        FakeEngine { spoil, ..FakeEngine::enru() }
    }

    /// Движок, которого нет (модели в поставке не оказалось).
    fn missing() -> Self {
        FakeEngine::default()
    }

    /// Сколько раз звали движок.
    fn calls(&self) -> usize {
        self.blocks.lock().unwrap().len()
    }

    /// Все предложения, которые движок увидел.
    fn seen(&self) -> Vec<String> {
        self.blocks.lock().unwrap().iter().flatten().cloned().collect()
    }

    /// Длины блоков по порядку.
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
        // Со строчной буквы — как настоящий движок.
        block
            .iter()
            .map(|s| {
                let mut answer = format!("перевод: {s}");
                // Имена «переводятся» — иначе подставной движок был бы честнее
                // живого (см. MANGLED_NAMES).
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

/// Имена, которые подставной движок «переводит», — ровно те, что живая модель
/// действительно склоняет и переводит (замеры в `live_names_…` ниже).
///
/// ⚠️ Без этого подставной движок оказался бы ЧЕСТНЕЕ настоящего: он возвращает
/// предложение дословно, значит имя в первом (голом) проходе «выживало» бы
/// всегда, и путь под заполнителями — второй проход — сверки не проверяли бы
/// вовсе. Порядок важен: длинное имя раньше короткого («KDE Plasma» до
/// «Plasma», «Nordic» до «Nord»).
const MANGLED_NAMES: &[(&str, &str)] = &[
    ("KDE Plasma", "кде плазма"),
    ("Kvantum", "квантская"),
    ("Cinnamon", "корица"),
    ("Aurorae", "аврора"),
    ("Plasma", "плазма"),
    ("Nordic", "скандинавская"),
    ("Nord", "север"),
];

/// Техника, которую подставной движок «переводит» в голом проходе, — по той же
/// причине, что [`MANGLED_NAMES`]: иначе путь в первом проходе выживал бы
/// всегда, и путь под заполнителем сверки не проверяли бы.
const MANGLED_TECH: &[(&str, &str)] =
    &[("~/.themes", "~/.темы"), ("/usr/share/sounds", "/usr/доля/звуки")];

/// Термины, которые подставной движок «переводит» так же криво, как живая
/// модель Firefox (замер 03.10: «Just unzip the file…» → «Просто расстегните
/// файл…»), — чтобы словарь поправок проверялся на всём пути, а не только
/// чистой функцией.
const MANGLED_TERMS: &[(&str, &str)] = &[("Unzip", "расстегните")];

/// Живое описание мастерской по составу: проза, заголовок, команда в `<pre>`,
/// строка с путём, список сред и ссылка.
const DESCRIPTION: &str = concat!(
    "<p>This global theme brings a warm, muted palette to the whole desktop. ",
    "It ships matching Plasma, Aurorae and Kvantum styles.</p>",
    "<p>Installation:</p>",
    "<pre>sudo apt-get install gtk2-engines-murrine</pre>",
    "<p>Copy the folder to ~/.themes and pick it in Settings.</p>",
    "<p>Preview: https://example.invalid/shot.png</p>"
);

// ============================================================================
// Сверки
// ============================================================================

/// ⭐ ГЛАВНОЕ: второй прогон того же текста НЕ зовёт движок ни разу, а вывод
/// байт-в-байт тот же. Это и есть доказательство, что кеш работает на запись,
/// а ключ считается одинаково в обоих прогонах.
#[test]
fn the_second_run_of_the_same_text_never_calls_the_engine() {
    let home = TempHome::new("cache");

    let first_engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let first = te::translate_live(DESCRIPTION, "ru_RU.UTF-8", &first_engine, &mut cache);
    assert!(first.was_translated, "перевод обязан был случиться");
    assert!(first_engine.calls() >= 1, "движок не позвали ни разу");

    // Другая база-соединение, другой счётчик — как у второго открытия товара.
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

/// Движку достаётся только проза, и притом ПРЕДЛОЖЕНИЯМИ. Технику он не
/// видит вовсе, а имена технологий уходят под заполнители ВТОРЫМ проходом — и
/// только те предложения, в которых движок имя потерял.
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
            // Проход 1 — предложения ГОЛЫМИ, как написал человек: заполнитель
            // сам сбивает декодировщик, и тратить его без нужды незачем.
            "This global theme brings a warm, muted palette to the whole desktop.".to_string(),
            "It ships matching Plasma, Aurorae and Kvantum styles.".to_string(),
            "Installation:".to_string(),
            // С 03.10 — и фразы с путём и ссылкой внутри (команда в `<pre>`
            // по-прежнему не видна движку вовсе).
            "Copy the folder to ~/.themes and pick it in Settings.".to_string(),
            "Preview: https://example.invalid/shot.png".to_string(),
            // Проход 2 — только те предложения, где имя или путь потерялись.
            // `Plasma, Aurorae` — цепочка имён через запятую, потому под ОДНИМ
            // заполнителем; `Kvantum` за словом `and` — под своим.
            "It ships matching @0@ and @1@ styles.".to_string(),
            "Copy the folder to @0@ and pick it in Settings.".to_string(),
        ],
        "движку досталось не то"
    );
    // Первый проход — ОДИН блок на все пять предложений (пакетный путь, а не
    // по одному), второй — блок из двух пострадавших.
    assert_eq!(engine.block_sizes(), vec![5, 2]);
}

/// ⭐ ГЛАВНОЕ ПРО ИМЕНА: движок потерял (размножил, обгрыз) заполнитель — в
/// вывод идёт ОРИГИНАЛ сегмента, а не испорченный перевод, и в кеш такой
/// сегмент не пишется (следующая попытка спросит движок заново).
///
/// Порча подделана подставным движком намеренно: на живой модели она случается
/// редко и не по заказу, а полагаться на «сегодня не случилось» нельзя.
#[test]
fn a_lost_placeholder_returns_the_original_segment() {
    // Абзац из ДВУХ предложений: второе с именем. Одно испорченное предложение
    // отменяет весь сегмент — иначе вышла бы каша из русского и английского.
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

        // В кеш не записано: второй прогон снова зовёт движок.
        let again = FakeEngine::spoiling(spoil);
        let mut cache = LiveCache::open(&home.0);
        let second = te::translate_live(source, "ru_RU.UTF-8", &again, &mut cache);
        assert!(again.calls() >= 1, "порча попала в кеш");
        assert_eq!(second.text, source);
    }
}

/// Имя технологии остаётся латиницей и на целом пути: движок имя перевёл —
/// предложение переспрашивается под заполнителями, и в вывод имя возвращается
/// дословно.
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
    // Перевод взят со ВТОРОГО прохода: имена уехали под заполнителями, и
    // именно его ответ пошёл в вывод.
    assert!(
        engine.seen().iter().any(|s| s.contains("@0@")),
        "второго прохода не было — имя спасать было нечем: {:?}",
        engine.seen()
    );
    // ⚠️ Голым имя движку показывается (проход 1) — но его ответу не верят:
    // переведённого имени в выводе нет ни в одном виде.
    for mangled in ["квантская", "корица", "кде плазма"] {
        assert!(
            !out.text.contains(mangled),
            "в вывод пошёл ответ, где имя переведено («{mangled}»): {}",
            out.text
        );
    }
}

/// ⭐ Дефект приёмки 24.09 (D4b): в узле, где проза стоит рядом со ссылкой,
/// переводится ПРОЗА, а ссылка остаётся дословно — раньше молча оставался
/// английским весь узел.
///
/// Живой случай: комментарий maxkaspar к «Awesome Widgets» — «hddtemp shows
/// only the first digit. here is a screenshot http://…». Движок при этом
/// получает ТОЛЬКО первое предложение: ссылку ему не показывают вовсе.
///
/// С 03.10 переводится и вторая фраза — ссылка в ней уходит под заполнитель
/// только если голый ответ её испортил; здесь движок её не тронул, и она
/// взята из первого прохода дословно.
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

/// Технические сегменты уходят в вывод ДОСЛОВНО, а теги целы. Путь и ссылка
/// ВНУТРИ фразы (с 03.10) — тоже дословно, а фраза вокруг них переведена.
#[test]
fn technical_segments_and_tags_survive_verbatim() {
    let home = TempHome::new("verbatim");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);
    let out = te::translate_live(DESCRIPTION, "ru_RU.UTF-8", &engine, &mut cache).text;

    for verbatim in [
        // Внутри `<pre>` не переводится ничего.
        "<pre>sudo apt-get install gtk2-engines-murrine</pre>",
        // Путь внутри фразы: голый ответ движка его испортил («~/.темы»),
        // взят ответ второго прохода, путь вернулся байт в байт.
        "<p>Перевод: Copy the folder to ~/.themes and pick it in Settings.</p>",
        // Ссылка внутри фразы цела.
        "<p>Перевод: Preview: https://example.invalid/shot.png</p>",
    ] {
        assert!(out.contains(verbatim), "пропал дословный кусок «{verbatim}»\n{out}");
    }
    assert!(!out.contains("~/.темы"), "в вывод пошёл испорченный путь\n{out}");
    // Разметка на месте: столько же тегов, сколько было.
    for tag in ["<p>", "</p>", "<pre>", "</pre>"] {
        assert_eq!(
            out.matches(tag).count(),
            DESCRIPTION.matches(tag).count(),
            "число тегов «{tag}» изменилось\n{out}"
        );
    }
    // Перевод сел в текстовые узлы, а не поверх разметки.
    assert!(out.starts_with("<p>Перевод: This global theme"), "начало вывода:\n{out}");
}

/// ⭐ Строчная первая буква, которой отвечает движок, поднимается склейкой —
/// и поднимается по ИСТОЧНИКУ.
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

    // Движок ответил «перевод: …» со строчной; в выводе — «Перевод: …».
    assert!(out.contains("<p>Перевод: This theme is warm."), "не поднята заглавная:\n{out}");
    // Узел «and also readable.» — продолжение фразы после `<b>`, и в источнике
    // он начинается со строчной: поднимать его нельзя.
    assert!(out.contains("перевод: and also readable."), "поднято лишнее:\n{out}");
}

/// Блок не длиннее предела: у длинного описания их становится несколько.
#[test]
fn a_long_description_is_split_into_blocks() {
    let home = TempHome::new("blocks");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);

    // Двадцать РАЗНЫХ предложений одним абзацем (одинаковые склеились бы в
    // один уникальный текст).
    let mut text = String::from("<p>");
    for i in 0..20 {
        text.push_str(&format!("The theme number {i} repaints the whole desktop. "));
    }
    text.push_str("</p>");
    te::translate_live(&text, "ru_RU.UTF-8", &engine, &mut cache);

    assert_eq!(engine.block_sizes(), vec![MAX_BLOCK_SENTENCES, 20 - MAX_BLOCK_SENTENCES]);
}

/// Модель из ПОСТАВКИ находится по той же раскладке, какой ищется вендорный
/// QML вкладки обоев: `<каталог данных>/translate/<пара>` (в дереве
/// разработчика каталог данных — `data/`). Файлы лепятся настоящие (пустые)
/// во временном каталоге — проба у поиска живая.
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

    // Другого направления в поставке нет — и это не ошибка, а «перевода нет».
    assert!(te::pick_model(None, &root.0, "/нет-такого-дома", "en", "de", &on_disk).is_none());
}

/// Нет модели — нет перевода, и нет ошибки: оригинал и `was_translated = false`.
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

/// Текст уже на языке пользователя — движок не зовут (решение ядра).
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

/// Пустое описание — пустой ответ без единого вызова.
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
// Пакет из многих коротких текстов (страница комментариев)
// ============================================================================

/// ⭐ ГЛАВНОЕ про пакет: двадцать коротких комментариев уезжают к движку
/// ПОЛНЫМИ блоками, а не по одному.
///
/// Это и есть причина, по которой [`te::translate_live_batch`] существует:
/// двадцать отдельных `translate_live` дали бы двадцать вызовов по одному
/// предложению — то есть пакетный путь движка вхолостую.
#[test]
fn a_page_of_short_comments_goes_to_the_engine_in_full_blocks() {
    let home = TempHome::new("comments");
    let engine = FakeEngine::enru();
    let mut cache = LiveCache::open(&home.0);

    // Двадцать РАЗНЫХ комментариев в одно предложение каждый — как на живой
    // странице мастерской.
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
    // Порядок ответа — порядок вопроса: перевод не съехал на чужой
    // комментарий.
    assert!(done[7].text.contains("theme number 7"), "съехал порядок: {:?}", done[7].text);
}

/// Русский комментарий среди английских к движку не уезжает вовсе и приходит
/// назад с `was_translated = false` — по этому признаку вкладка и не рисует
/// над ним ссылки «показать оригинал».
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
// Модель в поставке
// ============================================================================

/// Модель едет В ПАКЕТЕ, и кладут её ровно туда и под теми именами, где ядро
/// её ищет.
///
/// Разъехавшийся путь или имя — самый тихий отказ из возможных: движок
/// скажет «модели нет», перевода не будет, и никто не упадёт. Поэтому путь и
/// имена берутся ИЗ ЯДРА, а не переписываются в сверку руками.
#[test]
fn модель_едет_в_поставке_туда_где_её_ищет_ядро() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let pkgbuild = std::fs::read_to_string(root.join("packaging/aur/PKGBUILD")).unwrap();

    // Каталог направления: `<данные>/translate/<пара>`.
    let dir = format!("$data/{}/$_mt_pair", te::MODEL_DIR_REL);
    assert!(pkgbuild.contains(&dir), "пакет кладёт модель не в «{dir}»");
    assert!(pkgbuild.contains("_mt_pair=enru"), "в PKGBUILD нет направления en→ru");

    // Имена файлов — те, что ищет `model_files_in`; в PKGBUILD пара вынесена
    // в переменную, поэтому сверяется шаблон имени.
    let want = te::model_files_in("", "en", "ru", &|_: &str| true).expect("имена файлов модели");
    for path in [want.model, want.src_vocab] {
        let name = path.rsplit('/').next().unwrap().to_string();
        let tmpl = name.replace(&te::model_pair("en", "ru"), "${_mt_pair}");
        assert!(pkgbuild.contains(&tmpl), "пакет не кладёт файл модели {name}");
    }

    // Суммы сверяются ДВАЖДЫ: архивы — самим makepkg, распакованные файлы —
    // в prepare(). Вторая сверка и есть гарантия того, что в пакет уехала
    // измеренная редакция модели, а не порча распаковки.
    assert!(
        pkgbuild.contains("sha256sum --check"),
        "PKGBUILD не сверяет sha256 РАСПАКОВАННОЙ модели"
    );
    assert!(
        pkgbuild.contains("LICENSE.translations-model-MPL-2.0"),
        "пакет не ставит лицензию модели (MPL-2.0)"
    );
    // Обещания перевода на Python в пакете быть не должно: Rust-окно зовёт
    // свой движок, а не argostranslate.
    assert!(
        !pkgbuild.contains("argostranslate"),
        "в PKGBUILD осталось обещание перевода, которого окно не использует"
    );

    // Ручная установка (не пакетом) копирует каталог данных целиком, и модель
    // обязана доехать вместе с ним — под пропуск её имена не попадают.
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

/// ⭐ НАЗВАНИЕ ТОВАРА, переданное вызывающим, приходит в вывод ДОСЛОВНО — тем
/// же приёмом, что имена технологий: перевёл его движок в первом проходе —
/// предложение идёт вторым проходом под заполнителем.
///
/// Живой материал: «Nordic Kvantum theme» движок отдавал как «Скандинавская
/// Kvantum тема», «Sweet Kvantum theme» — как «Сладкая Kvantum тема» (прогоны
/// 21.09). Списка названий тем у ядра нет, зато название окно знает.
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
        // Перевод названия в вывод не попал: «Nordic» не стало
        // «скандинавской», а «Sweet Kvantum» — «сладкой квантской».
        for mangled in ["скандинавская", "квантская"] {
            assert!(
                !out.text.contains(mangled),
                "название переведено («{mangled}»): {}",
                out.text
            );
        }
    }
}

/// Имя автора прячется наравне с названием, а пустые и коротенькие имена в
/// списке ядро отбрасывает само — вызывающему нечего проверять.
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

/// Старый договор цел: [`te::translate_live`] без охраняемых имён работает ровно
/// как до правки — окну не нужно ничего менять там, где названия у него нет.
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
    // Без охраны название уходит к движку — то самое, что правка и лечит.
    assert!(
        plain_engine.seen().iter().any(|s| s.contains("Nordic")),
        "сверка ни о чём: движок не видел названия и без охраны"
    );
}

/// Порча заполнителя на охраняемом названии судится так же строго: сегмент
/// отдаётся ОРИГИНАЛОМ и в кеш не пишется.
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
// Живая сверка на настоящей модели
// ============================================================================

/// Каталог модели для живой сверки: по умолчанию стендовый (в дерево модель не
/// кладётся, её место — поставка).
/// ⭐ Дефект владельца 03.10 (KDE Store, «ZorinOS 6 Sound Theme»): фраза с
/// путём внутри переводится, путь возвращается байт в байт — даже когда голый
/// ответ движка путь испортил (второй проход, путь под заполнителем).
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

/// Метку пути движок потерял (съел, размножил, обгрыз) — в вывод идёт
/// ОРИГИНАЛ, а не перевод с дырой на месте пути.
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

/// ⛔ Строка-команда и строка из одного пути движку по-прежнему не достаются.
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

/// Словарь поправок терминов стоит на пути движка: «расстегните» от `unzip`
/// приходит покупателю как «распакуйте», техника и регистр целы.
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

/// Стендовый каталог модели — в домашнем каталоге того, кто запускает сверку.
fn stand_model_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/file-verify/mt-bench/models/firefox-enru")
}

/// Живой перевод абзаца en→ru: настоящая модель, настоящий кеш, вся склейка.
///
/// `#[ignore]` намеренно — нужна скачанная модель (43 МБ). Запуск:
///
/// ```sh
/// cargo test -p rexon-core --release --test translator_engine \
///     -- --ignored --nocapture live_paragraph
/// ```
///
/// Каталог модели — `REXON_TRANSLATE_MODEL_DIR` (та же переменная, которой
/// пользуется сторож вендора).
#[test]
#[ignore = "нужна скачанная модель Firefox en→ru (REXON_TRANSLATE_MODEL_DIR), в дерево не кладётся"]
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

    // Второй прогон — из кеша, движок молчит.
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
    // Техника цела, разметка цела.
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
    // Первая буква — заглавная (лечение регистра на живом ответе движка).
    // ⚠️ Считать «первую букву вывода» нельзя: ею окажется `p` из `<p>`. Буква
    // ищется в ПЕРВОМ текстовом узле, то есть после первого закрытого тега.
    let first_node = cold.text.trim_start_matches(|c: char| c != '>').trim_start_matches('>');
    let first_letter = first_node.chars().find(|c| c.is_alphabetic()).unwrap_or(' ');
    assert!(first_letter.is_uppercase(), "первая буква строчная: {}", cold.text);
}

/// Живая сверка ЗАЩИТЫ ИМЁН на настоящей модели: одна и та же фраза сначала
/// отдаётся движку голой (что он делает с именем сам), потом идёт полным путём
/// через [`te::translate_live`] (имя под заполнителем).
///
/// Запуск — как у сверки выше:
///
/// ```sh
/// REXON_TRANSLATE_MODEL_DIR=$HOME/file-verify/mt-bench/models/firefox-enru \
///     cargo test -p rexon-core --release --test translator_engine \
///     -- --ignored --nocapture live_names
/// ```
///
/// Замер 21.09 на модели Firefox `en→ru` (печатается прогоном):
///   * голым движком `A Kvantum theme heavily based on Microsoft's Fluent
///     Design elements.` → «квантская тема в значительной степени основана на
///     элементах Microsoft Fluent Design.» — имя переведено и склонено;
///   * полным путём → «Kvantum тема, в значительной степени основанная на
///     элементах Microsoft Fluent Design.» — имя латиницей;
///   * «Copy the Papirus folder and enable it in Dolphin.» голым движком —
///     «спровочивайте папирусную папирус …», полным путём — «Скройте папку
///     Papirus и включите ее в Dolphin.».
///
/// Замер 23.09 (ПОТЕРЯ СЛОВА, товар «Iridescent»): «A colorful theme inspired
/// by sweet kde theme.» голым движком — «красочная тема, вдохновленная сладкой
/// темой.» (слова `kde` в переводе НЕТ), полным путём — «Красочная тема,
/// вдохновленная сладкой kde темой.».
#[test]
#[ignore = "нужна скачанная модель Firefox en→ru (REXON_TRANSLATE_MODEL_DIR), в дерево не кладётся"]
fn live_names_survive_the_real_engine() {
    if std::env::var_os(te::MODEL_DIR_ENV).is_none() {
        std::env::set_var(te::MODEL_DIR_ENV, stand_model_dir());
    }
    let engine = te::LiveEngine::new();
    assert!(engine.available("en", "ru"), "модель не загрузилась");

    // Живые фразы: образец 9 («Fluent Dark»), образец 5 (Sweet-Mars),
    // образец 6 (Nordic KDE) и составной случай с тремя именами.
    let phrases = [
        "A Kvantum theme heavily based on Microsoft's Fluent Design elements.",
        "Sweet Kvantum theme",
        "Nordic Kvantum theme",
        "This theme is based on Cinnamon and works with GNOME and KDE Plasma.",
        "Copy the Papirus folder and enable it in Dolphin.",
        // ⭐ ПОТЕРЯ СЛОВА (товар «Iridescent», приёмка 23.09): строчное `kde`
        // модель выбрасывала из перевода целиком — «красочная тема,
        // вдохновленная сладкой темой.». Аббревиатуры списка охраняются в
        // любом регистре, и слово возвращается на место.
        "A colorful theme inspired by sweet kde theme.",
    ];

    // ДО: движок на голой фразе, без подмены.
    let bare = engine.translate_block("en", "ru", &phrases);
    // ПОСЛЕ: полный путь ядра (фильтр прозы + подмена имён + кеш).
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
        // Каждое имя из фразы стоит в выводе дословно.
        for name in ["Kvantum", "Cinnamon", "GNOME", "KDE Plasma", "Papirus", "Dolphin", "kde"] {
            if source.contains(name) {
                assert!(after.contains(name), "имя {name} потерялось: {after}");
            }
        }
    }
    // Хотя бы одна фраза действительно переведена (а не отдана оригиналом).
    assert!(
        guarded.iter().zip(phrases.iter()).any(|(a, s)| a != s
            && a.chars().any(|c| ('а'..='я').contains(&c))),
        "ни одна фраза не переведена: {guarded:?}"
    );
}

/// Живая сверка ТЕХНИКИ ВНУТРИ ПРОЗЫ на настоящей модели (дефект владельца
/// 03.10, «ZorinOS 6 Sound Theme»): фразы из живых описаний KDE Store (OCS API,
/// категории «Sounds» 316, «Cursors» 107, «Icons» 132, «GTK» 135, «SDDM» 101,
/// «Kvantum» 123, «Global» 722) — путь, имя файла, `код` внутри прозы.
///
/// Запуск — как у сверок выше:
///
/// ```sh
/// cargo test -p rexon-core --release --test translator_engine \
///     -- --ignored --nocapture live_technique
/// ```
#[test]
#[ignore = "нужна скачанная модель Firefox en→ru (REXON_TRANSLATE_MODEL_DIR), в дерево не кладётся"]
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

/// Живая сверка ЗАЩИТЫ НАЗВАНИЯ ТОВАРА на настоящей модели: та же фраза сначала
/// голым движком (что он делает с названием сам), потом полным путём через
/// [`te::translate_live_guarded`] с названием в охране.
///
/// Запуск — как у сверок выше:
///
/// ```sh
/// REXON_TRANSLATE_MODEL_DIR=$HOME/file-verify/mt-bench/models/firefox-enru \
///     cargo test -p rexon-core --release --test translator_engine \
///     -- --ignored --nocapture live_item_name
/// ```
///
/// Замер 21.09 на модели Firefox `en→ru`, ДОСЛОВНО как печатает прогон
/// (слева название товара, потом «до» — голый движок, «после» — полный путь):
///   * `Sweet Kvantum theme` (Sweet): до — «сладкий Kvantum тема», после —
///     «Sweet Kvantum тема»;
///
/// ⚠️ ОГОВОРКА про короткие подписи (замер 24.09, после перехода на два
/// прохода): `Nordic Kvantum theme` и `Nordic Plasma theme` теперь берутся
/// ПЕРВЫМ, голым проходом — имя в ответе цело, — и вместе с ним в вывод
/// попадает «Также», которое движок дописывает к подписи без глагола («Также
/// тема Nordic Kvantum»). Прежде подмена случайно прибирала это слово, давая
/// «Nordic Kvantum тема». Терпим осознанно: тот же голый проход и лечит
/// «Теммы» из приёмки 24.09 (`live_acceptance_defects_of_24_09_are_fixed`), а
/// выбирать между двумя ответами нам нечем — оба формально целы.
///   * `Sweet Mars is a dark global theme with red accents.` (Sweet Mars):
///     до — «сладкий Марс - темная глобальная тема с красными акцентами.»,
///     после — «Sweet Mars - это темная глобальная тема с красными акцентами.»;
///   * `Nordic Kvantum theme based on the Nord palette.` (Nordic): до — «также
///     тема Nordic Kvantum основана на палитре Севера.», после — «Nordic
///     Kvantum тема на основе Nord палитры.»;
///   * `NT Legacy is a global theme for Plasma.` (NT Legacy): до — «NT Legacy
///     является глобальной темой для плазмы.», после — «NT Legacy является
///     глобальной темой для Plasma.»;
///   * `Install Layan and enjoy the rounded look of the desktop.` (Layan):
///     до — «установите Layan и наслаждайтесь округлым видом рабочего стола.»,
///     после — «Установите Layan и наслаждайтесь округлым видом рабочего
///     стола.»;
///
/// Замер 23.09, СОСТАВНЫЕ названия (дословно как печатает прогон):
///   * `Nordic is a dark theme created using the awesome Nord color palette.`
///     (Nordic KDE): до — «и скандинавская - это темная тема, созданная с
///     использованием удивительной цветовой палитры Nord.», после — «Nordic -
///     это темная тема, созданная с использованием потрясающей Nord цветовой
///     палитры.»;
///   * `Nordic Plasma theme`: до — «также тема Nordic Plasma», после — «Также
///     тема Nordic Plasma» (см. оговорку про короткие подписи ниже);
///   * `Nordic color scheme`: до — «скандинавская цветовая схема», после —
///     «Nordic цветовая схема»;
///   * `Sweet-Mars Kvantum theme` (Sweet Mars LAF Plasma 6): до — «сладкий
///     марс квантум тема», после — «Sweet-Mars Kvantum тема»;
///   * `Sweet Plasma theme`: до — «сладкая плазменная тема», после — «Sweet
///     Plasma тема»;
///   * `Sweet wallpaper`: до — «сладкие обои», после — «Sweet обои».
///
///   * ⛔ обратная сторона у товара с именем из словарного слова: `A dark and
///     quiet theme for everyone` (Dark) — до «темная и тихая тема для всех»,
///     после «Темная и тихая тема для всех»: строчное `dark` названием НЕ
///     считается и переводится, как и должно.
#[test]
#[ignore = "нужна скачанная модель Firefox en→ru (REXON_TRANSLATE_MODEL_DIR), в дерево не кладётся"]
fn live_item_name_survives_the_real_engine() {
    if std::env::var_os(te::MODEL_DIR_ENV).is_none() {
        std::env::set_var(te::MODEL_DIR_ENV, stand_model_dir());
    }
    let engine = te::LiveEngine::new();
    assert!(engine.available("en", "ru"), "модель не загрузилась");

    // Названия — живые (образцы 5, 6 и подписи товаров мастерской), последняя
    // строка — словарное однословное название.
    // Третий столбец — слово, которое ОБЯЗАНО остаться латиницей, либо `None`
    // для обратного случая (строчное словарное название переводится).
    let cases = [
        ("Nordic", "Nordic Kvantum theme", Some("Nordic")),
        ("Sweet", "Sweet Kvantum theme", Some("Sweet")),
        ("NT Legacy", "NT Legacy is a global theme for Plasma.", Some("NT Legacy")),
        ("Layan", "Install Layan and enjoy the rounded look of the desktop.", Some("Layan")),
        ("Sweet Mars", "Sweet Mars is a dark global theme with red accents.", Some("Sweet Mars")),
        ("Nordic", "Nordic Kvantum theme based on the Nord palette.", Some("Nordic")),
        ("Dark", "A dark and quiet theme for everyone", None),
        // ⭐ Приёмка 23.09, СОСТАВНЫЕ названия: строки дословно из описаний
        // живых товаров «Nordic KDE» (eliverlara) и «Sweet Mars LAF Plasma 6».
        // Целой цепочки слов названия в них нет ни в одной.
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

    // ДО: движок на голой фразе, без всякой подмены. Блок из ОДНОЙ фразы — тот
    // же размер блока, каким её спрашивает полный путь, иначе «до» и «после»
    // сравнивались бы в разных условиях (соседи по блоку меняют ответ).
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
            // Словарное название в строчной записи — обычное слово, и оно
            // ПЕРЕВЕДЕНО (иначе правило съедало бы прозу).
            assert!(!after.contains("dark"), "слово `dark` осталось латиницей: {after}");
        }
    }
}

/// ⭐ ЧЕТЫРЕ ДЕФЕКТА ЖИВОЙ ПРИЁМКИ 24.09 на настоящей модели: дословные строки
/// товаров и комментариев мастерской, по которым покупатель и увидел «перевод
/// не доехал».
///
/// Запуск — как у сверок выше:
///
/// ```sh
/// REXON_TRANSLATE_MODEL_DIR=/usr/lib/rexon/rexon/translate/enru \
///     cargo test -p rexon-core --release --test translator_engine \
///     -- --ignored --nocapture live_acceptance_defects
/// ```
///
/// Что нашлось (замер 24.09, дословно как печатает прогон):
///   * **D4a, комментарий viz67 к «Nordic KDE»** — «Very nice. / Can we expect
///     soon the Nordic Bluish theme of Gnome for KDE too?». Виноват был НЕ
///     фильтр прозы (он обе фразы считает прозой), а порча заполнителя:
///     на «Can we expect soon the @0@ Bluish theme of @1@ for @2@ too?» модель
///     отвечает «…@0@ Bluish тему @1@ @1@ для @2@ тоже?» — `@1@` слипшимся
///     повтором, сито отвергало перевод, и англичанином оставался ВЕСЬ
///     комментарий, включая «Very nice.». ДО: «Very nice.\nCan we expect soon
///     the Nordic Bluish theme of Gnome for KDE too?» (переведено=false);
///     ПОСЛЕ: «Очень мило.\nМожем ли мы ожидать в ближайшее время Nordic Bluish
///     тему Gnome для KDE тоже?».
///   * **D4b, комментарий maxkaspar к «Awesome Widgets»** — правило 4 фильтра
///     («значимое слово технично по форме») судило ЦЕЛЫЙ узел по ссылке во
///     втором предложении. ДО: «hddtemp shows only the first digit. here is a
///     screenshot http://…» (переведено=false); ПОСЛЕ: «интрампература
///     показывает только первую цифру. here is a screenshot http://…» — проза
///     переведена, ссылка дословно. ⛔ `hddtemp` → «интрампература» —
///     ОСТАЮЩИЙСЯ дефект модели: слова этого в списке имён нет и быть не может,
///     а голый движок калечит его тем же порядком («темп показывает только
///     первую цифру.» на целом комментарии).
///   * **D1, общий абзац автора «Nordic KDE» и «Sweet Mars»** — «Теммы» с
///     лишней буквой делал НАШ заполнитель, а не модель: «becoming a supporter
///     for the KDE Themes and Extensions section» голым движком — «стать
///     сторонником раздела KDE «Темы и расширения»» (верно!), та же фраза с
///     `@0@` на месте `KDE` — «стать сторонником раздела @0@ «Теммы и
///     расширения»». ПОСЛЕ (подмена тратится только там, где имя теряется):
///     «стать сторонником раздела KDE «Темы и расширения»».
///   * **D2, строка описания «Awesome Widgets»** — «graphical item support -
///     tooltips, bars» модель отдаёт как «служба графических элементов -
///     toolbep, bars». Заполнителей в строке нет вовсе (имён в ней нет), а
///     `toolbep` — не смесь алфавитов, а выдуманное моделью латинское слово
///     (коды: `U+0074 U+006F U+006F U+006C U+0062 U+0065 U+0070`). Лечению у
///     нас не подлежит, зато ловится сравнением с источником: ПОСЛЕ строка
///     остаётся оригиналом, а не кашей.
///
/// ⛔ **D3 остаётся за движком целиком**: «Nordic is a dark theme created using
/// the awesome <a …>Nord</a> color palette.» — `Nord` стоит внутри ссылки,
/// поэтому узлов три, и хвост « color palette.» модель переводит в одиночку как
/// «цветовая палитра цветов» (та же тавтология у голого движка на том же
/// обрывке). Наши заполнители тут не при чём — имён в хвосте нет.
#[test]
#[ignore = "нужна скачанная модель Firefox en→ru (REXON_TRANSLATE_MODEL_DIR), в дерево не кладётся"]
fn live_acceptance_defects_of_24_09_are_fixed() {
    if std::env::var_os(te::MODEL_DIR_ENV).is_none() {
        std::env::set_var(te::MODEL_DIR_ENV, stand_model_dir());
    }
    let engine = te::LiveEngine::new();
    assert!(engine.available("en", "ru"), "модель не загрузилась");

    // (товар, автор, текст) — дословно из приёмки.
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

    // D4a: переведены ОБА предложения комментария, имена латиницей.
    let comment = &out[0];
    assert!(comment.starts_with("Очень"), "первая фраза осталась английской: {comment}");
    assert!(!comment.contains("Can we expect"), "вторая фраза осталась английской: {comment}");
    for name in ["Nordic", "Gnome", "KDE"] {
        assert!(comment.contains(name), "имя {name} потерялось: {comment}");
    }

    // D4b: проза переведена, ссылка дословно.
    let screenshot = &out[1];
    assert!(
        screenshot.contains("http://i.imgur.com/abc.png"),
        "ссылку тронули: {screenshot}"
    );
    assert!(
        screenshot.chars().any(|c| ('а'..='я').contains(&c)),
        "проза рядом со ссылкой так и не переведена: {screenshot}"
    );

    // D1: лишней буквы в «Темы» нет, имя латиницей.
    let supporter = &out[2];
    assert!(!supporter.contains("Теммы"), "лишняя буква на месте: {supporter}");
    assert!(supporter.contains("Темы"), "раздел назван не по-русски: {supporter}");
    assert!(supporter.contains("KDE"), "имя KDE потерялось: {supporter}");

    // D2: выдуманное слово движка в вывод не пошло — строка осталась оригиналом.
    let tooltips = &out[3];
    assert!(!tooltips.contains("toolbep"), "каша движка уехала покупателю: {tooltips}");
    assert_eq!(
        tooltips.trim(),
        "graphical item support - tooltips, bars",
        "ожидался честный оригинал: {tooltips}"
    );
}

/// Живая сверка СЛОВАРЯ ПОПРАВОК ТЕРМИНОВ на настоящей модели: 18 фраз
/// описаний KDE Store с unzip/unpack/extract/download (кеш OCS, 03.10) и две
/// контрольные, где `extract` значит «извлечь».
///
/// Запуск — как у сверок выше:
///
/// ```sh
/// REXON_TRANSLATE_MODEL_DIR=$HOME/file-verify/mt-bench/models/firefox-enru \
///     cargo test -p rexon-core --test translator_engine \
///     -- --ignored --nocapture live_term_fixes
/// ```
///
/// Что стережёт: (1) в выводе не осталось ни одной формы, которую словарь
/// умеет поправить (вывод — неподвижная точка [`apply_term_fixes`]);
/// (2) «распак…» есть у всех фраз про архив, где движок вообще дал глагол;
/// (3) контрольные фразы словарь не тронул — «извлеч…» на месте; (4) техника
/// цела, заполнителей в выводе нет.
#[test]
#[ignore = "нужна скачанная модель Firefox en→ru (REXON_TRANSLATE_MODEL_DIR), в дерево не кладётся"]
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
