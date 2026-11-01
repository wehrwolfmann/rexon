//! «Установить из файла…» — тема, которая УЖЕ лежит на диске.
//!
//! Кнопка в шапке каждого раздела «Оформления», как у KDE: человек скачал
//! тему сам (в том числе курсоры и шрифты из Windows) и показывает Rexon
//! файл, несколько файлов или папку. Здесь решается, ЧТО это и КУДА его
//! ставить, а ставит тот же исполнитель, что у магазина:
//!
//! * архив или папка темы — [`crate::appearance_installer::LiveInstaller::install_extracted`]
//!   (раскладка корней тем, переклассификация, проверка KPackage); системные
//!   виды (SDDM, Plymouth) — тем же помощником с проверками `system_plan`;
//! * один файл схемы цветов, темы Kvantum, картинки фона блокировки —
//!   [`crate::appearance_installer::LiveInstaller::install`] (ветка «товар
//!   одним файлом»);
//! * `.rxn` и форматы Windows (`.cur`/`.ani`/`.inf`, наборы курсоров
//!   архивом, `.themepack`, `.msstyles`, `.ico`…) — [`crate::ext_exec::install_from_file`]
//!   (импортёры `crate::importers`, анимация `.ani` сохраняется ими же);
//! * шрифты — [`install_font_file`] в `$XDG_DATA_HOME/fonts` (тот же путь,
//!   что «Управление шрифтами» в «Настройках»: одна дверь на продукт);
//! * картинки, видео и Flash для обоев — в библиотеку обоев Rexon пакетом
//!   `wallpaper-pack` (её показывает и применяет «Настройки → Обои» в любом
//!   окружении).
//!
//! Всё ставится по-пользовательски в каталоги XDG — их читают все окружения
//! (значки, курсоры, GTK, звуки, шрифты через fontconfig); «Применить» потом
//! решает родной ключ каждого окружения.
//!
//! РАСПОЗНАВАНИЕ — по содержимому, а не по имени: подписи файлов
//! ([`sniff`]) и устройство дерева ([`detect_tree`]). Тема не того вида, что
//! раздел, ставится туда, где ей место, и человеку это говорится словами;
//! системная тема из чужого раздела НЕ ставится (окно пароля без явного
//! «ставлю экран входа» — ловушка, правило #13) — ему предлагают открыть её
//! раздел.
//!
//! БЕЗОПАСНОСТЬ. Архив проверяется ДО распаковки (имена записей —
//! `live_safe_extract`, общий размер и число записей — [`unpacked_budget_ok`]),
//! после распаковки из дерева убираются ссылки наружу и особые файлы
//! ([`sanitize_tree`]). Папка человека не ставится «как есть», а копируется
//! во временный каталог тем же фильтром ([`stage_dir`]): ссылка из темы на
//! `~/.ssh` до каталога вида не доедет. Системные виды идут только через
//! помощника — его проверки (`installer::system_theme_problem`, allowlist
//! `system_plan`) здесь не ослабляются и не дублируются.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::appearance_installer::{find_content_root, find_theme_roots, IMAGE_EXTS};
use crate::i18n::{tr, trf};
use crate::installed_scan::FsSnapshot;
use crate::models::{ThemeCategory, ThemeItem};

// ============================================================================
// Пределы
// ============================================================================

/// Сколько байт может занять распакованное. Темы весят мегабайты, пачки обоев
/// KDE — сотни мегабайт; больше двух гигабайт — это не тема, а распаковка
/// всё равно идёт в `/tmp`, то есть в память.
pub const MAX_UNPACKED_BYTES: u64 = 2 << 30;

/// Сколько записей может быть в архиве или папке. У крупнейших тем значков
/// десятки тысяч файлов и ссылок; сотня тысяч — потолок с запасом.
pub const MAX_ENTRIES: u64 = 100_000;

/// Расширения архивов, которые распаковывает установщик (`live_safe_extract`:
/// ZIP — сам, TAR со сжатием — системный `tar`, 7z — `bsdtar` или 7-Zip;
/// так же приходят пакеты тем Conky `.cmtp.7z`). RAR понимает только
/// импортёр наборов курсоров Windows.
pub const ARCHIVE_EXTS: &[&str] = &[
    "zip", "tar", "tar.gz", "tgz", "tar.xz", "txz", "tar.bz2", "tbz2", "tbz", "tar.zst",
    "tzst", "7z",
];

// ============================================================================
// Подпись файла
// ============================================================================

/// Что за файл — по первым байтам, а где подписи нет — по имени.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FileSort {
    Zip,
    /// TAR, в том числе сжатый gzip/xz/bzip2/zstd (распаковывает `tar`).
    Tar,
    SevenZip,
    Rar,
    Font,
    Image,
    Video,
    Flash,
    /// Курсор Windows `.cur`.
    Cur,
    /// Анимированный курсор Windows `.ani` (RIFF/ACON).
    Ani,
    /// Значок Windows `.ico`.
    Ico,
    ColorScheme,
    Kvantum,
    Other,
}

/// Расширение имени в нижнем регистре без точки; у двойных архивных
/// (`tar.gz`) — целиком.
pub fn ext_of(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name).to_lowercase();
    for double in ["tar.gz", "tar.xz", "tar.bz2", "tar.zst", "pcf.gz"] {
        if base.ends_with(&format!(".{double}")) {
            return double.to_string();
        }
    }
    match base.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_string(),
        _ => String::new(),
    }
}

/// Подпись файла по первым байтам (`head` — до 512 байт) и имени.
///
/// Подпись решает раньше имени: курсор, присланный как `arrow.bin`, — всё
/// равно курсор, а «картинка.jpg», внутри которой ZIP, — архив. Имя спасает
/// там, где подписи у формата нет (`.colors`, `.kvconfig`, шрифты BDF/PFA).
pub fn sniff(name: &str, head: &[u8]) -> FileSort {
    let at = |off: usize, sig: &[u8]| head.len() >= off + sig.len() && &head[off..off + sig.len()] == sig;
    if at(0, b"PK\x03\x04") || at(0, b"PK\x05\x06") {
        return FileSort::Zip;
    }
    if at(0, b"7z\xbc\xaf\x27\x1c") {
        return FileSort::SevenZip;
    }
    if at(0, b"Rar!\x1a\x07") {
        return FileSort::Rar;
    }
    let ext = ext_of(name);
    // Сжатый одиночный шрифт (`.pcf.gz`) — тоже gzip; тут имя решает раньше.
    if ext == "pcf.gz" {
        return FileSort::Font;
    }
    if at(0, b"\x1f\x8b")
        || at(0, b"\xfd7zXZ\x00")
        || at(0, b"BZh")
        || at(0, b"\x28\xb5\x2f\xfd")
        || at(257, b"ustar")
    {
        return FileSort::Tar;
    }
    if at(0, b"\x00\x01\x00\x00")
        || at(0, b"OTTO")
        || at(0, b"true")
        || at(0, b"typ1")
        || at(0, b"ttcf")
        || at(0, b"wOFF")
        || at(0, b"wOF2")
    {
        return FileSort::Font;
    }
    if at(0, b"RIFF") && at(8, b"ACON") {
        return FileSort::Ani;
    }
    if at(0, b"RIFF") && at(8, b"AVI ") {
        return FileSort::Video;
    }
    if at(0, b"\x1a\x45\xdf\xa3") {
        return FileSort::Video;
    }
    if at(4, b"ftyp") {
        // У AVIF и HEIF тот же контейнер, что у MP4: решает марка.
        let image_brand = [b"avif", b"avis", b"heic", b"heix", b"mif1", b"msf1"]
            .iter()
            .any(|b| at(8, *b));
        return if image_brand { FileSort::Image } else { FileSort::Video };
    }
    if at(0, b"FWS") || at(0, b"CWS") || at(0, b"ZWS") {
        return FileSort::Flash;
    }
    if at(0, b"\x00\x00\x02\x00") {
        return FileSort::Cur;
    }
    if at(0, b"\x00\x00\x01\x00") {
        return FileSort::Ico;
    }
    if crate::appearance_installer::looks_like_image(false, head, &[]) {
        return FileSort::Image;
    }
    // Подписи нет — имя.
    let text = String::from_utf8_lossy(head);
    if ext == "colors" || text.contains("[ColorEffects:") || text.contains("[Colors:Window]") {
        return FileSort::ColorScheme;
    }
    if ext == "kvconfig" || text.contains("[%General]") {
        return FileSort::Kvantum;
    }
    if crate::qt_settings::FONT_INSTALL_EXTS.contains(&ext.as_str()) {
        return FileSort::Font;
    }
    match ext.as_str() {
        "cur" => FileSort::Cur,
        "ani" => FileSort::Ani,
        "ico" => FileSort::Ico,
        e if IMAGE_EXTS.contains(&format!(".{e}").as_str()) => FileSort::Image,
        e if crate::qt_settings::VIDEO_EXTS.contains(&e) => FileSort::Video,
        e if crate::qt_settings::FLASH_EXTS.contains(&e) => FileSort::Flash,
        _ => FileSort::Other,
    }
}

/// Файл годится в библиотеку обоев (картинка, видео, Flash).
pub fn is_media(sort: FileSort) -> bool {
    matches!(sort, FileSort::Image | FileSort::Video | FileSort::Flash)
}

// ============================================================================
// Устройство дерева: что за тема
// ============================================================================

/// Что нашлось в распакованном архиве или папке.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Found {
    /// Тема вида этого раздела (ключ раздела боковой панели).
    Section(String),
    /// Набор курсоров Windows (`.cur`/`.ani`) — его переводит импортёр.
    WindowsCursors,
    /// Только шрифты (и мелочь вроде readme).
    Fonts,
    /// Только картинки/видео — обои.
    Media,
}

/// Подпись пакета KDE (KPackage) по `metadata.json`/`metadata.desktop`:
/// `KPackageStructure` либо, у старых пакетов, `ServiceTypes`.
fn kpackage_structure(fs: &FsSnapshot, dir: &str) -> String {
    if let Some(text) = fs.read_text(&format!("{dir}/metadata.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
            if let Some(s) = v.get("KPackageStructure").and_then(|s| s.as_str()) {
                return s.to_string();
            }
            let types = v
                .get("KPlugin")
                .and_then(|k| k.get("ServiceTypes"))
                .and_then(|s| s.as_array())
                .and_then(|a| a.first())
                .and_then(|s| s.as_str());
            if let Some(s) = types {
                return s.to_string();
            }
        }
    }
    if let Some(text) = fs.read_text(&format!("{dir}/metadata.desktop")) {
        for line in text.lines() {
            let line = line.trim();
            for key in ["X-KDE-ServiceTypes=", "ServiceTypes="] {
                if let Some(v) = line.strip_prefix(key) {
                    return v.split([',', ';']).next().unwrap_or("").trim().to_string();
                }
            }
        }
    }
    String::new()
}

