//! Порт `rexon/qt_steam/window.py` — вкладка «Steam»: содержимое и решения.
//!
//! Вкладка склеивает две витрины, которые роднит только источник данных —
//! Steam:
//!
//!   * 🎬 **Видеозаставка при входе** — витрина роликов (встроенные Valve +
//!     купленные в Points Shop), выбор, избранное, ротация, автозапуск.
//!     Механика заставки давно живёт в [`crate::session_splash`]; здесь —
//!     то, что показывает витрина: бейдж происхождения, подпись-источник,
//!     чипы фильтра, строка счётчика, крупная карточка и итоговый чип.
//!   * 🖼 **Фоны профиля Steam** — сетка предметов инвентаря, применяемых
//!     обоями. Разбор ответов Steam уже перенесён
//!     ([`crate::steam_profile`], [`crate::steam_profile_items`]); здесь —
//!     страничная сетка, поиск, карточка, пометка «✓ Применено» и тексты
//!     состояний.
//!
//! Чего здесь НЕТ: сети, файловой системы и Qt. Пути к кэшам строятся, но не
//! читаются; адреса запросов собираются, но не выполняются — это дело
//! исполнителя (`rexon-gui/src/steam_workers.rs`).
//!
//! ⚠️ Починки оригинала (правило #16 — дефект при переносе чинится, а не
//! консервируется «ради паритета»), каждая помечена `Починка:` по месту:
//!   1. пути кэшей и конфигов вкладки были прибиты литералами
//!      `~/.cache/rexon…` и `~/.config/rexon…` мимо `$XDG_CACHE_HOME` и
//!      `$XDG_CONFIG_HOME` — тот же класс, что уже починен у
//!      [`crate::session_splash::preview_cache_dir`] и
//!      [`crate::utils::config_home`]. При нестандартном XDG список
//!      инвентаря кэшировался в один каталог, а читался из другого: витрина
//!      КАЖДЫЙ раз открывалась пустой и ждала сеть;
//!   2. мёртвая пара `_THUMB_W, _THUMB_H = 96, 54` (размер миниатюры
//!      списочного режима, которого во вкладке уже нет) не перенесена;
//!   3. `_download_community` при неудаче оставлял на диске недокачанный
//!      `<файл>.part`; здесь имя временного файла отдаётся отдельно
//!      ([`part_path`]) вместе с указанием убрать его на любом исходе;
//!   4. и `_InventoryWorker`, и `_SplashMetaWorker` тянули ВЕСЬ публичный
//!      инвентарь каждый сам по себе (до 20 страниц по 2000 предметов —
//!      дважды на каждое открытие вкладки). Разбор здесь разведён на
//!      «страницы» и «что из них взять», чтобы исполнитель качал один раз и
//!      кормил обе витрины ([`splash_item_meta`] и
//!      [`crate::steam_profile_items::parse_inventory_backgrounds`]).
//!
//! ПЕРЕВОД (с 28.09.2026). Подписи — английские `msgid` через
//! [`crate::i18n`]: константы стали функциями (`sv_pick_text()` вместо
//! `SV_PICK_TEXT` — функцию, в отличие от константы, не забудешь перевести),
//! русский дословно прежний — в `po/ru.po` (снимок
//! `tests/golden/qt_steam_ru_before.json`). Язык ответов Steam (метки и
//! названия предметов инвентаря) следует языку интерфейса —
//! [`steam_language`].
//!
//! Сверка — `rust/rexon-core/tests/qt_steam_parity.rs`.

use serde::Serialize;
use serde_json::{Map, Value};

use crate::i18n::{self, n_, tr, trf};
use crate::session_splash;
use crate::steam_profile_items::ProfileBackgroundItem;
use crate::ui_base;
use crate::utils;
use crate::wallpaper_exec::{WallpaperExec, WpSys};

// ============================================================================
// Окно и шапка
// ============================================================================

pub const WINDOW_TITLE: &str = "Rexon — Steam";
pub const WINDOW_MIN_SIZE: (u32, u32) = (900, 720);

pub const HEADER_TITLE: &str = "🎮 Steam";
pub const HEADER_TITLE_POINT_SIZE: i32 = 20;
/// Кегль заголовка группы («🎬 Видеозаставка при входе»).
pub const GROUP_TITLE_POINT_SIZE: i32 = 13;

/// Подпись справа в шапке: где нашёлся Steam либо что делать, если не нашёлся.
///
/// Оригинал отдаёт RichText с `<tt>` вокруг пути — разметка остаётся у вида,
/// ядро отдаёт пару «текст, путь-моноширинным». Пустой второй элемент =
/// моноширинного куска нет.
pub fn header_found_text(steam_root: Option<&str>) -> (String, String) {
    match steam_root {
        Some(root) if !root.is_empty() => (
            // TRANSLATORS: followed by the path of the Steam folder; keep the
            // trailing space.
            tr("Found: "),
            root.to_string(),
        ),
        _ => (
            tr("Steam not found — install Steam to use startup movies and backgrounds"),
            String::new(),
        ),
    }
}

// ============================================================================
// Сегментный переключатель подвкладок
// ============================================================================

/// Идентификатор подвкладки «Видеозаставка».
pub const SEG_SPLASH: &str = "splash";
/// Идентификатор подвкладки «Steam-фоны».
pub const SEG_BG: &str = "bg";
/// Идентификатор подвкладки «Витрины профиля» (нарезка картинки под витрину,
/// `steam_showcase`).
pub const SEG_SHOWCASE: &str = "showcase";

/// Состав и ПОРЯДОК чипов-переключателей. Заставка первой: она — флагманская
/// фича вкладки, фоны профиля вторичны.
///
/// Подпись — `msgid`; показывать через [`segments`].
pub const SEGMENTS: &[(&str, &str)] = &[
    (SEG_SPLASH, n_("🎬 Startup Movie")),
    (SEG_BG, n_("🖼️ Steam Backgrounds")),
    (SEG_SHOWCASE, n_("🖼️ Profile Showcases")),
];

/// Чипы-переключатели с подписями на языке интерфейса.
pub fn segments() -> Vec<(&'static str, String)> {
    SEGMENTS.iter().map(|(key, label)| (*key, tr(label))).collect()
}

/// Индекс страницы стека по ключу подвкладки (0 — заставка, 1 — фоны,
/// 2 — витрины). Незнакомый ключ — фоны, как было до витрин.
pub fn segment_index(key: &str) -> u32 {
    match key {
        SEG_SPLASH => 0,
        SEG_SHOWCASE => 2,
        _ => 1,
    }
}

/// Знакома ли подвкладка (переключение на чужой ключ молча игнорируется).
pub fn is_segment(key: &str) -> bool {
    SEGMENTS.iter().any(|(k, _)| *k == key)
}

// ============================================================================
// Группы-секции
// ============================================================================

pub const SPLASH_GROUP_OBJECT: &str = "steamSplashGroup";
pub fn splash_group_title() -> String {
    tr("🎬 Startup Movie at Login")
}
/// Главный тезис секции — одной строкой, без жаргона (правило #13).
pub fn splash_pitch() -> String {
    tr("A video at login is a Steam Deck signature. Rexon brings it to your regular computer: \
        Valve's built-in startup movies and the ones you've already bought in the Points Shop — \
        with full sound and without the desktop flashing through.")
}

pub const BG_GROUP_OBJECT: &str = "steamBgGroup";
pub fn bg_group_title() -> String {
    tr("🖼️ Steam Profile Backgrounds")
}
pub fn bg_group_desc() -> String {
    tr("Steam profile backgrounds from your inventory as desktop wallpapers: static ones as a \
        picture, animated ones as live wallpapers. Your inventory has to be public in Steam's \
        privacy settings.")
}

// ============================================================================
// Витрина заставок: размеры и подписи
// ============================================================================

/// Пауза перед сменой ролика при наведении: провёл мышью по всей сетке —
/// плеер не должен дёргаться на каждой карточке.
pub const SV_HOVER_DELAY_MS: u32 = 220;
/// Служебный ключ чипа «★ Избранное» (остальные чипы — названия групп).
pub const SV_GROUP_FAV: &str = "\u{0}favs";
/// На столько превью замирает, пока ролик показывается на весь экран
/// (снимается раньше — по возврату фокуса в окно).
pub const SV_TEST_PAUSE_S: f64 = 25.0;
/// Минимальный промежуток между разворачиванием кадров превью (~15 к/с):
/// кадр приходит в полном разрешении ролика и стоит ~8 МБ на разворот.
pub const SV_FRAME_MIN_INTERVAL: f64 = 1.0 / 15.0;

/// Превью на карточке витрины, 16:10 — родная пропорция роликов.
pub const SV_THUMB_W: i32 = 176;
pub const SV_THUMB_H: i32 = 110;
/// Ширина карточки витрины. ПОСТОЯННАЯ: по ней движок сеток считает столбцы.
pub const SV_CARD_W: i32 = SV_THUMB_W + 24;
pub const SV_MIN_COLS: u32 = 2;
pub const SV_MAX_COLS: u32 = 8;
/// Крупная превьюшка выбранного ролика.
pub const SV_HERO_W: i32 = 400;
pub const SV_HERO_H: i32 = 250;
/// Ширина, по которой обрезается название на карточке (минус место звёздочки).
pub const SV_TITLE_ELIDE_W: i32 = SV_THUMB_W - 22;

pub fn sv_search_placeholder() -> String {
    tr("🔍 Search by title, game or event…")
}
pub fn sv_pick_text() -> String {
    tr("📁 Choose File…")
}
pub fn sv_pick_tooltip() -> String {
    tr("Choose your own video (webm/mp4) from disk")
}
pub fn sv_pick_dialog_title() -> String {
    tr("Choose a Startup Movie")
}
pub fn sv_pick_filter() -> String {
    // TRANSLATORS: file dialog filter: translate only the group names ("Video", "All files"); keep the patterns in parentheses and the ";;" separator as they are.
    tr("Video (*.webm *.mp4 *.mkv);;All files (*)")
}

pub fn sv_autostart_text() -> String {
    tr("Play at login")
}
pub fn sv_autostart_tooltip() -> String {
    tr("Master switch: the movie plays right after you log in")
}
pub fn sv_rotate_text() -> String {
    tr("New movie at every login")
}
pub fn sv_rotate_tooltip() -> String {
    tr("The startup movie changes by itself. If at least two movies are starred, only those take \
        turns; otherwise all found ones do.")
}
pub fn sv_sound_text() -> String {
    tr("🔊 Sound")
}

pub fn sv_test_text() -> String {
    tr("▶️ Watch Full Screen")
}
pub fn sv_test_tooltip() -> String {
    tr("Play the movie right now — this is how it looks at login")
}
pub fn sv_test_playing() -> String {
    tr("▶️ Playing…")
}
/// Отказ показать заставку, пока ролик не выбран.
///
/// ⚠️ Копии этой строки было ТРИ (замер 07.09.2026): здесь, в
/// [`crate::session_splash::MSG_CHOOSE_FIRST`] и в хозяине QML-вкладки на
/// Python. Смысл у всех трёх один — «сначала выбери заставку», — и
/// расходиться им нельзя: витрина и автозапуск отказывают по одной и той
/// же причине. Осталась одна, остальные ссылаются сюда.
pub const SV_TEST_NO_ITEM: &str = crate::session_splash::MSG_CHOOSE_FIRST;

