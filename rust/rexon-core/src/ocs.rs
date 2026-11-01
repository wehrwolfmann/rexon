//! Клиент OCS API каталога «Оформление» — порт `rexon/appearance/ocs_client.py`
//! ЦЕЛИКОМ: и разбор ответов, и сетевой слой.
//!
//! Разбор: XML-ответы OCS → модели (`parse_list`/`parse_content`/
//! `parse_comments`), выбор лучшего для Linux download-линка
//! (`link_score`/`pick_best_link`), нормализация URL (`safe_url`, порт
//! `urllib.parse.urlsplit`+`quote`+`urlunsplit`).
//!
//! Сеть: [`http_get`]/[`http_download`] (порт `_http_get`/`_http_download`) и
//! [`OcsClient`] (порт класса `OCSClient`). Сам вызов спрятан за трейтом
//! [`NetIo`] (`net.rs`), SQLite-кеш — за [`OcsCacheIo`], поэтому логика
//! «кеш → сеть → устаревший кеш» тестируется на ЗАПИСАННЫХ ответах, а не на
//! живом api.opendesktop.org. Прежнее решение «HTTP остаётся на Python»
//! отменено: цель — убрать Python отовсюду, кроме интерфейса.
//!
//! Сверка с Python: эталоны сняты с самого `ocs_client.py` (`_parse_list`/
//! `_flatten_comments`/`_safe_url`/`_link_score`/`_pick_best_link` на РЕАЛЬНОМ
//! ответе api.opendesktop.org, снятом curl; сетевые ветки — с монипатчем
//! `urlopen`/кеша/файловых операций), Rust обязан вернуть те же модели,
//! те же строки и тот же порядок. Голдены — `tests/ocs_parity.rs` (разбор) и
//! `tests/net_parity.rs` (сетевые ветки).

use crate::models::ThemeItem;
use crate::net::{self, LogLevel, NetError, NetIo};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Serialize;
use serde_json::{json, Value};

/// Базовый URL OCS API (совпадает с `API_BASE`).
pub const API_BASE: &str = "https://api.opendesktop.org/ocs/v1";
/// User-Agent запросов (`USER_AGENT`).
pub const USER_AGENT: &str = "Rexon/5.0";
/// Таймаут запроса, секунды (`TIMEOUT`).
pub const TIMEOUT_SECS: u64 = 25;

/// Таймаут скачивания архива темы, секунды (`_http_download`, значение по
/// умолчанию аргумента `timeout`).
pub const DOWNLOAD_TIMEOUT_SECS: u64 = 120;

/// Потолок размера архива темы, байт (`max_size` по умолчанию, 600 МиБ).
pub const DOWNLOAD_MAX_SIZE: i64 = 600 * 1024 * 1024;

/// Таймаут скачивания превью, секунды (`get_preview_image`).
pub const PREVIEW_TIMEOUT_SECS: u64 = 20;

/// Потолок размера превью, байт (20 МиБ).
pub const PREVIEW_MAX_SIZE: i64 = 20 * 1024 * 1024;

/// Сколько живут ПОЛНЫЕ детали товара в кеше для ПОКАЗА, секунды (сутки,
/// `DETAIL_TTL` оригинала).
///
/// Детальный ответ pling шёл до 20 с, и столько человек смотрел на пустую
/// панель, открыв товар из уже показанной сетки; описание темы за сутки не
/// меняется. Установка этим сроком НЕ пользуется: ей нужны свежие ссылки на
/// архив, она зовёт [`OcsClient::get_item`].
pub const DETAIL_TTL_SECS: u64 = 24 * 3600;

/// Размер куска при потоковом скачивании (`resp.read(65536)`).
pub const DOWNLOAD_CHUNK: usize = 65536;

/// Допустимые режимы сортировки — ключи и значения `SORT_MAP` (у OCS они
/// совпадают, отображение нужно лишь чтобы отсечь мусор из UI-комбобокса).
pub const SORT_MODES: &[&str] = &["new", "down", "high", "alpha"];

/// Расширения архивов, которые умеет распаковать установщик (`_ARCHIVE_EXTS`).
/// Порядок важен: `_link_score` берёт ПЕРВОЕ совпадение (`.tar.gz` до `.tar`).
pub const ARCHIVE_EXTS: &[&str] = &[
    ".tar.xz", ".tar.zst", ".tar.gz", ".tgz", ".tar.bz2", ".tbz2", ".tar.lz",
    ".tar", ".zip", ".7z",
];

/// Готовые пакеты KPackage: внутри zip, распаковщик берёт их по содержимому.
/// Так выкладывают виджеты (`.plasmoid`), скрипты KWin (`.kwinscript`) и
/// прочие пакеты Plasma (`.kpackage`) — это и есть то, что ставит Discover.
/// До 05.10.2026 их не знали, и у товара «HTML Widget Host» при равных
/// баллах выбирался README.md, лежавший первым.
pub const KPACKAGE_EXTS: &[&str] = &[".plasmoid", ".kpackage", ".kwinscript"];

/// Пакеты дистрибутивов и установщики: ставятся своим менеджером под одну
/// систему, а не раскладываются в каталог темы. `.pkg.tar.zst` по
/// расширению — тот же tar, и до 05.10.2026 получал баллы архива: у плагина
/// «Wallpaper Engine» выбирался пакет pacman вместо лежащего рядом готового
/// KPackage `com.github.catsout.wallpaperEngineKde.tar.gz`.
pub const DISTRO_PACKAGE_EXTS: &[&str] = &[
    ".pkg.tar.zst", ".pkg.tar.xz", ".pkg.tar.gz", ".pkg.tar", ".deb", ".rpm",
    ".appimage", ".snap", ".flatpak", ".flatpakref", ".apk", ".exe", ".msi",
    ".dmg", ".pkg",
];

/// Текст при товаре (описание, лицензия, список изменений) — ставить нечего.
pub const DOC_EXTS: &[&str] = &[".md", ".txt", ".pdf", ".html", ".htm", ".rst"];

/// Штраф пакету дистрибутива/установщику (см. [`DISTRO_PACKAGE_EXTS`]):
/// ниже любого архива и готового пакета, но выше голого текста.
const DISTRO_PACKAGE_PENALTY: i64 = 150;

/// Штраф тексту (README и т.п.): самый низкий балл из всех, чтобы он не
/// выигрывал ни у чего, что вообще можно поставить.
const DOC_PENALTY: i64 = 1000;

/// Надбавка готовому пакету KPackage для вида, который ставится KPackage-ом
/// (виджеты, плагины обоев, скрипты и эффекты KWin…): такой пакет —
/// ровно то, что ставит Discover, его брать раньше прочих архивов.
const KPACKAGE_KIND_BONUS: i64 = 30;

/// Слова имени файла, которыми авторы помечают ДОПОЛНЕНИЕ к основному
/// архиву товара: значки, обои, шрифты «в нагрузку» (`Extra 1.4.tar.xz`,
/// `extra-Icons.zip`, `Extra's.tar.xz`). Сравниваются целые слова имени
/// (разделитель — любой знак, кроме буквы и цифры).
///
/// ⛔ Дефект 17.10.2026: у товаров McOS (GTK 135, Cinnamon 133) файлы идут
/// по алфавиту, «Extra 1.4.tar.xz» — первым, баллы у всех архивов равны, и
/// ставился архив дополнений вместо темы.
pub const SECONDARY_FILE_WORDS: &[&str] = &["extra", "extras", "addon", "addons", "bonus"];

/// Штраф дополнению ([`SECONDARY_FILE_WORDS`]): ниже любого основного
/// архива, но выше не-архива — если у товара других архивов нет, берётся он.
const SECONDARY_FILE_PENALTY: i64 = 30;

/// Надбавка файлу, имя которого (без расширения, без регистра, только буквы
/// и цифры) совпадает с названием товара, — и меньшая, если название в имени
/// содержится. Меньше любой разницы классов выше: это лишь выбор среди
/// равноценных архивов (`Rage-Cursor` у товара «Rage cursor», а не первый по
/// алфавиту `Rage-Blue-Cursor`).
const TITLE_EXACT_BONUS: i64 = 8;
const TITLE_CONTAINED_BONUS: i64 = 5;

/// Маркеры файлов НЕ для Linux (Windows/macOS-сборки) — `_NON_LINUX_MARKERS`.
pub const NON_LINUX_MARKERS: &[&str] = &[
    "windows", "win64", "win32", "-win", "win-", "_win", ".ani", ".cur",
    ".exe", ".msi", ".dmg", "macos", "-mac", "mac-", "osx",
];

/// Результат разбора одного `<content>` — `ThemeItem` плюс общее число
/// комментариев. В Python `comment_count` — динамический атрибут на dataclass
/// (`item.comment_count = num("comments")`), которого нет в `dataclasses.fields`;
/// здесь он вынесен рядом, чтобы `ThemeItem` остался побайтовым портом модели.
#[derive(Debug, Clone, PartialEq)]
pub struct OcsContent {
    pub item: ThemeItem,
    pub comment_count: i64,
}

/// Один комментарий (плоский, с глубиной вложенности) — порт словаря из
/// `_flatten_comments`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OcsComment {
    pub author: String,
    pub date: String,
    pub subject: String,
    pub text: String,
    pub score: String,
    pub depth: i64,
}

// ── XML DOM (аналог xml.etree.ElementTree по нужным операциям) ─────────────

/// Минимальный узел дерева: имя тега, прямой текст и прямые потомки.
///
/// `text` — конкатенация прямого текста узла; для листовых тегов (а именно на
/// них зовётся `findtext`) это совпадает с `ElementTree.text`.
struct Node {
    tag: String,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn new(tag: &str) -> Self {
        Node { tag: tag.to_string(), text: String::new(), children: Vec::new() }
    }