/// Все файлы под `root` (до глубины 12, скрытые каталоги пропускаются).
fn files_under(fs: &FsSnapshot, root: &str) -> Vec<String> {
    fn walk(fs: &FsSnapshot, dir: &str, depth: usize, out: &mut Vec<String>) {
        for name in fs.scandir(dir) {
            let path = format!("{dir}/{name}");
            if fs.is_dir(&path) {
                if depth < 12 && !name.starts_with('.') {
                    walk(fs, &path, depth + 1, out);
                }
            } else {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(fs, root.trim_end_matches('/'), 0, &mut out);
    out
}

/// Мелочь, которая лежит рядом с чем угодно и вида не меняет.
fn is_incidental(name: &str) -> bool {
    let l = name.rsplit('/').next().unwrap_or(name).to_lowercase();
    let ext = ext_of(&l);
    l.starts_with("readme")
        || l.starts_with("license")
        || l.starts_with("licence")
        || l.starts_with("copying")
        || matches!(ext.as_str(), "txt" | "md" | "html" | "htm" | "url" | "pdf" | "nfo" | "rtf" | "")
}

/// Что лежит в распакованном (`extract_dir`) — решение по устройству, а не
/// по имени архива. `None` — понять не удалось: тогда раздел ставит дерево
/// по своему правилу, как товар магазина.
pub fn detect_tree(fs: &FsSnapshot, extract_dir: &str) -> Option<Found> {
    let root = find_content_root(fs, extract_dir);
    let sec = |k: &str| Some(Found::Section(k.to_string()));
    let names = fs.scandir(&root);

    // Системные виды — первыми: их признаки однозначны.
    if names.iter().any(|n| n.ends_with(".plymouth")) {
        return sec("plymouth");
    }
    let sddm_meta = fs
        .read_text(&format!("{root}/metadata.desktop"))
        .is_some_and(|t| t.contains("[SddmGreeterTheme]"));
    if sddm_meta || (fs.is_file(&format!("{root}/Main.qml")) && fs.is_file(&format!("{root}/theme.conf"))) {
        return sec("sddm");
    }

    // Пакеты KDE: вид назван в описании пакета.
    let structure = kpackage_structure(fs, &root);
    match structure.as_str() {
        "Plasma/LookAndFeel" => return sec("global"),
        "Plasma/Theme" => return sec("plasma"),
        "Plasma/Applet" => return sec("plasmoids"),
        "Plasma/Wallpaper" => {
            // Пачка картинок KDE (`contents/images`) и плагин обоев (QML)
            // описаны одинаково — различает содержимое.
            let images = fs.is_dir(&format!("{root}/contents/images"));
            let ui = fs.is_dir(&format!("{root}/contents/ui"));
            return sec(if images && !ui { "wallpapers" } else { "wallpaper_plugins" });
        }
        "Wallpaper/Images" => return sec("wallpapers"),
        "KWin/Script" => return sec("kwinscripts"),
        "KWin/Effect" => return sec("kwineffects"),
        "KWin/WindowSwitcher" => return sec("tabbox"),
        _ => {}
    }
    if fs.is_dir(&format!("{root}/contents/images")) && !fs.is_dir(&format!("{root}/contents/ui")) {
        return sec("wallpapers");
    }

    // Темы, которые узнаёт сам установщик магазина.
    match crate::appearance_installer::detect_actual_kde_type(fs, &root).as_deref() {
        Some("look-and-feel") => return sec("global"),
        Some("plasma-theme") => return sec("plasma"),
        Some("color-scheme") => return sec("colors"),
        Some("aurorae") => return sec("aurorae"),
        Some("kvantum") => return sec("kvantum"),
        Some("icons") => return sec("icons"),
        _ => {}
    }

    // Темы с корнями где угодно в глубине обёртки. Курсоры раньше значков:
    // признак значков (`index.theme` с `[Icon Theme]`) есть и у курсоров.
    let has_cursor_dir = |r: &String| fs.is_dir(&format!("{r}/cursors"));
    let cursor_roots = find_theme_roots(fs, &root, "cursor");
    if !cursor_roots.is_empty() && cursor_roots.iter().all(has_cursor_dir) {
        return sec("cursor");
    }
    if !find_theme_roots(fs, &root, "sounds").is_empty() {
        return sec("sounds");
    }
    if !find_theme_roots(fs, &root, "aurorae").is_empty() {
        return sec("aurorae");
    }
    if !find_theme_roots(fs, &root, "gtk").is_empty() {
        return sec("gtk");
    }
    if !cursor_roots.is_empty() {
        return sec("icons");
    }

    // Пакет тем Conky: виджеты в скрытых `.conky/` (`.Conky/`,
    // `.config/conky/`) — обход по файлам ниже скрытые каталоги не смотрит;
    // набор виджетов `.cmtheme` — тоже Conky.
    let conky_pack = [extract_dir.trim_end_matches('/'), root.as_str()].iter().any(|base| {
        crate::appearance_installer::CONKY_PACK_DIRS.iter().any(|d| fs.is_dir(&format!("{base}/{d}")))
    });
    if conky_pack {
        return sec("conky");
    }

    // По файлам.
    let files = files_under(fs, &root);
    let meaningful: Vec<&String> = files.iter().filter(|f| !is_incidental(f)).collect();
    let ext_is = |f: &str, exts: &[&str]| exts.contains(&ext_of(f).as_str());
    if files.iter().any(|f| ext_is(f, &["colors"])) {
        return sec("colors");
    }
    if files.iter().any(|f| ext_is(f, &["kvconfig"])) {
        return sec("kvantum");
    }
    if files.iter().any(|f| ext_is(f, &["cmtheme"])) {
        return sec("conky");
    }
    if files.iter().any(|f| ext_is(f, &["cur", "ani"])) {
        return Some(Found::WindowsCursors);
    }
    if meaningful.is_empty() {
        return None;
    }
    if meaningful.iter().all(|f| ext_is(f, crate::qt_settings::FONT_INSTALL_EXTS)) {
        return Some(Found::Fonts);
    }
    let media = |f: &str| {
        IMAGE_EXTS.contains(&format!(".{}", ext_of(f)).as_str())
            || ext_is(f, crate::qt_settings::VIDEO_EXTS)
            || ext_is(f, crate::qt_settings::FLASH_EXTS)
    };
    if meaningful.iter().all(|f| media(f)) {
        return Some(Found::Media);
    }
    None
}

// ============================================================================
// Раздел против найденного
// ============================================================================

/// Ключ найденного для сравнения с разделом.
fn found_key(found: &Found) -> &str {
    match found {
        Found::Section(k) => k,
        Found::WindowsCursors => "cursor",
        Found::Fonts => "fonts",
        Found::Media => "wallpapers",
    }
}

/// Системные виды: ставятся помощником от root.
pub fn is_root_section(key: &str) -> bool {
    matches!(key, "sddm" | "plymouth")
}

/// Подходит ли найденный вид разделу, из которого его ставят.
///
/// Кроме точного совпадения: заставка и экран блокировки — это пакеты
/// Look-and-Feel; «Стиль приложений» — темы Kvantum; фон блокировки — те же
/// картинки, что обои. «Загруженные» принимают всё: там человек не выбирал
/// вид.
pub fn section_accepts(section: &str, found: &Found) -> bool {
    let key = found_key(found);
    if section == key || section == "downloaded" {
        return true;
    }
    match section {
        "splash" | "lockscreen" => key == "global",
        "widgetstyle" => key == "kvantum",
        "lockwallpaper" => key == "wallpapers",
        _ => false,
    }
}

/// Раздел окружения (рамки XFWM4, оболочка GNOME…)?
fn is_de_section(key: &str) -> bool {
    crate::categories::DE_SECTIONS.iter().any(|(_, k)| *k == key)
}

/// Куда ставить то, что нашлось, из раздела `section`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Placement {
    /// В раздел с этим ключом. `moved` — не в тот, из которого ставили.
    Section { key: String, moved: bool },
    /// Шрифты.
    Fonts { moved: bool },
    /// Библиотека обоев Rexon.
    Library { moved: bool },
    /// Набор курсоров Windows — импортёр.
    WindowsCursors { moved: bool },
    /// Системная тема из чужого раздела: не ставим, предлагаем её раздел.
    OtherRootSection { key: String },
}

/// Решение о месте. `found = None` — вид не опознан: дерево ставится по
/// правилу раздела (как товар магазина); в «Загруженных» правила нет —
/// тогда `None`.
pub fn place(section: &str, found: Option<&Found>) -> Option<Placement> {
    let Some(found) = found else {
        if section == "downloaded" {
            return None;
        }
        return Some(Placement::Section { key: install_key(section), moved: false });
    };
    // Раздел окружения ставит своё сам: признаки GTK есть и у тем оболочки
    // GNOME или рамок XFWM4, и «переезд» в «Темы GTK» был бы ошибкой.
    // Уводятся только явно чужие виды.
    if is_de_section(section) {
        let foreign = match found {
            Found::Section(k) => !(k == "gtk" || is_de_section(k)),
            _ => true,
        };
        if !foreign {
            return Some(Placement::Section { key: section.to_string(), moved: false });
        }
    }
    let moved = !section_accepts(section, found);
    Some(match found {
        Found::Fonts => Placement::Fonts { moved },
        Found::WindowsCursors => Placement::WindowsCursors { moved },
        Found::Media if section == "lockwallpaper" => {
            Placement::Section { key: "lockwallpaper".into(), moved: false }
        }
        Found::Media => Placement::Library { moved },
        Found::Section(k) => {
            if is_root_section(k) && section != k {
                return Some(Placement::OtherRootSection { key: k.clone() });
            }
            let key = if moved { k.clone() } else { install_key_for(section, k) };
            Placement::Section { key, moved }
        }
    })
}

/// Ключ категории установки для раздела (у «Стиля приложений» своего
/// каталога нет — темы Kvantum ставятся в раздел Kvantum).
fn install_key(section: &str) -> String {
    match section {
        "widgetstyle" => "kvantum".to_string(),
        other => other.to_string(),
    }
}

/// Категория установки, когда найденное подходит разделу.
fn install_key_for(section: &str, found: &str) -> String {
    match section {
        // Заставка и экран блокировки ставят пакет Look-and-Feel в свой
        // каталог по своей категории (её применение — своё).
        "splash" | "lockscreen" | "lockwallpaper" => section.to_string(),
        "downloaded" | "widgetstyle" => found.to_string(),
        _ => install_key(section),
    }
}

// ============================================================================
// Окно выбора файлов
// ============================================================================

/// Показывать ли кнопку в разделе с этим ключом. Обзоры (активные,
/// системные, избранное) и витрины (популярное, новое) — не разделы вида;
/// «Загруженные» — да: там тема узнаётся сама.
pub fn button_for(section: &str, is_virtual: bool) -> bool {
    if section == crate::categories::PLASMA_LOGIN_KEY {
        return false;
    }
    !is_virtual || section == "downloaded"
}

fn masks(exts: &[&str]) -> Vec<String> {
    exts.iter().map(|e| format!("*.{e}")).collect()
}

/// Расширения, которые раздел ждёт в окне выбора (помимо архивов и `.rxn`).
fn section_exts(section: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    let images: Vec<&'static str> = IMAGE_EXTS.iter().map(|e| e.trim_start_matches('.')).collect();
    match section {
        "fonts" => out.extend(crate::qt_settings::FONT_INSTALL_EXTS),
        "cursor" => out.extend(["cur", "ani", "inf", "crs", "curxptheme", "7z", "rar"]),
        "icons" => out.extend(["ico", "icl", "iconpackage", "ip", "7tsp"]),
        "colors" => out.push("colors"),
        "widgetstyle" | "kvantum" => out.push("kvconfig"),
        "global" => out.extend(["themepack", "deskthemepack", "theme", "msstyles"]),
        "wallpapers" => {
            out.extend(images);
            out.extend(crate::qt_settings::VIDEO_EXTS);
            out.extend(crate::qt_settings::FLASH_EXTS);
            // `wmv`/`mpg`/`mpeg` уже пришли общим списком видео.
            out.push("dream");
        }
        "lockwallpaper" => out.extend(images),
        "downloaded" => {
            for s in ["fonts", "cursor", "icons", "colors", "kvantum", "global", "wallpapers"] {
                for e in section_exts(s) {
                    if !out.contains(&e) {
                        out.push(e);
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// Фильтры окна выбора файлов (`nameFilters` QML): своё раздела + архивы
/// и `.rxn` одной группой, затем «Все файлы». Распознавание всё равно по
/// содержимому — фильтр только помогает найти нужное в папке.
pub fn dialog_filters(section: &str) -> Vec<String> {
    let mut exts: Vec<&str> = section_exts(section);
    for e in ARCHIVE_EXTS.iter().copied().chain(["rxn"]) {
        if !exts.contains(&e) {
            exts.push(e);
        }
    }
    vec![
        // TRANSLATORS: file dialog filter; %1 is the list of file masks.
        trf("Supported files (%1)", &[&masks(&exts).join(" ")]),
        tr("All files (*)"),
    ]
}

/// Адрес из окна выбора (`file:///…`, проценты) — путь на диске. Строка без
/// схемы — уже путь.
pub fn file_url_to_path(url: &str) -> String {
    let Some(rest) = url.strip_prefix("file://") else {
        return url.to_string();
    };
    // `file://host/путь` у локального файла не бывает; хост пуст.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ============================================================================
// Итог
// ============================================================================

/// Итог установки для страницы.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LocalOutcome {
    /// Всё выбранное поставлено.
    pub ok: bool,
    /// Поставлено хоть что-то (часть файлов могла не встать).
    pub installed: bool,
    /// Что сказать человеку (строки по файлам).
    pub message: String,
    /// Раздел, который стоит открыть («открыть раздел «Курсоры»?»); пусто —
    /// никакой.
    #[serde(rename = "openKey")]
    pub open_key: String,
    /// Подпись кнопки открытия раздела.
    #[serde(rename = "openText")]
    pub open_text: String,
    /// Поставлено что-то, что видно в открытом разделе с диска: перечитать.
    pub reload: bool,
}

/// Имя раздела для человека по ключу (ключ, если такого нет).
fn section_name(home: &Path, key: &str) -> String {
    crate::categories::get_category(home, key)
        .or_else(|| crate::categories::categories(home).into_iter().find(|c| c.key == key))
        .map(|c| c.name)
        .unwrap_or_else(|| key.to_string())
}

/// «X — это тема другого раздела: поставлено туда».
pub fn moved_note(file: &str, section: &str) -> String {
    trf("“%1” is a theme for the “%2” section, so it was installed there.", &[&file, &section])
}

/// Системная тема из чужого раздела.
pub fn other_root_section_message(file: &str, section: &str) -> String {
    trf(
        "“%1” is a theme for the “%2” section. It changes a system screen and needs the \
         administrator password, so install it from that section.",
        &[&file, &section],
    )
}

/// Не опознано.
pub fn unknown_file_message(file: &str) -> String {
    trf(
        "Couldn't tell what “%1” is: it doesn't look like a theme, font, cursor set or \
         wallpaper. Open the section of the right kind and install it from there.",
        &[&file],
    )
}

/// Архив RAR не с курсорами.
pub fn unsupported_archive_message(file: &str, format: &str) -> String {
    trf(
        "“%1” is a %2 archive. Rexon unpacks only ZIP, TAR and 7z archives here — unpack it \
         yourself and choose the folder.",
        &[&file, &format],
    )
}

/// Слишком большое.
pub fn too_large_message(file: &str) -> String {
    trf(
        "“%1” is too large to unpack: over 2 GB or more than 100,000 files. A theme is \
         never that big.",
        &[&file],
    )
}

/// Ссылки наружу выброшены.
pub fn links_dropped_note(n: usize) -> String {
    trf("Links pointing outside the theme were left out: %1.", &[&n])
}

/// Обои легли в библиотеку.
pub fn library_added_message(names: &str) -> String {
    trf("Added to the 📚 Wallpaper Library (Settings → Wallpapers): %1", &[&names])
}

/// Где применять поставленное, если сам раздел показывает магазин.
pub fn apply_where_note(view: &str) -> String {
    trf("You can apply it in “%1”.", &[&view])
}

/// Имя выбранного для строки хода: один файл — его имя, несколько — первое
/// и сколько ещё.
pub fn busy_label(paths: &[String]) -> String {
    let name = |p: &str| short(Path::new(&file_url_to_path(p)));
    match paths {
        [] => String::new(),
        [one] => name(one),
        [first, rest @ ..] => trf("%1 and %2 more", &[&name(first), &rest.len()]),
    }
}

/// Подпись кнопки «Открыть раздел».
pub fn open_section_btn(name: &str) -> String {
    trf("Open “%1”", &[&name])
}

// ============================================================================
// Живое исполнение
// ============================================================================

/// Прочитать до `n` первых байт.
fn read_head(path: &Path, n: usize) -> Vec<u8> {
    use std::io::Read as _;
    let mut buf = vec![0u8; n];
    let Ok(mut f) = std::fs::File::open(path) else { return Vec::new() };
    let mut got = 0;
    while got < n {
        match f.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(k) => got += k,
            Err(_) => break,
        }
    }
    buf.truncate(got);
    buf
}

/// Временный каталог под `base` (имя с меткой и случайным хвостом).
fn mkdtemp(base: &Path, prefix: &str) -> Result<PathBuf, String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    for k in 0..64u32 {
        let p = base.join(format!("{prefix}{:x}{:x}{k}", std::process::id(), nanos));
        match std::fs::create_dir(&p) {
            Ok(()) => {
                use std::os::unix::fs::PermissionsExt as _;
                let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700));
                return Ok(p);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", p.display())),
        }
    }
    Err(format!("{}: {prefix}…", base.display()))
}

/// Сумма размеров и число записей архива — ДО распаковки. `None` — архив
/// не читается (тогда скажет распаковщик).
pub fn archive_budget(path: &Path, sort: FileSort) -> Option<(u64, u64)> {
    match sort {
        FileSort::Zip => {
            let file = std::fs::File::open(path).ok()?;
            let mut zip = zip::ZipArchive::new(file).ok()?;
            let mut total = 0u64;
            for i in 0..zip.len() {
                total = total.saturating_add(zip.by_index(i).ok()?.size());
            }
            Some((total, zip.len() as u64))
        }
        FileSort::Tar => {
            let report = crate::utils::run(
                &crate::utils::RunSpec {
                    argv: vec!["tar".into(), "-tvf".into(), path.to_string_lossy().into_owned()],
                    output: crate::utils::RunOutput::Capture,
                    timeout_secs: Some(120),
                    lang_c: true,
                    ..Default::default()
                },
                None,
            );
            if report.returncode != Some(0) {
                return None;
            }
            Some(tar_listing_budget(&crate::utils::decode_text(&report.stdout)))
        }
        // У bsdtar список 7z в том же виде, что у tar; у 7-Zip — другой, и
        // без bsdtar пределы проверит только распаковка (`sanitize_tree`).
        FileSort::SevenZip if crate::utils::which("bsdtar", None).is_some() => {
            let report = crate::utils::run(
                &crate::utils::RunSpec {
                    argv: vec!["bsdtar".into(), "-tvf".into(), path.to_string_lossy().into_owned()],
                    output: crate::utils::RunOutput::Capture,
                    timeout_secs: Some(120),
                    lang_c: true,
                    ..Default::default()
                },
                None,
            );
            if report.returncode != Some(0) {
                return None;
            }
            Some(tar_listing_budget(&crate::utils::decode_text(&report.stdout)))
        }
        _ => None,
    }
}

/// Сумма размеров и число записей по выводу `tar -tvf` (третье поле строки —
/// размер, у GNU tar и bsdtar одинаково).
pub fn tar_listing_budget(listing: &str) -> (u64, u64) {
    let mut total = 0u64;
    let mut count = 0u64;
    for line in listing.lines().filter(|l| !l.trim().is_empty()) {
        count += 1;
        let size = line.split_whitespace().nth(2).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        total = total.saturating_add(size);
    }
    (total, count)
}

/// Укладывается ли распакованное в пределы.
pub fn unpacked_budget_ok(bytes: u64, entries: u64) -> bool {
    bytes <= MAX_UNPACKED_BYTES && entries <= MAX_ENTRIES
}

/// Ведёт ли ссылка `target`, лежащая в `link_dir`, за пределы `root`
/// (лексически: абсолютная ссылка — всегда наружу).
pub fn link_escapes(root: &Path, link_dir: &Path, target: &Path) -> bool {
    if target.is_absolute() {
        return true;
    }
    let Ok(rel_dir) = link_dir.strip_prefix(root) else { return true };
    let mut depth: i64 = rel_dir.components().count() as i64;
    for c in target.components() {
        match c {
            std::path::Component::ParentDir => depth -= 1,
            std::path::Component::Normal(_) => depth += 1,
            std::path::Component::CurDir => {}
            _ => return true,
        }
        if depth < 0 {
            return true;
        }
    }
    false
}

/// Что убрано из дерева.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Sanitized {
    pub links_dropped: usize,
    pub bytes: u64,
    pub entries: u64,
}

/// Вычистить распакованное: ссылки наружу и особые файлы (каналы,
/// устройства) — вон; посчитать размер. `Err` — вышли за пределы.
pub fn sanitize_tree(root: &Path) -> Result<Sanitized, String> {
    fn walk(root: &Path, dir: &Path, st: &mut Sanitized) -> Result<(), String> {
        let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in rd.flatten() {
            let path = entry.path();
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            st.entries += 1;
            if st.entries > MAX_ENTRIES {
                return Err("entries".into());
            }
            let ft = md.file_type();
            if ft.is_symlink() {
                let target = std::fs::read_link(&path).unwrap_or_default();
                if link_escapes(root, dir, &target) {
                    let _ = std::fs::remove_file(&path);
                    st.links_dropped += 1;
                }
            } else if ft.is_dir() {
                walk(root, &path, st)?;
            } else if ft.is_file() {
                st.bytes = st.bytes.saturating_add(md.len());
                if st.bytes > MAX_UNPACKED_BYTES {
                    return Err("bytes".into());
                }
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
        Ok(())
    }
    let mut st = Sanitized::default();
    walk(root, root, &mut st)?;
    Ok(st)
}

/// Скопировать папку человека во временный каталог тем же фильтром: ссылки
/// внутри темы сохраняются ссылками, ссылки наружу и особые файлы — нет.
pub fn stage_dir(src: &Path, dest: &Path) -> Result<Sanitized, String> {
    fn walk(root: &Path, dir: &Path, dest: &Path, st: &mut Sanitized) -> Result<(), String> {
        std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .flatten()
            .collect();
        names.sort_by_key(|e| e.file_name());
        for entry in names {
            let path = entry.path();
            let to = dest.join(entry.file_name());
            let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
            st.entries += 1;
            if st.entries > MAX_ENTRIES {
                return Err("entries".into());
            }
            let ft = md.file_type();
            if ft.is_symlink() {
                let target = std::fs::read_link(&path).unwrap_or_default();
                if link_escapes(root, dir, &target) {
                    st.links_dropped += 1;
                } else {
                    std::os::unix::fs::symlink(&target, &to).map_err(|e| format!("{}: {e}", to.display()))?;
                }
            } else if ft.is_dir() {
                walk(root, &path, &to, st)?;
            } else if ft.is_file() {
                st.bytes = st.bytes.saturating_add(md.len());
                if st.bytes > MAX_UNPACKED_BYTES {
                    return Err("bytes".into());
                }
                std::fs::copy(&path, &to).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
        Ok(())
    }
    let mut st = Sanitized::default();
    walk(src, src, dest, &mut st)?;
    Ok(st)
}

/// Снимок дерева для распознавания (тексты описаний пакетов — как у
/// установщика).
fn snapshot(dir: &Path) -> FsSnapshot {
    crate::appearance_installer::install_tree_snapshot(&dir.to_string_lossy(), &[])
}

/// Каталог данных пользователя (`$XDG_DATA_HOME`, иначе `~/.local/share`).
fn data_home(home: &Path) -> String {
    match std::env::var("XDG_DATA_HOME") {
        Ok(v) if !v.is_empty() => v,
        _ => format!("{}/.local/share", home.display()),
    }
}

/// Запустить и вернуть stdout (пусто — не вышло).
fn read_cmd(argv: &[String], timeout: u64) -> String {
    let report = crate::utils::run(
        &crate::utils::RunSpec {
            argv: argv.to_vec(),
            output: crate::utils::RunOutput::Capture,
            timeout_secs: Some(timeout),
            lang_c: true,
            ..Default::default()
        },
        None,
    );
    if report.returncode != Some(0) {
        return String::new();
    }
    crate::utils::decode_text(&report.stdout)
}

/// Поставить файл шрифта в каталог шрифтов пользователя. Файл сперва
/// проверяет сам fontconfig (`fc-query`): что он не считает шрифтом, не
/// ставится. `refresh` — пересобрать кэш каталога сразу (пачку шрифтов
/// вызывающий освежает один раз в конце).
///
/// Одна дверь на продукт: ей же пользуется «Управление шрифтами» в
/// «Настройках».
pub fn install_font_file(source: &str, data_home: &str, refresh: bool) -> crate::qt_settings::FontInstallOutcome {
    use crate::qt_settings as qs;
    let name = Path::new(source)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.to_string());
    let families = qs::parse_fc_query_families(&read_cmd(&qs::fc_query_families_argv(source), 5));
    if families.is_empty() {
        return qs::FontInstallOutcome::NotAFont(name);
    }
    let Some(dest) = qs::font_install_dest(data_home, source) else {
        return qs::FontInstallOutcome::CopyFailed(name);
    };
    if Path::new(&dest).exists() {
        let same = std::fs::read(&dest)
            .ok()
            .zip(std::fs::read(source).ok())
            .is_some_and(|(a, b)| a == b);
        return if same {
            qs::FontInstallOutcome::AlreadyInstalled(name)
        } else {
            qs::FontInstallOutcome::NameTaken(name)
        };
    }
    let dir = qs::font_install_dir(data_home);
    if std::fs::create_dir_all(&dir).is_err() || std::fs::copy(source, &dest).is_err() {
        return qs::FontInstallOutcome::CopyFailed(name);
    }
    if refresh {
        let _ = read_cmd(&qs::fc_cache_argv(&dir), 30);
    }
    qs::FontInstallOutcome::Installed(families)
}

/// Поставить файл шрифта для всех пользователей — в
/// [`crate::qt_settings::SHARED_FONT_DIR`] через сеанс прав окна
/// ([`crate::privilege::op::PrivOp::FontInstall`]). До пароля окно само
/// спрашивает fontconfig и смотрит, не занято ли имя (каталог читается
/// всеми): заведомый отказ не стоит пароля. Решает всё равно помощник от
/// root — он повторяет проверки по своей копии файла.
///
/// `fc_query` — вывод [`crate::qt_settings::fc_query_families_argv`] для
/// файла, `read` — содержимое файла по пути (`None` — нет файла), `run` —
/// исполнитель операции с путями её файлов
/// ([`crate::privilege::run::run_live`] на живой машине; `None` — помощника
/// сеанса нет). Прежнего пути через `pkexec` нет намеренно (З5).
pub fn install_font_file_shared_with(
    source: &str,
    fc_query: &dyn Fn(&str) -> String,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp, &[String]) -> Option<crate::system_exec::RunOut>,
) -> crate::qt_settings::FontInstallOutcome {
    use crate::privilege::op::{FontFile, LocalFile, PrivOp};
    use crate::qt_settings as qs;
    let name = Path::new(source)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.to_string());
    let families = qs::parse_fc_query_families(&fc_query(source));
    if families.is_empty() || crate::privilege::validate::check_font_file_name(&name).is_err() {
        return qs::FontInstallOutcome::NotAFont(name);
    }
    let Some(dest) = qs::font_shared_dest(source) else {
        return qs::FontInstallOutcome::CopyFailed(name);
    };
    if let Some(existing) = read(&dest) {
        return if read(source).is_some_and(|mine| mine == existing) {
            qs::FontInstallOutcome::AlreadyInstalled(name)
        } else {
            qs::FontInstallOutcome::NameTaken(name)
        };
    }
    let op = PrivOp::FontInstall {
        fonts: vec![FontFile { source: LocalFile { slot: 0, label: source.to_string() }, name: name.clone() }],
    };
    let Some(out) = run(&op, &[source.to_string()]) else {
        return qs::FontInstallOutcome::NotForAll(crate::privilege::client::ClientError::HelperMissing.human_text());
    };
    if out.returncode == Some(0) {
        return qs::FontInstallOutcome::InstalledForAll(families);
    }
    // Отказ проверки и прочие ошибки сеанса — готовый текст в stderr.
    match out.stderr.lines().map(str::trim).rfind(|l| !l.is_empty()) {
        Some(why) => qs::FontInstallOutcome::NotForAll(why.to_string()),
        None => qs::FontInstallOutcome::CopyFailed(name),
    }
}

/// [`install_font_file_shared_with`] на живой машине.
pub fn install_font_file_shared(source: &str) -> crate::qt_settings::FontInstallOutcome {
    install_font_file_shared_with(
        source,
        &|p| read_cmd(&crate::qt_settings::fc_query_families_argv(p), 5),
        &|p| std::fs::read(p).ok(),
        &mut |op, files| crate::privilege::run::run_live(op, files, None, &mut |_| {}),
    )
}

/// Удалить шрифты, поставленные для всех пользователей, через сеанс прав
/// окна ([`crate::privilege::op::PrivOp::FontRemove`]). Окно отбирает
/// только файлы из [`crate::qt_settings::SHARED_FONT_DIR`]; что это
/// обычные файлы шрифтов там, а не ссылки и не шрифты пакетов, проверяет
/// помощник от root сам. `Err` — готовый текст для человека.
pub fn remove_shared_fonts_with(
    files: &[String],
    run: &mut dyn FnMut(&crate::privilege::op::PrivOp) -> Option<crate::system_exec::RunOut>,
) -> Result<(), String> {
    let files: Vec<String> = files.iter().filter(|f| crate::qt_settings::font_file_is_shared(f)).cloned().collect();
    if files.is_empty() {
        return Err(crate::qt_settings::font_remove_status("", 0, 0));
    }
    let op = crate::privilege::op::PrivOp::FontRemove { files };
    let Some(out) = run(&op) else {
        return Err(crate::privilege::client::ClientError::HelperMissing.human_text());
    };
    if out.returncode == Some(0) {
        return Ok(());
    }
    let why = out.stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_string);
    Err(why.unwrap_or_else(|| tr("Some font files couldn't be removed.")))
}

/// [`remove_shared_fonts_with`] на живой машине.
pub fn remove_shared_fonts(files: &[String]) -> Result<(), String> {
    remove_shared_fonts_with(files, &mut |op| crate::privilege::run::run_live(op, &[], None, &mut |_| {}))
}

/// Короткое имя для сообщений.
fn short(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Латиница-цифры-дефисы для id пакета обоев.
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(40).collect();
    if out.is_empty() {
        "file".to_string()
    } else {
        out
    }
}

/// Короткий отпечаток содержимого (FNV-1a по первым мегабайтам и размеру):
/// одна и та же картинка дважды — тот же пакет, «уже установлено».
fn fingerprint(path: &Path) -> String {
    let head = read_head(path, 4 << 20);
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut h: u64 = 0xcbf29ce484222325;
    for b in head.iter().chain(len.to_le_bytes().iter()) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:08x}", h as u32)
}

/// Манифест пакета обоев из одного файла (картинка, видео или Flash).
pub fn library_manifest(id: &str, name: &str, file: &str) -> serde_json::Value {
    let format = crate::qt_settings::detect_wallpaper_format(file);
    serde_json::json!({
        "id": id,
        "name": name,
        "type": "wallpaper-pack",
        "version": "1.0",
        "author": "",
        "description": "",
        "wallpaper_format": format,
        "wallpapers": [{"file": file, "name": name, "format": format}],
    })
}

/// Живой установщик «из файла» — исполнитель решений этого модуля.
pub struct LocalInstaller {
    home: PathBuf,
    section: String,
    installer: crate::appearance_installer::LiveInstaller,
    lines: Vec<String>,
    any_ok: bool,
    any_fail: bool,
    open_key: String,
    reload: bool,
    fonts_added: bool,
    /// Раздел, в котором видно последний поставленный пакет (`installed_section`).
    last_rxn_section: Option<&'static str>,
}

impl LocalInstaller {
    /// `section` — ключ раздела, из которого ставят; `log` — строки хода.
    pub fn new(section: &str, log: Box<dyn FnMut(&str) + Send>) -> Self {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".to_string()));
        LocalInstaller {
            home,
            section: section.to_string(),
            installer: crate::appearance_installer::LiveInstaller::new("", log),
            lines: Vec::new(),
            any_ok: false,
            any_fail: false,
            open_key: String::new(),
            reload: false,
            fonts_added: false,
            last_rxn_section: None,
        }
    }

    fn ok(&mut self, line: String) {
        self.any_ok = true;
        self.lines.push(line);
    }

    fn fail(&mut self, line: String) {
        self.any_fail = true;
        self.lines.push(line);
    }

    /// Поставить всё выбранное и подвести итог.
    pub fn run(mut self, paths: &[String]) -> LocalOutcome {
        for p in paths {
            let path = PathBuf::from(file_url_to_path(p));
            if path.as_os_str().is_empty() {
                continue;
            }
            self.one(&path);
        }
        if self.fonts_added {
            let dir = crate::qt_settings::font_install_dir(&data_home(&self.home));
            let _ = read_cmd(&crate::qt_settings::fc_cache_argv(&dir), 60);
        }
        let open_text = if self.open_key.is_empty() {
            String::new()
        } else {
            open_section_btn(&section_name(&self.home, &self.open_key))
        };
        LocalOutcome {
            ok: self.any_ok && !self.any_fail,
            installed: self.any_ok,
            message: self.lines.join("\n"),
            open_key: self.open_key,
            open_text,
            reload: self.reload,
        }
    }

    /// Один выбранный путь.
    fn one(&mut self, path: &Path) {
        let label = short(path);
        // Выбранное человеком читается сквозь ссылку: её он выбрал сам. Ссылки
        // ВНУТРИ папки или архива — другое дело (`stage_dir`, `sanitize_tree`).
        let md = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => return self.fail(trf("File not found: %1", &[&path.display()])),
        };
        if md.is_dir() {
            return self.staged(path, &label, None);
        }
        if !md.is_file() {
            return self.fail(unknown_file_message(&label));
        }
        let head = read_head(path, 512);
        let sort = sniff(&label, &head);
        let ext = ext_of(&label);

        // Готовый пакет Rexon.
        if sort == FileSort::Zip && (ext == "rxn" || zip_has_root_manifest(path)) {
            self.rxn(path, &label);
            return;
        }
        // Курсоры и значки Windows, у которых имя не совпало с подписью:
        // импортёр выбирает путь по расширению — даём ему верное.
        let win_ext = match sort {
            FileSort::Cur => Some("cur"),
            FileSort::Ani => Some("ani"),
            FileSort::Ico => Some("ico"),
            _ => None,
        };
        if let Some(want) = win_ext {
            if ext != want {
                return self.with_renamed(path, &label, want);
            }
        }
        // Форматы Windows — импортёр (курсоры с анимацией, темы, значки).
        // Архив, внутри которого готовая тема Xcursor, — не набор Windows.
        let mut probe = crate::imp_live::LiveImpIo::new(None);
        let windows = crate::importers::windows_theme_can_import(&mut probe, &path.to_string_lossy())
            && !(sort == FileSort::Zip && zip_has_xcursor_theme(path));
        // Ролик DreamScene (`.wmv`/`.mpg`/`.mpeg`) — тоже видео из общего
        // списка, но ставит его по-прежнему импортёр: он снимает кадр-превью,
        // которого у простого добавления в библиотеку нет.
        let dreamscene = crate::importers::import_kind(&label, "") == "dreamscene";
        if windows && (!is_media(sort) || dreamscene) {
            return self.windows_file(path, &label);
        }
        match sort {
            FileSort::Zip | FileSort::Tar | FileSort::SevenZip => self.staged(path, &label, Some(sort)),
            FileSort::Rar => self.fail(unsupported_archive_message(&label, "RAR")),
            FileSort::Font => {
                self.font_files(&[path.to_path_buf()], &label);
                let moved = !section_accepts(&self.section, &Found::Fonts);
                self.note_moved(&label, "fonts", moved);
            }
            s if is_media(s) => {
                if self.section == "lockwallpaper" && s == FileSort::Image {
                    self.single(path, &label, "lockwallpaper")
                } else {
                    self.library(path, &label);
                    let moved = !section_accepts(&self.section, &Found::Media);
                    self.note_moved(&label, "wallpapers", moved);
                }
            }
            FileSort::ColorScheme => self.single(path, &label, "colors"),
            FileSort::Kvantum => self.single(path, &label, "kvantum"),
            FileSort::Cur | FileSort::Ani | FileSort::Ico => self.windows_file(path, &label),
            _ => self.fail(unknown_file_message(&label)),
        }
    }

    /// Заметка «поставлено в другой раздел», если так вышло.
    fn note_moved(&mut self, label: &str, key: &str, moved: bool) {
        if moved {
            let name = section_name(&self.home, key);
            self.lines.push(moved_note(label, &name));
            if self.open_key.is_empty() {
                self.open_key = key.to_string();
            }
        }
    }

    /// Где искать поставленную тему, чтобы применить.
    fn note_where(&mut self, key: &str) {
        // Раздел, который сам показывает темы с диска (обзор диска или
        // раздел без магазина), просто перечитать.
        let shows_disk = crate::categories::LOCAL_VIEW_KEYS.contains(&self.section.as_str())
            || self.category(&self.section).is_some_and(|c| c.local_only());
        if shows_disk {
            self.reload = true;
            return;
        }
        // Раздел с сеткой установленного (любой со сканом и магазином:
        // разделы KDE, заставка, Conky, разделы KWin) показывает
        // поставленное сам: свой — перечитать (страница откроет сетку, даже
        // если был открыт магазин), чужой уже назван кнопкой «Открыть «…»»
        // (`note_moved`). «Загруженные» здесь не нужны.
        // Тема Kvantum, поставленная из «Стиля приложений», видна в его же
        // сетке (установка идёт по категории Kvantum, `install_key`).
        if key != self.section
            && install_key(&self.section) == key
            && crate::appearance_page::has_installed_grid(&self.section)
        {
            self.reload = true;
            return;
        }
        if crate::appearance_page::has_installed_grid(key) {
            if key == self.section {
                self.reload = true;
            }
            return;
        }
        // Остальные разделы показывают магазин: поставленное видно в
        // «Загруженных» — туда и ведёт кнопка.
        let shown = crate::installed_scan::types_for("", "")
            .iter()
            .any(|t| t.apply_kind == key)
            || is_de_section(key);
        if shown && self.open_key.is_empty() {
            let view = section_name(&self.home, "downloaded");
            self.lines.push(apply_where_note(&view));
            self.open_key = "downloaded".to_string();
        }
    }

    /// Готовый `.rxn` и всё, что умеют импортёры Windows.
    fn rxn(&mut self, path: &Path, label: &str) -> bool {
        match crate::ext_exec::install_from_file(&path.to_string_lossy()) {
            Ok(done) => {
                let text = crate::qt_extensions::installed_with_notes(&done.name, &done.notes);
                self.last_rxn_section = installed_section(&done.name);
                // Набор обоев применяется из «Библиотеки обоев»: голое
                // «Установлено» оставляло человека гадать, где он теперь.
                if self.last_rxn_section == Some("wallpapers") {
                    self.ok(library_added_message(&text));
                } else {
                    self.ok(trf("Installed: %1", &[&text]));
                }
                self.reload = true;
                true
            }
            Err(e) => {
                self.fail(format!("{label}: {e}"));
                false
            }
        }
    }

    /// Файл Windows — импортёром. Курсоры и значки из Windows встают как
    /// обычные темы: их видно в своих разделах и в «Загруженных».
    fn windows_file(&mut self, path: &Path, label: &str) {
        let key = windows_kind(&path.to_string_lossy());
        if self.rxn(path, label) {
            // Куда встало на деле, знает поставленный пакет: `.themepack` по
            // расширению обещал «Глобальные темы», а выходит набором обоев
            // или темой значков (приёмка 10.10.2026: после установки звали
            // в раздел, где пакета нет).
            let key = self.last_rxn_section.take().or(key);
            if let Some(key) = key {
                let moved = !section_accepts(&self.section, &Found::Section(key.to_string()));
                self.note_moved(label, key, moved);
                self.note_where(key);
            }
        }
    }

    /// Курсор или значок с неверным расширением — копия с верным.
    fn with_renamed(&mut self, path: &Path, label: &str, ext: &str) {
        let Ok(tmp) = mkdtemp(&std::env::temp_dir(), "rexon-local-") else {
            return self.fail(unknown_file_message(label));
        };
        let stem = label.rsplit_once('.').map(|(s, _)| s).unwrap_or(label);
        let renamed = tmp.join(format!("{stem}.{ext}"));
        if std::fs::copy(path, &renamed).is_ok() {
            self.windows_file(&renamed, label);
        } else {
            self.fail(unknown_file_message(label));
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Один файл через ветку «товар одним файлом» установщика магазина.
    fn single(&mut self, path: &Path, label: &str, key: &str) {
        let Some(cat) = self.category(key) else {
            return self.fail(unknown_file_message(label));
        };
        let moved = !matches!(
            (self.section.as_str(), key),
            (s, k) if s == k || s == "downloaded" || (s == "widgetstyle" && k == "kvantum")
        );
        let item = local_item(&cat, label);
        let (ok, msg, _) = self.installer.install(&item, &path.to_string_lossy());
        if ok {
            self.ok(msg);
            self.note_moved(label, key, moved);
            self.note_where(key);
        } else {
            self.fail(format!("{label}: {msg}"));
        }
    }

    fn category(&self, key: &str) -> Option<ThemeCategory> {
        crate::categories::categories(&self.home).into_iter().find(|c| c.key == key)
    }

    /// Архив или папка: во временный каталог, вычистить, распознать, поставить.
    fn staged(&mut self, src: &Path, label: &str, archive: Option<FileSort>) {
        if let Some(sort) = archive {
            if let Some((bytes, entries)) = archive_budget(src, sort) {
                if !unpacked_budget_ok(bytes, entries) {
                    return self.fail(too_large_message(label));
                }
            }
        }
        // Системным видам помощник принимает только каталог под /tmp.
        let work = match mkdtemp(Path::new("/tmp"), "rexon-local-") {
            Ok(p) => p,
            Err(e) => return self.fail(trf("Installation error: %1", &[&e])),
        };
        let extract = work.join("extracted");
        let staged = match archive {
            Some(_) => crate::appearance_installer::live_safe_extract(
                &src.to_string_lossy(),
                &extract.to_string_lossy(),
            )
            .map_err(|e| trf("Couldn't unpack the archive: %1", &[&e]))
            .and_then(|()| sanitize_tree(&extract).map_err(|_| too_large_message(label))),
            None => stage_dir(src, &extract).map_err(|e| {
                if e == "bytes" || e == "entries" {
                    too_large_message(label)
                } else {
                    trf("Installation error: %1", &[&e])
                }
            }),
        };
        match staged {
            Ok(st) => {
                self.install_tree(&extract, src, label);
                if st.links_dropped > 0 {
                    self.lines.push(links_dropped_note(st.links_dropped));
                }
            }
            Err(e) => self.fail(format!("{label}: {e}")),
        }
        let _ = std::fs::remove_dir_all(&work);
    }

    fn install_tree(&mut self, extract: &Path, src: &Path, label: &str) {
        let fs = snapshot(extract);
        let found = detect_tree(&fs, &extract.to_string_lossy());
        let Some(placement) = place(&self.section, found.as_ref()) else {
            return self.fail(unknown_file_message(label));
        };
        match placement {
            Placement::OtherRootSection { key } => {
                let name = section_name(&self.home, &key);
                self.fail(other_root_section_message(label, &name));
                self.open_key = key;
            }
            Placement::Fonts { moved } => {
                let files: Vec<PathBuf> = files_under(&fs, &extract.to_string_lossy())
                    .into_iter()
                    .filter(|f| crate::qt_settings::FONT_INSTALL_EXTS.contains(&ext_of(f).as_str()))
                    .map(PathBuf::from)
                    .collect();
                self.font_files(&files, label);
                self.note_moved(label, "fonts", moved);

            }
            Placement::Library { moved } => {
                let media: Vec<String> = files_under(&fs, &extract.to_string_lossy())
                    .into_iter()
                    .filter(|f| is_media(sniff(f, &read_head(Path::new(f), 512))))
                    .collect();
                for f in media {
                    let p = PathBuf::from(&f);
                    self.library(&p, &short(&p));
                }
                self.note_moved(label, "wallpapers", moved);
            }
            Placement::WindowsCursors { moved } => {
                self.windows_cursor_dir(extract, src, label);
                self.note_moved(label, "cursor", moved);
            }
            Placement::Section { key, moved } => {
                let Some(cat) = self.category(&key) else {
                    return self.fail(unknown_file_message(label));
                };
                let item = local_item(&cat, label);
                // Системные виды (SDDM, Plymouth) установщик сам отдаёт
                // помощнику: распаковано под `/tmp`, как тот требует.
                let (ok, msg, _) = self.installer.install_extracted(&item, &extract.to_string_lossy());
                if ok {
                    self.ok(msg);
                    self.note_moved(label, &key, moved);
                    self.note_where(&key);
                } else {
                    self.fail(format!("{label}: {msg}"));
                }
            }
        }
    }

    /// Папка с `.cur`/`.ani` — импортёр (все кадры анимации) → `.rxn`.
    fn windows_cursor_dir(&mut self, dir: &Path, src: &Path, label: &str) {
        let Ok(out) = mkdtemp(&std::env::temp_dir(), "rexon-wincur-") else {
            return self.fail(unknown_file_message(label));
        };
        // Имя набора — имя того, что выбрал человек, а не временного каталога.
        let stem = short(src);
        let stem = match ext_of(&stem) {
            e if e.is_empty() => stem.clone(),
            e => stem.trim_end_matches(&format!(".{e}")).to_string(),
        };
        let source = crate::importers::CursorSource { name: stem, ..Default::default() };
        let root = find_content_root(&snapshot(dir), &dir.to_string_lossy());
        match crate::imp_live::import_windows_cursor_archive(Path::new(&root), &out, &source) {
            Ok(rxn) => {
                if self.rxn(&rxn, label) {
                    self.note_where("cursor");
                }
            }
            Err(e) => self.fail(format!("{label}: {e}")),
        }
        let _ = std::fs::remove_dir_all(&out);
    }

    fn font_files(&mut self, files: &[PathBuf], label: &str) {
        use crate::qt_settings::{font_install_status, FontInstallOutcome};
        if files.is_empty() {
            return self.fail(unknown_file_message(label));
        }
        let dh = data_home(&self.home);
        for f in files {
            let outcome = install_font_file(&f.to_string_lossy(), &dh, false);
            let text = font_install_status(&outcome);
            match outcome {
                FontInstallOutcome::Installed(_) => {
                    self.fonts_added = true;
                    self.ok(text);
                }
                FontInstallOutcome::AlreadyInstalled(_) => self.ok(text),
                _ => self.fail(text),
            }
        }
    }

    /// Картинка, видео или Flash — пакетом в библиотеку обоев.
    fn library(&mut self, path: &Path, label: &str) {
        let stem = label.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_else(|| label.to_string());
        let id = format!("wallpaper-{}-{}", slug(&stem), fingerprint(path));
        let built = (|| -> Result<PathBuf, String> {
            let work = mkdtemp(&std::env::temp_dir(), "rexon-wp-")?;
            let pack = work.join("pack");
            let data = pack.join("data");
            std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
            std::fs::copy(path, data.join(label)).map_err(|e| e.to_string())?;
            if ext_of(label) == "png" {
                let _ = std::fs::copy(path, pack.join("preview.png"));
            }
            let manifest = library_manifest(&id, &stem, label);
            std::fs::write(pack.join("manifest.json"), serde_json::to_string_pretty(&manifest).unwrap_or_default())
                .map_err(|e| e.to_string())?;
            let rxn = work.join(format!("{id}.rxn"));
            crate::ext_utils::pack_rxn(&pack, &rxn).map_err(|e| format!("{e:?}"))?;
            Ok(rxn)
        })();
        match built {
            Ok(rxn) => {
                match crate::ext_exec::install_from_file(&rxn.to_string_lossy()) {
                    Ok(_) => self.ok(library_added_message(&stem)),
                    Err(e) => self.fail(format!("{label}: {e}")),
                }
                if let Some(work) = rxn.parent() {
                    let _ = std::fs::remove_dir_all(work);
                }
            }
            Err(e) => self.fail(trf("Installation error: %1", &[&e])),
        }
    }
}

/// Карточка товара для установщика магазина: без номера (в журнал покупок
/// Магазина местная тема не пишется), с именем файла — по нему ветка
/// «одним файлом» узнаёт расширение.
fn local_item(cat: &ThemeCategory, label: &str) -> ThemeItem {
    ThemeItem {
        name: label.to_string(),
        category_id: cat.id,
        download_name: label.to_string(),
        ..Default::default()
    }
}

/// Какого вида тема выйдет из файла Windows (по расширению — им же
/// выбирает путь импортёр). `None` — гаджеты и прочее без своего раздела.
fn windows_kind(path: &str) -> Option<&'static str> {
    match ext_of(path).as_str() {
        "cur" | "ani" | "inf" | "crs" | "curxptheme" | "zip" | "rar" | "7z" => Some("cursor"),
        "ico" | "icl" | "iconpackage" | "ip" | "7tsp" | "dll" | "exe" => Some("icons"),
        "themepack" | "deskthemepack" | "theme" | "msstyles" => Some("global"),
        "dream" | "wmv" | "mpeg" | "mpg" => Some("wallpapers"),
        _ => None,
    }
}

/// Раздел «Оформления», в котором видно только что поставленное расширение
/// `name`: вид берётся из его манифеста в каталоге расширений, раздел — из
/// таблицы `categories::category_kind`. `None` — расширения не нашли либо у
/// его вида нет раздела.
fn installed_section(name: &str) -> Option<&'static str> {
    // Вид — из общего журнала установки: запись `.rxn` сверена с каталогом
    // расширений и манифестом пакета (шаг 3 слияния установщиков).
    use crate::install_journal as journal;
    let dir = crate::qt_extensions::extensions_dir();
    let rexon = dir.parent()?;
    let kind = journal::view_at(rexon)
        .items
        .into_values()
        .find(|e| e.source == journal::Source::Rxn && e.name == name)
        .map(|e| e.kind)?;
    ["wallpapers", "icons", "cursor", "global", "sounds", "fonts", "gtk", "plasma", "colors"]
        .into_iter()
        .find(|key| crate::categories::category_kind(key) == Some(kind.as_str()))
}

/// ZIP с `manifest.json` в корне — пакет Rexon, даже с чужим расширением.
fn zip_has_root_manifest(path: &Path) -> bool {
    zip_names(path).is_some_and(|names| names.iter().any(|n| n == "manifest.json"))
}

/// В ZIP лежит готовая тема Xcursor (каталог `cursors/`) — это не набор
/// Windows, даже если рядом приложены исходные `.cur`.
fn zip_has_xcursor_theme(path: &Path) -> bool {
    zip_names(path).is_some_and(|names| {
        names.iter().any(|n| n.starts_with("cursors/") || n.contains("/cursors/"))
    })
}

fn zip_names(path: &Path) -> Option<Vec<String>> {
    let f = std::fs::File::open(path).ok()?;
    let zip = zip::ZipArchive::new(f).ok()?;
    Some(zip.file_names().map(str::to_string).collect())
}

/// Поставить выбранное из раздела `section` — вход для вкладки.
pub fn live_install(paths: &[String], section: &str, log: Box<dyn FnMut(&str) + Send>) -> LocalOutcome {
    LocalInstaller::new(section, log).run(paths)
}

// ============================================================================
// Проверки
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snap(dirs: serde_json::Value, files: serde_json::Value) -> FsSnapshot {
        FsSnapshot::from_json(&json!({"dirs": dirs, "files": files}))
    }

    #[test]
    fn sniff_reads_content_not_name() {
        assert_eq!(sniff("theme.jpg", b"PK\x03\x04rest"), FileSort::Zip);
        assert_eq!(sniff("x", b"\x1f\x8b\x08"), FileSort::Tar);
        assert_eq!(sniff("Arial.bin", b"\x00\x01\x00\x00\x00\x10"), FileSort::Font);
        assert_eq!(sniff("a.otf", b"OTTO\x00"), FileSort::Font);
        assert_eq!(sniff("busy", b"RIFF\x10\x00\x00\x00ACONanih"), FileSort::Ani);
        assert_eq!(sniff("arrow.bin", b"\x00\x00\x02\x00\x01\x00"), FileSort::Cur);
        assert_eq!(sniff("clip", b"\x00\x00\x00\x18ftypmp42"), FileSort::Video);
        assert_eq!(sniff("pic", b"\x00\x00\x00\x18ftypavif"), FileSort::Image);
        assert_eq!(sniff("Breeze.colors", b"[General]\nName=Breeze\n"), FileSort::ColorScheme);
        assert_eq!(sniff("noext", b"[ColorEffects:Disabled]\nColor=1"), FileSort::ColorScheme);
        assert_eq!(sniff("t.kvconfig", b"[%General]\n"), FileSort::Kvantum);
        assert_eq!(sniff("readme.txt", b"hello"), FileSort::Other);
        assert_eq!(sniff("f.pcf.gz", b"\x1f\x8b\x08"), FileSort::Font);
        assert_eq!(sniff("a.swf", b"CWS\x0a"), FileSort::Flash);
    }

    #[test]
    fn tree_kinds_are_recognized() {
        let cursor = snap(
            json!({"/x": ["Pack"], "/x/Pack": ["Bibata"], "/x/Pack/Bibata": ["cursors", "index.theme"], "/x/Pack/Bibata/cursors": []}),
            json!({"/x/Pack/Bibata/index.theme": "[Icon Theme]\nName=Bibata\n"}),
        );
        assert_eq!(detect_tree(&cursor, "/x"), Some(Found::Section("cursor".into())));

        let icons = snap(
            json!({"/x": ["Papirus"], "/x/Papirus": ["apps", "index.theme"], "/x/Papirus/apps": []}),
            json!({"/x/Papirus/index.theme": "[Icon Theme]\nName=Papirus\n"}),
        );
        assert_eq!(detect_tree(&icons, "/x"), Some(Found::Section("icons".into())));

        let lnf = snap(
            json!({"/x": ["org.foo"], "/x/org.foo": ["contents", "metadata.json"], "/x/org.foo/contents": []}),
            json!({"/x/org.foo/metadata.json": "{\"KPackageStructure\": \"Plasma/LookAndFeel\"}"}),
        );
        assert_eq!(detect_tree(&lnf, "/x"), Some(Found::Section("global".into())));

        let fonts = snap(
            json!({"/x": ["a.ttf", "b.otf", "README.txt"]}),
            json!({"/x/a.ttf": "", "/x/b.otf": "", "/x/README.txt": ""}),
        );
        assert_eq!(detect_tree(&fonts, "/x"), Some(Found::Fonts));

        let win = snap(
            json!({"/x": ["set"], "/x/set": ["arrow.cur", "busy.ani", "install.inf"]}),
            json!({"/x/set/arrow.cur": "", "/x/set/busy.ani": "", "/x/set/install.inf": ""}),
        );
        assert_eq!(detect_tree(&win, "/x"), Some(Found::WindowsCursors));

        let plymouth = snap(json!({"/x": ["t"], "/x/t": ["t.plymouth", "t.script"]}), json!({"/x/t/t.plymouth": "", "/x/t/t.script": ""}));
        assert_eq!(detect_tree(&plymouth, "/x"), Some(Found::Section("plymouth".into())));

        let colors = snap(json!({"/x": ["A.colors", "B.colors"]}), json!({"/x/A.colors": "", "/x/B.colors": ""}));
        assert_eq!(detect_tree(&colors, "/x"), Some(Found::Section("colors".into())));

        let media = snap(json!({"/x": ["a.jpg", "b.mp4"]}), json!({"/x/a.jpg": "", "/x/b.mp4": ""}));
        assert_eq!(detect_tree(&media, "/x"), Some(Found::Media));

        let junk = snap(json!({"/x": ["notes.odt"]}), json!({"/x/notes.odt": ""}));
        assert_eq!(detect_tree(&junk, "/x"), None);

        // Пакет тем Conky: виджеты в скрытой `.conky/`, иначе дерево пустое.
        let cmtp = snap(
            json!({"/x": [".conky"], "/x/.conky": ["Gotham"], "/x/.conky/Gotham": ["Gotham"]}),
            json!({"/x/.conky/Gotham/Gotham": ""}),
        );
        assert_eq!(detect_tree(&cmtp, "/x"), Some(Found::Section("conky".into())));
        let set = snap(json!({"/x": ["Set"], "/x/Set": ["Set.cmtheme", "a"]}), json!({"/x/Set/Set.cmtheme": "", "/x/Set/a": ""}));
        assert_eq!(detect_tree(&set, "/x"), Some(Found::Section("conky".into())));
    }

    #[test]
    fn placement_follows_section_rules() {
        let sec = |k: &str| Found::Section(k.to_string());
        // Своё — в свой раздел.
        assert_eq!(place("icons", Some(&sec("icons"))), Some(Placement::Section { key: "icons".into(), moved: false }));
        // Курсоры, поставленные из «Значков», уходят в «Курсоры».
        assert_eq!(place("icons", Some(&sec("cursor"))), Some(Placement::Section { key: "cursor".into(), moved: true }));
        // Заставка ставит пакет Look-and-Feel своей категорией.
        assert_eq!(place("splash", Some(&sec("global"))), Some(Placement::Section { key: "splash".into(), moved: false }));
        // Kvantum из «Стиля приложений».
        assert_eq!(place("widgetstyle", Some(&sec("kvantum"))), Some(Placement::Section { key: "kvantum".into(), moved: false }));
        // Системная тема из чужого раздела не ставится.
        assert_eq!(place("global", Some(&sec("sddm"))), Some(Placement::OtherRootSection { key: "sddm".into() }));
        assert_eq!(place("downloaded", Some(&sec("plymouth"))), Some(Placement::OtherRootSection { key: "plymouth".into() }));
        assert_eq!(place("sddm", Some(&sec("sddm"))), Some(Placement::Section { key: "sddm".into(), moved: false }));
        // Тема оболочки GNOME похожа на GTK — раздел окружения её не отдаёт.
        assert_eq!(place("gnomeshell", Some(&sec("gtk"))), Some(Placement::Section { key: "gnomeshell".into(), moved: false }));
        // Не опознано: раздел ставит по своему правилу, «Загруженные» — нет.
        assert_eq!(place("gtk", None), Some(Placement::Section { key: "gtk".into(), moved: false }));
        assert_eq!(place("downloaded", None), None);
        // Обои и шрифты.
        assert_eq!(place("wallpapers", Some(&Found::Media)), Some(Placement::Library { moved: false }));
        assert_eq!(place("lockwallpaper", Some(&Found::Media)), Some(Placement::Section { key: "lockwallpaper".into(), moved: false }));
        assert_eq!(place("colors", Some(&Found::Fonts)), Some(Placement::Fonts { moved: true }));
        assert_eq!(place("cursor", Some(&Found::WindowsCursors)), Some(Placement::WindowsCursors { moved: false }));
    }

    #[test]
    fn links_out_of_the_theme_are_caught() {
        let root = Path::new("/s");
        assert!(link_escapes(root, Path::new("/s/a"), Path::new("/etc/passwd")));
        assert!(link_escapes(root, Path::new("/s/a"), Path::new("../../x")));
        assert!(!link_escapes(root, Path::new("/s/a"), Path::new("../b/c")));
        assert!(!link_escapes(root, Path::new("/s/a/b"), Path::new("left_ptr")));
    }

    #[test]
    fn tar_budget_sums_sizes() {
        let listing = "-rw-r--r-- u/g 100 2024-01-01 00:00 a\n\
                       drwxr-xr-x u/g 0 2024-01-01 00:00 d/\n\
                       lrwxrwxrwx u/g 0 2024-01-01 00:00 l -> a\n";
        assert_eq!(tar_listing_budget(listing), (100, 3));
        assert!(unpacked_budget_ok(100, 3));
        assert!(!unpacked_budget_ok(MAX_UNPACKED_BYTES + 1, 1));
        assert!(!unpacked_budget_ok(1, MAX_ENTRIES + 1));
    }

    #[test]
    fn file_urls_become_paths() {
        assert_eq!(file_url_to_path("file:///home/u/%D0%A8%D1%80%D0%B8%D1%84%D1%82.ttf"), "/home/u/Шрифт.ttf");
        assert_eq!(file_url_to_path("file:///a%20b/c"), "/a b/c");
        assert_eq!(file_url_to_path("/plain/path"), "/plain/path");
        assert_eq!(busy_label(&["file:///a/b%20c.zip".into()]), "b c.zip");
        let many = busy_label(&["/a/x.ttf".into(), "/a/y.ttf".into(), "/a/z.ttf".into()]);
        assert!(many.contains("x.ttf") && many.contains('2'), "{many}");
    }

    #[test]
    fn dialog_filters_name_section_files_and_archives() {
        let f = dialog_filters("fonts");
        assert_eq!(f.len(), 2);
        assert!(f[0].contains("*.ttf") && f[0].contains("*.zip") && f[0].contains("*.tar.xz"));
        assert!(dialog_filters("cursor")[0].contains("*.ani"));
        assert!(dialog_filters("wallpapers")[0].contains("*.mp4"));
        assert!(dialog_filters("conky")[0].contains("*.7z"));
        assert!(button_for("icons", false));
        assert!(button_for("downloaded", true));
        assert!(!button_for("active", true));
        assert!(!button_for(crate::categories::PLASMA_LOGIN_KEY, false));
    }

    // ── Сквозная проверка в подменённом доме ─────────────────────────────
    //
    // Установщик читает HOME/XDG процесса, а тесты бегут параллельно, поэтому
    // сама проверка — в ДОЧЕРНЕМ процессе того же бинаря с чистым окружением
    // (env_clear: ни сессии D-Bus, ни живого дома — живая система не задета).

    const E2E_BASE_ENV: &str = "REXON_LOCAL_INSTALL_E2E_BASE";

    #[test]
    fn installs_into_substituted_home() {
        let base = mkdtemp(&std::env::temp_dir(), "rexon_test_local_install_").expect("каталог");
        let home = base.join("home");
        std::fs::create_dir_all(&home).expect("дом");
        let out = std::process::Command::new(std::env::current_exe().expect("путь бинаря"))
            .args(["--exact", "local_install::tests::e2e_child", "--ignored", "--test-threads=1", "--nocapture"])
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("LANG", "C.UTF-8")
            .env(E2E_BASE_ENV, &base)
            .output()
            .expect("дочерний прогон");
        let _ = std::fs::remove_dir_all(&base);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && stdout.contains("1 passed"),
            "дочерний прогон не прошёл:\n{stdout}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn quiet() -> Box<dyn FnMut(&str) + Send> {
        Box::new(|_: &str| {})
    }

    fn system_font() -> Option<PathBuf> {
        let out = read_cmd(&["fc-list".into(), "--format".into(), "%{file}\n".into()], 10);
        out.lines()
            .map(PathBuf::from)
            .find(|p| p.extension().is_some_and(|e| e == "ttf") && p.is_file())
    }

    /// Курсор `.ani` из трёх кадров 2×2 (32 бит), как его пишет Windows.
    fn ani_bytes() -> Vec<u8> {
        fn cur(color: u8) -> Vec<u8> {
            let mut img = Vec::new();
            img.extend(40u32.to_le_bytes());
            img.extend(2i32.to_le_bytes());
            img.extend(4i32.to_le_bytes());
            img.extend(1u16.to_le_bytes());
            img.extend(32u16.to_le_bytes());
            img.extend([0u8; 24]);
            for _ in 0..4 {
                img.extend([color, 0, 0, 255]);
            }
            img.extend([0u8; 8]); // маска AND: 2 строки по 4 байта
            let mut f = vec![0, 0, 2, 0, 1, 0];
            f.extend([2, 2, 0, 0, 0, 0, 0, 0]);
            f.extend((img.len() as u32).to_le_bytes());
            f.extend(22u32.to_le_bytes());
            f.extend(img);
            f
        }
        let chunk = |id: &[u8], data: &[u8]| {
            let mut c = id.to_vec();
            c.extend((data.len() as u32).to_le_bytes());
            c.extend(data);
            if data.len() % 2 == 1 {
                c.push(0);
            }
            c
        };
        let mut anih = Vec::new();
        for v in [36u32, 3, 3, 0, 0, 0, 0, 6, 1] {
            anih.extend(v.to_le_bytes());
        }
        let mut fram = b"fram".to_vec();
        for c in [10u8, 120, 240] {
            fram.extend(chunk(b"icon", &cur(c)));
        }
        let mut body = b"ACON".to_vec();
        body.extend(chunk(b"anih", &anih));
        body.extend(chunk(b"LIST", &fram));
        let mut out = b"RIFF".to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }

    #[test]
    #[ignore = "запускается дочерним процессом installs_into_substituted_home"]
    fn e2e_child() {
        let base = PathBuf::from(std::env::var_os(E2E_BASE_ENV).expect("только дочерний прогон"));
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        assert!(home.starts_with(&base), "дом не подменён: {home:?}");
        let share = home.join(".local/share");
        let input = base.join("input");
        std::fs::create_dir_all(&input).unwrap();

        // 1. Шрифт .ttf из раздела «Шрифты».
        if let Some(font) = system_font() {
            let copy = input.join("Мой шрифт.ttf");
            std::fs::copy(&font, &copy).unwrap();
            let url = format!("file://{}", copy.display()).replace(' ', "%20");
            let r = live_install(&[url], "fonts", quiet());
            eprintln!("шрифт: {r:?}");
            assert!(r.ok, "шрифт: {r:?}");
            assert!(share.join("fonts/Мой шрифт.ttf").is_file(), "шрифт не лёг в fonts");
        } else {
            eprintln!("НЕ ПРОВЕРЕНО: в системе нет ни одного .ttf");
        }

        // 2. Курсор Windows .ani с неверным расширением, из «Значков» —
        //    уходит в «Курсоры», анимация сохраняется.
        let ani = input.join("Rammstein busy.bin");
        std::fs::write(&ani, ani_bytes()).unwrap();
        let r = live_install(&[ani.to_string_lossy().into_owned()], "icons", quiet());
        eprintln!("ani: {r:?}");
        assert!(r.ok, "ani: {r:?}");
        let icons = share.join("icons");
        let theme = std::fs::read_dir(&icons)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.join("cursors").is_dir())
            .expect("тема курсоров в ~/.local/share/icons");
        let left_ptr = theme.join("cursors/left_ptr");
        let wait = theme.join("cursors/wait");
        let animated = [left_ptr, wait]
            .iter()
            .filter(|p| p.exists())
            .map(|p| crate::wincursor::read_xcursor(&std::fs::read(p).unwrap()).map(|v| v.len()).unwrap_or(0))
            .max()
            .unwrap_or(0);
        eprintln!("тема курсоров {theme:?}, кадров {animated}");
        assert!(animated >= 3, "анимация .ani потеряна: кадров {animated}");

        // 3. Архив значков (.tar.gz) с обёрткой и ссылкой наружу.
        let pack = input.join("icons-src/Wrapper/MyIcons");
        std::fs::create_dir_all(pack.join("apps/48")).unwrap();
        std::fs::write(pack.join("index.theme"), "[Icon Theme]\nName=MyIcons\nDirectories=apps/48\n").unwrap();
        std::fs::write(pack.join("apps/48/foo.svg"), "<svg/>").unwrap();
        std::os::unix::fs::symlink("foo.svg", pack.join("apps/48/bar.svg")).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", pack.join("apps/48/evil.svg")).unwrap();
        let tgz = input.join("MyIcons.tar.gz");
        let st = std::process::Command::new("tar")
            .args(["-czf", &tgz.to_string_lossy(), "-C", &input.join("icons-src").to_string_lossy(), "Wrapper"])
            .status()
            .unwrap();
        assert!(st.success());
        let r = live_install(&[tgz.to_string_lossy().into_owned()], "icons", quiet());
        eprintln!("значки: {r:?}");
        assert!(r.ok, "значки: {r:?}");
        let dest = icons.join("MyIcons");
        assert!(dest.join("index.theme").is_file(), "{r:?}");
        assert!(std::fs::symlink_metadata(dest.join("apps/48/bar.svg")).is_ok(), "ссылка внутри темы потеряна");
        assert!(std::fs::symlink_metadata(dest.join("apps/48/evil.svg")).is_err(), "ссылка наружу доехала");
        assert!(r.message.contains('1'), "нет заметки о выброшенной ссылке: {r:?}");
        // Раздел KDE сам показывает установленное: перечитать его, без
        // подсказки про «Загруженные».
        assert!(r.open_key.is_empty(), "{r:?}");
        assert!(r.reload, "{r:?}");
        assert!(!r.message.contains(&section_name(&home, "downloaded")), "{r:?}");

        // 4. Схема цветов одним файлом из «Загруженных».
        let colors = input.join("Ocean.colors");
        std::fs::write(&colors, "[ColorEffects:Disabled]\nColor=56,56,56\n[Colors:Window]\nBackgroundNormal=1,2,3\n[General]\nName=Ocean\n").unwrap();
        let r = live_install(&[colors.to_string_lossy().into_owned()], "downloaded", quiet());
        eprintln!("цвета: {r:?}");
        assert!(r.ok, "цвета: {r:?}");
        assert!(share.join("color-schemes/Ocean.colors").is_file());
        // «Загруженные» сами показывают диск — их перечитать.
        assert!(r.reload && r.open_key.is_empty(), "{r:?}");

        // 5. Обои: картинка и видео — в библиотеку обоев.
        let png = input.join("Закат.png");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR").unwrap();
        let mp4 = input.join("clip.mp4");
        std::fs::write(&mp4, b"\x00\x00\x00\x18ftypmp42\x00\x00\x00\x00").unwrap();
        let r = live_install(
            &[png.to_string_lossy().into_owned(), mp4.to_string_lossy().into_owned()],
            "wallpapers",
            quiet(),
        );
        eprintln!("обои: {r:?}");
        assert!(r.ok, "обои: {r:?}");
        let lib = crate::wallpaper_engine::list_installed_wallpapers(
            &crate::qt_extensions::extensions_dir().to_string_lossy(),
        );
        let formats: Vec<&str> = lib.iter().map(|w| w.format.as_str()).collect();
        assert!(formats.contains(&"static") && formats.contains(&"video"), "библиотека: {lib:?}");
        // Повтор той же картинки — «уже установлено», а не второй пакет.
        let again = live_install(&[png.to_string_lossy().into_owned()], "wallpapers", quiet());
        assert!(!again.ok);

        // 6. Системная тема из чужого раздела не ставится.
        let ply = input.join("ply/MyBoot");
        std::fs::create_dir_all(&ply).unwrap();
        std::fs::write(ply.join("MyBoot.plymouth"), "[Plymouth Theme]\nName=MyBoot\n").unwrap();
        let r = live_install(&[ply.to_string_lossy().into_owned()], "global", quiet());
        assert!(!r.ok);
        assert_eq!(r.open_key, "plymouth");

        // 7. Мусор — понятный отказ.
        let junk = input.join("notes.odt");
        std::fs::write(&junk, b"just text").unwrap();
        let r = live_install(&[junk.to_string_lossy().into_owned()], "downloaded", quiet());
        assert!(!r.ok && r.message.contains("notes.odt"), "{r:?}");

        // 8. ZIP-архив с выходом за каталог отвергнут целиком.
        let slip = input.join("slip.zip");
        {
            use std::io::Write as _;
            let mut zw = zip::ZipWriter::new(std::fs::File::create(&slip).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            zw.start_file("../../escape.colors", o).unwrap();
            zw.write_all(b"[Colors:Window]\n").unwrap();
            zw.finish().unwrap();
        }
        let r = live_install(&[slip.to_string_lossy().into_owned()], "colors", quiet());
        assert!(!r.ok, "{r:?}");
        assert!(!base.join("escape.colors").exists() && !input.join("../escape.colors").exists());

        // 9. Пакет тем Conky `.cmtp.7z` из «Загруженных»: виджеты из скрытой
        //    `.conky/` — каждый своей папкой, шрифты пакета — шрифтами.
        if crate::utils::which("bsdtar", None).is_none() {
            eprintln!("НЕ ПРОВЕРЕНО: нет bsdtar, чтобы собрать .7z");
            return;
        }
        let src = input.join("cmtp-src");
        let put = |rel: &str, text: &str| {
            let p = src.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        put(".conky/Gotham/Gotham", "conky.config = { gap_x = 10 }\nconky.text = [[${time}]]\n");
        put(".conky/Seamod/conky_seamod", "TEXT\n${cpu}\n");
        put(".conky/Seamod/rings.lua", "function conky_main() end\n");
        put(".conky/Set/Set.cmtheme", "~/.conky/Gotham/Gotham\nwallpaper-scaling:zoomed\n");
        put(".conky/Set/Gotham", "conky.text = [[x]]\n");
        put(".conky/fonts/Fake.ttf", "ttf");
        let pack = input.join("mini.cmtp.7z");
        let st = std::process::Command::new("bsdtar")
            .args(["--format", "7zip", "-cf", &pack.to_string_lossy(), "-C", &src.to_string_lossy(), ".conky"])
            .status()
            .unwrap();
        assert!(st.success());
        let r = live_install(&[pack.to_string_lossy().into_owned()], "conky", quiet());
        eprintln!("пакет Conky: {r:?}");
        assert!(r.ok, "пакет Conky: {r:?}");
        let conky = home.join(".config/conky");
        for f in ["Gotham/Gotham", "Seamod/conky_seamod", "Seamod/rings.lua", "Set/Set.cmtheme"] {
            assert!(conky.join(f).is_file(), "{f} не лёг: {r:?}");
        }
        assert!(!conky.join("fonts").exists() && !conky.join(".conky").exists(), "{r:?}");
        assert!(share.join("fonts/rexon-conky-mini/Fake.ttf").is_file(), "{r:?}");
        assert!(r.reload, "{r:?}");
    }

    /// Шрифт «для всех»: заведомый отказ (не шрифт, имя занято) — без
    /// пароля; иначе ровно одна операция сеанса прав с файлом-дескриптором;
    /// без помощника — честный текст, без `pkexec`.
    #[test]
    fn шрифт_для_всех_через_сеанс_прав() {
        use crate::privilege::op::{FontFile, LocalFile, PrivOp};
        use crate::qt_settings::FontInstallOutcome as O;
        use crate::system_exec::RunOut;
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let out = |rc: i32, stderr: &str| RunOut {
            outcome: crate::utils::RunOutcome::Ok,
            returncode: Some(rc),
            stdout: String::new(),
            stderr: stderr.into(),
        };
        let font = |_: &str| "Stand Sans\n".to_string();
        let none = |_: &str| None;
        let src = "/home/u/Загрузки/Мой шрифт.ttf";
        let mut seen = Vec::new();
        let r = install_font_file_shared_with(src, &font, &none, &mut |op, files| {
            seen.push((op.clone(), files.to_vec()));
            Some(out(0, ""))
        });
        assert_eq!(r, O::InstalledForAll(vec!["Stand Sans".into()]));
        assert_eq!(
            seen,
            vec![(
                PrivOp::FontInstall {
                    fonts: vec![FontFile { source: LocalFile { slot: 0, label: src.into() }, name: "Мой шрифт.ttf".into() }],
                },
                vec![src.to_string()],
            )]
        );
        // Не шрифт и негодное имя — до пароля.
        let mut called = false;
        let mut no_call = |_: &PrivOp, _: &[String]| {
            called = true;
            None
        };
        assert_eq!(install_font_file_shared_with("/h/x.txt", &|_| String::new(), &none, &mut no_call), O::NotAFont("x.txt".into()));
        assert_eq!(install_font_file_shared_with("/h/.x.ttf", &font, &none, &mut no_call), O::NotAFont(".x.ttf".into()));
        // Имя занято: тот же файл — «уже стоит», другой — «имя занято».
        let same = |_: &str| Some(b"same".to_vec());
        assert_eq!(install_font_file_shared_with("/h/a.ttf", &font, &same, &mut no_call), O::AlreadyInstalled("a.ttf".into()));
        let other = |p: &str| Some(if p.starts_with("/usr/local") { b"theirs".to_vec() } else { b"mine".to_vec() });
        assert_eq!(install_font_file_shared_with("/h/a.ttf", &font, &other, &mut no_call), O::NameTaken("a.ttf".into()));
        assert!(!called, "заведомый отказ не спрашивает пароль");
        // Отказ помощника — его готовый текст; помощника нет — честно, без pkexec.
        let refused = install_font_file_shared_with("/h/a.ttf", &font, &none, &mut |_, _| Some(out(1, "Rexon refused: x")));
        assert_eq!(refused, O::NotForAll("Rexon refused: x".into()));
        let O::NotForAll(missing) = install_font_file_shared_with("/h/a.ttf", &font, &none, &mut |_, _| None) else {
            panic!("ждали текст");
        };
        assert!(!missing.is_empty() && !missing.contains("pkexec"), "{missing}");
    }

    #[test]
    fn удаление_шрифтов_для_всех_через_сеанс_прав() {
        use crate::privilege::op::PrivOp;
        use crate::system_exec::RunOut;
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let out = |rc: i32, stderr: &str| RunOut {
            outcome: crate::utils::RunOutcome::Ok,
            returncode: Some(rc),
            stdout: String::new(),
            stderr: stderr.into(),
        };
        let files = vec![
            "/usr/local/share/fonts/a.ttf".to_string(),
            "/usr/share/fonts/noto/N.ttf".to_string(),
            "/home/u/.local/share/fonts/b.ttf".to_string(),
        ];
        let mut seen = Vec::new();
        assert_eq!(remove_shared_fonts_with(&files, &mut |op| {
            seen.push(op.clone());
            Some(out(0, ""))
        }), Ok(()));
        // Шрифт пакета и свой файл помощнику не уходят.
        assert_eq!(seen, vec![PrivOp::FontRemove { files: vec!["/usr/local/share/fonts/a.ttf".into()] }]);
        assert_eq!(remove_shared_fonts_with(&files, &mut |_| Some(out(1, "Rexon refused: y"))), Err("Rexon refused: y".into()));
        assert!(remove_shared_fonts_with(&files, &mut |_| None).is_err());
        let mut called = false;
        assert!(remove_shared_fonts_with(&files[1..], &mut |_| {
            called = true;
            None
        })
        .is_err());
        assert!(!called, "без файлов «для всех» — ни одной операции");
    }
}
