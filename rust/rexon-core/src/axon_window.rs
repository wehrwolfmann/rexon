//! Вкладка «Razer Axon»: что показывает библиотека и что в ней называется.
//!
//! Модель вкладки (Qt-сторона) ничего не решает сама: какие обои попадают в
//! сетку, в каком порядке, сколько фильтров включено, какой экран показать,
//! когда показывать нечего, и все подписи — здесь. Читает библиотеку
//! [`crate::axon_library`]; файлы Axon этот модуль не трогает вовсе.
//!
//! Мерило — раздел «Библиотека» самого Razer Axon (2.11.1.0): вкладка чужой
//! программы повторяет оригинал, ничего сверх. Как он устроен (по коду его
//! интерфейса):
//!
//! * обои идут двумя разделами: скачанные (заголовок «Недавно добавленные»)
//!   и «Можно синхронизировать» — те, что в облачной библиотеке учётки, но
//!   не скачаны (`WallPaperStatus` = `Uninstalled`);
//! * сортировка — «по имени» (название без учёта регистра) или «по дате
//!   добавления», и она же по умолчанию. Отдельного поля даты у записи нет
//!   (`FinishedTime` у скачанных — 0): «по дате» Axon оставляет список в том
//!   порядке, в каком его отдаёт библиотека (`WallPaperData`), а новые
//!   скачанные обои он ставит в её начало. Значит, «по дате» = порядок
//!   записей, свежие первыми; так и здесь — порядок [`AxonLibrary`] уже
//!   такой (`read_roots` сохраняет порядок самой свежей учётной записи);
//! * фильтры библиотеки: «Только избранное», «Премиальное», «Награды», «Со
//!   звуком», «ИИ-обои» и группа «Тип» (динамические / статичные /
//!   интерактивные), кнопка «Очистить» и число включённых на кнопке фильтра.
//!   Группы «Разрешение» в библиотеке у Axon нет — она есть только у
//!   каталога магазина, и в отбор библиотеки разрешение не входит;
//! * пусто: при включённом поиске или фильтре — «Обои не найдены.» и ссылка
//!   «Очистить фильтр», иначе — «Нет элементов».
//!
//! ⚠️ Починки оригинала (правило: баги при переносе чиним, не консервируем):
//!   1. поиск: Axon делит строку по пробелам, и двойной пробел даёт пустое
//!      слово, которое совпадает с ЛЮБЫМИ обоями. Пустые слова пропускаем;
//!   2. переключатель поиска «Названия обоев | Теги» в библиотеке Axon ни на
//!      что не влиял: и так и так искало по названию, автору и тегам. Здесь
//!      режим работает: «Названия обоев» — название и автор, «Теги» — теги;
//!   3. фильтр «Тип»: у обоев без `EffectType` (пустая строка) Axon находил
//!      пустую строку в списке выбранных типов и пропускал их при любом
//!      выборе. Без типа обои под фильтр типа не подходят.

use crate::axon_library::{display_title, find_roots, AxonEnv, AxonWallpaper};
use crate::i18n::trc;

// ============================================================================
// Есть ли вкладка
// ============================================================================

/// Показывать ли вкладку «Razer Axon»: да, если в известных местах найден
/// Razer Axon с библиотекой либо человек сам указал папку с его обоями
/// (`env.source_dirs` — из настроек Rexon). Иначе вкладки нет: тому, у кого
/// Axon не стоит, она ни к чему. Указанная папка держит вкладку, даже если
/// пропала, — тогда вкладка говорит, что папка недоступна, а не исчезает
/// молча.
pub fn show_tab(env: &AxonEnv) -> bool {
    !env.source_dirs.is_empty() || !find_roots(env).is_empty()
}

/// [`show_tab`] для живого процесса: окружение и папки из настроек Rexon.
pub fn show_tab_live() -> bool {
    show_tab(&crate::axon_prepare::live_env().0)
}

// Подписи переводятся в контексте «Razer Axon»: строки дословно как у Axon,
// и общий с другими вкладками перевод («Тип», «Статичные»…) их не перебивает.

// ============================================================================
// Запрос
// ============================================================================

/// Порядок сетки.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortOrder {
    /// «Сортировать по дате добавления» — как у Axon по умолчанию.
    #[default]
    DateAdded,
    /// «Сортировать по имени».
    Name,
}

impl SortOrder {
    /// Все пункты меню сортировки — в порядке оригинала.
    pub const ALL: [SortOrder; 2] = [SortOrder::Name, SortOrder::DateAdded];

