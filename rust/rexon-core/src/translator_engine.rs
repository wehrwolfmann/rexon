//! Живой автоперевод описаний: сборка ядра перевода из готовых частей в ОДНУ
//! дверь для окна — [`translate_live`] (и [`translate_live_batch`], та же
//! дверь на несколько текстов сразу).
//!
//! Части были готовы, а перевода не было: [`crate::translator`] отдаёт решение
//! («переводить или нет»), сегментацию HTML, ключ кеша и фильтр «проза или
//! техника»; [`crate::appearance_cache::LiveCache`] умеет читать и писать
//! переводы; вендоренный `fxtranslate` умеет переводить. Здесь они склеены, и
//! наружу смотрит одна функция: дал текст и локаль — получил перевод. Вкладке
//! не нужно знать ни про блоки предложений, ни про ключи кеша, ни про то, где
//! лежит модель.
//!
//! ⚠️ Что здесь ЕСТЬ, кроме склейки (в Python этого не было):
//!   * **движок один на процесс** ([`LiveEngine`], `OnceLock<Mutex<…>>`).
//!     Модель — 43 МБ отображённого файла и ~70 МБ подготовленных матриц;
//!     второй экземпляр на второй открытый товар удвоил бы это молча;
//!   * **блок для движка — не больше [`MAX_BLOCK_SENTENCES`] предложений**.
//!     Кодировщик пакетного пути добивает ВЕСЬ блок до самой длинной строки в
//!     нём, поэтому «переведём разом все сто предложений страницы» —
//!     не ускорение, а трата: одна длинная строка удорожает все короткие;
//!   * **блок набирается ПОПЕРЁК текстов** ([`translate_live_batch`]).
//!     Страница комментариев — это двадцать текстов по одной-две фразы, и
//!     по-одному они дали бы двадцать почти пустых блоков, то есть пакетный
//!     путь движка вхолостую;
//!   * **охраняемые имена от вызывающего** ([`translate_live_guarded`]).
//!     Название темы движок переводит («Nordic Kvantum theme» →
//!     «Скандинавская Kvantum тема»), а списка названий у ядра нет и быть не
//!     может — зато имя товара окно знает и передаёт его рядом с описанием;
//!   * **подмена имён тратится только по нужде** — два прохода в
//!     [`translate_misses`]: сначала предложение идёт к движку голым, и лишь
//!     если имя в ответе потерялось, оно переспрашивается под заполнителями.
//!     Заполнитель сам сбивает декодировщик (замер 24.09: `@0@` вместо `KDE`
//!     превращал «Темы» в «Теммы»), поэтому дешевле его не тратить;
//!   * **регистр первой буквы поднимаем сами** ([`lift_first_letter`]).
//!     Замер 20.09 на FLORES-200: со строчной буквы приходят 88,6 % строк
//!     (897 из 1012), и это не лечится ни ядром умножения, ни нашим патчем
//!     вендора — патч чинит ПОРЧУ первого слова, а регистр остаётся за нами;
//!   * **кеш — на КАЖДЫЙ сегмент**, а не на текст целиком. Описания
//!     мастерской повторяют друг друга абзацами («Installation:», «Copy the
//!     folder to ~/.themes», списки сред), и сегментный ключ делает второй
//!     показ страницы бесплатным.
//!
//! Модель в дереве НЕ лежит (43 МБ) — её место в поставке, каталог
//! [`MODEL_DIR_REL`] внутри пакета `rexon/`. Нет модели — движок честно
//! недоступен, [`translate_live`] возвращает оригинал и `was_translated =
//! false`; ошибок в лицо покупателю при этом нет, перевод — удобство, а не
//! условие работы вкладки.
//!
//! Сверки — `rust/rexon-core/tests/translator_engine.rs`: подставной движок со
//! счётчиком вызовов на временной базе (второй прогон того же текста — ноль
//! вызовов) и `#[ignore]`-сверка живого перевода на настоящей модели.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::appearance_cache::LiveCache;
use crate::translator::{
    detect_lang_heuristic, invented_latin_word, is_prose_segment, mask_names, plan_translate_text,
    probe_text, render_segments_with_flags, segment_html, translatable_flags_by_sentence,
    translation_cache_key, unmask_tech_names, user_lang_from_locale, MaskedNames, Segment,
    TranslatePlan,
};

// ============================================================================
// Движок: трейт
// ============================================================================