    /// Первый прямой потомок с данным тегом (`Element.find`).
    fn child(&self, tag: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.tag == tag)
    }

    /// Все прямые потомки с данным тегом (`Element.findall`).
    fn children_named<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a Node> {
        self.children.iter().filter(move |c| c.tag == tag)
    }

    /// Текст первого потомка (`Element.findtext`): `None` — тега нет,
    /// `Some("")` — тег есть, но пуст (как ET).
    fn findtext(&self, tag: &str) -> Option<&str> {
        self.child(tag).map(|c| c.text.as_str())
    }
}

/// Построить DOM из XML. `Err(())` — ошибка разбора (аналог `ET.ParseError`,
/// на который Python отвечает пустым результатом).
fn parse_dom(xml: &str) -> Result<Node, ()> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Node> = vec![Node::new("__root__")];
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                stack.push(Node::new(&tag));
            }
            Ok(Event::End(_)) => {
                // корень (__root__) не закрывается тегом — защищаемся len>1.
                if stack.len() > 1 {
                    let node = stack.pop().unwrap();
                    stack.last_mut().unwrap().children.push(node);
                }
            }
            Ok(Event::Empty(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                stack.last_mut().unwrap().children.push(Node::new(&tag));
            }
            Ok(Event::Text(e)) => {
                // В quick-xml 0.39 сущности идут отдельными GeneralRef-событиями,
                // поэтому Text — уже сырой литеральный текст (без &..;).
                let t = e.decode().map_err(|_| ())?;
                stack.last_mut().unwrap().text.push_str(&t);
            }
            Ok(Event::GeneralRef(e)) => {
                // Числовая ссылка (&#NN; / &#xNN;) либо предопределённая сущность
                // (lt/gt/amp/quot/apos) — ET резолвит ровно эти; неизвестная
                // именованная сущность у ET = ParseError → у нас Err.
                let ch = match e.resolve_char_ref().map_err(|_| ())? {
                    Some(c) => c,
                    None => match e.decode().map_err(|_| ())?.as_ref() {
                        "lt" => '<',
                        "gt" => '>',
                        "amp" => '&',
                        "quot" => '"',
                        "apos" => '\'',
                        _ => return Err(()),
                    },
                };
                stack.last_mut().unwrap().text.push(ch);
            }
            Ok(Event::CData(e)) => {
                let s = String::from_utf8_lossy(e.as_ref()).into_owned();
                stack.last_mut().unwrap().text.push_str(&s);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return Err(()),
        }
    }
    Ok(stack.pop().unwrap())
}

/// `(findtext(tag) or default).strip()` — точный порт хелпера `txt`.
fn txt(node: &Node, tag: &str, default: &str) -> String {
    let s = match node.findtext(tag) {
        Some(t) if !t.is_empty() => t,
        _ => default,
    };
    s.trim().to_string()
}

/// `int(txt(tag,"0") or "0")` в `try/except ValueError -> 0` — порт `num`.
fn num(node: &Node, tag: &str) -> i64 {
    let s = txt(node, tag, "0");
    let s = if s.is_empty() { "0" } else { s.as_str() };
    s.parse::<i64>().unwrap_or(0)
}

// ── Выбор лучшего download-линка ───────────────────────────────────────────

/// Оценка пригодности линка для установки Linux-темы — порт `_link_score`,
/// без знания вида товара (см. [`link_score_for`]).
pub fn link_score(link: &Value) -> i64 {
    link_score_for(link, false)
}

/// Оценка пригодности линка; `kpackage_kind` — товар вида, который ставится
/// KPackage-ом (тогда готовый `.plasmoid`/`.kpackage`/`.kwinscript` получает
/// надбавку).
///
/// Шкала: архив +100 (tar ещё +10), готовый KPackage +110, не архив −100;
/// маркер Windows/macOS −200; пакет дистрибутива/установщик −150; текст
/// (README, `.md`, `.txt`) −1000 — ниже всего прочего.
pub fn link_score_for(link: &Value, kpackage_kind: bool) -> i64 {
    let name = link.get("name").and_then(Value::as_str).unwrap_or("");
    let url = link.get("url").and_then(Value::as_str).unwrap_or("");
    // hay = (name or url or "").lower()
    let hay = if !name.is_empty() {
        name
    } else if !url.is_empty() {
        url
    } else {
        ""
    }
    .to_lowercase();

    let mut score = 0i64;
    if KPACKAGE_EXTS.iter().any(|e| hay.ends_with(e)) {
        score += 110;
        if kpackage_kind {
            score += KPACKAGE_KIND_BONUS;
        }
    } else {
        match ARCHIVE_EXTS.iter().find(|e| hay.ends_with(**e)) {
            Some(ext) => {
                score += 100;
                if ext.starts_with(".tar") {
                    score += 10;
                }
            }
            None => score -= 100,
        }
    }
    if NON_LINUX_MARKERS.iter().any(|m| hay.contains(m)) {
        score -= 200;
    }
    if DISTRO_PACKAGE_EXTS.iter().any(|e| hay.ends_with(e)) {
        score -= DISTRO_PACKAGE_PENALTY;
    }
    let file = hay.rsplit('/').next().unwrap_or("");
    if DOC_EXTS.iter().any(|e| hay.ends_with(e)) || file.starts_with("readme") {
        score -= DOC_PENALTY;
    }
    if file.split(|c: char| !c.is_alphanumeric()).any(|w| SECONDARY_FILE_WORDS.contains(&w)) {
        score -= SECONDARY_FILE_PENALTY;
    }
    score
}

/// Только буквы и цифры, в нижнем регистре: «Rage cursor» и «Rage-Cursor»
/// сравниваются как одно.
fn alnum_lower(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Надбавка за совпадение имени файла с названием товара
/// ([`TITLE_EXACT_BONUS`], [`TITLE_CONTAINED_BONUS`]); короткое название
/// (меньше трёх знаков) не сравнивается.
pub fn title_bonus(link: &Value, title: &str) -> i64 {
    let want = alnum_lower(title);
    if want.chars().count() < 3 {
        return 0;
    }
    let name = link.get("name").and_then(Value::as_str).unwrap_or("");
    let url = link.get("url").and_then(Value::as_str).unwrap_or("");
    let file = if name.is_empty() { url } else { name }.rsplit('/').next().unwrap_or("").to_lowercase();
    let stem = KPACKAGE_EXTS
        .iter()
        .chain(ARCHIVE_EXTS)
        .find_map(|e| file.strip_suffix(e))
        .unwrap_or(file.as_str());
    // Метка времени, которую магазин дописывает к повторно выложенному
    // файлу с тем же именем (`Nilium-20200426034630.tar.gz` рядом с
    // `Nilium.tar.gz`), имени не меняет: какой из двух новее, по имени не
    // понять, и при равенстве остаётся порядок выдачи.
    let stem = match stem.rsplit_once('-') {
        Some((base, tail)) if tail.len() == 14 && tail.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => stem,
    };
    let have = alnum_lower(stem);
    if have == want {
        TITLE_EXACT_BONUS
    } else if have.contains(&want) {
        TITLE_CONTAINED_BONUS
    } else {
        0
    }
}

/// Индекс наиболее подходящего линка; `-1` если список пуст — порт
/// `_pick_best_link`, без знания вида товара (см. [`pick_best_link_for`]).
pub fn pick_best_link(links: &[Value]) -> isize {
    pick_best_link_for(links, false)
}

/// Индекс наиболее подходящего линка с учётом вида товара. При равенстве
/// баллов сохраняется порядок (первый лучший), как `max(range(len), key=…)`.
pub fn pick_best_link_for(links: &[Value], kpackage_kind: bool) -> isize {
    pick_best_link_titled(links, kpackage_kind, "")
}

/// [`pick_best_link_for`] с названием товара: среди равноценных архивов
/// выигрывает тот, чьё имя совпадает с названием ([`title_bonus`]). Порядок
/// файлов в выдаче магазина — алфавитный, а не «основной первым», поэтому
/// одного порядка для выбора мало. При полном равенстве — первый.
pub fn pick_best_link_titled(links: &[Value], kpackage_kind: bool, title: &str) -> isize {
    let score = |l: &Value| link_score_for(l, kpackage_kind) + title_bonus(l, title);
    let mut best: Option<(usize, i64)> = None;
    for (i, l) in links.iter().enumerate() {
        let s = score(l);
        if best.is_none_or(|(_, b)| s > b) {
            best = Some((i, s));
        }
    }
    best.map_or(-1, |(i, _)| i as isize)
}

/// Номер файла, который [`OcsClient::download`] возьмёт у товара без явного
/// выбора; `-1` — списка файлов нет.
pub fn auto_file_index(item: &ThemeItem) -> isize {
    let kpackage = item.download_links.len() > 1 && store_type_is_kpackage(item.category_id);
    pick_best_link_titled(&item.download_links, kpackage, &item.name)
}

/// Имя файла для человека и для выбора руками: поле `name`, а без него —
/// последний кусок адреса.
fn link_file_name(link: &Value) -> String {
    let name = link.get("name").and_then(Value::as_str).unwrap_or("");
    if !name.is_empty() {
        return name.to_string();
    }
    let url = link.get("url").and_then(Value::as_str).unwrap_or("");
    url.rsplit('/').next().unwrap_or("").to_string()
}

/// Номер файла товара, который ставить: выбранный руками (`wanted` — имя из
/// [`file_choices`]), а если имени нет среди файлов (пусто, список
/// обновился) — автоматический ([`auto_file_index`]).
pub fn file_index_for(item: &ThemeItem, wanted: &str) -> isize {
    if !wanted.is_empty() {
        if let Some(i) = item.download_links.iter().position(|l| link_file_name(l) == wanted) {
            return i as isize;
        }
    }
    auto_file_index(item)
}

/// Имя файла `idx` у товара; `None` — номера нет или у файла нет имени.
pub fn file_name_at(item: &ThemeItem, idx: isize) -> Option<String> {
    let name = item.download_links.get(usize::try_from(idx).ok()?)?.get("name").and_then(Value::as_str)?;
    (!name.is_empty()).then(|| name.to_string())
}

/// Имя файла, который [`OcsClient::download`] возьмёт у товара без явного
/// выбора; `None` — списка файлов нет или у файла нет имени.
pub fn chosen_file_name(item: &ThemeItem) -> Option<String> {
    file_name_at(item, auto_file_index(item))
}

/// Какой файл товара берётся при установке — строка для журнала установки,
/// когда у товара файлов несколько (иначе `None`: выбирать было не из чего).
pub fn chosen_file_note(item: &ThemeItem) -> Option<String> {
    file_note_at(item, auto_file_index(item))
}

/// [`chosen_file_note`] для файла `idx` — выбранного руками или
/// автоматически.
pub fn file_note_at(item: &ThemeItem, idx: isize) -> Option<String> {
    let total = item.download_links.len();
    if total < 2 {
        return None;
    }
    let name = file_name_at(item, idx)?;
    Some(crate::i18n::trf(
        // TRANSLATORS: %1 is a file name from the store, %2 is how many files the item has.
        "File: “%1” (the item has %2 files)",
        &[&name, &total.to_string()],
    ))
}

/// Файл товара в списке выбора панели деталей.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileChoice {
    /// Имя файла — оно же ключ выбора ([`file_index_for`]).
    pub name: String,
    /// Размер в КиБ, как отдаёт магазин (`downloadsize`).
    pub size_kib: i64,
    /// День выкладки `ГГГГ-ММ-ДД` — только из метки времени, которую магазин
    /// дописывает к имени повторно выложенного файла (`-20230519162903`).
    /// Своей даты у файла в ответе магазина нет, поэтому чаще всего пусто.
    pub date: String,
    /// Этот файл берётся автоматически ([`auto_file_index`]) — тот же, что
    /// ставит кнопка плитки.
    pub chosen: bool,
}