    pub fn label(self) -> String {
        match self {
            SortOrder::Name => trc("Razer Axon", "Sort by name"),
            SortOrder::DateAdded => trc("Razer Axon", "Sort by date added"),
        }
    }

    /// Ключ для модели и настроек.
    pub fn key(self) -> &'static str {
        match self {
            SortOrder::Name => "name",
            SortOrder::DateAdded => "date",
        }
    }

    pub fn from_key(key: &str) -> SortOrder {
        if key == "name" { SortOrder::Name } else { SortOrder::DateAdded }
    }
}

/// Где искать строку поиска.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchMode {
    /// «Названия обоев»: название и автор.
    #[default]
    Titles,
    /// «Теги».
    Tags,
}

impl SearchMode {
    pub const ALL: [SearchMode; 2] = [SearchMode::Titles, SearchMode::Tags];

    pub fn label(self) -> String {
        match self {
            SearchMode::Titles => trc("Razer Axon", "Wallpaper titles"),
            SearchMode::Tags => trc("Razer Axon", "Tags"),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SearchMode::Titles => "titles",
            SearchMode::Tags => "tags",
        }
    }

    pub fn from_key(key: &str) -> SearchMode {
        if key == "tags" { SearchMode::Tags } else { SearchMode::Titles }
    }
}

/// Пункт группы «Тип» — значение `EffectType` у Axon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    Dynamic,
    Static,
    Interactive,
}

impl EffectKind {
    pub const ALL: [EffectKind; 3] = [EffectKind::Dynamic, EffectKind::Static, EffectKind::Interactive];

    /// Как Axon пишет тип в записи (`EffectType`).
    pub fn axon_value(self) -> &'static str {
        match self {
            EffectKind::Dynamic => "Dynamic",
            EffectKind::Static => "Static",
            EffectKind::Interactive => "Interactive",
        }
    }

    pub fn from_axon(value: &str) -> Option<EffectKind> {
        EffectKind::ALL.into_iter().find(|k| k.axon_value().eq_ignore_ascii_case(value.trim()))
    }

    pub fn label(self) -> String {
        match self {
            EffectKind::Dynamic => trc("Razer Axon", "Dynamic"),
            EffectKind::Static => trc("Razer Axon", "Static"),
            EffectKind::Interactive => trc("Razer Axon", "Interactive"),
        }
    }
}

/// Флажок панели фильтров библиотеки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterFlag {
    /// «Только избранное».
    Favorites,
    /// «Премиальное»: обои за серебро.
    Premiums,
    /// «Награды»: обои с наградой за скачивание.
    Rewards,
    /// «Со звуком».
    AudioEnabled,
    /// «ИИ-обои».
    AiWallpaper,
}

impl FilterFlag {
    /// Флажки в порядке панели оригинала (за ними — группа «Тип»).
    pub const ALL: [FilterFlag; 5] = [
        FilterFlag::Favorites,
        FilterFlag::Premiums,
        FilterFlag::Rewards,
        FilterFlag::AudioEnabled,
        FilterFlag::AiWallpaper,
    ];

    pub fn label(self) -> String {
        match self {
            FilterFlag::Favorites => trc("Razer Axon", "Favorites"),
            FilterFlag::Premiums => trc("Razer Axon", "Premiums"),
            FilterFlag::Rewards => trc("Razer Axon", "Rewards"),
            FilterFlag::AudioEnabled => trc("Razer Axon", "Audio-enabled"),
            FilterFlag::AiWallpaper => trc("Razer Axon", "AI wallpaper"),
        }
    }

    /// Подходят ли обои под флажок — признаки как у Axon: избранное
    /// (`IsFavorite`), серебро больше нуля (`Silver`), награда за скачивание
    /// больше нуля, звук (`Audible` = 1), номер ИИ-творения (`DetailId`).
    pub fn accepts(self, w: &AxonWallpaper) -> bool {
        let d = &w.details;
        match self {
            FilterFlag::Favorites => d.is_favorite,
            FilterFlag::Premiums => d.silver > 0,
            FilterFlag::Rewards => d.download_reward > 0,
            FilterFlag::AudioEnabled => w.audible,
            FilterFlag::AiWallpaper => d.is_ai(),
        }
    }
}

/// Состояние панели фильтров.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    /// Включённые флажки (без повторов).
    pub flags: Vec<FilterFlag>,
    /// Отмеченные пункты «Тип»: подходят обои любого из них.
    pub kinds: Vec<EffectKind>,
}

impl Filters {
    pub fn has(&self, flag: FilterFlag) -> bool {
        self.flags.contains(&flag)
    }