/// Движок перевода — ровно то, что от него нужно склейке.
///
/// Форма трейта отвечает двум фактам, а не вкусу:
///
///   1. **пара языков — часть вопроса о доступности.** Модели Firefox — по
///      одной на направление (`en→ru` — отдельный файл), поэтому «движок
///      доступен» без пары не имеет смысла: `en→ru` может быть в поставке,
///      `en→de` — нет. Это же и `_ensure_pair` оригинала, наблюдение которого
///      ждёт [`plan_translate_text`];
///   2. **перевод — пакетный.** Единица работы — БЛОК предложений, потому что
///      на блоке движок и мерился (кодировщик обрабатывает блок разом). Дай
///      трейту одну строку — и пакетный путь стал бы недостижим, а разница
///      измерима: 217 предложений/с на блоках против ~12 по одному.
///
/// Разбивать текст на предложения, пакетировать, лечить регистр и ходить в кеш
/// движку НЕ поручено: это делает склейка ([`translate_live`]), одинаково для
/// живого движка и для подставного в сверках.
pub trait TranslateEngine {
    /// Готов ли перевод `src`→`tgt` прямо сейчас: модель на месте и
    /// загрузилась. Ложь — не ошибка, а «перевода не будет».
    fn available(&self, src: &str, tgt: &str) -> bool;

    /// Перевести блок предложений одним заходом.
    ///
    /// Договор: длина и порядок ответа РАВНЫ входу; пустая строка на месте
    /// предложения — «не справился, бери оригинал». Ответ другой длины
    /// склейка считает отказом на весь блок.
    fn translate_block(&self, src: &str, tgt: &str, block: &[&str]) -> Vec<String>;
}

/// Сколько предложений отдавать движку за раз.
///
/// ⚠️ Не «чем больше, тем лучше». Пакетный кодировщик добивает блок до самой
/// длинной строки в нём, то есть одно длинное предложение оплачивается за все
/// короткие. Замер 20.09 гонял блок 8 (1012 предложений FLORES-200 за 4,67 с
/// на четырёх потоках); рабочий предел — 8–16, дальше растёт только паддинг.
/// Берём верхнюю границу: описания оформления короткие и похожей длины.
pub const MAX_BLOCK_SENTENCES: usize = 16;

/// Сколько потоков данных отдавать пакетному пути движка.
///
/// Не `0` (= все ядра машины) намеренно: перевод идёт, пока человек смотрит на
/// вкладку, и забирать под него весь процессор незачем — страница из двадцати
/// описаний переводится около секунды и на одном. Четыре — то число, на котором
/// сделан замер пропускной способности.
const ENGINE_THREADS: usize = 4;

// ============================================================================
// Где лежит модель
// ============================================================================

/// Переменная окружения с каталогом модели — путь для РАЗРАБОТКИ (модель в
/// дерево не кладётся, а стендовая копия лежит у каждого своя).
pub const MODEL_DIR_ENV: &str = "REXON_TRANSLATE_MODEL_DIR";

/// Каталог моделей перевода внутри пакета данных `rexon/`; внутри него — по
/// каталогу на направление (`translate/enru`).
///
/// Владелец решил класть модель В ПАКЕТ, поэтому ищется она там же и тем же
/// порядком, что вендорный QML вкладки обоев и `themes.json`, —
/// [`crate::installer::package_data_dir_candidates`] (дерево разработчика →
/// пакет дистрибутива → установка в систему → установка в домашнюю папку).
/// Своего порядка поиска здесь не заводится: вторая правда о том, где лежит
/// продукт, уже однажды стоила нам ненайденных QML-плагинов.
pub const MODEL_DIR_REL: &str = "translate";

/// Файлы одной модели направления.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFiles {
    /// Веса (`model.enru.intgemm.alphas.bin`).
    pub model: String,
    /// Словарь источника (`vocab.enru.spm`).
    pub src_vocab: String,
    /// Словарь цели: у пар с общим словарём — тот же файл.
    pub trg_vocab: String,
}

/// Имя направления в именах файлов Firefox: `en` + `ru` = `enru`.
pub fn model_pair(src: &str, tgt: &str) -> String {
    format!("{}{}", src.to_lowercase(), tgt.to_lowercase())
}