/// День из метки времени магазина в имени файла (`Name-20230519162903.tar.gz`
/// → `2023-05-19`); пусто — метки нет.
fn store_stamp_day(file: &str) -> String {
    let lower = file.to_lowercase();
    let stem = KPACKAGE_EXTS
        .iter()
        .chain(ARCHIVE_EXTS)
        .find_map(|e| lower.strip_suffix(e))
        .unwrap_or(lower.as_str());
    match stem.rsplit_once('-') {
        Some((_, t)) if t.len() == 14 && t.bytes().all(|b| b.is_ascii_digit()) => {
            format!("{}-{}-{}", &t[..4], &t[4..6], &t[6..8])
        }
        _ => String::new(),
    }
}

/// Варианты файлов товара для выбора руками — все по порядку выдачи, с
/// пометкой автоматического. Файл один или их нет — пусто: выбирать не из
/// чего, и блока выбора на панели нет.
pub fn file_choices(item: &ThemeItem) -> Vec<FileChoice> {
    if item.download_links.len() < 2 {
        return Vec::new();
    }
    let auto = auto_file_index(item);
    item.download_links
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let name = link_file_name(l);
            FileChoice {
                date: store_stamp_day(&name),
                size_kib: l.get("size").and_then(Value::as_i64).unwrap_or(0),
                chosen: i as isize == auto,
                name,
            }
        })
        .collect()
}

/// Ставится ли товар магазина с этим `typeid` KPackage-ом — решает раздел
/// каталога (подкатегории ведут в зонт) и матрица окружений, как у
/// установщика. Каталог установки тут не нужен, поэтому домашний каталог
/// пустой.
pub fn store_type_is_kpackage(type_id: i64) -> bool {
    crate::categories::category_by_id(std::path::Path::new(""), type_id)
        .is_some_and(|c| crate::appearance_installer::is_kpackage_kind(&c.apply_kind))
}

// ── Разбор контента ────────────────────────────────────────────────────────

/// Метки магазина, которыми автор указывает лицензию, → её короткое имя.
///
/// Поле `license` в выдаче OCS почти всегда пусто (в сводке его нет вовсе),
/// а лицензию автор выбирает на сайте, и она приходит МЕТКОЙ в `tags`.
/// Таблица собрана с живой выдачи 04.10.2026 (5348 товаров 54 категорий,
/// среди них курсоры 107, глобальные темы 722, эффекты KWin 719): там
/// встретились ровно эти метки. Версию, которой метка не называет, не
/// придумываем: «cc-by» остаётся «CC-BY», «bsd-license» — «BSD».
///
/// ⚠️ Голые «bsd», «x11», «apache», «cc» в таблицу НЕ входят: в выдаче ими
/// помечают систему, оконную систему и сервер («linux,bsd», «x11,wayland»),
/// а не лицензию.
pub const LICENSE_TAGS: &[(&str, &str)] = &[
    ("gplv3", "GPL-3.0"),
    ("gplv3-later", "GPL-3.0-or-later"),
    ("gplv2", "GPL-2.0"),
    ("gplv2-later", "GPL-2.0-or-later"),
    ("gpl", "GPL"),
    ("agplv3", "AGPL-3.0"),
    ("lgplv3", "LGPL-3.0"),
    ("lgplv2", "LGPL-2.0"),
    ("lgplv2.1", "LGPL-2.1"),
    ("lgpl", "LGPL"),
    ("mit-license", "MIT"),
    ("mit", "MIT"),
    ("bsd-license", "BSD"),
    ("freebsd", "BSD-2-Clause"),
    ("apache-license", "Apache-2.0"),
    ("x11-license", "X11"),
    ("mpl", "MPL"),
    ("mpl-2.0", "MPL-2.0"),
    ("artistic-2", "Artistic-2.0"),
    ("cc0", "CC0-1.0"),
    ("cc-by", "CC-BY"),
    ("cc-by-sa", "CC-BY-SA"),
    ("cc-by-nd", "CC-BY-ND"),
    ("cc-by-nc", "CC-BY-NC"),
    ("cc-by-nc-sa", "CC-BY-NC-SA"),
    ("cc-by-nc-nd", "CC-BY-NC-ND"),
];

/// Лицензия по меткам товара: все опознанные, без повторов, в порядке меток,
/// через запятую. Ничего не опознано — пусто (в карточке будет «—»).
pub fn license_from_tags(tags: &str) -> String {
    let mut found: Vec<&str> = Vec::new();
    for tag in tags.split(',') {
        let tag = tag.trim().to_ascii_lowercase();
        if let Some((_, spdx)) = LICENSE_TAGS.iter().find(|(t, _)| *t == tag) {
            if !found.contains(spdx) {
                found.push(spdx);
            }
        }
    }
    found.join(", ")
}

/// Разбор одного `<content>` в `OcsContent` — порт `_parse_content`.
/// `None`, если `id` пуст.
fn parse_content(content: &Node) -> Option<OcsContent> {
    let cid = txt(content, "id", "");
    if cid.is_empty() {
        return None;
    }

    // Превью — большая картинка (previewpic1), потом мелкая.
    let mut preview = String::new();
    for k in ["previewpic1", "smallpreviewpic1", "previewpic2"] {
        let v = txt(content, k, "");
        if !v.is_empty() {
            preview = v;
            break;
        }
    }

    // ВСЕ скриншоты previewpic1..12 (дыры в нумерации пропускаем, дубли — тоже).
    let mut previews: Vec<String> = Vec::new();
    for i in 1..=12 {
        let v = txt(content, &format!("previewpic{i}"), "");
        if !v.is_empty() && !previews.contains(&v) {
            previews.push(v);
        }
    }

    // Скачиваемые файлы (downloadlink1..N, обрыв на первом пустом).
    let mut links: Vec<Value> = Vec::new();
    for i in 1..20 {
        let link = txt(content, &format!("downloadlink{i}"), "");
        if link.is_empty() {
            break;
        }
        let name = txt(content, &format!("downloadname{i}"), "");
        let size = num(content, &format!("downloadsize{i}"));
        // порядок ключей url/name/size = как в Python dict (preserve_order).
        links.push(json!({ "url": link, "name": name, "size": size }));
    }

    // Лучший для Linux линк (не обязательно первый).
    let type_id = num(content, "typeid");
    let (dl_url, dl_name, dl_size) = {
        let kpackage = links.len() > 1 && store_type_is_kpackage(type_id);
        let best = pick_best_link_titled(&links, kpackage, &txt(content, "name", ""));
        if best >= 0 {
            let l = &links[best as usize];
            (
                l.get("url").and_then(Value::as_str).unwrap_or("").to_string(),
                l.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                l.get("size").and_then(Value::as_i64).unwrap_or(0),
            )
        } else {
            (String::new(), String::new(), 0)
        }
    };

    let item = ThemeItem {
        id: cid,
        name: txt(content, "name", ""),
        description: txt(content, "description", ""),
        author: txt(content, "personid", ""),
        license: {
            let own = txt(content, "license", "");
            if own.is_empty() {
                license_from_tags(&txt(content, "tags", ""))
            } else {
                own
            }
        },
        version: txt(content, "version", ""),
        rating: num(content, "score"),
        downloads: num(content, "downloads"),
        preview_url: preview,
        preview_urls: previews,
        download_url: dl_url,
        download_name: dl_name,
        download_size: dl_size,
        category_id: type_id,
        created: txt(content, "created", ""),
        updated: txt(content, "changed", ""),
        download_links: links,
        ..ThemeItem::default()
    };
    Some(OcsContent { item, comment_count: num(content, "comments") })
}

/// Диагностика неуспешного разбора OCS-ответа — для лог-паритета вызывающего
/// слоя (Python в этих ветках пишет log.error/log.warning; поведенческий
/// результат от диагностики не зависит).
#[derive(Debug, Clone, PartialEq)]
pub enum OcsDiag {
    /// Некорректный XML (аналог `ET.ParseError`).
    Xml,
    /// Магазин ответил отказом: `meta/status != ok` либо `statuscode` не 100.
    /// `code` может быть пустым, `message` — текст отказа от самого магазина.
    Status { code: String, message: String },
}