    pub fn has_kind(&self, kind: EffectKind) -> bool {
        self.kinds.contains(&kind)
    }

    pub fn set(&mut self, flag: FilterFlag, on: bool) {
        self.flags.retain(|f| *f != flag);
        if on {
            self.flags.push(flag);
        }
    }

    pub fn set_kind(&mut self, kind: EffectKind, on: bool) {
        self.kinds.retain(|k| *k != kind);
        if on {
            self.kinds.push(kind);
        }
    }

    /// Число на кнопке фильтра: каждый флажок и каждый пункт «Тип» — по
    /// одному, как считает Axon.
    pub fn active_count(&self) -> usize {
        self.flags.len() + self.kinds.len()
    }

    /// Включено ли хоть что-то (кнопка «Очистить» тогда доступна).
    pub fn any(&self) -> bool {
        self.active_count() > 0
    }

    /// «Очистить».
    pub fn clear(&mut self) {
        *self = Filters::default();
    }

    /// Подходят ли обои под все включённые фильтры.
    pub fn accepts(&self, w: &AxonWallpaper) -> bool {
        if !self.flags.iter().all(|f| f.accepts(w)) {
            return false;
        }
        // Починка 3: без `EffectType` под фильтр типа обои не подходят.
        self.kinds.is_empty() || EffectKind::from_axon(&w.effect).is_some_and(|k| self.kinds.contains(&k))
    }
}

/// Всё, что задаёт сетку: строка поиска, её режим, сортировка, фильтры.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    pub mode: SearchMode,
    pub sort: SortOrder,
    pub filters: Filters,
}

impl Query {
    /// Сужает ли запрос список (поиск или фильтр): тогда пустой итог —
    /// «Обои не найдены.», а не «Нет элементов».
    pub fn narrows(&self) -> bool {
        !self.text.trim().is_empty() || self.filters.any()
    }

    /// Ссылка «Очистить фильтр»: снимает фильтры и поиск, сортировку и
    /// режим поиска оставляет.
    pub fn clear_filter(&mut self) {
        self.text.clear();
        self.filters.clear();
    }
}

/// Подходит ли текст поиска: любое из слов строки — часть названия или
/// автора («Названия обоев») либо тегов («Теги»), без учёта регистра.
pub fn matches_text(w: &AxonWallpaper, text: &str, mode: SearchMode) -> bool {
    // Починка 1: пустые слова (двойной пробел) не совпадают со всем подряд.
    let words: Vec<String> = text.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return true;
    }
    // Починка 2: режим поиска действует.
    let hay = match mode {
        SearchMode::Titles => format!("{}\n{}", w.title, w.author),
        SearchMode::Tags => w.tags.clone(),
    }
    .to_lowercase();
    words.iter().any(|word| hay.contains(word.as_str()))
}

/// Ушли ли обои в раздел «Можно синхронизировать»: Axon ставит туда записи
/// облачной библиотеки, которых нет на диске (`Uninstalled`).
pub fn is_available_for_sync(w: &AxonWallpaper) -> bool {
    w.status.trim().eq_ignore_ascii_case("uninstalled")
}

/// Сетка по запросу: номера обоев в `list` по разделам.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryView {
    /// Скачанные (раздел «Недавно добавленные»).
    pub downloaded: Vec<usize>,
    /// «Можно синхронизировать».
    pub available: Vec<usize>,
}

impl LibraryView {
    pub fn is_empty(&self) -> bool {
        self.downloaded.is_empty() && self.available.is_empty()
    }
}

/// Ключ сортировки «по имени»: название, как его видит человек, без
/// учёта регистра.
fn name_key(w: &AxonWallpaper) -> String {
    display_title(w).to_lowercase()
}

/// Отобрать и упорядочить обои библиотеки по запросу.
pub fn query(list: &[AxonWallpaper], q: &Query) -> LibraryView {
    let mut order: Vec<usize> = (0..list.len())
        .filter(|&i| matches_text(&list[i], &q.text, q.mode) && q.filters.accepts(&list[i]))
        .collect();
    if q.sort == SortOrder::Name {
        // Устойчивая: одинаковые названия остаются в порядке добавления.
        order.sort_by_cached_key(|&i| name_key(&list[i]));
    }
    let (available, downloaded) = order.into_iter().partition(|&i| is_available_for_sync(&list[i]));
    LibraryView { downloaded, available }
}