/// Причина отказа, когда подсистема не назвала своей.
///
/// Побочные действия витрины (автозапуск, показ на весь экран) отвечают
/// парой «получилось / почему нет». Пустое «почему» — не повод показать
/// покупателю пустую строку состояния: он нажал кнопку и обязан увидеть
/// ответ. Виджетный эталон такого случая не знал вовсе и печатал пустоту.
pub fn sv_no_reason() -> String {
    // TRANSLATORS: reason shown after a failed action when the system gave no reason of its own.
    tr("it didn't work")
}

/// Заголовок отказа включить заставку при входе.
pub fn sv_autostart_on_failed() -> String {
    tr("❌ Couldn't turn on the startup movie at login")
}
/// Заголовок отказа выключить её.
pub fn sv_autostart_off_failed() -> String {
    tr("❌ Couldn't fully turn off the startup movie at login")
}
/// Хвост, когда причину назвать простыми словами нельзя.
pub fn sv_autostart_try_again() -> String {
    tr("Please try again.")
}
/// Причины, которые покупатель может понять и исправить сам.
pub fn sv_cause_no_access() -> String {
    // TRANSLATORS: reason after "Couldn't turn on…: ", starts in lower case.
    tr("no permission to write to the settings folder")
}
pub fn sv_cause_no_space() -> String {
    // TRANSLATORS: reason after "Couldn't turn on…: ", starts in lower case.
    tr("the disk is full")
}
pub fn sv_cause_read_only() -> String {
    // TRANSLATORS: reason after "Couldn't turn on…: ", starts in lower case.
    tr("the settings folder is read-only")
}

/// Системная ошибка файла → причина простыми словами, если её можно назвать.
///
/// Приходит сырой текст: у Rust-исполнителя `… (os error 13)` (первая часть
/// ещё и на языке системы), у Python-хозяина `[Errno 13] …`. Номер ошибки —
/// единственное, что в них не зависит от языка, по нему и узнаём.
fn plain_file_cause(reason: &str) -> Option<String> {
    let low = reason.to_lowercase();
    let errno = |n: u32| {
        low.contains(&format!("os error {n})")) || low.contains(&format!("errno {n}]"))
    };
    if errno(13) || errno(1) || low.contains("permission denied") {
        Some(sv_cause_no_access())
    } else if errno(28) || low.contains("no space left") {
        Some(sv_cause_no_space())
    } else if errno(30) || low.contains("read-only file system") {
        Some(sv_cause_read_only())
    } else {
        None
    }
}

/// Что показать покупателю после переключателя «Показывать при входе».
///
/// `on` — что он просил (включить или выключить), `reason` — ответ побочного
/// действия: пусто — получилось (галочку страница и так перечитает),
/// иначе причина отказа.
///
/// ЗАЧЕМ ОТДЕЛЬНО. Причину собирает маршрут заставки для журнала, и она
/// говорит на языке устройства: «Не удалось включить: Permission denied
/// (os error 13)», «юнит: …; ksplashrc: kwriteconfig6 не сработал;
/// autostart: …». Виджетная вкладка печатала это покупателю как есть, а
/// QML-версия не печатала вовсе — галочка молча отскакивала назад. Здесь
/// — одна фраза: что не вышло и, если это можно назвать простыми словами,
/// почему. Подробность остаётся журналу.
pub fn autostart_refusal_text(on: bool, reason: &str) -> Option<String> {
    let reason = reason.trim();
    if reason.is_empty() {
        return None;
    }
    // Эта причина уже сказана человеческим языком и понятна без заголовка.
    // Узнаём по `msgid` и по переводу: маршрут заставки отдаёт её уже
    // на языке интерфейса (`session_splash::msg_choose_first`).
    if reason == SV_TEST_NO_ITEM || reason == tr(SV_TEST_NO_ITEM) {
        return Some(format!("❌ {}", tr(SV_TEST_NO_ITEM)));
    }
    let head = if on {
        sv_autostart_on_failed()
    } else {
        sv_autostart_off_failed()
    };
    Some(match plain_file_cause(reason) {
        Some(cause) => format!("{head}: {cause}."),
        None => format!("{head}. {}", sv_autostart_try_again()),
    })
}

/// Доклеивать ли к витрине СВОЙ ролик из конфига.
///
/// Витрину строит скан стартовых видео Steam, а покупатель мог выбрать
/// файл откуда угодно с диска. Тогда его выбор в скан не попадает, и без
/// этой доклейки он пропадал бы из виду — при том, что именно он и
/// применён.
///
/// `exists` — есть ли файл на диске; это наблюдение среды, и приносит его
/// исполнитель. Пути к несуществующему файлу карточки не бывает: конфиг
/// мог пережить удаление ролика.
pub fn needs_custom_movie(current: &str, scanned: &[String], exists: bool) -> bool {
    !current.is_empty() && exists && !scanned.iter().any(|p| p == current)
}

pub fn sv_star_on() -> String {
    // TRANSLATORS: button state: the startup movie is in favorites (click removes it).
    tr("⭐ In Favorites")
}
pub fn sv_star_off() -> String {
    tr("☆ Add to Favorites")
}
pub fn sv_star_tooltip() -> String {
    tr("Favorite movies take turns when “New movie at every login” is on")
}
pub fn sv_card_star_tooltip() -> String {
    tr("Add to favorites — “New movie at every login” takes turns among favorites")
}
pub const SV_STAR_GLYPH_ON: &str = "⭐";
pub const SV_STAR_GLYPH_OFF: &str = "☆";

pub fn sv_all_chip() -> String {
    // TRANSLATORS: filter chip: show startup movies from all sources.
    tr("All")
}
pub fn sv_fav_chip() -> String {
    tr("⭐ Favorites")
}
pub fn sv_empty_text() -> String {
    tr("No startup movies found. Steam isn't installed or the movies haven't been downloaded yet — \
        you can pick your own video with the “Choose File…” button.")
}
pub fn sv_hero_empty_title() -> String {
    tr("No startup movie selected")
}
pub fn sv_hero_preview_hint() -> String {
    tr("Preview — click the card to use this movie at login")
}
pub fn sv_rotate_need_two() -> String {
    tr("Taking turns needs at least two videos")
}

/// Что сказать покупателю, когда он переключил «Каждый вход — новый ролик».
///
/// Включил, а крутить нечего, — честно сказать об этом, а не делать вид
/// (правило #13; так отвечала виджетная вкладка). Выключение и достаточный
/// пул ответа не требуют: итог и так виден в чипе состояния. Порог — общий
/// с жребием при входе ([`session_splash::rotation_possible`]).
pub fn rotation_refusal(on: bool, favs_len: usize, items_len: usize) -> Option<String> {
    (on && !session_splash::rotation_possible(favs_len, items_len)).then(sv_rotate_need_two)
}
/// Заглушка карточки, пока кадр ролика не готов.
pub const SV_THUMB_PLACEHOLDER: &str = "🎬";

// ============================================================================
// Витрина заставок: решения
// ============================================================================

/// Метки предмета Steam для одного ролика — то, что показывает инвентарь.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SplashMeta {
    /// Название предмета в инвентаре.
    pub name: String,
    /// «Видеозаставка из Days Gone» — строка `type` предмета.
    pub source: String,
    /// Событие/распродажа (метка `event`), если есть.
    pub event: String,
    /// Редкость, игра, тип предмета, запреты обмена и продажи.
    pub labels: Vec<String>,
}

/// Строковое поле объекта: "" если ключа нет/`null`/не строка.
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// `appid` записи ролика (0, если его нет).
pub fn movie_appid(item: &Value) -> i64 {
    item.get("appid").and_then(Value::as_i64).unwrap_or(0)
}

/// Метки ролика по его `appid` (пустые, если инвентарь ещё не пришёл).
fn meta_of<'a>(meta: &'a MetaMap, item: &Value) -> Option<&'a SplashMeta> {
    meta.get(&movie_appid(item))
}

/// appid → метки. `BTreeMap`, а не `HashMap`: порядок обхода обязан быть
/// устойчивым, иначе чипы фильтра переставлялись бы от запуска к запуску.
pub type MetaMap = std::collections::BTreeMap<i64, SplashMeta>;

/// Короткий бейдж происхождения ролика: событие важнее магазина.
pub fn badge_text(item: &Value, meta: &MetaMap) -> String {
    if let Some(m) = meta_of(meta, item) {
        if !m.event.is_empty() {
            return m.event.clone();
        }
    }
    let game = s(item, "game");
    if !game.is_empty() {
        return game.to_string();
    }
    s(item, "origin_label").to_string()
}

/// Тон бейджа: купленные в Points Shop выделяются акцентом.
pub fn badge_tone(item: &Value) -> &'static str {
    if s(item, "origin") == session_splash::ORIGIN_SHOP {
        BADGE_ACCENT
    } else {
        BADGE_NEUTRAL
    }
}

/// Строка-источник в духе инвентаря Steam: «Видеозаставка из X».
pub fn source_text(item: &Value, meta: &MetaMap) -> String {
    if let Some(m) = meta_of(meta, item) {
        if !m.source.is_empty() {
            return m.source.clone();
        }
    }
    let game = s(item, "game");
    if !game.is_empty() {
        // TRANSLATORS: %1 is the name of a game or a Steam sale event.
        return trf("Startup movie from %1", &[&game]);
    }
    match s(item, "origin") {
        session_splash::ORIGIN_BUILTIN => tr("Built-in Steam startup movie"),
        session_splash::ORIGIN_CUSTOM => tr("Your own video from disk"),
        _ => tr("Startup movie from the Points Shop"),
    }
}

/// Группы, которые реально есть у роликов (в порядке появления).
pub fn group_keys(items: &[Value], meta: &MetaMap) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for it in items {
        let k = badge_text(it, meta);
        if !k.is_empty() && !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys
}

/// Есть ли среди роликов хоть один избранный.
pub fn has_favorites(items: &[Value], favs: &[String]) -> bool {
    items
        .iter()
        .any(|it| favs.iter().any(|f| f == s(it, "path")))
}

/// Выбранная группа могла исчезнуть (сняли звёздочку, пропал ролик) — молча
/// возвращаемся к «Все», чтобы витрина не оказалась пустой.
pub fn normalize_group_key(selected: &str, items: &[Value], favs: &[String], meta: &MetaMap) -> String {
    if selected.is_empty() {
        return String::new();
    }
    if selected == SV_GROUP_FAV {
        return if has_favorites(items, favs) {
            SV_GROUP_FAV.to_string()
        } else {
            String::new()
        };
    }
    if group_keys(items, meta).iter().any(|k| k == selected) {
        selected.to_string()
    } else {
        String::new()
    }
}

/// Один чип фильтра «откуда заставка».
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupChip {
    pub key: String,
    pub label: String,
    pub checked: bool,
}

/// Чипы фильтра под текущий набор роликов.
///
/// Пустой ответ = строку фильтра показывать НЕ надо: пустых кнопок не бывает,
/// а если группа всего одна и избранного нет — выбора попросту не существует
/// (правило #13).
pub fn group_chips(items: &[Value], favs: &[String], meta: &MetaMap, selected: &str) -> Vec<GroupChip> {
    let keys = group_keys(items, meta);
    let favs_present = has_favorites(items, favs);
    if keys.len() < 2 && !favs_present {
        return Vec::new();
    }
    let cur = normalize_group_key(selected, items, favs, meta);
    let mut out = vec![GroupChip {
        key: String::new(),
        label: sv_all_chip().to_string(),
        checked: cur.is_empty(),
    }];
    for k in keys {
        out.push(GroupChip {
            checked: cur == k,
            label: k.clone(),
            key: k,
        });
    }
    if favs_present {
        out.push(GroupChip {
            key: SV_GROUP_FAV.to_string(),
            label: sv_fav_chip().to_string(),
            checked: cur == SV_GROUP_FAV,
        });
    }
    out
}