/// Каталоги, где может лежать модель направления, по убыванию предпочтения.
///
/// `env_dir` — значение [`MODEL_DIR_ENV`] (каталог с файлами модели ПЛОСКО,
/// как его отдаёт стенд); `repo_root` — корень дерева разработчика
/// ([`crate::cli::repo_root`]), `home` — домашний каталог.
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

/// Файлы модели в ОДНОМ каталоге либо `None`, если их там нет.
///
/// Раскладка плоская — та же, что у скачанной модели Firefox:
/// `model.<пара>.intgemm.alphas.bin` и `vocab.<пара>.spm`. У пар с раздельными
/// словарями (CJK) вместо общего `vocab.…` лежат `srcvocab.…`/`trgvocab.…`;
/// поддержано, хотя в поставке пока только `en→ru` с общим словарём.
///
/// ⛔ Короткий список (`lex.*.bin`) не ищется намеренно: замер 20.09 — минус
/// 0,4 chrF++ и плюс 2,8 МБ поставки за ускорение, которого на десятках
/// коротких описаний не видно.
///
/// Проба файловой системы приходит колбэком: ядро на диск само не ходит.
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

/// Первая найденная модель направления из [`model_dir_candidates`].
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

/// Модель направления на ЖИВОЙ машине: окружение + дерево/поставка.
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
// Движок: живая реализация поверх fxtranslate
// ============================================================================

/// Работа для потока модели: блок предложений и куда положить ответ.
struct Job {
    block: Vec<String>,
    reply: std::sync::mpsc::Sender<Vec<String>>,
}

/// Поток, на котором живёт загруженная модель.
struct Worker {
    jobs: std::sync::mpsc::Sender<Job>,
}

/// Направление, для которого движок уже поднимали. `worker = None` — пробовали
/// и не вышло (модели нет либо файл не прочитался); попытка не повторяется на
/// каждый сегмент, иначе на каждой строке описания молотили бы диск.
struct Loaded {
    pair: String,
    worker: Option<Worker>,
}

/// Движок — ОДИН на процесс: одна загруженная модель, один поток.
///
/// ⚠️ Почему модель живёт на ОТДЕЛЬНОМ потоке, а в замке лежит только его
/// почтовый ящик. `Engine` крейта — `Sync` (это в крейте даже проверено
/// статически), но НЕ `Send`: внутри подготовленной матрицы лежит сырой
/// указатель на память, выданную шимом на C++. Значит `Mutex<Engine>` в
/// статической переменной невозможен в принципе — не из-за нашей
/// осторожности, а по типам, и «пробить» это `unsafe impl Send` значило бы
/// подписаться за чужую библиотеку без доказательств. Модель загружается там,
/// где живёт, и никуда не переезжает; наружу ходят только строки.
///
/// Замок при этом по-прежнему нужен, и не ради памяти: он не даёт двум
/// вкладкам поднять модель дважды (43 МБ отображённого файла и ~70 МБ
/// подготовленных матриц) и заодно не даёт тяжёлому пакетному проходу
/// делиться процессором сам с собой.
static ENGINE: OnceLock<Mutex<Option<Loaded>>> = OnceLock::new();

fn engine_slot() -> &'static Mutex<Option<Loaded>> {
    ENGINE.get_or_init(|| Mutex::new(None))
}

/// Поднять поток с моделью направления. `None` — модели нет либо не прочиталась.
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
                // Модель на месте, но не прочиталась (обрезанный файл, чужая
                // версия) — движок недоступен, вкладка работает без перевода.
                Err(_) => {
                    let _ = ready.send(false);
                    return;
                }
            };
            if ready.send(true).is_err() {
                return;
            }
            drop(ready);
            // Ящик закрылся вместе с процессом — поток уходит сам.
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

/// Дать делу `f` почтовый ящик модели направления `src`→`tgt`, либо `None`.
///
/// ⚠️ Второе направление в том же процессе НЕ поднимается: это ещё 43 МБ
/// модели и ~70 МБ матриц за перевод, которого никто не просил (целевой язык у
/// пользователя один на сессию — его локаль). Спросили другое направление —
/// ответ «недоступно».
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

/// Живой движок: вендоренный `fxtranslate` на модели из поставки.
///
/// Экземпляр ничего не держит — вся тяжесть в [`ENGINE`]; заводить его на
/// каждую работу вкладки можно свободно.
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
        // Поток умер или ящик закрыт — отказ на весь блок (договор трейта).
        done.flatten().unwrap_or_else(|| vec![String::new(); block.len()])
    }
}