/// Что показать на месте сетки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Сетка: есть что показать.
    Grid,
    /// Поиск или фильтр ничего не нашли: «Обои не найдены.» + «Очистить
    /// фильтр».
    NotFound,
    /// Показывать нечего и ничего не отобрано: «Нет элементов».
    NoItems,
}

pub fn screen(view: &LibraryView, q: &Query) -> Screen {
    if !view.is_empty() {
        Screen::Grid
    } else if q.narrows() {
        Screen::NotFound
    } else {
        Screen::NoItems
    }
}

// ============================================================================
// Подписи — дословно как у Axon
// ============================================================================

/// Подсказка строки поиска.
pub fn search_placeholder() -> String {
    trc("Razer Axon", "Search wallpapers")
}

/// Подсказка кнопки сортировки.
pub fn sort_tooltip() -> String {
    trc("Razer Axon", "Sort")
}

/// Подсказка и заголовок группы «Тип».
pub fn type_label() -> String {
    trc("Razer Axon", "Type")
}

/// Кнопка «Очистить» панели фильтров — заглавными, как у Axon.
pub fn clear_text() -> String {
    trc("Razer Axon", "Clear").to_uppercase()
}

/// Заголовок раздела скачанных обоев — заглавными, как у Axon.
pub fn downloaded_section_title() -> String {
    trc("Razer Axon", "Recently added").to_uppercase()
}

/// Заголовок раздела облачных, ещё не скачанных обоев.
pub fn available_section_title() -> String {
    trc("Razer Axon", "AVAILABLE FOR SYNC")
}

/// Пусто, ничего не отобрано.
pub fn no_items_text() -> String {
    trc("Razer Axon", "No items")
}

/// Поиск или фильтр ничего не нашли.
pub fn not_found_text() -> String {
    trc("Razer Axon", "No wallpapers found.")
}