/// Проходит ли ролик выбранный фильтр «откуда заставка».
pub fn in_group(item: &Value, key: &str, favs: &[String], meta: &MetaMap) -> bool {
    if key.is_empty() {
        return true;
    }
    if key == SV_GROUP_FAV {
        return favs.iter().any(|f| f == s(item, "path"));
    }
    badge_text(item, meta) == key
}

/// Поиск витрины: название, игра, событие. `needle` уже приведён к нижнему
/// регистру и обрезан по краям (как `.strip().lower()` оригинала).
pub fn sv_matches(item: &Value, needle: &str, key: &str, favs: &[String], meta: &MetaMap) -> bool {
    if !in_group(item, key, favs, meta) {
        return false;
    }
    if needle.is_empty() {
        return true;
    }
    let m = meta_of(meta, item);
    let hay = [
        s(item, "title"),
        s(item, "name"),
        s(item, "game"),
        s(item, "origin_label"),
        m.map(|x| x.source.as_str()).unwrap_or(""),
        m.map(|x| x.event.as_str()).unwrap_or(""),
    ]
    .join(" ")
    .to_lowercase();
    hay.contains(needle)
}

/// Отобранные поиском и фильтром ролики (порядок исходного списка).
pub fn sv_shown<'a>(items: &'a [Value], needle: &str, key: &str, favs: &[String], meta: &MetaMap) -> Vec<&'a Value> {
    items
        .iter()
        .filter(|it| sv_matches(it, needle, key, favs, meta))
        .collect()
}

/// Строка-счётчик под чипами фильтра.
pub fn sv_count_text(shown: usize, total: usize, filtered: bool) -> String {
    if total == 0 {
        sv_empty_text().to_string()
    } else if filtered {
        showing_text(shown, total)
    } else {
        trf("Startup movies available: %1", &[&total])
    }
}

/// «Показано 3 из 10» — одна фраза на обе витрины.
fn showing_text(shown: usize, total: usize) -> String {
    // TRANSLATORS: search result counter: %1 shown out of %2 in total.
    trf("Showing %1 of %2", &[&shown, &total])
}

/// Крупная карточка выбранного (или наведённого) ролика.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HeroView {
    /// Есть ли что показывать вообще.
    pub present: bool,
    pub title: String,
    /// «🎮 Видеозаставка из X».
    pub source: String,
    /// «Метки: …» либо пусто (пустую строку показывать не надо).
    pub labels: String,
    /// «Событие: …» либо пусто.
    pub event: String,
    /// Подпись кнопки звёздочки.
    pub star_text: String,
    pub star_on: bool,
    /// Разъяснение «смотрю одно, а при входе будет другое».
    pub status: String,
}

/// Собрать крупную карточку. `shown` — что показывает карточка (наведённое
/// либо выбранное), `current` — что реально пойдёт при входе.
pub fn hero(shown: Option<&Value>, current: Option<&Value>, favs: &[String], meta: &MetaMap) -> HeroView {
    let Some(item) = shown else {
        return HeroView {
            present: false,
            title: sv_hero_empty_title().to_string(),
            ..HeroView::default()
        };
    };
    let title = {
        let t = s(item, "title");
        let n = s(item, "name");
        if !t.is_empty() {
            t.to_string()
        } else if !n.is_empty() {
            n.to_string()
        } else {
            "?".to_string()
        }
    };
    let m = meta_of(meta, item);
    let labels = m.map(|x| x.labels.clone()).unwrap_or_default();
    let event = m.map(|x| x.event.clone()).unwrap_or_default();
    let star_on = favs.iter().any(|f| f == s(item, "path"));
    // «Предпросмотр» — когда показываем НЕ тот ролик, что пойдёт при входе.
    let is_current = current.map(|c| s(c, "path") == s(item, "path")).unwrap_or(false);
    HeroView {
        present: true,
        title,
        source: format!("🎮 {}", source_text(item, meta)),
        labels: if labels.is_empty() {
            String::new()
        } else {
            // TRANSLATORS: %1 is a comma-separated list of Steam item tags
            // (rarity, game, item type…).
            trf("Tags: %1", &[&labels.join(", ")])
        },
        event: if event.is_empty() {
            String::new()
        } else {
            // TRANSLATORS: %1 is the name of a Steam sale or event.
            trf("Event: %1", &[&event])
        },
        star_text: if star_on { sv_star_on() } else { sv_star_off() }.to_string(),
        star_on,
        status: if is_current {
            String::new()
        } else {
            sv_hero_preview_hint().to_string()
        },
    }
}

/// Тон крупного чипа итогового состояния.
pub const CHIP_OK: &str = "ok";
pub const CHIP_WARN: &str = "warn";
pub const CHIP_OFF: &str = "off";

/// Итоговое состояние ОДНОЙ строкой (вместо россыпи галочек).
///
/// `favs_len` — сколько роликов в избранном, `items_len` — сколько найдено
/// всего: при двух и более избранных ротация крутит именно их.
pub fn state_chip(
    current: Option<&Value>,
    enabled: bool,
    rotate: bool,
    favs_len: usize,
    items_len: usize,
) -> (String, &'static str) {
    let Some(cur) = current else {
        return (tr("⚪ No startup movie selected"), CHIP_OFF);
    };
    let title = {
        let t = s(cur, "title");
        if t.is_empty() {
            s(cur, "name").to_string()
        } else {
            t.to_string()
        }
    };
    if !enabled {
        return (
            tr("⚪ Off — nothing plays at login"),
            CHIP_OFF,
        );
    }
    // ⚠️ Починка при переносе (правило #16): с одним роликом на диске чип
    // обещал «случайный ролик из всех (1 шт.)», хотя жребий при входе такой
    // пул пропускает (`session_splash::rotation_choices`) и покажется тот же
    // ролик. Теперь ротация, которой не будет, в чипе не обещается.
    if rotate && session_splash::rotation_possible(favs_len, items_len) {
        let by_favs = session_splash::rotation_uses_favorites(favs_len);
        let pool = session_splash::rotation_pool_size(favs_len, items_len);
        let text = if by_favs {
            // TRANSLATORS: %1 is how many favorite movies take turns, %2 is
            // the title of the movie that is up next.
            trf(
                "✅ Ready: the next login plays a random favorite (%1 in the pool), “%2” is up next",
                &[&pool, &title],
            )
        } else {
            // TRANSLATORS: %1 is how many movies take turns, %2 is the title
            // of the movie that is up next.
            trf(
                "✅ Ready: the next login plays a random movie out of all (%1 in the pool), “%2” is up next",
                &[&pool, &title],
            )
        };
        return (text, CHIP_OK);
    }
    (
        // TRANSLATORS: %1 is the title of the startup movie.
        trf("✅ Ready: the next login plays “%1”", &[&title]),
        CHIP_OK,
    )
}

// ============================================================================
// Витрина фонов профиля: размеры и подписи
// ============================================================================

/// Превью на карточке фона, 16:9 — как обои рабочего стола.
pub const BG_THUMB_W: i32 = 140;
pub const BG_THUMB_H: i32 = 79;
/// Ширина карточки фона. ПОСТОЯННАЯ по той же причине, что и у витрины:
/// расчёт столбцов — арифметика по ширине карточки.
pub const BG_CARD_W: i32 = 180;
/// Пагинация задаётся РЯДАМИ, а не числом предметов: при пяти столбцах
/// страница выходит 25 штук, при трёх — 15, и последний ряд всегда полный.
pub const BG_ROWS: u32 = 5;
pub const BG_MIN_COLS: u32 = 2;
/// Потолок столбцов. Был 8, а сетка QML раскладывала карточки сама и на
/// окне 1920 ставила девять: страница из 8 × 5 = 40 кончалась рядом из
/// четырёх карточек. Теперь столбцы сетке задаёт ядро ([`bg_columns`]), а
/// потолок поднят, чтобы широкое окно не растягивало карточки вдвое.
pub const BG_MAX_COLS: u32 = 16;

/// Столбцов в сетке фонов при ширине самой сетки `width` (без полей группы):
/// сколько карточек `BG_CARD_W` с зазором `CARD_GAP` влезает. Это же число
/// стоит у сетки QML в `columns`, поэтому страница `cols × BG_ROWS` — всегда
/// целые ряды.
pub fn bg_columns(width: i32) -> u32 {
    ui_base::grid_columns(width, BG_CARD_W, CARD_GAP, 0, BG_MIN_COLS, BG_MAX_COLS)
}

pub fn bg_search_placeholder() -> String {
    tr("🔍 Search by name or game…")
}
pub fn bg_audio_text() -> String {
    tr("🔊 Live wallpaper sound")
}
pub fn bg_loading_text() -> String {
    tr("⏳ Loading your Steam inventory…")
}
pub fn bg_apply_text() -> String {
    tr("Apply")
}
pub fn bg_applied_label() -> String {
    tr("✅ Applied")
}
pub fn bg_prev_text() -> String {
    // TRANSLATORS: pager button: previous page.
    tr("‹ Prev")
}
pub fn bg_next_text() -> String {
    // TRANSLATORS: pager button: next page.
    tr("Next ›")
}
pub fn bg_equipped_sub() -> String {
    // TRANSLATORS: short note under a background card: this background is
    // equipped on the Steam profile. Keep it short, the card is narrow.
    tr("🎮 equipped")
}
pub fn bg_animated_tooltip() -> String {
    tr("Animated background (webm) — applies as a live wallpaper")
}
/// 📼 у анимированных, 🖼️ у статичных (VS16 = цветное эмодзи, различимы).
pub const BG_ICON_ANIMATED: &str = "📼";
pub const BG_ICON_STATIC: &str = "🖼️";

pub fn bg_no_steam() -> String {
    tr("Steam not found.")
}
pub fn bg_no_account() -> String {
    tr("Couldn't find your Steam account (no userdata folder).")
}
pub fn bg_nothing_found() -> String {
    tr("No profile backgrounds in your inventory — get or buy one (for example in the Points Shop) \
        and it will show up here.")
}
pub fn bg_busy() -> String {
    tr("⏳ Already applying a wallpaper — please wait")
}
pub fn bg_no_engine() -> String {
    tr("⚠️ Wallpapers can't be applied right now")
}
pub fn bg_apply_failed() -> String {
    tr("Couldn't apply")
}

/// Сколько символов ошибки инвентаря показываем (`error[:160]` оригинала).
pub const BG_ERROR_CLIP: usize = 160;

/// Текст неудачи загрузки инвентаря.
pub fn bg_error_text(error: &str) -> String {
    let clipped: String = error.chars().take(BG_ERROR_CLIP).collect();
    // TRANSLATORS: %1 is the error text (may be technical, in English).
    trf(
        "⚠️ Couldn't load the inventory (it may be hidden by Steam's privacy settings): %1",
        &[&clipped],
    )
}

/// Подпись секции фонов профиля: причина важнее счёта.
///
/// Причина («Steam не найден.», «нет userdata», «инвентарь не загрузился»,
/// «фонов нет») в оригинале пишется в ту же метку, что и счёт фонов
/// (`rexon/qt_steam/window.py`, `_bg_status`), — и это единственное честное
/// для неё место: «Найдено фонов: 0» при ненайденном Steam человеку ничего
/// не объясняет, а всплывающее сообщение поверх соседней секции и подавно
/// (оно ещё и уходит через три секунды).
pub fn bg_section_text(notice: &str, total: usize, shown: usize, needle: &str) -> String {
    bg_section_text_filtered(notice, total, shown, !needle.is_empty())
}