// ============================================================================
// Лечение регистра первой буквы
// ============================================================================

/// Поднять первую букву перевода, если в источнике она заглавная.
///
/// Дефект движка, воспроизведённый на 88,6 % строк FLORES-200: перевод
/// приходит со строчной буквы («поддерживаемые настольные компьютеры: …»).
/// Лечим по ИСТОЧНИКУ, а не безусловно: текстовый узел HTML может быть
/// продолжением фразы после `<b>…</b>` и начинаться со строчной законно.
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
// Склейка: перевод одного описания
// ============================================================================

/// Итог [`translate_live`]: текст (переведённый либо исходный) и признак того,
/// что перевод действительно случился (порт `was_translated` оригинала).
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

/// Перевести описание: от локали пользователя до готовой строки.
///
/// Порядок — ровно тот, что был у Python, плюс наши две вставки (фильтр прозы
/// и сегментный кеш):
///   1. целевой язык — [`user_lang_from_locale`] из строки локали;
///   2. язык источника — [`detect_lang_heuristic`] по [`probe_text`]
///      (видимый текст без разметки);
///   3. РЕШЕНИЕ — [`plan_translate_text`]: пусто, уже на целевом языке или
///      движка нет → оригинал без всякой работы;
///   4. разбор на сегменты [`segment_html`] и отбор прозы
///      ([`translatable_flags`]) — техника движку не достаётся;
///   5. кеш на каждый сегмент ([`translation_cache_key`]) — ЧТЕНИЕ до движка;
///   6. движок на то, чего в кеше не нашлось: предложения, блоки по
///      [`MAX_BLOCK_SENTENCES`], лечение регистра;
///   7. ЗАПИСЬ в кеш и сборка строки [`render_segments_filtered`].
///
/// `locale` — строка локали пользователя (`ru_RU.UTF-8`, `en_US`, пусто);
/// получение её — дело вызывающего, как и в оригинале.
///
/// Это один текст [`translate_live_batch`], и второго пути перевода в дереве
/// нет намеренно (правило #12): описание панели и страница комментариев ходят
/// к движку одной дорогой.
pub fn translate_live(
    text: &str,
    locale: &str,
    engine: &dyn TranslateEngine,
    cache: &mut LiveCache,
) -> LiveTranslation {
    translate_live_guarded(text, locale, engine, cache, &[])
}

/// То же, что [`translate_live`], но вызывающий добавляет СВОИ охраняемые
/// имена: название открытого товара, имя автора — всё, чего в списке ядра быть
/// не может.
///
/// ⭐ ЗАЧЕМ. Имена технологий ядро защищает по своему списку, а НАЗВАНИЕ ТЕМЫ
/// защитить было нечем — списка названий нет и быть не может. Живой движок
/// отвечал «Скандинавская Kvantum тема» на «Nordic Kvantum theme» и «Сладкая
/// Kvantum тема» на «Sweet Kvantum theme». Название товара окно ЗНАЕТ (оно
/// приходит рядом с описанием), поэтому ядро берёт имена от него и прячет их
/// тем же приёмом заполнителей — см. [`crate::translator::mask_names`].
///
/// Договор списка: пустые и коротенькие (1-2 знака) имена ядро отбрасывает
/// само, так что вызывающему нечего проверять — можно отдать название и имя
/// автора как есть. Регистр: имя из нескольких слов узнаётся в любом регистре,
/// имя из ОДНОГО слова — только с большой буквы или заглавными (иначе товар
/// «Dark» съел бы слово `dark` в прозе).
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

/// Перевести НЕСКОЛЬКО текстов одним походом к движку. Ответ — по тексту на
/// текст, в том же порядке.
///
/// Порядок работы тот же, что у [`translate_live`], но пакетирование сквозное:
/// решение и разбор считаются на КАЖДЫЙ текст свои (иначе русский комментарий
/// среди английских уехал бы к движку), а вот блоки предложений набираются
/// ПОПЕРЁК ТЕКСТОВ — один заход на всю страницу.
///
/// ⭐ Ради этого функция и появилась: комментарии мастерской короткие (одна-две
/// фразы) и их двадцать на страницу. Двадцать вызовов `translate_live` дали бы
/// двадцать блоков по одному-два предложения, то есть пакетный путь движка
/// вхолостую (217 предложений/с на блоках против ~12 по одному, замер 20.09).
/// Здесь те же двадцать комментариев уезжают двумя-тремя полными блоками.
///
/// Текст, которому [`plan_translate_text`] сказал «не переводить» (пусто, уже
/// на языке покупателя, движка нет), возвращается неизменным и с
/// `was_translated = false` — по нему вкладка и понимает, что ссылку «показать
/// оригинал» рисовать не надо.
pub fn translate_live_batch(
    texts: &[&str],
    locale: &str,
    engine: &dyn TranslateEngine,
    cache: &mut LiveCache,
) -> Vec<LiveTranslation> {
    translate_live_batch_guarded(texts, locale, engine, cache, &[])
}