/// Ссылка рядом с «Обои не найдены.».
pub fn clear_filter_text() -> String {
    trc("Razer Axon", "Clear filter")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::axon_library::{AxonDetails, AxonKind};
    use std::path::PathBuf;

    fn english() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    fn w(id: &str, title: &str) -> AxonWallpaper {
        AxonWallpaper {
            id: id.into(),
            title: title.into(),
            author: format!("Автор {id}"),
            kind: AxonKind::Video,
            effect: "Dynamic".into(),
            audible: false,
            resolution: Some((3840, 2160)),
            cover_url: String::new(),
            cover_local: None,
            resource_dir: None,
            container: None,
            downloaded: true,
            playing: false,
            status: "Installed".into(),
            description: String::new(),
            tags: String::new(),
            details: AxonDetails::default(),
            prefix: PathBuf::new(),
        }
    }

    /// Библиотека в порядке Axon: свежие первыми.
    fn library() -> Vec<AxonWallpaper> {
        let mut a = w("1", "skyline");
        a.tags = " City,Night".into();
        a.audible = true;
        a.details.is_favorite = true;
        let mut b = w("2", "Aurora");
        b.effect = "Static".into();
        b.details.silver = 120;
        let mut c = w("3", "Neon");
        c.status = "Uninstalled".into();
        c.downloaded = false;
        c.details.detail_id = 7;
        c.effect = String::new();
        let mut d = w("4", "Beach");
        d.effect = "Interactive".into();
        d.details.download_reward = 5;
        d.tags = "Sea".into();
        vec![a, b, c, d]
    }

    fn ids(list: &[AxonWallpaper], idx: &[usize]) -> Vec<String> {
        idx.iter().map(|&i| list[i].id.clone()).collect()
    }

    #[test]
    fn date_added_keeps_library_order_and_splits_sections() {
        let lib = library();
        let v = query(&lib, &Query::default());
        assert_eq!(ids(&lib, &v.downloaded), ["1", "2", "4"]);
        assert_eq!(ids(&lib, &v.available), ["3"], "не скачанные — в «Можно синхронизировать»");
        assert_eq!(SortOrder::default(), SortOrder::DateAdded, "по умолчанию — по дате");
    }

    #[test]
    fn name_sort_ignores_case() {
        let lib = library();
        let q = Query { sort: SortOrder::Name, ..Query::default() };
        let v = query(&lib, &q);
        assert_eq!(ids(&lib, &v.downloaded), ["2", "4", "1"], "Aurora, Beach, skyline");
        assert_eq!(SortOrder::from_key(SortOrder::Name.key()), SortOrder::Name);
        assert_eq!(SortOrder::from_key("что-то"), SortOrder::DateAdded);
    }

    #[test]
    fn search_by_titles_and_by_tags() {
        let lib = library();
        let find = |text: &str, mode| {
            let v = query(&lib, &Query { text: text.into(), mode, ..Query::default() });
            ids(&lib, &v.downloaded)
        };
        assert_eq!(find("SKY", SearchMode::Titles), ["1"]);
        assert_eq!(find("автор 4", SearchMode::Titles), ["1", "2", "4"], "любое слово: «автор» есть у всех");
        assert_eq!(find("aurora  beach", SearchMode::Titles), ["2", "4"], "двойной пробел не находит всё");
        assert_eq!(find("night", SearchMode::Titles), Vec::<String>::new(), "теги — не в режиме названий");
        assert_eq!(find("night", SearchMode::Tags), ["1"]);
        assert_eq!(find("sea", SearchMode::Tags), ["4"]);
        assert_eq!(find("   ", SearchMode::Tags), ["1", "2", "4"], "пустой поиск не сужает");
    }

    #[test]
    fn filters_flags_kinds_and_counter() {
        let lib = library();
        let run = |f: &Filters| {
            let v = query(&lib, &Query { filters: f.clone(), ..Query::default() });
            (ids(&lib, &v.downloaded), ids(&lib, &v.available))
        };
        let mut f = Filters::default();
        assert_eq!((f.active_count(), f.any()), (0, false));
        f.set(FilterFlag::Favorites, true);
        assert_eq!(run(&f).0, ["1"]);
        f.set(FilterFlag::Favorites, true);
        assert_eq!(f.active_count(), 1, "повторное включение не считается дважды");
        f.set(FilterFlag::AudioEnabled, true);
        assert_eq!((run(&f).0, f.active_count()), (vec!["1".to_string()], 2));
        f.clear();
        f.set(FilterFlag::Premiums, true);
        assert_eq!(run(&f).0, ["2"]);
        f.clear();
        f.set(FilterFlag::Rewards, true);
        assert_eq!(run(&f).0, ["4"]);
        f.clear();
        f.set(FilterFlag::AiWallpaper, true);
        assert_eq!(run(&f), (vec![], vec!["3".to_string()]), "ИИ-обои — по номеру творения");
        f.clear();
        f.set_kind(EffectKind::Static, true);
        f.set_kind(EffectKind::Interactive, true);
        assert_eq!((run(&f).0, f.active_count()), (vec!["2".to_string(), "4".to_string()], 2));
        assert!(run(&f).1.is_empty(), "без типа под фильтр типа не подходят");
        f.set_kind(EffectKind::Static, false);
        assert_eq!(run(&f).0, ["4"]);
        assert_eq!(EffectKind::from_axon(" dynamic "), Some(EffectKind::Dynamic));
        assert_eq!(EffectKind::from_axon(""), None);
    }

    #[test]
    fn screens_and_clear_filter() {
        let lib = library();
        let mut q = Query { text: "нет таких".into(), sort: SortOrder::Name, mode: SearchMode::Tags, ..Query::default() };
        q.filters.set(FilterFlag::Favorites, true);
        assert_eq!(screen(&query(&lib, &q), &q), Screen::NotFound);
        q.clear_filter();
        assert_eq!((q.text.as_str(), q.filters.any()), ("", false));
        assert_eq!((q.sort, q.mode), (SortOrder::Name, SearchMode::Tags), "сортировка и режим остаются");
        assert_eq!(screen(&query(&lib, &q), &q), Screen::Grid);
        assert_eq!(screen(&query(&[], &q), &q), Screen::NoItems);
    }

    #[test]
    fn labels_are_the_originals() {
        english();
        assert_eq!(SortOrder::ALL.map(SortOrder::label), ["Sort by name", "Sort by date added"]);
        assert_eq!(SearchMode::ALL.map(SearchMode::label), ["Wallpaper titles", "Tags"]);
        assert_eq!(
            FilterFlag::ALL.map(FilterFlag::label),
            ["Favorites", "Premiums", "Rewards", "Audio-enabled", "AI wallpaper"]
        );
        assert_eq!(EffectKind::ALL.map(EffectKind::label), ["Dynamic", "Static", "Interactive"]);
        assert_eq!(clear_text(), "CLEAR");
        assert_eq!(downloaded_section_title(), "RECENTLY ADDED");
        assert_eq!(available_section_title(), "AVAILABLE FOR SYNC");
        assert_eq!(
            [search_placeholder(), sort_tooltip(), type_label(), no_items_text(), not_found_text(), clear_filter_text()],
            ["Search wallpapers", "Sort", "Type", "No items", "No wallpapers found.", "Clear filter"]
        );
    }
}