/// То же, но «отобрано ли» говорит вызывающий: отбирает не только поиск,
/// но и фильтр вида (статичные / анимированные).
pub fn bg_section_text_filtered(notice: &str, total: usize, shown: usize, filtered: bool) -> String {
    if !notice.is_empty() {
        return notice.to_string();
    }
    if total == 0 {
        trf("Backgrounds found: %1", &[&0])
    } else if filtered {
        showing_text(shown, total)
    } else {
        trf("Backgrounds found: %1", &[&total])
    }
}

/// Строка состояния под поиском.
pub fn bg_status_text(total: usize, shown: usize, needle: &str) -> String {
    bg_section_text_filtered("", total, shown, !needle.is_empty())
}

/// Строка «показываю кэш, обновляю по сети».
pub fn bg_refreshing_text(cached: usize) -> String {
    // TRANSLATORS: %1 is how many backgrounds are shown from the cache while
    // the list is being refreshed from Steam.
    trf("🔄 Updating… (%1 from cache)", &[&cached])
}

/// Тост «скачиваю и применяю».
pub fn bg_applying_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a profile background.
    trf("⏳ Downloading and applying: %1…", &[&name])
}

/// Тост успеха.
pub fn bg_applied_text(name: &str) -> String {
    // TRANSLATORS: %1 is the name of a profile background.
    trf("✅ Applied as wallpaper: %1", &[&name])
}

/// Тост неудачи.
pub fn bg_apply_error_text(msg: &str) -> String {
    if msg.is_empty() {
        format!("❌ {}", bg_apply_failed())
    } else {
        format!("❌ {msg}")
    }
}

/// Применить скачанный фон профиля обоями — общим исполнителем ядра.
///
/// Порт тела `_ApplyBgWorker.run` после скачивания: формат по расширению
/// ([`apply_format`]) и название из витрины — исполнитель сам его не
/// выведет, а мосту оно нужно, чтобы сказать слушателю, ЧТО пошло.
/// Возвращает «получилось ли» и текст исполнителя для тоста.
///
/// ⚠️ Своих правил применения тут нет и быть не должно: картинка, видео,
/// отказ форматам, которых исполнитель ещё не носит, — всё решает
/// [`crate::wallpaper_exec::WallpaperExec::apply`], тот же, что у вкладки
/// «Настройки». До этой функции вкладка «Steam» на Rust не применяла
/// ничего: на месте движка стояла заглушка «движок обоев недоступен».
///
/// `audio` — галочка «🔊 Звук видео-обоев»: уходит исполнителю тем же
/// аргументом, каким звук передают «Настройки»
/// ([`crate::wallpaper_exec::WallpaperExec::apply_with_audio`]); картинке
/// он не нужен. Громкость и «звук вкл/выкл» живут в `settings.json`
/// (`wallpaper_audio`) и действуют поверх неё.
pub fn apply_background_with<S: WpSys>(
    exec: &mut WallpaperExec<S>,
    path: &str,
    audio: bool,
    name: &str,
) -> (bool, String) {
    let res = exec.apply_with_audio(path, apply_format(path), name, audio);
    (res.ok, res.message)
}

// ============================================================================
// Витрина фонов профиля: решения
// ============================================================================

/// Поиск по сетке фонов: название и игра.
pub fn bg_matches(item: &ProfileBackgroundItem, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    format!("{} {}", item.name, item.game)
        .to_lowercase()
        .contains(needle)
}

/// Анимированный ли фон — ОДНО решение на всю вкладку.
///
/// Тот же признак, по которому «Применить» выбирает между картинкой и
/// видео-обоями: Steam прислал ролик (`animated`) либо сам адрес фона ведёт
/// на ролик или GIF ([`apply_format`] по расширению адреса). Карточка,
/// отметка на миниатюре и крупный просмотр спрашивают его отсюда, а не
/// гадают по расширению каждый у себя.
pub fn bg_is_animated(item: &ProfileBackgroundItem) -> bool {
    item.animated || apply_format(&format!("bg{}", url_ext_or_jpg(&item.url))) != "static"
}

/// Отметка анимированного фона в подписи крупного просмотра.
pub fn bg_animated_badge() -> String {
    // TRANSLATORS: short mark shown next to a Steam profile background that
    // is animated (a video), as opposed to a still picture.
    tr("🎞️ Animated background")
}

/// Отметка анимированного фона в углу миниатюры.
pub const BG_ANIMATED_MARK: &str = "🎞️";

// ── Фильтр вида: все / статичные / анимированные ────────────────────────

/// Ключи фильтра вида фонов. Пустой — «все».
pub const BG_KIND_ALL: &str = "";
pub const BG_KIND_STATIC: &str = "static";
pub const BG_KIND_ANIMATED: &str = "animated";

/// Привести ключ фильтра к известному: незнакомый значит «все».
pub fn normalize_bg_kind(key: &str) -> &'static str {
    match key {
        BG_KIND_STATIC => BG_KIND_STATIC,
        BG_KIND_ANIMATED => BG_KIND_ANIMATED,
        _ => BG_KIND_ALL,
    }
}

/// Проходит ли фон фильтр вида.
pub fn bg_kind_matches(item: &ProfileBackgroundItem, kind: &str) -> bool {
    match normalize_bg_kind(kind) {
        BG_KIND_STATIC => !bg_is_animated(item),
        BG_KIND_ANIMATED => bg_is_animated(item),
        _ => true,
    }
}

/// Переключатель вида фонов со счётчиками в подписях.
///
/// Считается то, что прошло ПОИСК: при набранном «portal» в подписях стоит,
/// сколько статичных и анимированных среди найденного, а не во всём
/// инвентаре — иначе число на кнопке не сходилось бы с тем, что человек
/// увидит, нажав её. Пустой ответ — показывать нечего (фонов нет вовсе).
pub fn bg_kind_chips(items: &[ProfileBackgroundItem], needle: &str, selected: &str) -> Vec<GroupChip> {
    if items.is_empty() {
        return Vec::new();
    }
    let found: Vec<&ProfileBackgroundItem> =
        items.iter().filter(|it| bg_matches(it, needle)).collect();
    let animated = found.iter().filter(|it| bg_is_animated(it)).count();
    let still = found.len() - animated;
    let cur = normalize_bg_kind(selected);
    vec![
        GroupChip {
            key: BG_KIND_ALL.to_string(),
            // TRANSLATORS: filter button above the Steam profile backgrounds
            // grid; %1 is how many backgrounds there are.
            label: trf("All (%1)", &[&found.len()]),
            checked: cur == BG_KIND_ALL,
        },
        GroupChip {
            key: BG_KIND_STATIC.to_string(),
            // TRANSLATORS: filter button: backgrounds that are still
            // pictures (not animated); %1 is how many.
            label: trf("Still (%1)", &[&still]),
            checked: cur == BG_KIND_STATIC,
        },
        GroupChip {
            key: BG_KIND_ANIMATED.to_string(),
            // TRANSLATORS: filter button: animated (video) backgrounds;
            // %1 is how many.
            label: trf("Animated (%1)", &[&animated]),
            checked: cur == BG_KIND_ANIMATED,
        },
    ]
}

// ── Почему в списке нет анимированных фонов ─────────────────────────────

/// Чем кончился запрос владеемых предметов профиля — единственный источник
/// ссылок на анимацию. Публичный инвентарь анимацию не отдаёт вовсе, и когда
/// этот запрос не вышел, все фоны в списке статичные.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AnimatedGap {
    /// Список владеемых пришёл: пробела нет.
    #[default]
    None,
    /// Ключа доступа нет; внутри — причина словами ядра
    /// ([`crate::steam_token::no_login_reason`] и соседние).
    NoKey(String),
    /// Steam отказал и со свежим ключом.
    Refused,
    /// Steam не ответил: сеть, срок ожидания, неразборчивый ответ.
    NoAnswer,
}

/// Отказ одного запроса владеемых предметов.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnedError {
    /// 401/403: ключ не принят.
    Refused,
    /// Всё прочее: сеть, срок, ответ не разобрался.
    NoAnswer,
}

/// Отказ по коду ответа: «ключ не принят» — это 401 и 403, остальное —
/// «не ответил».
pub fn owned_error_of_status(code: Option<u16>) -> OwnedError {
    match code {
        Some(401) | Some(403) => OwnedError::Refused,
        _ => OwnedError::NoAnswer,
    }
}

/// Добыть владеемые фоны: ключ → запрос, при отказе — свежий ключ и ОДИН
/// повтор (ключ короткоживущий и мог протухнуть раньше срока кэша).
///
/// `key(force)` отдаёт `(ключ, причина)`: пустой ключ — входа нет, причина
/// идёт человеку. `fetch(ключ)` — сам запрос. Сеть и чтение ключа приходят
/// снаружи, здесь только порядок шагов и приговор.
pub fn owned_backgrounds_with<T>(
    mut key: impl FnMut(bool) -> (String, String),
    mut fetch: impl FnMut(&str) -> Result<Vec<T>, OwnedError>,
) -> (Vec<T>, AnimatedGap) {
    let (token, reason) = key(false);
    if token.is_empty() {
        return (Vec::new(), AnimatedGap::NoKey(reason));
    }
    match fetch(&token) {
        Ok(items) => (items, AnimatedGap::None),
        Err(OwnedError::NoAnswer) => (Vec::new(), AnimatedGap::NoAnswer),
        Err(OwnedError::Refused) => {
            let (token, reason) = key(true);
            if token.is_empty() {
                return (Vec::new(), AnimatedGap::NoKey(reason));
            }
            match fetch(&token) {
                Ok(items) => (items, AnimatedGap::None),
                Err(OwnedError::Refused) => (Vec::new(), AnimatedGap::Refused),
                Err(OwnedError::NoAnswer) => (Vec::new(), AnimatedGap::NoAnswer),
            }
        }
    }
}

/// Строка над сеткой фонов: почему анимированных нет и что с этим делать.
/// Пусто — пробела нет, строка не показывается.
pub fn bg_animated_gap_text(gap: &AnimatedGap) -> String {
    match gap {
        AnimatedGap::None => String::new(),
        // TRANSLATORS: %1 is the reason in lower case, e.g. "sign in to
        // Steam on this computer". "Retry" is the button next to it.
        AnimatedGap::NoKey(reason) => {
            // Причина без слов — та же «ключа не дали»: обрывать фразу
            // двоеточием нельзя.
            let fallback = crate::steam_token::no_token_reason();
            let reason = if reason.trim().is_empty() { &fallback } else { reason };
            trf(
                "⚠️ Animated backgrounds are not shown: %1. Start Steam, sign in and press \"Retry\".",
                &[reason],
            )
        }
        AnimatedGap::Refused => tr(
            "⚠️ Animated backgrounds are not shown: Steam refused to list your profile items. Start Steam, sign in and press \"Retry\".",
        ),
        AnimatedGap::NoAnswer => tr(
            "⚠️ Animated backgrounds are not shown: Steam didn't answer. Check the internet connection and press \"Retry\".",
        ),
    }
}

/// Кнопка рядом со строкой [`bg_animated_gap_text`].
pub fn bg_retry_text() -> String {
    tr("Retry")
}

/// Отфильтрованный поиском срез (индексы исходного списка).
pub fn bg_filtered<'a>(items: &'a [ProfileBackgroundItem], needle: &str) -> Vec<&'a ProfileBackgroundItem> {
    bg_filtered_by(items, needle, BG_KIND_ALL)
}