/// То же, что [`translate_live_batch`], с охраняемыми именами вызывающего
/// ([`translate_live_guarded`]). Список ОДИН на весь пакет: страница
/// комментариев — это комментарии к ОДНОМУ товару, и название у них общее.
pub fn translate_live_batch_guarded(
    texts: &[&str],
    locale: &str,
    engine: &dyn TranslateEngine,
    cache: &mut LiveCache,
    guarded: &[&str],
) -> Vec<LiveTranslation> {
    let target = user_lang_from_locale(locale);

    // Доступность пары спрашивается ОДИН раз на пару: у живого движка это
    // поиск модели на диске и её загрузка, а текстов здесь двадцать.
    let mut availability: HashMap<String, bool> = HashMap::new();

    // Разбор и решение — на каждый текст свои.
    let mut prepared: Vec<Option<Prepared>> = Vec::with_capacity(texts.len());
    // Пары языков по порядку появления: обычно она одна (`en`→`ru`), но
    // страница комментариев вполне может оказаться разноязыкой.
    let mut pairs: Vec<(String, String)> = Vec::new();
    // Уникальные тексты к переводу под каждой парой: описания и комментарии
    // повторяют друг друга, и дважды платить за одну строку незачем.
    let mut wanted: Vec<Vec<String>> = Vec::new();

    for text in texts {
        let probe = probe_text(text);
        let detected = detect_lang_heuristic(&probe);
        // Кандидат источника — тот же, что выберет решение: детект либо `en`.
        let candidate = detected.as_deref().filter(|s| !s.is_empty()).unwrap_or("en");
        // `src_pair_ensurable` оригинала = «путь для ЭТОГО кандидата
        // собирается»; `engine_available` = «хоть какой-то путь есть» (решение
        // откатывается на `en`, и если модели нет и для него — переводить
        // нечем).
        let pair_ready = pair_available(engine, &mut availability, candidate, &target);
        let any_ready = pair_ready || pair_available(engine, &mut availability, "en", &target);

        let plan = plan_translate_text(text, &target, detected.as_deref(), any_ready, pair_ready);
        let TranslatePlan::Translate { src, tgt, .. } = plan else {
            prepared.push(None);
            continue;
        };

        // ⚠️ Путь один на HTML и на плоский текст, и это не упрощение: у
        // Python плоский путь выбирался, когда `looks_like_html` ложно, то
        // есть в тексте НЕТ ни тегов, ни сущностей, — а на таком тексте
        // `segment_html` даёт ровно один переводимый узел. Поведение то же,
        // ветки на одну меньше.
        let segments = segment_html(text);
        // Проза решается ПО ПРЕДЛОЖЕНИЯМ теми же границами, какими текст потом
        // уезжает движку: узел «hddtemp shows only the first digit. here is a
        // screenshot http://…» отдаётся не целиком и не отвергается целиком.
        let flags = translatable_flags_by_sentence(&segments, &sentence_texts);

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
        prepared.push(Some(Prepared { segments, flags, pair }));
    }

    // Кеш — ДО движка; движок — на остаток, и ЗАПИСЬ в кеш сразу за ним.
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
        // ⚠️ Ключ кеша охраняемых имён НЕ содержит — намеренно. Ключ считается
        // по тексту сегмента, а имя подменяется только там, где оно в этом
        // тексте есть; добавь имя в ключ — и общая для всех товаров строка
        // («Installation:», «Copy the folder to ~/.themes») кешировалась бы
        // заново на каждый товар, то есть сегментный кеш потерял бы смысл.
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
        let rendered = render_segments_with_flags(&prep.segments, &prep.flags, |s| {
            done.get(s).cloned().unwrap_or_default()
        });
        out.push(LiveTranslation {
            was_translated: rendered.trim() != text.trim(),
            text: rendered,
        });
    }
    out
}