/// Почему магазин не дал страницу — в отличие от «товаров нет».
///
/// 20.10.2026 API магазина на любой запрос каталога отвечал HTTP 200 с
/// отказом «999 unknown request», а раздел писал «ничего не найдено».
/// Отказ, сбой связи и нечитаемый ответ — это НЕ пустой результат: человеку
/// говорится, что магазин не отвечает, а кеш таким ответом не затирается.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreFailure {
    /// Не достучались: текст сетевой ошибки.
    Network(String),
    /// Магазин ответил, но отказом (`status failed` / `statuscode` ≠ 100).
    Refused { code: String, message: String },
    /// Ответ пришёл, но прочитать его нельзя.
    Malformed,
}

/// Отказ в `<meta>`: `status != ok` либо код не 100 (успех OCS). Пустой код
/// при `status ok` отказом не считается — так отвечают старые зеркала.
fn meta_refusal(meta: &Node) -> Option<OcsDiag> {
    let code = txt(meta, "statuscode", "");
    if txt(meta, "status", "") != "ok" || !(code.is_empty() || code == "100") {
        return Some(OcsDiag::Status { code, message: txt(meta, "message", "") });
    }
    None
}

/// Отказ в JSON-форме (`{"status":"failed","statuscode":999,…}`): так API
/// отвечает при `format=json` и на ряде путей независимо от формата.
/// Успешного JSON-каталога мы не просим — он тоже «не прочитать».
fn json_refusal(text: &str) -> Option<OcsDiag> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let status = v.get("status").and_then(Value::as_str).unwrap_or("");
    let code = match v.get("statuscode") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.trim().to_string(),
        _ => String::new(),
    };
    if status == "ok" && (code.is_empty() || code == "100") {
        return Some(OcsDiag::Xml);
    }
    let message = v.get("message").and_then(Value::as_str).unwrap_or("").trim().to_string();
    Some(OcsDiag::Status { code, message })
}

/// Разбор ответа-списка (`/content/data`) — порт `_parse_list`.
/// Возвращает `(контент, totalitems)`. Ошибка XML или `status != ok` → `([], 0)`
/// (при not-ok totalitems Python тоже не читает → 0).
pub fn parse_list(xml: &[u8]) -> (Vec<OcsContent>, i64) {
    let (items, total, _diag) = parse_list_checked(xml);
    (items, total)
}

/// `parse_list` + диагностика неуспеха (какую ветку Python бы залогировал).
pub fn parse_list_checked(xml: &[u8]) -> (Vec<OcsContent>, i64, Option<OcsDiag>) {
    let Ok(text) = std::str::from_utf8(xml) else {
        return (Vec::new(), 0, Some(OcsDiag::Xml));
    };
    if text.trim_start().starts_with('{') {
        return (Vec::new(), 0, Some(json_refusal(text).unwrap_or(OcsDiag::Xml)));
    }
    let Ok(dom) = parse_dom(text) else {
        return (Vec::new(), 0, Some(OcsDiag::Xml));
    };
    let Some(ocs) = dom.child("ocs") else {
        return (Vec::new(), 0, Some(OcsDiag::Xml));
    };

    let meta = ocs.child("meta");
    if let Some(refusal) = meta.and_then(meta_refusal) {
        return (Vec::new(), 0, Some(refusal));
    }
    let total = meta.map(|m| num(m, "totalitems")).unwrap_or(0);

    let Some(data) = ocs.child("data") else {
        return (Vec::new(), total, None);
    };
    let items = data.children_named("content").filter_map(parse_content).collect();
    (items, total, None)
}

/// Деталка не разобрана (битый XML / нет `<data>` / нет `<content>`) —
/// Python в этих ветках уходит в SQLite-кеш.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MalformedDetail;

/// Разбор ответа-деталки (`/content/data/{id}`) — порт разборной ветки
/// `OCSClient.get_item` (meta-статус там НЕ проверяется, в отличие от списка).
///
/// * `Err(MalformedDetail)` — «не разобрать»: вызывающий уходит в кеш.
/// * `Ok(None)` — разобрано, но `id` пуст: Python возвращает None БЕЗ кеша.
/// * `Ok(Some(_))` — элемент.
pub fn parse_item_detail(xml: &[u8]) -> Result<Option<OcsContent>, MalformedDetail> {
    let text = std::str::from_utf8(xml).map_err(|_| MalformedDetail)?;
    let dom = parse_dom(text).map_err(|_| MalformedDetail)?;
    let ocs = dom.child("ocs").ok_or(MalformedDetail)?;
    let data = ocs.child("data").ok_or(MalformedDetail)?;
    let content = data.child("content").ok_or(MalformedDetail)?;
    Ok(parse_content(content))
}