/// Срез, отобранный поиском И фильтром вида разом.
pub fn bg_filtered_by<'a>(
    items: &'a [ProfileBackgroundItem],
    needle: &str,
    kind: &str,
) -> Vec<&'a ProfileBackgroundItem> {
    items
        .iter()
        .filter(|it| bg_matches(it, needle) && bg_kind_matches(it, kind))
        .collect()
}

/// Карточка сетки фонов.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BgCardView {
    pub id: String,
    /// 📼 либо 🖼️.
    pub icon: &'static str,
    /// Полное название (обрезку по ширине делает вид — у него метрики шрифта).
    pub name: String,
    /// Вторая строка: «надет в профиле», игра либо пусто. Пустую строку всё
    /// равно рисуем — иначе карточки ряда разъезжаются по высоте.
    pub sub: String,
    /// Подсказка второй строки (пусто, если подсказки нет).
    pub sub_tooltip: String,
    pub animated: bool,
    /// Подсказка превью (только у анимированных).
    pub thumb_tooltip: String,
    /// Отметка в углу миниатюры: значок у анимированного, пусто у
    /// статичного. Подсказка у неё общая с миниатюрой (`thumb_tooltip`).
    pub mark: &'static str,
    /// Адрес, по которому тянуть миниатюру (preview, иначе сам фон).
    pub preview_url: String,
}

/// Собрать карточку фона.
pub fn bg_card(item: &ProfileBackgroundItem) -> BgCardView {
    let animated = bg_is_animated(item);
    let icon = if animated {
        BG_ICON_ANIMATED
    } else {
        BG_ICON_STATIC
    };
    let (sub, sub_tooltip) = if item.equipped {
        (bg_equipped_sub().to_string(), String::new())
    } else if !item.game.is_empty() {
        (item.game.clone(), item.game.clone())
    } else {
        (String::new(), String::new())
    };
    BgCardView {
        id: item.id.clone(),
        icon,
        name: if item.name.is_empty() {
            "?".to_string()
        } else {
            item.name.clone()
        },
        sub,
        sub_tooltip,
        animated,
        thumb_tooltip: if animated { bg_animated_tooltip() } else { String::new() },
        mark: if animated { BG_ANIMATED_MARK } else { "" },
        preview_url: if item.preview.is_empty() {
            item.url.clone()
        } else {
            item.preview.clone()
        },
    }
}

/// Полоса пагинации.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PagerView {
    pub visible: bool,
    pub prev_enabled: bool,
    pub next_enabled: bool,
    pub text: String,
    /// Номер страницы после нормализации (0-based).
    pub page: u32,
    pub pages: u32,
    /// Срез текущей страницы: [start, end).
    pub start: usize,
    pub end: usize,
}

/// Нарезать страницу и собрать полосу пагинации.
///
/// `cols` считает [`ui_base::grid_columns`] по живой ширине — здесь только
/// арифметика страниц (тот же движок, что у остальных сеток продукта).
pub fn bg_pager(shown: usize, page: u32, cols: u32) -> PagerView {
    let per_page = ui_base::grid_page_size(cols, BG_ROWS);
    let pages = ui_base::grid_page_count(shown as u32, per_page);
    let page = page.min(pages.saturating_sub(1));
    let start = (page as usize) * (per_page as usize);
    let end = (start + per_page as usize).min(shown);
    PagerView {
        visible: shown > per_page as usize,
        prev_enabled: page > 0,
        next_enabled: page + 1 < pages,
        // TRANSLATORS: pager: page %1 of %2.
        text: trf("Page %1 / %2", &[&(page + 1), &pages]),
        page,
        pages,
        start: start.min(shown),
        end,
    }
}

/// Номер страницы при смене числа столбцов: что было первым на экране, тем и
/// остаётся (`grid_page_for_index` оригинала).
pub fn bg_page_after_resize(page: u32, old_cols: u32, new_cols: u32) -> u32 {
    let first = page * ui_base::grid_page_size(old_cols, BG_ROWS);
    ui_base::grid_page_for_index(first, ui_base::grid_page_size(new_cols, BG_ROWS))
}

/// Формат применения фона обоями по расширению файла.
///
/// webm/mp4 → видео-обои, gif → анимированные, остальное → статичная картинка.
pub fn apply_format(path: &str) -> &'static str {
    match splitext_lower(path).as_str() {
        ".webm" | ".mp4" => "video",
        ".gif" => "animated",
        _ => "static",
    }
}

// ============================================================================
// Аккаунт Steam, инвентарь и кэши
// ============================================================================

/// Смещение SteamID64: accountid (имя папки `userdata/<id>`) + это число.
pub const STEAMID64_BASE: i64 = 76561197960265728;

/// User-Agent запросов вкладки.
pub const USER_AGENT: &str = "Rexon/5.0";

/// Таймаут страницы инвентаря, секунд (`_fetch_inventory_pages`).
pub const INVENTORY_TIMEOUT_S: u64 = 25;
/// Потолок числа страниц инвентаря — защита от бесконечного `more_items`.
pub const INVENTORY_MAX_PAGES: u32 = 20;
/// Таймаут скачивания полноразмерного фона, секунд.
pub const BG_DOWNLOAD_TIMEOUT_S: u64 = 60;
/// Таймаут скачивания миниатюры, секунд.
pub const THUMB_DOWNLOAD_TIMEOUT_S: u64 = 30;

/// SteamID64 по имени папки `userdata/<accountid>`.
///
/// `None` — имя не число, либо это служебная «0».
pub fn steamid64_from_accountid(accountid: &str) -> Option<String> {
    if accountid.is_empty() || accountid == "0" || !accountid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let id: i64 = accountid.parse().ok()?;
    Some((id + STEAMID64_BASE).to_string())
}

/// Выбор аккаунта: тот, чей `localconfig.vdf` свежее (последний входивший).
///
/// Вход — пары «имя папки → время правки localconfig». Отсутствующие файлы
/// исполнитель просто не кладёт в список.
pub fn pick_account(entries: &[(String, f64)]) -> Option<String> {
    let mut best: Option<(&str, f64)> = None;
    for (sid, mtime) in entries {
        if sid == "0" || !sid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        match best {
            Some((_, bm)) if *mtime <= bm => {}
            _ => best = Some((sid.as_str(), *mtime)),
        }
    }
    best.and_then(|(sid, _)| steamid64_from_accountid(sid))
}

/// Язык ответов Steam (`l=` инвентаря, `language=` WebAPI) по языку
/// интерфейса Rexon: названия предметов, типы и метки Steam отдаёт уже
/// переведёнными, и просить их надо на том языке, на каком говорит вкладка.
///
/// Прежде язык был прибит: `l=russian`, — и английская вкладка показывала бы
/// русские метки инвентаря. Коды — из списка языков Steam Web API; язык,
/// которого у Steam нет, — английский.
pub fn steam_api_language(ui_language: &str) -> &'static str {
    let lang = ui_language.to_ascii_lowercase().replace('-', "_");
    let base = lang.split(['_', '.', '@']).next().unwrap_or("");
    match (base, lang.as_str()) {
        ("pt", l) if l.starts_with("pt_br") => "brazilian",
        ("es", l) if l.starts_with("es_") && !l.starts_with("es_es") => "latam",
        ("zh", l) if l.starts_with("zh_tw") || l.starts_with("zh_hk") => "tchinese",
        ("zh", _) => "schinese",
        ("ru", _) => "russian",
        ("uk", _) => "ukrainian",
        ("de", _) => "german",
        ("fr", _) => "french",
        ("es", _) => "spanish",
        ("it", _) => "italian",
        ("pt", _) => "portuguese",
        ("pl", _) => "polish",
        ("cs", _) => "czech",
        ("nl", _) => "dutch",
        ("da", _) => "danish",
        ("fi", _) => "finnish",
        ("nb" | "nn" | "no", _) => "norwegian",
        ("sv", _) => "swedish",
        ("hu", _) => "hungarian",
        ("ro", _) => "romanian",
        ("bg", _) => "bulgarian",
        ("el", _) => "greek",
        ("tr", _) => "turkish",
        ("th", _) => "thai",
        ("vi", _) => "vietnamese",
        ("id", _) => "indonesian",
        ("ja", _) => "japanese",
        ("ko", _) => "koreana",
        _ => "english",
    }
}

/// Язык ответов Steam для текущего языка интерфейса.
pub fn steam_language() -> &'static str {
    steam_api_language(&i18n::current_language())
}

/// Адрес одной страницы публичного инвентаря (753/6).
///
/// `start` пустой — первая страница; иначе `start_assetid` из прошлого ответа.
/// Язык ответа — язык интерфейса ([`steam_language`]).
pub fn inventory_page_url(sid: &str, start: &str) -> String {
    let base = format!(
        "https://steamcommunity.com/inventory/{sid}/753/6?l={}&count=2000",
        steam_language()
    );
    if start.is_empty() {
        base
    } else {
        format!("{base}&start_assetid={}", url_quote(start))
    }
}