/// Текст, которому решение сказало «переводить»: его сегменты, признаки «звать
/// ли движок» и номер пары языков в общем списке пакета.
///
/// Признаки хранятся, а не считаются заново на сборке: считать их дважды —
/// значит рисковать тем, что отбор и сборка разойдутся, и переведённый узел
/// уедет в вывод оригиналом.
struct Prepared {
    segments: Vec<Segment>,
    flags: Vec<bool>,
    pair: usize,
}

/// Границы предложений одного текста — тем разбивателем, каким пользуется сам
/// крейт перевода. ОДНА правда о границах на весь модуль: по ней и отбирается
/// проза, и набираются блоки для движка.
fn sentence_texts(text: &str) -> Vec<String> {
    use fxtranslate::segment::{BasicSegmenter, Segmenter};
    BasicSegmenter.sentences(text).iter().map(|s| s.of(text).to_string()).collect()
}

/// Готова ли пара `src`→`tgt`, с памятью об уже спрошенном.
///
/// Памятка не украшение: у живого движка [`TranslateEngine::available`] — это
/// поиск файлов модели и её загрузка, а в пакете страницы комментариев вопрос
/// задавался бы на каждый из двадцати текстов заново.
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

/// Перевести тексты, которых не было в кеше. `None` на месте текста — движок
/// не справился (ни одного предложения), такой результат в кеш не пишется:
/// пусть следующая попытка спросит движок снова.
///
/// ⭐ ДВА ПРОХОДА, и это замер, а не осторожность. Заполнитель имени САМ сбивает
/// декодировщик: «becoming a supporter for the KDE Themes and Extensions
/// section» живая модель переводит правильно — «стать сторонником раздела KDE
/// «Темы и расширения»», — а та же фраза с `@0@` на месте `KDE` возвращается с
/// «Теммы и расширения» (приёмка 24.09, общий абзац автора у товаров «Nordic
/// KDE» и «Sweet Mars»). Значит подмену надо тратить только там, где без неё
/// имя действительно теряется:
///   * **проход 1** — предложение ГОЛЫМ, как написал человек. Ответ берётся,
///     если каждое охраняемое имя стоит в нём ДОСЛОВНО;
///   * **проход 2** — только те предложения, где движок имя перевёл или
///     склонил («Nordic» → «скандинавская»): они уходят под заполнителями, и
///     дальше работают прежние сита [`unmask_tech_names`].
/// Второй поход к движку платится только за испорченные предложения, а
/// большинство фраз описания вообще перестало видеть заполнители.
fn translate_misses(
    misses: &[&str],
    src: &str,
    tgt: &str,
    engine: &dyn TranslateEngine,
    guarded: &[&str],
) -> Vec<Option<String>> {
    use fxtranslate::segment::{BasicSegmenter, Segmenter, Span};

    // ⚠️ Предложения, а не целые узлы: пакетный путь движка кормит каждую
    // строку декодировщику ЦЕЛИКОМ, и абзац длиннее окна модели (127 токенов)
    // молча обрубается — декодировщик выдаёт EOS раньше конца. Границы берёт
    // тот же разбиватель, каким пользуется сам крейт.
    let spans: Vec<Vec<Span>> = misses.iter().map(|t| BasicSegmenter.sentences(t)).collect();

    // Плоский список предложений всех текстов — чтобы блок набирался ПОПЕРЁК
    // узлов: иначе абзац из одного предложения ехал бы к движку в одиночку.
    let mut flat: Vec<&str> = Vec::new();
    for (text, spans) in misses.iter().zip(spans.iter()) {
        for span in spans.iter() {
            flat.push(span.of(text));
        }
    }

    // Кого движку не отдавать вовсе: предложение-техника (команда, строка из
    // одного пути или ссылки) остаётся оригиналом даже посреди переводимого
    // абзаца — это то же правило, каким отбирался сам узел
    // ([`translatable_flags_by_sentence`]). Путь или ссылка ВНУТРИ прозы
    // (с 03.10) предложение не отвергают: [`mask_names`] прячет их наравне с
    // именами, и ниже они сверяются и возвращаются тем же путём.
    let prose: Vec<bool> = flat.iter().map(|s| is_prose_segment(s)).collect();

    let mut done: Vec<String> = vec![String::new(); flat.len()];
    // Пометка «заполнители не сошлись»: перевод этого предложения испорчен.
    let mut spoiled: Vec<bool> = vec![false; flat.len()];

    // ── проход 1: голые предложения, без заполнителей.
    let bare: Vec<usize> = (0..flat.len()).filter(|i| prose[*i]).collect();
    let asked: Vec<&str> = bare.iter().map(|i| flat[*i]).collect();
    // ⚠️ Имена (технологий — по списку ядра, товара и автора — от вызывающего)
    // прячутся ЗДЕСЬ, то есть ПОСЛЕ разбиения на предложения, а не до него.
    // Порядок важен: границы предложений считает сам крейт по исходному тексту,
    // и подмена, сделанная раньше, сдвинула бы их (а `reassemble` ниже
    // восстанавливает пробелы по ОРИГИНАЛУ и требует спанов именно от него).
    let mut retry: Vec<usize> = Vec::new();
    for (i, answer) in bare.iter().zip(translate_blocks(engine, src, tgt, &asked).iter()) {
        let source = flat[*i];
        if answer.is_empty() {
            // Движок не справился — в выводе останется оригинал предложения.
            continue;
        }
        let names = mask_names(source, guarded).names;
        if !names.iter().all(|n| name_stands_verbatim(answer, n)) {
            retry.push(*i);
            continue;
        }
        if let Some(text) = take_if_clean(src, tgt, source, answer) {
            done[*i] = text;
        }
    }

    // ── проход 2: те же предложения, но имена под заполнителями.
    let hidden: Vec<MaskedNames> = retry.iter().map(|i| mask_names(flat[*i], guarded)).collect();
    let asked: Vec<&str> = hidden.iter().map(|m| m.text.as_str()).collect();
    for ((i, masked), answer) in
        retry.iter().zip(hidden.iter()).zip(translate_blocks(engine, src, tgt, &asked).iter())
    {
        if answer.is_empty() {
            continue;
        }
        match unmask_tech_names(answer, &masked.names) {
            Some(restored) => {
                if let Some(text) = take_if_clean(src, tgt, flat[*i], &restored) {
                    done[*i] = text;
                }
            }
            // Заполнители не сошлись — предложение пустое, а сегмент целиком
            // помечен испорченным (ниже).
            None => spoiled[*i] = true,
        }
    }

    let mut out: Vec<Option<String>> = Vec::with_capacity(misses.len());
    let mut at = 0usize;
    for (text, spans) in misses.iter().zip(spans.iter()) {
        let mine = &done[at..at + spans.len()];
        let broken = spoiled[at..at + spans.len()].iter().any(|b| *b);
        at += spans.len();
        // ⚠️ Одно испорченное предложение отменяет ВЕСЬ сегмент, а не только
        // себя. Лучше не перевести, чем отдать кашу: абзац, где одна фраза
        // русская, а вторая осталась английской, выглядит хуже честного
        // оригинала, а перевод с потерянным именем — просто неправда. В кеш
        // такой сегмент не пишется (`None`), и следующая попытка спросит
        // движок заново.
        if broken || mine.is_empty() || mine.iter().all(|s| s.is_empty()) {
            out.push(None);
            continue;
        }
        // Пустое место — оригинал предложения; пробелы между предложениями
        // восстанавливает сам крейт из исходной строки.
        let pieces: Vec<String> = spans
            .iter()
            .zip(mine.iter())
            .map(|(span, t)| if t.is_empty() { span.of(text).to_string() } else { t.clone() })
            .collect();
        out.push(Some(fxtranslate::segment::reassemble(text, spans, &pieces)));
    }
    out
}