/// Разбор ответа с комментариями (`/comments/data/...`) в плоский список —
/// порт разворачивания `_flatten_comments` над узлом `<data>`.
///
/// Статус-проверки и обработка кода 101 из сетевого метода `comments()` СЮДА не
/// входят (это сетевой слой). Ошибка XML или отсутствие `<data>` → пустой список.
pub fn parse_comments(xml: &[u8]) -> Vec<OcsComment> {
    let Ok(text) = std::str::from_utf8(xml) else {
        return Vec::new();
    };
    let Ok(dom) = parse_dom(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(data) = dom.child("ocs").and_then(|o| o.child("data")) {
        flatten_comments(data, 0, &mut out);
    }
    out
}

/// Разбор ответа комментариев С ПРОВЕРКОЙ meta-статуса — порт разборной ветки
/// `OCSClient.comments` (после `_http_get`).
///
/// Возвращает `(результат, диагностика)`:
/// * `(None, Some(_))` — endpoint недоступен: битый XML либо `status != ok`
///   с кодом ≠ 101 (Python возвращает None + log.warning);
/// * `(Some([]), None)` — код 101 «комментариев нет» либо нет `<data>`;
/// * `(Some(flat), None)` — плоский список с depth-вложенностью.
pub fn parse_comments_checked(xml: &[u8]) -> (Option<Vec<OcsComment>>, Option<OcsDiag>) {
    let Ok(text) = std::str::from_utf8(xml) else {
        return (None, Some(OcsDiag::Xml));
    };
    let Ok(dom) = parse_dom(text) else {
        return (None, Some(OcsDiag::Xml));
    };
    let Some(ocs) = dom.child("ocs") else {
        return (None, Some(OcsDiag::Xml));
    };
    if let Some(refusal) = ocs.child("meta").and_then(meta_refusal) {
        // 101 = пустой результат: у контента 0 комментариев.
        if matches!(&refusal, OcsDiag::Status { code, .. } if code == "101") {
            return (Some(Vec::new()), None);
        }
        return (None, Some(refusal));
    }
    let mut out = Vec::new();
    if let Some(data) = ocs.child("data") {
        flatten_comments(data, 0, &mut out);
    }
    (Some(out), None)
}

/// Рекурсивный сбор `<comment>` (+ вложенные `<children>`) — порт
/// `_flatten_comments`.
fn flatten_comments(parent: &Node, depth: i64, out: &mut Vec<OcsComment>) {
    for c in parent.children_named("comment") {
        out.push(OcsComment {
            author: txt(c, "user", ""),
            date: txt(c, "date", ""),
            subject: txt(c, "subject", ""),
            text: txt(c, "text", ""),
            score: txt(c, "score", ""),
            depth,
        });
        if let Some(children) = c.child("children") {
            flatten_comments(children, depth + 1, out);
        }
    }
}

// ── Нормализация URL (safe_url) ────────────────────────────────────────────

// Схемы, для которых urlunsplit восстанавливает `//netloc` (подмножество
// urllib `uses_netloc`, покрывающее реальные OCS-ссылки).
const USES_NETLOC: &[&str] = &[
    "", "ftp", "http", "gopher", "nntp", "telnet", "imap", "wais", "file",
    "mms", "https", "shttp", "snews", "prospero", "rtsp", "rtspu", "rsync",
    "svn", "svn+ssh", "sftp", "nfs", "git", "git+ssh", "ws", "wss",
];

/// Percent-encode как `urllib.parse.quote(s, safe)`: always-safe = ALPHA/DIGIT/
/// `_.-~`, плюс переданные `safe`-символы; остальные байты (в т.ч. не-ASCII) →
/// `%XX` (uppercase, по UTF-8-байтам).
fn quote(s: &str, safe: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        let is_safe = b.is_ascii_alphanumeric()
            || matches!(b, b'_' | b'.' | b'-' | b'~')
            || (b < 0x80 && safe.as_bytes().contains(&b));
        if is_safe {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Сетевая часть ссылки — порт `urllib.parse.urlparse(url).netloc`
/// (вместе с логином и портом, если они есть: сравнение всегда точное).
/// Публичная обёртка над общим разбором — второго разборщика ссылок в ядре
/// заводить нельзя (правило #12).
pub fn url_netloc(url: &str) -> String {
    urlsplit(url).1
}

/// Порт `urllib.parse.urlsplit` (без валидации порта): `(scheme, netloc, path,
/// query, fragment)`. Порядок отделения — как в CPython: scheme → `//netloc` →
/// `#fragment` → `?query`.
fn urlsplit(url: &str) -> (String, String, String, String, String) {
    let mut rest = url;
    let mut scheme = String::new();
    if let Some(i) = rest.find(':') {
        if i > 0 {
            let cand = &rest[..i];
            // scheme_chars: ALPHANUM + '+' '-' '.'
            if cand
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
            {
                scheme = cand.to_ascii_lowercase();
                rest = &rest[i + 1..];
            }
        }
    }

    let mut netloc = String::new();
    if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        netloc = after[..end].to_string();
        rest = &after[end..];
    }

    let mut fragment = String::new();
    if let Some(i) = rest.find('#') {
        fragment = rest[i + 1..].to_string();
        rest = &rest[..i];
    }

    let mut query = String::new();
    if let Some(i) = rest.find('?') {
        query = rest[i + 1..].to_string();
        rest = &rest[..i];
    }

    (scheme, netloc, rest.to_string(), query, fragment)
}

/// Порт `urllib.parse.urlunsplit`.
fn urlunsplit(scheme: &str, netloc: &str, path: &str, query: &str, fragment: &str) -> String {
    let mut url = path.to_string();
    if !netloc.is_empty()
        || (!scheme.is_empty() && USES_NETLOC.contains(&scheme) && !url.starts_with("//"))
    {
        if !url.is_empty() && !url.starts_with('/') {
            url = format!("/{url}");
        }
        url = format!("//{netloc}{url}");
    }
    if !scheme.is_empty() {
        url = format!("{scheme}:{url}");
    }
    if !query.is_empty() {
        url = format!("{url}?{query}");
    }
    if !fragment.is_empty() {
        url = format!("{url}#{fragment}");
    }
    url
}

/// Percent-encode небезопасных символов в OCS-URL — порт `_safe_url`.
/// Уже закодированные `%XX` сохраняются (`%` в safe → нет двойного кодирования),
/// fragment не трогается (как в оригинале).
pub fn safe_url(url: &str) -> String {
    let (scheme, netloc, path, query, fragment) = urlsplit(url);
    let path = quote(&path, "/%:@!$&'()*+,;=~-._");
    let query = quote(&query, "/%:@!$&'()*+,;=?~-._");
    urlunsplit(&scheme, &netloc, &path, &query, &fragment)
}

// ── Полноразмерное превью (галерея детального вида) ────────────────────────

/// URL previewpic → URL оригинала полного разрешения — порт `full_preview_url`.
///
/// Убирает ПЕРВОЕ вхождение сегмента CDN-ресайзера pling
/// `/cache/<W>x<H><хвост>/img/` → `/img/` (Python: `_CACHE_SEG_RE.sub(…, count=1)`
/// по регэкспу `/cache/\d+x\d+[^/]*/img/`). URL без такого сегмента возвращается
/// как есть — трансформация безопасна для не-pling ссылок (no-op).
///
/// Регэксп воспроизведён вручную (без regex-крейта): `[^/]*` — надмножество
/// `\d+`, поэтому жадному `\d+` откат не нужен, и достаточно линейного разбора:
/// после `\d+ x \d+` дойти до следующего `/` и проверить, что дальше `/img/`.
pub fn full_preview_url(url: &str) -> String {
    const CACHE: &str = "/cache/";
    let b = url.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = url[from..].find(CACHE) {
        let start = from + rel;
        let mut p = start + CACHE.len();
        // \d+
        let d1 = p;
        while p < b.len() && b[p].is_ascii_digit() {
            p += 1;
        }
        // 'x' между группами цифр
        if p > d1 && p < b.len() && b[p] == b'x' {
            p += 1;
            let d2 = p;
            while p < b.len() && b[p].is_ascii_digit() {
                p += 1;
            }
            if p > d2 {
                // [^/]* — до следующего '/' (сегмент не может его содержать)
                let mut q = p;
                while q < b.len() && b[q] != b'/' {
                    q += 1;
                }
                if url[q..].starts_with("/img/") {
                    // count=1: заменяем это вхождение и выходим.
                    return format!("{}/img/{}", &url[..start], &url[q + "/img/".len()..]);
                }
            }
        }
        // Этот `/cache/` не подошёл — regex сдвинул бы старт на символ вперёд.
        from = start + 1;
    }
    url.to_string()
}

// ── Сетевой слой: детерминированные части (ключи кеша, URL, имена файлов) ───

/// `SORT_MAP.get(sort, "new")` — режим сортировки OCS из значения UI-комбобокса.
/// Неизвестное значение (в т.ч. пустое) → `"new"`, как `dict.get` с дефолтом.
pub fn sort_mode(sort: &str) -> &'static str {
    SORT_MODES.iter().copied().find(|m| *m == sort).unwrap_or("new")
}

/// Ключ кеша страницы каталога — порт `OCSClient.list_key`
/// (`f"cat{id}:p{page}:s{sortmode}:q{search.strip().lower()}"`).
///
/// `lower()` в Python — Unicode-полный (`«Тёмная» → «тёмная»`), поэтому здесь
/// `to_lowercase()` (а не `to_ascii_lowercase`). `strip()` обрезает пробельные
/// по Unicode — `trim()` делает то же.
pub fn list_key(category_id: &str, page: i64, sort: &str, search: &str) -> String {
    let sortmode = sort_mode(sort);
    let q = search.trim().to_lowercase();
    format!("cat{category_id}:p{page}:s{sortmode}:q{q}")
}

/// `urllib.parse.quote_plus`: как [`quote`] с пустым `safe`, но пробел → `+`.
/// Используется `urlencode` для каждого значения параметра.
fn quote_plus(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b == b' ' {
            out.push('+');
        } else if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// URL запроса каталога — порт сборки в `OCSClient.search`
/// (`urlencode` по параметрам в порядке объявления dict; `search` добавляется
/// ТОЛЬКО если непустой после `strip()` — и уже обрезанным).
///
/// `category_id` строкой: категории-агрегаты идут списком «716,488» (Python
/// подставляет `str(category_id)`, где id может быть и строкой из `store_cats`).
pub fn search_url(category_id: &str, page: i64, pagesize: i64, sortmode: &str, search: &str) -> String {
    let mut q = format!(
        "categories={}&page={}&pagesize={}&sortmode={}",
        quote_plus(category_id),
        quote_plus(&page.to_string()),
        quote_plus(&pagesize.to_string()),
        quote_plus(sortmode),
    );
    let s = search.trim();
    if !s.is_empty() {
        q.push_str("&search=");
        q.push_str(&quote_plus(s));
    }
    format!("{API_BASE}/content/data?{q}")
}

/// URL деталки — порт `f"{API_BASE}/content/data/{content_id}"`.
/// Экранирования в Python нет (id — цифры от самого API); нормализацией
/// небезопасных символов занимается [`safe_url`] уже перед запросом.
pub fn item_url(content_id: &str) -> String {
    format!("{API_BASE}/content/data/{content_id}")
}

/// URL комментариев — порт `OCSClient.comments`
/// (`/comments/data/1/{id}/0?page=…&pagesize=…`; type=1 — контент, second_id=0).
pub fn comments_url(content_id: &str, page: i64, pagesize: i64) -> String {
    format!(
        "{API_BASE}/comments/data/1/{content_id}/0?page={}&pagesize={}",
        quote_plus(&page.to_string()),
        quote_plus(&pagesize.to_string()),
    )
}

/// Имя файла превью в кеше — порт вычисления в `OCSClient.get_preview_image`:
/// `sha1(url).hexdigest() + ext`, где `ext` — расширение из ПУТИ URL
/// (`urlparse(url).path`, т.е. без `?query`/`#fragment`); пустое или длиннее
/// 5 символов → `.jpg` (защита от «расширений» вида `.verylongext`).
pub fn preview_cache_name(url: &str) -> String {
    let (_, _, path, _, _) = urlsplit(url);
    let ext = splitext(&path);
    // len(ext) > 5 — Python считает СИМВОЛЫ, не байты (важно для кириллицы).
    let ext = if ext.is_empty() || ext.chars().count() > 5 { ".jpg" } else { &ext };
    format!("{}{}", crate::translator::sha1_hex(url.as_bytes()), ext)
}

/// Расширение как `os.path.splitext(p)[1]`: последняя точка в базовом имени,
/// причём ведущие точки базового имени НЕ считаются (`.hidden` → `""`).
fn splitext(path: &str) -> String {
    let base_at = path.rfind('/').map(|i| i + 1).unwrap_or(0);
    let base = &path[base_at..];
    // CPython: пропускаем ведущие точки, затем ищем последнюю точку в остатке.
    let lead = base.len() - base.trim_start_matches('.').len();
    match base[lead..].rfind('.') {
        Some(i) => base[lead + i..].to_string(),
        None => String::new(),
    }
}

// ── Сетевой слой: HTTP-запросы и оркестрация с кешем ───────────────────────
//
// Порт `_http_get`/`_http_download` и класса `OCSClient`. Сам вызов — за
// трейтом [`NetIo`] (см. `net.rs`), кеш — за [`OcsCacheIo`]; здесь только
// решения: какой URL, что считать успехом, когда уходить в кеш, когда
// удалять недокачанный файл.

/// Кеш каталога — публичный API `OCSCache` (`appearance/cache.py`) в объёме,
/// которым пользуется `OCSClient`.
///
/// Реализация остаётся снаружи: сегодня это живой SQLite в
/// `~/.cache/rexon/ocs_cache.db`, чистая часть которого уже портирована в
/// [`crate::appearance_cache`] (тексты SQL, TTL-инвалидация списков,
/// сериализация полей). Инвалидация здесь не дублируется: `cache_get_list`
/// обязан вернуть `None` для протухшей (старше часа) страницы, а
/// `cache_get_preview` — для записи, чей файл на диске исчез (порт проверки
/// `os.path.isfile` в `get_preview`).
pub trait OcsCacheIo {
    /// `OCSCache.get_list(key)` — `None`, если записи нет либо она протухла.
    fn cache_get_list(&mut self, key: &str) -> Option<(Vec<ThemeItem>, i64)>;
    /// `OCSCache.put_list(key, items, total)`.
    fn cache_put_list(&mut self, key: &str, items: &[ThemeItem], total: i64);
    /// `OCSCache.get_item(content_id)`.
    fn cache_get_item(&mut self, content_id: &str) -> Option<ThemeItem>;
    /// `OCSCache.put_item(item)`.
    fn cache_put_item(&mut self, item: &ThemeItem);
    /// `OCSCache.get_item_detail(content_id, max_age)` — ПОЛНЫЕ детали, если
    /// детальный ответ приходил не раньше `max_age_secs` назад; запись из
    /// ответа-списка полными деталями не считается.
    fn cache_get_item_detail(&mut self, content_id: &str, max_age_secs: u64) -> Option<ThemeItem>;
    /// `OCSCache.get_preview(url)` — путь к скачанному файлу превью.
    fn cache_get_preview(&mut self, url: &str) -> Option<String>;
    /// `OCSCache.put_preview(url, local_path)`.
    fn cache_put_preview(&mut self, url: &str, local_path: &str);
}

/// Исполнитель клиента: сеть + кеш.
pub trait OcsIo: NetIo + OcsCacheIo {}
impl<T: NetIo + OcsCacheIo> OcsIo for T {}

/// `os.path.dirname(path)` — всё до последнего `/` (без него — пустая строка).
fn dirname(path: &str) -> &str {
    match path.rfind('/') {
        // "/a" → "/", как у Python (срез не съедает единственный слеш).
        Some(0) => "/",
        Some(i) => &path[..i],
        None => "",
    }
}

/// GET с логированием сбоя — порт `_http_get`.
///
/// Ловятся ВСЕ сетевые сбои (`URLError`/`HTTPError`/`OSError` оригинала — то
/// есть и код вне 2xx, и отказ соединения), результат — `None` и
/// предупреждение в журнал с тем же текстом, что у Python. URL проходит через
/// [`safe_url`]: OCS-ссылки приносят сырые пробелы в именах файлов.
///
/// ⚠️ Расхождение с оригиналом (правило #12): временный сбой ПОВТОРЯЕТСЯ —
/// см. [`crate::net::with_retry`]. У Python один обрыв соединения означал
/// пустую страницу каталога, хотя следующая же попытка отдала бы ответ.
pub fn http_get<IO: NetIo + ?Sized>(io: &mut IO, url: &str, timeout_secs: u64) -> Option<Vec<u8>> {
    http_get_result(io, url, timeout_secs).ok()
}

/// [`http_get`], но с текстом окончательной ошибки — чтобы наверху отличить
/// сбой связи от пустого ответа.
pub fn http_get_result<IO: NetIo + ?Sized>(
    io: &mut IO,
    url: &str,
    timeout_secs: u64,
) -> Result<Vec<u8>, String> {
    let safe = safe_url(url);
    let what = format!("OCS GET {url}");
    let res = net::with_retry(io, net::Retry::IDEMPOTENT, &what, |io| {
        io.get(&safe, USER_AGENT, timeout_secs)
    });
    match res {
        Ok(body) => Ok(body),
        Err(e) => {
            io.log(LogLevel::Warning, &format!("OCS GET failed: {url} — {e}"));
            Err(e.to_string())
        }
    }
}

/// Общий хвост неудачи скачивания — порт `except`-ветки `_http_download`:
/// сообщение в журнал и удаление недокачанного файла.
fn download_failed<IO: NetIo + ?Sized>(io: &mut IO, url: &str, dest: &str, e: &NetError) -> bool {
    io.log(LogLevel::Error, &format!("OCS download failed: {url} — {e}"));
    if io.exists(dest) {
        io.remove(dest);
    }
    false
}

/// Скачать `url` в `dest` — порт `_http_download`.
///
/// `on_progress` — есть ли колбэк прогресса (в Python это `on_progress is not
/// None`): при `false` [`NetIo::progress`] не зовётся вовсе. `max_size` = 0
/// снимает потолок (в Python это falsy-значение аргумента).
///
/// Потолок проверяется дважды, как в оригинале: до скачивания по
/// `Content-Length` (тогда файл даже не создаётся) и по факту принятых байт —
/// сервер вправе соврать в заголовке или не прислать его совсем.
///
/// ⚠️ Починка оригинала (правило #16): `dest` без каталога (голое имя файла)
/// у Python попадал в `os.makedirs("")` → `FileNotFoundError` → скачивание
/// «падало» на ровном месте. Здесь пустой каталог просто не создаётся.
///
/// ⚠️ Расхождение с оригиналом (правило #12): временный сбой ПОВТОРЯЕТСЯ, и
/// повторяется скачивание ЦЕЛИКОМ (см. [`crate::net::with_retry`]). Обрыв на
/// середине архива у Python означал «тема не установилась», хотя вторая
/// попытка обычно доходит.
pub fn http_download<IO: NetIo + ?Sized>(
    io: &mut IO,
    url: &str,
    dest: &str,
    on_progress: bool,
    timeout_secs: u64,
    max_size: i64,
) -> bool {
    let safe = safe_url(url);
    let what = format!("OCS скачивание {url}");
    // Повтор берёт скачивание ЦЕЛИКОМ: ответ открывается заново, файл
    // создаётся заново (`create` его затирает) и прогресс идёт с нуля.
    // Дописывать вторую попытку в конец огрызка первой нельзя — получится
    // битый архив, который распаковщик отвергнет уже после «Загружено».
    let res = net::with_retry(io, net::Retry::IDEMPOTENT, &what, |io| {
        download_once(io, &safe, dest, on_progress, timeout_secs, max_size)
    });
    match res {
        Ok(ok) => ok,
        Err(e) => download_failed(io, url, dest, &e),
    }
}

/// Одна попытка скачивания: `Ok(true)` — файл на месте, `Ok(false)` — отказ
/// БЕЗ сетевого сбоя (перебор по размеру, повторять незачем), `Err` — сбой
/// сети либо записи, который решает повторять [`net::with_retry`].
fn download_once<IO: NetIo + ?Sized>(
    io: &mut IO,
    safe_url: &str,
    dest: &str,
    on_progress: bool,
    timeout_secs: u64,
    max_size: i64,
) -> Result<bool, NetError> {
    let total = io.open(safe_url, USER_AGENT, timeout_secs)?;
    if max_size > 0 && total > max_size {
        io.log(LogLevel::Error, &format!("Архив слишком большой: {total}"));
        io.close();
        return Ok(false);
    }
    let mut done: i64 = 0;
    let dir = dirname(dest).to_string();
    if !dir.is_empty() {
        io.makedirs(&dir);
    }
    if let Err(e) = io.create(dest) {
        io.close();
        return Err(e);
    }
    loop {
        let chunk = match io.read(DOWNLOAD_CHUNK) {
            Ok(c) => c,
            Err(e) => {
                io.close_file();
                io.close();
                return Err(e);
            }
        };
        if chunk.is_empty() {
            break;
        }
        done += chunk.len() as i64;
        if max_size > 0 && done > max_size {
            // Оригинал здесь закрывает файл, стирает его и выходит БЕЗ записи
            // в журнал — размер уже проверялся выше, это дубль-страховка.
            io.close_file();
            io.remove(dest);
            io.close();
            return Ok(false);
        }
        if let Err(e) = io.write(&chunk) {
            io.close_file();
            io.close();
            return Err(e);
        }
        if on_progress {
            io.progress(done, total);
        }
    }
    io.close_file();
    io.close();
    Ok(true)
}

/// Элемент из кеша: `comment_count` через SQLite не проходит (колонки нет),
/// оригинал в этом случае оставляет атрибут неустановленным, а интерфейс
/// читает его через `getattr(item, "comment_count", None)`. Ноль — ровно то,
/// что видит интерфейс сегодня через PyO3-мост (`d.get("_comment_count", 0)`).
fn from_cache(item: ThemeItem) -> OcsContent {
    OcsContent { item, comment_count: 0 }
}

/// Синхронный клиент каталога — порт класса `OCSClient`.
///
/// Звать из рабочего потока: методы блокируются на сети ровно как оригинал,
/// который живёт в QThread-воркерах вкладки «Оформление». Исключение —
/// [`cached_search`](Self::cached_search): он не ходит в сеть и безопасен из
/// потока интерфейса.
pub struct OcsClient<IO: OcsIo> {
    io: IO,
    preview_dir: String,
}

impl<IO: OcsIo> OcsClient<IO> {
    /// `preview_dir` — каталог кеша превью (`PREVIEW_DIR`, см.
    /// [`crate::appearance_cache::preview_dir`]).
    pub fn new(io: IO, preview_dir: &str) -> Self {
        OcsClient { io, preview_dir: preview_dir.to_string() }
    }

    /// Забрать исполнителя обратно (parity-тестам — для финальных проверок).
    pub fn into_io(self) -> IO {
        self.io
    }

    /// Поиск контента — порт `OCSClient.search`; возвращает `(элементы,
    /// totalitems)`.
    ///
    /// `category_id` строкой: категории-зонты идут списком «716,488».
    /// Порядок решений оригинала сохранён: свежий кеш (если `use_cache`) →
    /// сеть → при сетевом сбое УСТАРЕВШИЙ кеш → пусто. Отказ магазина и
    /// сбой сети сворачиваются в пустую страницу — кому важна причина, зовёт
    /// [`search_checked`](Self::search_checked).
    ///
    /// ⚠️ Починка оригинала (правило #16): Python добивал пустой `typeid`
    /// строкой `it.category_id = it.category_id or category_id`, и для
    /// категории-зонта в целочисленное поле модели попадала строка «716,488»
    /// (дальше — сравнения int со str в интерфейсе). Здесь строка сначала
    /// разбирается в число, а неразбираемая (тот самый зонт) оставляет 0.
    pub fn search(
        &mut self,
        category_id: &str,
        page: i64,
        pagesize: i64,
        sort: &str,
        search: &str,
        use_cache: bool,
    ) -> (Vec<OcsContent>, i64) {
        self.search_checked(category_id, page, pagesize, sort, search, use_cache)
            .unwrap_or_default()
    }

    /// [`search`](Self::search) с причиной неудачи: `Err` — магазин не дал
    /// страницу (сбой связи, отказ, нечитаемый ответ) и подменить её из кеша
    /// нечем; `Ok` с пустым списком — товаров действительно нет.
    ///
    /// ⚠️ Расхождение с оригиналом: Python клал в кеш и ответ-отказ (пустой
    /// страницей) — и после сбоя магазина раздел час стоял пустым, а хорошая
    /// страница в кеше затиралась. Здесь в кеш ложится только успешный ответ.
    pub fn search_checked(
        &mut self,
        category_id: &str,
        page: i64,
        pagesize: i64,
        sort: &str,
        search: &str,
        use_cache: bool,
    ) -> Result<(Vec<OcsContent>, i64), StoreFailure> {
        let sortmode = sort_mode(sort);
        let key = list_key(category_id, page, sort, search);
        if use_cache {
            if let Some((items, total)) = self.io.cache_get_list(&key) {
                return Ok((items.into_iter().map(from_cache).collect(), total));
            }
        }

        let url = search_url(category_id, page, pagesize, sortmode, search);
        let raw = match http_get_result(&mut self.io, &url, TIMEOUT_SECS) {
            Ok(raw) => raw,
            // сеть упала — пробуем устаревший кеш как запасной вариант
            Err(e) => return self.cached_or(&key, StoreFailure::Network(e)),
        };
        let (mut items, total, diag) = parse_list_checked(&raw);
        match diag {
            Some(OcsDiag::Xml) => {
                self.io
                    .log(LogLevel::Error, "OCS XML parse error: некорректный XML-ответ");
                return self.cached_or(&key, StoreFailure::Malformed);
            }
            Some(OcsDiag::Status { code, message }) => {
                self.io.log(LogLevel::Warning, &format!("OCS status not ok: {code}"));
                return self.cached_or(&key, StoreFailure::Refused { code, message });
            }
            None => {}
        }
        let fallback_cat = category_id.parse::<i64>().unwrap_or(0);
        for it in &mut items {
            if it.item.category_id == 0 {
                it.item.category_id = fallback_cat;
            }
        }
        let plain: Vec<ThemeItem> = items.iter().map(|c| c.item.clone()).collect();
        self.io.cache_put_list(&key, &plain, total);
        Ok((items, total))
    }

    /// Магазин страницу не дал: страница из кеша, если она там есть, иначе
    /// причина неудачи.
    fn cached_or(
        &mut self,
        key: &str,
        why: StoreFailure,
    ) -> Result<(Vec<OcsContent>, i64), StoreFailure> {
        match self.io.cache_get_list(key) {
            Some((items, total)) => Ok((items.into_iter().map(from_cache).collect(), total)),
            None => Err(why),
        }
    }

    /// Только кеш, БЕЗ сети — порт `OCSClient.cached_search` (мгновенная
    /// отрисовка из потока интерфейса).
    pub fn cached_search(
        &mut self,
        category_id: &str,
        page: i64,
        sort: &str,
        search: &str,
    ) -> Option<(Vec<OcsContent>, i64)> {
        let key = list_key(category_id, page, sort, search);
        self.io
            .cache_get_list(&key)
            .map(|(items, total)| (items.into_iter().map(from_cache).collect(), total))
    }

    /// Детали одного элемента — порт `OCSClient.get_item`.
    ///
    /// Сетевой сбой ИЛИ неразбираемый ответ → элемент из кеша. Разобранный
    /// элемент кладётся в кеш; пустой `id` возвращает `None` без обращения к
    /// кешу (как в оригинале).
    pub fn get_item(&mut self, content_id: &str) -> Option<OcsContent> {
        let url = item_url(content_id);
        let Some(raw) = http_get(&mut self.io, &url, TIMEOUT_SECS) else {
            return self.io.cache_get_item(content_id).map(from_cache);
        };
        match parse_item_detail(&raw) {
            // не разобрать (битый XML / нет <data>/<content>) → кеш
            Err(MalformedDetail) => self.io.cache_get_item(content_id).map(from_cache),
            Ok(None) => None,
            Ok(Some(content)) => {
                self.io.cache_put_item(&content.item);
                Some(content)
            }
        }
    }

    /// Детали для ПОКАЗА — порт `OCSClient.get_item(content_id, max_age)`.
    ///
    /// Свежие полные детали (не старше `max_age_secs`) отдаются из кеша без
    /// сети; иначе — обычный [`get_item`](Self::get_item). Панель деталей
    /// зовёт это с [`DETAIL_TTL_SECS`]; установка — нет.
    pub fn get_item_cached(&mut self, content_id: &str, max_age_secs: u64) -> Option<OcsContent> {
        if let Some(hit) = self.io.cache_get_item_detail(content_id, max_age_secs) {
            return Some(from_cache(hit));
        }
        self.get_item(content_id)
    }

    /// Комментарии к контенту — порт `OCSClient.comments`.
    ///
    /// `Some([])` — комментариев нет (в т.ч. код 101); `None` — endpoint
    /// недоступен либо ответ не разобрать (интерфейс тихо прячет секцию).
    pub fn comments(
        &mut self,
        content_id: &str,
        page: i64,
        pagesize: i64,
    ) -> Option<Vec<OcsComment>> {
        let url = comments_url(content_id, page, pagesize);
        let raw = http_get(&mut self.io, &url, TIMEOUT_SECS)?;
        let (out, diag) = parse_comments_checked(&raw);
        match diag {
            Some(OcsDiag::Xml) => self.io.log(
                LogLevel::Warning,
                "OCS comments XML parse error: некорректный XML-ответ",
            ),
            Some(OcsDiag::Status { code, .. }) => self
                .io
                .log(LogLevel::Warning, &format!("OCS comments status not ok: {code}")),
            None => {}
        }
        out
    }

    /// Скачать превью в кеш, вернуть локальный путь — порт
    /// `OCSClient.get_preview_image`.
    pub fn get_preview_image(&mut self, url: &str) -> Option<String> {
        if url.is_empty() {
            return None;
        }
        // `if local:` — пустая строка у Python тоже означает промах кеша.
        if let Some(local) = self.io.cache_get_preview(url).filter(|p| !p.is_empty()) {
            return Some(local);
        }
        let dest = format!("{}/{}", self.preview_dir, preview_cache_name(url));
        if http_download(
            &mut self.io,
            url,
            &dest,
            false,
            PREVIEW_TIMEOUT_SECS,
            PREVIEW_MAX_SIZE,
        ) {
            self.io.cache_put_preview(url, &dest);
            return Some(dest);
        }
        None
    }

    /// Скачать архив темы в `dest` — порт `OCSClient.download`.
    ///
    /// `link_index` вне диапазона (по умолчанию `-1`) → авто-выбор лучшего для
    /// Linux линка ([`auto_file_index`]); файл, выбранный руками в панели
    /// деталей, приходит номером из [`file_index_for`]. `on_progress` — есть
    /// ли колбэк прогресса.
    pub fn download(
        &mut self,
        item: &ThemeItem,
        dest: &str,
        link_index: isize,
        on_progress: bool,
    ) -> bool {
        let url = if !item.download_links.is_empty() {
            let mut idx = link_index;
            if !(idx >= 0 && (idx as usize) < item.download_links.len()) {
                idx = auto_file_index(item);
            }
            item.download_links[idx as usize]
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        } else {
            item.download_url.clone()
        };
        if url.is_empty() {
            return false;
        }
        http_download(
            &mut self.io,
            &url,
            dest,
            on_progress,
            DOWNLOAD_TIMEOUT_SECS,
            DOWNLOAD_MAX_SIZE,
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════
//  Живой исполнитель клиента магазина
// ═══════════════════════════════════════════════════════════════════════════
//
// ЗАЧЕМ. Шапка [`OcsCacheIo`] обещает, что реализация «остаётся снаружи», и
// до 08.09.2026 снаружи она была ровно одна — на Python
// (`rexon/appearance/ocs_client.py` поверх `appearance/cache.py`). Значит
// каталог оформления работал только в процессе с интерпретатором, а вкладка
// «Оформление» под Rust-хозяином окна открывалась пустой витриной.
//
// Здесь обещание исполняется: сеть берётся у [`crate::net::UreqNet`], кеш —
// у [`crate::appearance_cache::LiveCache`], и ни одного нового правила при
// этом не появляется — обе половины уже были, не хватало только сборки.

/// Живой исполнитель [`OcsIo`]: настоящая сеть плюс настоящий SQLite-кеш.
///
/// Заводится НА ОДНУ работу (как `OCSClient()` у Python-воркера): внутри
/// лежит соединение sqlite, а оно не пересекает потоки.
pub struct LiveOcsIo {
    net: crate::net::UreqNet,
    cache: crate::appearance_cache::LiveCache,
}

impl LiveOcsIo {
    /// Сеть по умолчанию плюс кеш в `~/.cache/rexon`.
    pub fn new(home: &str) -> Self {
        LiveOcsIo {
            net: crate::net::UreqNet::new(),
            cache: crate::appearance_cache::LiveCache::open(home),
        }
    }

    /// То же, но с уже настроенным транспортом: отменяемым
    /// ([`crate::net::UreqNet::with_cancel`]) либо считающим ход скачивания
    /// ([`crate::net::UreqNet::with_progress`]).
    pub fn with_net(net: crate::net::UreqNet, home: &str) -> Self {
        LiveOcsIo { net, cache: crate::appearance_cache::LiveCache::open(home) }
    }

    /// Каталог кеша превью — второй аргумент [`OcsClient::new`].
    pub fn preview_dir(&self) -> String {
        self.cache.preview_dir().to_string()
    }

    /// Кеш напрямую: сканы диска берут из него сырьё для превью локальных
    /// тем, а чистка переводов — свою таблицу.
    pub fn cache_mut(&mut self) -> &mut crate::appearance_cache::LiveCache {
        &mut self.cache
    }
}

impl NetIo for LiveOcsIo {
    fn get(&mut self, url: &str, user_agent: &str, timeout_secs: u64) -> Result<Vec<u8>, NetError> {
        self.net.get(url, user_agent, timeout_secs)
    }
    fn open(&mut self, url: &str, user_agent: &str, timeout_secs: u64) -> Result<i64, NetError> {
        self.net.open(url, user_agent, timeout_secs)
    }
    fn read(&mut self, n: usize) -> Result<Vec<u8>, NetError> {
        self.net.read(n)
    }
    fn close(&mut self) {
        self.net.close();
    }
    fn makedirs(&mut self, path: &str) {
        self.net.makedirs(path);
    }
    fn create(&mut self, path: &str) -> Result<(), NetError> {
        self.net.create(path)
    }
    fn write(&mut self, chunk: &[u8]) -> Result<(), NetError> {
        self.net.write(chunk)
    }
    fn close_file(&mut self) {
        self.net.close_file();
    }
    fn exists(&mut self, path: &str) -> bool {
        self.net.exists(path)
    }
    fn remove(&mut self, path: &str) {
        self.net.remove(path);
    }
    fn progress(&mut self, done: i64, total: i64) {
        self.net.progress(done, total);
    }
    fn log(&mut self, level: LogLevel, message: &str) {
        self.net.log(level, message);
    }
    fn sleep(&mut self, ms: u64) {
        self.net.sleep(ms);
    }
    fn content_type(&mut self) -> String {
        self.net.content_type()
    }
}

impl OcsCacheIo for LiveOcsIo {
    fn cache_get_list(&mut self, key: &str) -> Option<(Vec<ThemeItem>, i64)> {
        self.cache.get_list(key)
    }
    fn cache_put_list(&mut self, key: &str, items: &[ThemeItem], total: i64) {
        self.cache.put_list(key, items, total);
    }
    fn cache_get_item(&mut self, content_id: &str) -> Option<ThemeItem> {
        self.cache.get_item(content_id)
    }
    fn cache_put_item(&mut self, item: &ThemeItem) {
        self.cache.put_item(item);
    }
    fn cache_get_item_detail(&mut self, content_id: &str, max_age_secs: u64) -> Option<ThemeItem> {
        self.cache.get_item_detail(content_id, max_age_secs as f64)
    }
    fn cache_get_preview(&mut self, url: &str) -> Option<String> {
        self.cache.get_preview(url)
    }
    fn cache_put_preview(&mut self, url: &str, local_path: &str) {
        self.cache.put_preview(url, local_path);
    }
}

#[cfg(test)]
mod license_tag_tests {
    use super::license_from_tags;

    #[test]
    fn license_comes_from_tags() {
        // Живые метки 04.10.2026: курсоры «Aemeath» и эффект «ba-click-fx».
        assert_eq!(
            license_from_tags("animatedcursor,efi,cursor,cursor-theme,gplv3"),
            "GPL-3.0"
        );
        assert_eq!(
            license_from_tags("original-product,unix,gplv3,linux,kwin,mouse-trail"),
            "GPL-3.0"
        );
        assert_eq!(license_from_tags("GPLv2-later, CC-BY-SA"), "GPL-2.0-or-later, CC-BY-SA");
        assert_eq!(license_from_tags("mit-license,mit"), "MIT");
    }

    #[test]
    fn unknown_tags_give_nothing() {
        assert_eq!(license_from_tags(""), "");
        assert_eq!(license_from_tags("linux,bsd,x11,apache,cc"), "");
    }
}

#[cfg(test)]
mod file_choice_tests {
    use super::{file_choices, file_index_for, file_note_at, pick_best_link_titled, Value};
    use crate::models::ThemeItem;
    use serde_json::json;

    fn item_with(names: &[&str], title: &str) -> ThemeItem {
        ThemeItem {
            id: "7".into(),
            name: title.into(),
            download_links: names
                .iter()
                .enumerate()
                .map(|(i, n)| json!({ "url": format!("https://f/{i}/{n}"), "name": n, "size": 100 * (i as i64 + 1) }))
                .collect(),
            ..ThemeItem::default()
        }
    }

    /// Варианты для выбора руками: все файлы по порядку выдачи, помечен
    /// ровно тот, что берётся автоматически (то же правило, что у
    /// установки с плитки), с размером и датой выкладки из имени.
    #[test]
    fn choices_list_all_files_and_mark_the_automatic_one() {
        let item = item_with(
            &["Extra 1.4.tar.xz", "McOS-MJV-Gnome-3.30-20230519162903.tar.xz", "McOS-MJV-Gnome-3.30-1.1.tar.xz"],
            "McOS-MJV-Gnome-3.30",
        );
        let got = file_choices(&item);
        let names: Vec<&str> = got.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["Extra 1.4.tar.xz", "McOS-MJV-Gnome-3.30-20230519162903.tar.xz", "McOS-MJV-Gnome-3.30-1.1.tar.xz"]
        );
        let chosen: Vec<bool> = got.iter().map(|c| c.chosen).collect();
        assert_eq!(chosen, [false, true, false], "автоматически — тот же, что у плитки");
        assert_eq!(got.iter().filter(|c| c.chosen).count(), 1);
        assert_eq!(got[2].size_kib, 300);
        // Дата — только из метки времени магазина в имени; без неё пусто,
        // а не выдумано.
        assert_eq!(got[1].date, "2023-05-19");
        assert_eq!(got[0].date, "");
        assert_eq!(got[2].date, "");
    }

    /// Файл один (или ни одного) — выбирать не из чего, блока нет.
    #[test]
    fn single_file_has_no_choice() {
        assert!(file_choices(&item_with(&["Only.tar.xz"], "Only")).is_empty());
        assert!(file_choices(&item_with(&[], "x")).is_empty());
    }

    /// Выбранный руками файл находится по имени; неизвестное или пустое
    /// имя — автоматический выбор, а не отказ и не первый файл.
    #[test]
    fn explicit_file_name_overrides_the_automatic_choice() {
        let item = item_with(&["Extra 1.4.tar.xz", "Theme.tar.xz"], "Theme");
        assert_eq!(file_index_for(&item, ""), 1);
        assert_eq!(file_index_for(&item, "Extra 1.4.tar.xz"), 0);
        assert_eq!(file_index_for(&item, "Gone.tar.xz"), 1);
        // Строка журнала называет тот файл, который действительно берётся.
        let note = file_note_at(&item, 0).unwrap();
        assert!(note.contains("Extra 1.4.tar.xz"), "{note}");
        assert!(file_note_at(&item_with(&["A.zip"], ""), 0).is_none());
    }

    fn links(names: &[&str]) -> Vec<Value> {
        names.iter().map(|n| json!({ "url": format!("https://f/{n}"), "name": n, "size": 1 })).collect()
    }

    fn chosen<'a>(names: &[&'a str], title: &str) -> &'a str {
        let i = pick_best_link_titled(&links(names), false, title);
        names[usize::try_from(i).unwrap()]
    }

    /// Живые товары магазина 19.10.2026: файлы приходят по алфавиту, и
    /// архив дополнений («Extra …») шёл первым при равных баллах.
    #[test]
    fn extras_archive_is_not_the_theme() {
        assert_eq!(
            chosen(
                &["Extra 1.4.tar.xz", "McOS-MJV-Cinnamon-Edition-2.0.tar.gz", "McOS-Mint-Cinnamon-Edition-master.tar.xz"],
                "McOS-Mint-Cinnamon-Edition",
            ),
            "McOS-Mint-Cinnamon-Edition-master.tar.xz"
        );
        assert_eq!(
            chosen(
                &["Extra 1.4.tar.xz", "McOS-MJV-Dark-mode-Gnome-3.30-1.1.tar.xz", "McOS-MJV-Gnome-3.30-1.1.tar.xz"],
                "McOS-MJV-Gnome-3.30",
            ),
            "McOS-MJV-Gnome-3.30-1.1.tar.xz"
        );
        assert_eq!(chosen(&["Extra's.tar.xz", "U8.DARK.tar.xz", "U8.tar.xz"], "UNITY8 - RE-DESIGNED"), "U8.DARK.tar.xz");
        assert_eq!(chosen(&["extra-Icons.zip", "Theme.zip"], ""), "Theme.zip");
        // Других архивов нет — берётся и дополнение (лучше, чем ничего).
        assert_eq!(chosen(&["Extra 1.4.tar.xz", "preview.png"], ""), "Extra 1.4.tar.xz");
        // Слово, а не часть слова: «Extravaganza» — не дополнение.
        assert_eq!(chosen(&["Extravaganza.tar.xz", "B.tar.xz"], ""), "Extravaganza.tar.xz");
    }

    /// Среди равноценных архивов — тот, чьё имя совпадает с названием
    /// товара; точное совпадение — раньше вхождения.
    #[test]
    fn file_named_like_the_item_wins_ties() {
        assert_eq!(chosen(&["Rage-Blue-Cursor.tar.gz", "Rage-Cursor.tar.gz"], "Rage cursor"), "Rage-Cursor.tar.gz");
        assert_eq!(
            chosen(&["Redmond-firefox.tar.gz", "Redmond97-icons.tar.gz", "Redmond97.tar.gz"], "Redmond97"),
            "Redmond97.tar.gz"
        );
        // Совпадение имени не перебивает класс файла: Windows-сборка и
        // пакет дистрибутива остаются ниже архива.
        assert_eq!(chosen(&["Rage-Cursor-windows.zip", "Other.tar.gz"], "Rage cursor"), "Other.tar.gz");
        assert_eq!(chosen(&["Rage-Cursor.deb", "Other.tar.gz"], "Rage cursor"), "Other.tar.gz");
        // Метка времени магазина к имени не относится: из двух выкладок
        // одного файла остаётся первая по порядку выдачи.
        assert_eq!(
            chosen(&["Nilium-20200426034630.tar.gz", "Nilium.tar.gz"], "Nilium"),
            "Nilium-20200426034630.tar.gz"
        );
        assert_eq!(
            chosen(&["BeautyFolders-20230519162903.tar.gz", "BeautyLine-20230918134713.tar.gz"], "BeautyLine"),
            "BeautyLine-20230918134713.tar.gz"
        );
        // Без названия — прежний порядок: первый из равных.
        assert_eq!(chosen(&["A.tar.xz", "B.tar.xz"], ""), "A.tar.xz");
        assert_eq!(pick_best_link_titled(&[], false, "x"), -1);
    }
}