/// Курсор следующей страницы инвентаря: `None` — страниц больше нет.
///
/// Пустой `last_assetid` при `more_items` тоже означает конец — иначе цикл
/// вечно перезапрашивал бы первую страницу.
pub fn inventory_next_start(page: &Value) -> Option<String> {
    let more = page.get("more_items").map(truthy).unwrap_or(false);
    if !more {
        return None;
    }
    let last = page
        .get("last_assetid")
        .map(|v| match v {
            Value::String(x) => x.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default();
    if last.is_empty() {
        None
    } else {
        Some(last)
    }
}

/// Слить страницы инвентаря в один ответ (`assets` + `descriptions`).
pub fn merge_inventory_pages(pages: &[Value]) -> Value {
    let mut assets: Vec<Value> = Vec::new();
    let mut descs: Vec<Value> = Vec::new();
    for p in pages {
        if let Some(a) = p.get("assets").and_then(Value::as_array) {
            assets.extend(a.iter().cloned());
        }
        if let Some(d) = p.get("descriptions").and_then(Value::as_array) {
            descs.extend(d.iter().cloned());
        }
    }
    let mut m = Map::new();
    m.insert("assets".into(), Value::Array(assets));
    m.insert("descriptions".into(), Value::Array(descs));
    Value::Object(m)
}

/// Локализованное значение метки предмета (или пустая строка).
pub fn item_tag(desc: &Value, category: &str) -> String {
    let Some(tags) = desc.get("tags").and_then(Value::as_array) else {
        return String::new();
    };
    for t in tags {
        if s(t, "category").to_lowercase() == category {
            return s(t, "localized_tag_name").to_string();
        }
    }
    String::new()
}

/// Метки предмета в том же составе, что показывает инвентарь Steam:
/// редкость, игра/событие, тип предмета, запреты обмена и продажи.
pub fn item_labels(desc: &Value) -> Vec<String> {
    let mut out = vec![
        item_tag(desc, "droprate"),
        item_tag(desc, "game"),
        item_tag(desc, "item_class"),
    ];
    if !desc.get("tradable").map(truthy).unwrap_or(false) {
        // TRANSLATORS: Steam inventory label, as Steam writes it.
        out.push(tr("Not Tradable"));
    }
    if !desc.get("marketable").map(truthy).unwrap_or(false) {
        // TRANSLATORS: Steam inventory label, as Steam writes it.
        out.push(tr("Not Marketable"));
    }
    let mut uniq: Vec<String> = Vec::new();
    for x in out {
        if !x.is_empty() && !uniq.contains(&x) {
            uniq.push(x);
        }
    }
    uniq
}

/// `internal_name` предмета «видеозаставка».
pub const SPLASH_ITEM_CLASS: &str = "item_class_17";

/// appid игры-источника → метки видеозаставки из инвентаря.
///
/// `market_fee_app` указывает на игру (или на страницу события — тогда «Игра»
/// в метках и есть название распродажи, ровно как в инвентаре Steam).
pub fn splash_item_meta(descs: &[Value]) -> MetaMap {
    let mut out = MetaMap::new();
    for d in descs {
        let is_splash = d
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| tags.iter().any(|t| s(t, "internal_name") == SPLASH_ITEM_CLASS))
            .unwrap_or(false);
        if !is_splash {
            continue;
        }
        let appid = match d.get("market_fee_app") {
            Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
            Some(Value::String(x)) => x.parse::<i64>().unwrap_or(0),
            _ => 0,
        };
        if appid == 0 {
            continue;
        }
        out.insert(
            appid,
            SplashMeta {
                name: s(d, "name").to_string(),
                source: s(d, "type").to_string(),
                event: item_tag(d, "event"),
                labels: item_labels(d),
            },
        );
    }
    out
}

/// Разобрать дисковый кэш меток заставок (`appid` строкой → метки).
pub fn parse_meta_cache(data: &Value) -> MetaMap {
    let mut out = MetaMap::new();
    let Some(obj) = data.as_object() else {
        return out;
    };
    for (k, v) in obj {
        if k.is_empty() || !k.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(appid) = k.parse::<i64>() else { continue };
        out.insert(
            appid,
            SplashMeta {
                name: s(v, "name").to_string(),
                source: s(v, "source").to_string(),
                event: s(v, "event").to_string(),
                labels: v
                    .get("labels")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default(),
            },
        );
    }
    out
}

/// Содержимое дискового кэша меток заставок (то, что писал `json.dump`).
pub fn meta_cache_json(meta: &MetaMap) -> String {
    let mut m = Map::new();
    for (appid, v) in meta {
        m.insert(
            appid.to_string(),
            serde_json::to_value(v).unwrap_or(Value::Null),
        );
    }
    serde_json::to_string(&Value::Object(m)).unwrap_or_else(|_| "{}".into())
}

// ── Надетый фон из localconfig.vdf ──────────────────────────────────────

/// Ключ, под которым Steam кэширует ответ `GetEquippedProfileItemsForUser`.
const EQUIPPED_KEY_PREFIX: &str = "GetEquippedProfileItemsForUser";

/// Выемка JSON надетых предметов профиля из текста `localconfig.vdf`.
///
/// Порт регулярки `"GetEquippedProfileItemsForUser\d+"\s*"(.*?)"\s*[\r\n]` с
/// последующим un-escape VDF. Регулярного движка в ядре нет и заводить его
/// ради одного места незачем — разбор построчный и делает ровно то же:
/// находит ключ, берёт значение до кавычки, за которой (через пробелы) стоит
/// перевод строки, и снимает экранирование.
pub fn extract_equipped_json(vdf_text: &str) -> Option<String> {
    let bytes = vdf_text.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = vdf_text[from..].find(EQUIPPED_KEY_PREFIX) {
        let key_start = from + rel;
        // Ключ обязан быть в кавычках: `"GetEquippedProfileItemsForUser<цифры>"`.
        if key_start == 0 || bytes[key_start - 1] != b'"' {
            from = key_start + EQUIPPED_KEY_PREFIX.len();
            continue;
        }
        let mut i = key_start + EQUIPPED_KEY_PREFIX.len();
        let digits_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == digits_start || i >= bytes.len() || bytes[i] != b'"' {
            from = key_start + EQUIPPED_KEY_PREFIX.len();
            continue;
        }
        i += 1; // закрывающая кавычка ключа
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'"' {
            from = key_start + EQUIPPED_KEY_PREFIX.len();
            continue;
        }
        i += 1; // открывающая кавычка значения
        let val_start = i;
        // Не-жадный `(.*?)"\s*[\r\n]`: первая кавычка, за которой через
        // пробелы идёт перевод строки.
        let mut j = val_start;
        while j < bytes.len() {
            if bytes[j] == b'"' {
                let mut k = j + 1;
                while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
                    k += 1;
                }
                if k < bytes.len() && (bytes[k] == b'\r' || bytes[k] == b'\n') {
                    return Some(unescape_vdf(&vdf_text[val_start..j]));
                }
            }
            j += 1;
        }
        from = key_start + EQUIPPED_KEY_PREFIX.len();
    }
    None
}

/// `raw.replace('\\"', '"').replace('\\\\', '\\')` оригинала.
fn unescape_vdf(raw: &str) -> String {
    raw.replace("\\\"", "\"").replace("\\\\", "\\")
}

// ── Пути кэшей и конфигов ───────────────────────────────────────────────

/// Корень пользовательского кэша: `$XDG_CACHE_HOME` либо `~/.cache`.
///
/// Починка №1: оригинал вшивал `~/.cache` литералом. Тот же приём, что у
/// [`crate::session_splash::preview_cache_dir`].
pub fn cache_home(xdg_cache_home: Option<&str>, home: &str) -> String {
    match xdg_cache_home {
        Some(v) if !v.is_empty() => v.trim_end_matches('/').to_string(),
        _ => format!("{}/.cache", home.trim_end_matches('/')),
    }
}

/// Каталог кэша вкладки: `<кэш>/rexon`.
pub fn rexon_cache_dir(xdg_cache_home: Option<&str>, home: &str) -> String {
    format!("{}/rexon", cache_home(xdg_cache_home, home))
}

/// Кэш скачанных полноразмерных фонов профиля.
pub fn bg_cache_dir(xdg_cache_home: Option<&str>, home: &str) -> String {
    format!("{}/steam_profile_bg", rexon_cache_dir(xdg_cache_home, home))
}

/// Кэш превью-миниатюр сетки фонов. Отдельно от [`bg_cache_dir`]: миниатюры
/// мелкие и общие для всех.
pub fn thumb_cache_dir(xdg_cache_home: Option<&str>, home: &str) -> String {
    format!("{}/steam_profile_thumb", rexon_cache_dir(xdg_cache_home, home))
}

/// Дисковый кэш меток видеозаставок для аккаунта.
pub fn splash_meta_cache_path(xdg_cache_home: Option<&str>, home: &str, sid: &str) -> String {
    format!(
        "{}/steam_splash_items_{}.json",
        rexon_cache_dir(xdg_cache_home, home),
        safe_name(sid)
    )
}

/// Дисковый кэш СПИСКА инвентаря фонов для аккаунта.
pub fn inventory_cache_path(xdg_cache_home: Option<&str>, home: &str, sid: &str) -> String {
    format!(
        "{}/steam_profile_inventory_{}.json",
        rexon_cache_dir(xdg_cache_home, home),
        safe_name(sid)
    )
}

/// Конфиг «какой фон профиля сейчас применён обоями».
///
/// Починка №1: оригинал раскрывал `~/.config/rexon/…` через `expand_user`,
/// который `$XDG_CONFIG_HOME` уважает, — здесь то же самое явным вызовом
/// [`crate::utils::config_home`].
pub fn applied_config_path(xdg_config_home: Option<&str>, home: &str) -> String {
    format!(
        "{}/rexon/steam_profile_bg.json",
        utils::config_home(xdg_config_home, home)
    )
}

/// Имя файла кэша полноразмерного фона (`<id><расширение адреса>`).
pub fn bg_cache_file_name(id: &str, url: &str) -> String {
    format!("{}{}", id, url_ext_or_jpg(url))
}

/// Имя файла кэша миниатюры. `id` может содержать «/» (фолбэк-ключ из имени) —
/// обезопасим имя файла.
pub fn thumb_cache_file_name(id: &str, url: &str) -> String {
    format!("{}{}", safe_name(id), url_ext_or_jpg(url))
}

/// Временный файл скачивания. Починка №3: убирать его надо на ЛЮБОМ исходе,
/// иначе кэш зарастает недокачанными `.part`.
pub fn part_path(path: &str) -> String {
    format!("{path}.part")
}

// ── Дисковый кэш списка инвентаря ───────────────────────────────────────

/// Содержимое файла кэша списка инвентаря.
pub fn inventory_cache_json(sid: &str, items: &[ProfileBackgroundItem], ts: i64) -> String {
    let mut m = Map::new();
    m.insert("ts".into(), Value::from(ts));
    m.insert("sid".into(), Value::from(sid));
    m.insert(
        "items".into(),
        serde_json::to_value(items).unwrap_or(Value::Array(Vec::new())),
    );
    serde_json::to_string(&Value::Object(m)).unwrap_or_else(|_| "{}".into())
}