/// Отдать предложения движку блоками по [`MAX_BLOCK_SENTENCES`]. Ответ — по
/// строке на каждое предложение и в том же порядке; пустая строка означает
/// «движок не справился, бери оригинал».
fn translate_blocks(
    engine: &dyn TranslateEngine,
    src: &str,
    tgt: &str,
    asked: &[&str],
) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(asked.len());
    for block in asked.chunks(MAX_BLOCK_SENTENCES) {
        let mut answer = engine.translate_block(src, tgt, block);
        // Ответ не той длины — отказ на весь блок (договор трейта). Молча
        // сдвинуть переводы на чужие предложения было бы хуже отказа.
        if answer.len() != block.len() {
            answer = vec![String::new(); block.len()];
        }
        out.extend(answer.into_iter().map(|t| t.trim().to_string()));
    }
    out
}

/// Стоит ли имя в ответе движка ДОСЛОВНО и отдельным словом.
///
/// ⚠️ Вхождения подстрокой мало, и это не придирка: `Nord` находится внутри
/// `Nordic`, поэтому ответ «также тема Nordic Kvantum основана на палитре
/// Севера.» сошёл бы за целый — хотя само слово `Nord` в нём переведено
/// («Севера»), а именно его мы и охраняем. Границей считается любой
/// не-буквенно-цифровой знак либо край строки.
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

/// Взять перевод предложения, подняв первую букву, либо `None` — брать нельзя.
///
/// Отказ один, и он про порчу, которую не видят ни фильтр прозы, ни сита
/// заполнителей: ВЫДУМАННОЕ латинское слово ([`invented_latin_word`]) — «служба
/// графических элементов - toolbep, bars» вместо `tooltips` (приёмка 24.09,
/// «Awesome Widgets»). Отказ действует на ОДНО предложение: остальные фразы
/// абзаца переводом остаются, а на месте испорченной встанет оригинал.
///
/// Перед подъёмом буквы термины правятся по словарю
/// ([`crate::translator::apply_term_fixes`]: «расстегните» от `unzip` →
/// «распакуйте»). Здесь, а не раньше: перевод уже с возвращёнными именами и
/// путями, и сито выдуманной латиницы уже пройдено.
fn take_if_clean(src: &str, tgt: &str, source: &str, translated: &str) -> Option<String> {
    if invented_latin_word(source, translated).is_some() {
        return None;
    }
    let fixed = crate::translator::apply_term_fixes(src, tgt, source, translated);
    Some(lift_first_letter(source, &fixed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Порядок поиска модели: окружение первым, дальше — раскладки поставки,
    /// и ровно те же, какими ищется вендорный QML (одна правда о том, где
    /// лежит продукт).
    #[test]
    fn model_dirs_start_with_the_environment_then_follow_the_package() {
        let dirs = model_dir_candidates(Some("/стенд/модель"), "/дерево", "/home/w", "en", "ru");
        assert_eq!(dirs[0], "/стенд/модель");
        let pkg = crate::installer::package_data_dir_candidates("/дерево", "/home/w");
        assert_eq!(dirs.len(), 1 + pkg.len());
        for (candidate, base) in dirs[1..].iter().zip(pkg.iter()) {
            assert_eq!(candidate, &format!("{base}/translate/enru"));
        }
        // Пустая переменная — не кандидат «» (иначе искали бы от корня).
        let blank = model_dir_candidates(Some("  "), "", "", "en", "ru");
        assert!(
            blank.iter().all(|d| d.starts_with('/')),
            "пустая переменная попала в кандидаты: {blank:?}"
        );
    }

    /// Модель признаётся по весам + словарю; общий словарь и раздельные —
    /// обе раскладки Firefox.
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

        // Веса без словаря — не модель.
        let weights_only = |p: &str| p == "/m/model.enru.intgemm.alphas.bin";
        assert!(model_files_in("/m", "en", "ru", &weights_only).is_none());
        // Словарь без весов — тоже.
        let vocab_only = |p: &str| p == "/m/vocab.enru.spm";
        assert!(model_files_in("/m", "en", "ru", &vocab_only).is_none());
    }

    /// Регистр поднимается по ИСТОЧНИКУ: заглавная в источнике — поднимаем,
    /// строчная — оставляем (продолжение фразы после тега).
    #[test]
    fn the_first_letter_follows_the_source() {
        assert_eq!(lift_first_letter("This theme", "эта тема"), "Эта тема");
        assert_eq!(lift_first_letter("This theme", "Эта тема"), "Эта тема");
        assert_eq!(lift_first_letter("and also", "и ещё"), "и ещё");
        // Ведущая не-буква сохраняется, поднимается первая БУКВА.
        assert_eq!(lift_first_letter("«Dark» theme", "«тёмная» тема"), "«Тёмная» тема");
        // Букв нет вовсе — строка не трогается.
        assert_eq!(lift_first_letter("Theme", "— 1.2.3 —"), "— 1.2.3 —");
    }
}