/// Разобрать файл кэша списка инвентаря.
///
/// `None` — кэша нет, он битый, пустой либо принадлежит другому аккаунту.
pub fn parse_inventory_cache(data: &Value, sid: &str) -> Option<Vec<ProfileBackgroundItem>> {
    let obj = data.as_object()?;
    let cached_sid = match obj.get("sid") {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    if cached_sid != sid {
        return None;
    }
    let items = obj.get("items")?.as_array()?;
    if items.is_empty() {
        return None;
    }
    Some(items.iter().map(parse_cached_item).collect())
}

/// Один предмет из кэша. Недостающие поля — пустые: кэш мог быть записан
/// прежней версией продукта, и ронять из-за этого витрину нельзя.
fn parse_cached_item(v: &Value) -> ProfileBackgroundItem {
    ProfileBackgroundItem {
        id: s(v, "id").to_string(),
        name: s(v, "name").to_string(),
        game: s(v, "game").to_string(),
        url: s(v, "url").to_string(),
        preview: s(v, "preview").to_string(),
        animated: v.get("animated").map(truthy).unwrap_or(false),
        equipped: v.get("equipped").map(truthy).unwrap_or(false),
    }
}

/// Содержимое конфига «что применено» (с отступами, как `json.dump(indent=2)`).
pub fn applied_config_json(id: &str, name: &str) -> String {
    let mut m = Map::new();
    m.insert("id".into(), Value::from(id));
    m.insert("name".into(), Value::from(name));
    serde_json::to_string_pretty(&Value::Object(m)).unwrap_or_else(|_| "{}".into())
}

/// id применённого фона из конфига (пустая строка = нет).
pub fn parse_applied_config(data: &Value) -> String {
    match data.get("id") {
        Some(Value::String(x)) => x.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

// ============================================================================
// Стили: QSS для виджетной вкладки и голые цвета для QML
// ============================================================================

pub const BADGE_NEUTRAL: &str = "neutral";
pub const BADGE_ACCENT: &str = "accent";
pub const BADGE_GOLD: &str = "gold";

/// Акцент вкладки — тот же голубой, что у основных кнопок продукта.
pub const ACCENT_RGB: (u8, u8, u8) = (59, 130, 246);
/// Зелёный «применено».
pub const APPLIED_RGB: (u8, u8, u8) = (34, 197, 94);
/// Золотой «в избранном».
pub const GOLD_RGB: (u8, u8, u8) = (234, 179, 8);

/// `rgba(r,g,b,a)` из тройки и альфы — ровно та запись, что стоит в QSS.
///
/// Альфа передаётся СТРОКОЙ, а не числом: у оригинала она набрана литералами
/// (`0.85`, `1.0`, `0.90`), и любое переформатирование через `%g` сделало бы
/// таблицу стилей другой строкой при том же цвете. Сверка сравнивает строки.
fn rgba(c: (u8, u8, u8), a: &str) -> String {
    format!("rgba({},{},{},{})", c.0, c.1, c.2, a)
}

/// `#rrggbb` из тройки — та же краска для QML, который QSS не понимает.
pub fn hex(c: (u8, u8, u8)) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// Поля группы-секции: слева/справа, сверху/снизу и зазор между строками.
pub const GROUP_MARGIN_H: i32 = 16;
pub const GROUP_MARGIN_V: i32 = 14;
pub const GROUP_SPACING: i32 = 8;
/// Скругление группы-секции.
pub const GROUP_RADIUS: i32 = 6;

/// Пара «фон, рамка» группы-секции. Одно место на обе стороны: QSS для
/// виджетов собирается тут же ([`group_qss`]), голые цвета уходят в QML.
pub fn group_colors(dark: bool) -> (String, String) {
    if dark {
        (
            "rgba(255,255,255,0.04)".to_string(),
            "rgba(255,255,255,0.12)".to_string(),
        )
    } else {
        (
            "rgba(0,0,0,0.03)".to_string(),
            "rgba(0,0,0,0.15)".to_string(),
        )
    }
}

/// QSS группы-секции (theme-aware, сброс рамки ТОЛЬКО для QLabel — правило #9).
pub fn group_qss(name: &str, dark: bool) -> String {
    let sel = format!("QFrame#{name}");
    let (bg, border) = group_colors(dark);
    format!(
        "{sel} {{ background-color: {bg}; border: 1px solid {border}; \
border-radius: 6px; }} {sel} QLabel {{ border: none; background: transparent; }}"
    )
}

/// Цвет текста на «главной» и зелёной кнопках.
pub const BTN_TEXT_ON_FILL: &str = "white";
/// Скругление «главной» кнопки.
pub const BTN_RADIUS: i32 = 4;

/// Четвёрка «фон, фон под курсором, фон выключенной, текст» главной кнопки.
pub fn primary_btn_colors() -> (String, String, String, String) {
    (
        rgba(ACCENT_RGB, "0.85"),
        rgba(ACCENT_RGB, "1.0"),
        "rgba(120,120,120,0.35)".to_string(),
        BTN_TEXT_ON_FILL.to_string(),
    )
}

/// Тройка «фон, фон под курсором, текст» зелёной кнопки «✓ Применено».
pub fn applied_btn_colors() -> (String, String, String) {
    (
        rgba(APPLIED_RGB, "0.90"),
        rgba(APPLIED_RGB, "1.0"),
        BTN_TEXT_ON_FILL.to_string(),
    )
}

/// QSS основной кнопки.
pub fn primary_btn_qss() -> String {
    let (bg, hover, disabled, fg) = primary_btn_colors();
    format!(
        "QPushButton {{ background-color: {bg}; color: {fg}; border: none; \
border-radius: 4px; padding: 6px 14px; font-weight: 600; }} \
QPushButton:hover {{ background-color: {hover}; }} \
QPushButton:disabled {{ background-color: {disabled}; }}"
    )
}

/// QSS зелёной кнопки «✓ Применено».
pub fn applied_btn_qss() -> String {
    let (bg, hover, fg) = applied_btn_colors();
    format!(
        "QPushButton {{ background-color: {bg}; color: {fg}; border: none; \
border-radius: 4px; padding: 6px 14px; font-weight: 600; }} \
QPushButton:hover {{ background-color: {hover}; }}"
    )
}

/// Скругление карточки витрины и её внутренние поля.
pub const CARD_RADIUS: i32 = 6;
pub const CARD_MARGIN: i32 = 8;
pub const CARD_SPACING: i32 = 6;
/// Зазор между карточками в сетке.
pub const CARD_GAP: i32 = 10;

/// Тройка «фон, рамка, рамка под курсором» карточки витрины.
pub fn card_colors(selected: bool, dark: bool) -> (String, String, String) {
    // Заливка — общая поверхность продукта (`ui_base::CARD_FILL_*`): той же
    // записью красит карточку и QML, только там ядро отдаёт её уже
    // смешанной с фоном страницы (`surface_card`). Две копии литерала
    // разошлись бы молча.
    let (mut bg, mut border, mut hover) = if dark {
        (
            ui_base::CARD_FILL_DARK.to_string(),
            "rgba(255,255,255,0.12)".to_string(),
            "rgba(255,255,255,0.30)".to_string(),
        )
    } else {
        (
            ui_base::CARD_FILL_LIGHT.to_string(),
            "rgba(0,0,0,0.15)".to_string(),
            "rgba(0,0,0,0.35)".to_string(),
        )
    };
    if selected {
        bg = rgba(ACCENT_RGB, "0.18");
        border = rgba(ACCENT_RGB, "0.85");
        hover = rgba(ACCENT_RGB, "1.0");
    }
    (bg, border, hover)
}

/// QSS карточки витрины — ЕДИНЫЙ для обеих подвкладок (правило #12).
pub fn card_qss(name: &str, selected: bool, dark: bool) -> String {
    let sel = format!("QFrame#{name}");
    let (bg, border, hover) = card_colors(selected, dark);
    format!(
        "{sel} {{ background-color: {bg}; border: 1px solid {border}; \
border-radius: 6px; }} {sel}:hover {{ border-color: {hover}; }} \
{sel} QLabel {{ border: none; background: transparent; }}"
    )
}

/// Пара «фон, текст» бейджа-пилюли.
pub fn badge_colors(tone: &str, dark: bool) -> (String, String) {
    match tone {
        BADGE_ACCENT => (rgba(ACCENT_RGB, "0.85"), "white".to_string()),
        BADGE_GOLD => (rgba(GOLD_RGB, "0.85"), "#1a1a1a".to_string()),
        _ => (
            ui_base::neutral_rgba(0.14),
            ui_base::primary_text_color(dark).to_string(),
        ),
    }
}

/// QSS бейджа-«пилюли» на карточке.
pub fn badge_qss(tone: &str, dark: bool) -> String {
    let (bg, fg) = badge_colors(tone, dark);
    format!(
        "QLabel {{ background-color: {bg}; color: {fg}; border-radius: 7px; \
padding: 1px 7px; font-size: 11px; }}"
    )
}

/// Тройка «фон, рамка, текст» крупного чипа состояния.
pub fn chip_colors(tone: &str, dark: bool) -> (String, String, String) {
    match tone {
        CHIP_OK => (
            rgba(APPLIED_RGB, "0.16"),
            rgba(APPLIED_RGB, "0.65"),
            rgba(APPLIED_RGB, "1.0"),
        ),
        CHIP_WARN => (
            rgba(GOLD_RGB, "0.16"),
            rgba(GOLD_RGB, "0.65"),
            rgba(GOLD_RGB, "1.0"),
        ),
        _ => (
            ui_base::neutral_rgba(0.08),
            ui_base::neutral_rgba(0.22),
            ui_base::secondary_text_color(dark).to_string(),
        ),
    }
}

/// QSS крупного чипа итогового состояния.
pub fn chip_qss(tone: &str, dark: bool) -> String {
    let (bg, border, fg) = chip_colors(tone, dark);
    format!(
        "QLabel {{ background-color: {bg}; border: 1px solid {border}; \
color: {fg}; border-radius: 12px; padding: 5px 12px; font-weight: 600; }}"
    )
}

/// Четвёрка «рамка, фон выбранного, рамка выбранного, фон под курсором»
/// чипа-переключателя.
pub fn switch_chip_colors() -> (String, String, String, String) {
    (
        ui_base::neutral_rgba(0.20),
        rgba(ACCENT_RGB, "0.4"),
        rgba(ACCENT_RGB, "0.6"),
        ui_base::neutral_rgba(0.14),
    )
}

/// QSS чипа-переключателя (подвкладки и фильтр витрины выглядят одинаково).
pub fn switch_chip_qss() -> String {
    let (border, checked_bg, checked_border, hover_bg) = switch_chip_colors();
    format!(
        "QToolButton {{ padding: 4px 10px; border-radius: 4px; \
border: 1px solid {border}; }}QToolButton:checked {{ background-color: \
{checked_bg}; border-color: {checked_border}; }}QToolButton:hover:!checked \
{{ background-color: {hover_bg}; }}"
    )
}

/// Цвет звёздочки «в избранное».
pub fn star_color(on: bool) -> String {
    if on {
        rgba(GOLD_RGB, "1.0")
    } else {
        ui_base::neutral_rgba(0.45)
    }
}

/// QSS звёздочки — прозрачная кнопка без рамки.
pub fn star_btn_qss(on: bool) -> String {
    format!(
        "QToolButton {{ background: transparent; border: none; color: {}; \
font-size: 15px; padding: 0px; }} QToolButton:hover {{ color: {}; }}",
        star_color(on),
        rgba(GOLD_RGB, "1.0")
    )
}

/// Ширина цветной полосы слева у тезиса-врезки и её поля.
pub const PITCH_BAR_W: i32 = 3;
pub const PITCH_RADIUS: i32 = 4;
pub const PITCH_MARGIN_H: i32 = 12;
pub const PITCH_MARGIN_V: i32 = 8;

/// Пара «фон, полоса слева» тезиса-врезки над витриной заставок.
pub fn pitch_colors() -> (String, String) {
    (rgba(ACCENT_RGB, "0.12"), rgba(ACCENT_RGB, "0.8"))
}

/// Подложка миниатюры карточки витрины заставок (за кадром-превью).
pub const SV_THUMB_BG: &str = "rgba(0,0,0,0.45)";
/// Подложка миниатюры карточки фона профиля (за картинкой).
pub const BG_THUMB_BG: &str = "rgba(128,128,128,0.18)";
/// Скругление миниатюры на карточке.
pub const THUMB_RADIUS: i32 = 3;
/// Скругление крупной превьюшки в карточке выбранного ролика.
pub const HERO_RADIUS: i32 = 5;
/// Подложка крупной превьюшки.
pub const HERO_THUMB_BG: &str = "rgba(0,0,0,0.45)";

/// QSS тезиса-врезки над витриной заставок.
pub fn pitch_qss() -> String {
    let (bg, bar) = pitch_colors();
    format!(
        "QLabel {{ background-color: {bg}; border-left: 3px solid {bar}; \
border-radius: 4px; padding: 8px 12px; }}"
    )
}

// ============================================================================
// Мелочи разбора
// ============================================================================

/// Python-истинность значения JSON (пустая строка/0/пустой список — ложь).
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `os.path.splitext(path)[1].lower()`.
fn splitext_lower(path: &str) -> String {
    let base = path.rsplit('/').next().unwrap_or(path);
    match base.rfind('.') {
        // Ведущая точка — не расширение (`.bashrc`), как и у Python.
        Some(0) | None => String::new(),
        Some(i) => base[i..].to_lowercase(),
    }
}

/// Расширение из пути АДРЕСА (без query и fragment) либо `.jpg`.
pub fn url_ext_or_jpg(url: &str) -> String {
    let no_frag = url.split('#').next().unwrap_or(url);
    let path = no_frag.split('?').next().unwrap_or(no_frag);
    // Схема и хост нам не мешают: расширение ищется в последнем сегменте.
    let ext = splitext_lower(path);
    if ext.is_empty() {
        ".jpg".to_string()
    } else {
        ext
    }
}

/// `re.sub(r"[^\w.-]", "_", value)` — безопасное имя файла.
///
/// `\w` питоновской строки — буквы, цифры и подчёркивание в юникоде; здесь то
/// же самое через `char::is_alphanumeric`.
pub fn safe_name(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `urllib.parse.quote` для значения курсора инвентаря (там только цифры, но
/// экранируем честно — курсор приходит от Steam, а не от нас).
fn url_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn movie(path: &str, title: &str, game: &str, origin: &str, appid: i64) -> Value {
        serde_json::json!({
            "path": path,
            "title": title,
            "name": title,
            "game": game,
            "origin": origin,
            "origin_label": session_splash::origin_label(origin),
            "appid": appid,
        })
    }

    #[test]
    fn equipped_json_is_extracted_and_unescaped() {
        let vdf = "\t\t\t\t\"GetEquippedProfileItemsForUser76561198000000000\"\t\t\"{\\\"response\\\":{\\\"profile_background\\\":{\\\"image_large\\\":\\\"a.jpg\\\"}}}\"\n";
        let raw = extract_equipped_json(vdf).expect("значение должно найтись");
        let parsed: Value = serde_json::from_str(&raw).expect("валидный JSON");
        assert_eq!(
            parsed["response"]["profile_background"]["image_large"],
            Value::from("a.jpg")
        );
    }

    #[test]
    fn account_pick_takes_freshest_localconfig() {
        let entries = vec![
            ("0".to_string(), 999.0),
            ("111".to_string(), 10.0),
            ("222".to_string(), 20.0),
            ("не_число".to_string(), 100.0),
        ];
        assert_eq!(
            pick_account(&entries).as_deref(),
            Some((222 + STEAMID64_BASE).to_string().as_str())
        );
    }

    #[test]
    fn group_chips_hidden_when_no_choice() {
        let items = vec![movie("/a.webm", "A", "Игра", session_splash::ORIGIN_SHOP, 1)];
        let meta = MetaMap::new();
        assert!(group_chips(&items, &[], &meta, "").is_empty());
        // Одна группа, но есть избранное — выбор появился.
        let favs = vec!["/a.webm".to_string()];
        assert_eq!(group_chips(&items, &favs, &meta, "").len(), 3);
    }

    #[test]
    fn pager_slices_and_labels() {
        let p = bg_pager(202, 0, 5);
        assert_eq!((p.start, p.end), (0, 25));
        assert_eq!(p.pages, 9);
        assert!(!p.prev_enabled && p.next_enabled && p.visible);
        // Страница за пределами списка подтягивается к последней.
        let p = bg_pager(202, 99, 5);
        assert_eq!(p.page, 8);
        assert_eq!((p.start, p.end), (200, 202));
        assert!(!p.next_enabled);
    }

    #[test]
    fn apply_format_by_extension() {
        assert_eq!(apply_format("/x/a.WEBM"), "video");
        assert_eq!(apply_format("/x/a.mp4"), "video");
        assert_eq!(apply_format("/x/a.gif"), "animated");
        assert_eq!(apply_format("/x/a.jpg"), "static");
        assert_eq!(apply_format("/x/noext"), "static");
    }

    #[test]
    fn url_ext_ignores_query() {
        assert_eq!(url_ext_or_jpg("https://h/a/b.PNG?x=1"), ".png");
        assert_eq!(url_ext_or_jpg("https://h/a/b"), ".jpg");
    }

    // ── «≥2 видео»: один порог на витрину и на жребий при входе ─────────

    #[test]
    fn rotation_threshold_is_one_truth() {
        use session_splash as ss;
        // Витрина отказывает ровно там, где жребий при входе крутить нечего.
        for favs in 0..4usize {
            for all in 0..4usize {
                let fav_paths: Vec<String> = (0..favs).map(|i| format!("/f{i}")).collect();
                let all_paths: Vec<String> = (0..all).map(|i| format!("/a{i}")).collect();
                let pool = ss::rotation_pool(&fav_paths, &all_paths);
                assert_eq!(pool.len(), ss::rotation_pool_size(favs, all), "{favs}/{all}");
                let rotates = !ss::rotation_choices(&pool, None).is_empty();
                assert_eq!(rotates, ss::rotation_possible(favs, all), "{favs}/{all}");
                assert_eq!(rotation_refusal(true, favs, all).is_some(), !rotates, "{favs}/{all}");
                assert_eq!(rotation_refusal(false, favs, all), None);
            }
        }
        assert_eq!(rotation_refusal(true, 0, 1), Some(sv_rotate_need_two()));
        assert_eq!(rotation_refusal(true, 1, 5), None);
    }

    /// Язык сверок модуля — исходный (английский): каталога у юнит-сверок
    /// нет, и `msgid` показывается как есть. Русский «как было» сверяет
    /// `tests/qt_steam_parity.rs` на настоящем `po/ru.po`.
    fn en() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    #[test]
    fn chip_does_not_promise_rotation_that_will_not_happen() {
        en();
        let cur = movie("/a.webm", "Days Gone", "", session_splash::ORIGIN_SHOP, 1);
        assert_eq!(
            state_chip(Some(&cur), true, true, 0, 1),
            ("✅ Ready: the next login plays “Days Gone”".to_string(), CHIP_OK)
        );
        assert!(state_chip(Some(&cur), true, true, 0, 2).0.contains("out of all (2 in the pool)"));
    }

    // ── Отказ автозапуска: простыми словами, на «вы», без устройства ────

    #[test]
    fn autostart_success_says_nothing() {
        assert_eq!(autostart_refusal_text(true, ""), None);
        assert_eq!(autostart_refusal_text(false, "  "), None);
    }

    #[test]
    fn autostart_refusal_names_plain_causes() {
        use session_splash as ss;
        en();
        // Причина «сначала выберите» — из чужого модуля (session_splash),
        // её текст приходит оттуда как есть.
        assert_eq!(
            autostart_refusal_text(true, ss::MSG_CHOOSE_FIRST),
            Some(format!("❌ {}", crate::i18n::tr(ss::MSG_CHOOSE_FIRST)))
        );
        assert_eq!(
            autostart_refusal_text(true, &ss::msg_choose_first()).as_deref(),
            Some("❌ Choose a startup movie first")
        );
        assert_eq!(
            autostart_refusal_text(true, &ss::msg_autostart_write_failed("Permission denied (os error 13)"))
                .as_deref(),
            Some("❌ Couldn't turn on the startup movie at login: no permission to write to the settings folder.")
        );
        // Первая часть системной ошибки — на языке системы; узнаём по номеру.
        assert_eq!(
            autostart_refusal_text(true, "Не удалось включить: Нет места на устройстве (os error 28)")
                .as_deref(),
            Some("❌ Couldn't turn on the startup movie at login: the disk is full.")
        );
        // Python-хозяин отдаёт текст исключения.
        assert_eq!(
            autostart_refusal_text(false, "[Errno 30] Read-only file system: '/x'").as_deref(),
            Some("❌ Couldn't fully turn off the startup movie at login: the settings folder is read-only.")
        );
        assert_eq!(
            autostart_refusal_text(true, &sv_no_reason()).as_deref(),
            Some("❌ Couldn't turn on the startup movie at login. Please try again.")
        );
    }

    #[test]
    fn autostart_refusal_has_no_device_words() {
        use session_splash as ss;
        en();
        let raw = [
            ss::msg_autostart_write_failed("Permission denied (os error 13)"),
            ss::msg_config_write_failed("No such file or directory (os error 2)"),
            ss::hybrid_fallback_message("юнит не встал", false, "Не удалось включить: x"),
            [
                ss::msg_problem_unit("Device or resource busy (os error 16)"),
                ss::msg_problem_ksplashrc(),
                ss::msg_problem_theme("x"),
                ss::msg_problem_autostart("x"),
            ]
            .join("; "),
            ss::msg_rollback_problems(&ss::msg_problem_unit("x")),
        ];
        let jargon = [
            "autostart", "юнит", "ksplash", "kwriteconfig", "os error", "errno", "гибрид",
            "systemd", ".desktop", "конфиг",
        ];
        for on in [true, false] {
            for r in &raw {
                let text = autostart_refusal_text(on, r).expect("отказ обязан быть сказан");
                let low = text.to_lowercase();
                for word in jargon {
                    assert!(!low.contains(word), "«{word}» в тексте покупателю: {text}");
                }
                assert!(text.starts_with("❌ "), "{text}");
            }
        }
    }

    // ── Применение фона: общий исполнитель под подменным швом ───────────

    /// Подменная машина: ничего не делает, только записывает команды.
    #[derive(Default)]
    struct FakeSys {
        files: Vec<String>,
        commands: Vec<Vec<String>>,
        clock: f64,
    }

    impl WpSys for FakeSys {
        fn run(&mut self, argv: &[String], _timeout: f64) -> Option<session_splash::RunOut> {
            self.commands.push(argv.to_vec());
            Some(session_splash::RunOut { rc: 0, stdout: String::new(), stderr: String::new() })
        }
        fn spawn(&mut self, argv: &[String], _new_session: bool) -> Result<i64, String> {
            self.commands.push(argv.to_vec());
            Ok(self.commands.len() as i64)
        }
        fn poll(&mut self, _proc: i64) -> Option<i64> {
            None
        }
        fn pid(&mut self, proc: i64) -> Option<i64> {
            Some(1000 + proc)
        }
        fn kill_process(&mut self, _proc: i64) {}
        fn has_tool(&mut self, _tool: &str) -> bool {
            true
        }
        fn package_hint(&mut self, _tool: &str) -> String {
            String::new()
        }
        fn exists(&mut self, path: &str) -> bool {
            self.files.iter().any(|f| f == path)
        }
        fn is_dir(&mut self, _path: &str) -> bool {
            false
        }
        fn listdir(&mut self, _path: &str) -> Vec<String> {
            Vec::new()
        }
        fn read_text(&mut self, _path: &str) -> Option<String> {
            None
        }
        fn write_text(&mut self, _path: &str, _content: &str) {}
        fn remove_file(&mut self, _path: &str) {}
        fn env(&mut self, name: &str) -> String {
            if name == "HOME" { "/fake-home".into() } else { String::new() }
        }
        fn monotonic(&mut self) -> f64 {
            self.clock += 1.0;
            self.clock
        }
        fn sleep(&mut self, seconds: f64) {
            self.clock += seconds;
        }
        fn bridge_notify(&mut self, _live: bool, _name: &str, _announce: bool) {}
    }

    fn fake_exec(files: &[&str]) -> WallpaperExec<FakeSys> {
        let sys = FakeSys { files: files.iter().map(|f| f.to_string()).collect(), ..FakeSys::default() };
        WallpaperExec::with_env(sys, "sway", "wayland")
    }

    #[test]
    fn background_picture_goes_through_the_executor() {
        let path = "/cache/steam-bg/42.jpg";
        let mut exec = fake_exec(&[path]);
        let _ = apply_background_with(&mut exec, path, true, "Ночной город");
        let cmds = &exec.sys_mut().commands;
        assert!(
            cmds.iter().any(|argv| argv.iter().any(|a| a.contains(path))),
            "картинка должна уйти в план исполнителя: {cmds:?}"
        );
    }

    #[test]
    fn background_video_takes_the_video_route() {
        let path = "/cache/steam-bg/7.webm";
        let mut exec = fake_exec(&[path]);
        // Хоста видео у подменной машины нет — отказ ровно видеоветки
        // исполнителя, а не «движок обоев недоступен».
        assert_eq!(
            apply_background_with(&mut exec, path, false, "Анимированный"),
            (false, crate::wallpaper_engine::video_host_missing_message())
        );
        assert_ne!(crate::wallpaper_engine::video_host_missing_message(), bg_no_engine());
    }

    #[test]
    fn background_missing_file_is_told() {
        let path = "/cache/steam-bg/нет.png";
        let mut exec = fake_exec(&[]);
        assert_eq!(
            apply_background_with(&mut exec, path, true, "x"),
            (false, crate::wallpaper_engine::file_missing_message(path))
        );
        assert!(exec.sys_mut().commands.is_empty(), "без файла ничего не запускается");
    }
}
