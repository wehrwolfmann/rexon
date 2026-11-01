//! Клиент AUR RPC v5 для каталога «Оформление» — порт
//! `rexon/appearance/aur_client.py` ЦЕЛИКОМ: и разбор ответа, и сетевой слой.
//!
//! Разбор: JSON-ответ RPC → модели (`parse`), вычисляемые атрибуты пакета
//! (`id`/`aur_url`/`preview_url`/`icon`), проверка RPC-ошибки, сортировка по
//! голосам и оркестрация над УЖЕ полученным ответом — фильтр многословного
//! поиска (`search_filter`) и дедуп+пин категории (`dedup_category`).
//!
//! Сеть: [`http_json`] (порт `_http_json`) плюс сборщики URL и сами
//! [`fetch_curated`]/[`fetch_for_category`]/[`search_aur`]. Вызов спрятан за
//! трейтом [`NetIo`] (`net.rs`) — это единственный способ вообще проверить
//! ветки: `aur.archlinux.org` из среды разработки недоступен (TLS рвёт
//! фильтр), поэтому тесты гоняют ЗАПИСАННЫЕ ответы. Прежнее решение «HTTP
//! остаётся на Python» отменено: цель — убрать Python отовсюду, кроме
//! интерфейса.
//!
//! Сверка с Python: эталоны сняты с самого `aur_client.py` (монипатч
//! `_http_json`/`urlopen` → канонический ответ, вызваны реальные `search_aur`/
//! `fetch_for_category`/`_parse`/атрибуты), Rust обязан вернуть тот же список
//! и те же строки. Голдены — `tests/aur_parity.rs` (разбор) и
//! `tests/net_parity.rs` (сетевые ветки).

use crate::net::{self, NetError, NetIo};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Базовый URL RPC v5 (совпадает с `RPC_BASE`).
pub const RPC_BASE: &str = "https://aur.archlinux.org/rpc/v5";
/// Шаблон страницы пакета (`AUR_PKG_URL`).
pub const AUR_PKG_URL_PREFIX: &str = "https://aur.archlinux.org/packages/";
/// User-Agent запросов (`USER_AGENT`).
pub const USER_AGENT: &str = "Rexon/5.0";

/// Один пакет AUR (подмножество полей RPC v5) — порт dataclass `AurPackage`.
///
/// Порядок/дефолты полей повторяют Python. `category` — ключ категории каталога
/// ("" = свободный поиск). `installed` рантайм-флаг проставляется UI-слоем по
/// `installed_set` (тут не участвует).
///
/// serde-дериваты — для PyO3-моста (rexon-py): имена полей совпадают с
/// dataclass, поэтому dict через мост собирается/разбирается напрямую.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AurPackage {
    pub name: String,
    pub description: String,
    pub version: String,
    pub votes: i64,
    pub popularity: f64,
    pub maintainer: String,
    pub out_of_date: bool,
    pub upstream_url: String,
    pub installed: bool,
    pub category: String,
    pub last_modified: i64,
}

impl AurPackage {
    /// `id` для grid-механики (`it.id`) — совпадает с именем пакета.
    pub fn id(&self) -> &str {
        &self.name
    }

    /// Страница пакета на aur.archlinux.org (`aur_url`): имя percent-энкодится
    /// как `urllib.parse.quote` (safe по умолчанию = только `/`).
    pub fn aur_url(&self) -> String {
        format!("{AUR_PKG_URL_PREFIX}{}", url_quote(&self.name))
    }

    /// Best-effort превью (`preview_url`): для GitHub-upstream — OpenGraph-
    /// карточка репозитория, иначе "". Порт регэкспа
    /// `https?://(?:www\.)?github\.com/(owner)/(repo)` с отсечением `.git` и
    /// хвоста `[/#?].*`.
    pub fn preview_url(&self) -> String {
        match github_owner_repo(&self.upstream_url) {
            Some((owner, repo)) => {
                format!("https://opengraph.githubassets.com/0/{owner}/{repo}")
            }
            None => String::new(),
        }
    }

    /// Эмодзи карточки (`icon`): по категории, иначе эвристика по имени пакета.
    /// Порядок проверок точно повторяет Python.
    pub fn icon(&self) -> &'static str {
        if let Some(ic) = category_icon(&self.category) {
            return ic;
        }
        let n = self.name.to_lowercase();
        if n.contains("cursor") {
            "🖱️"
        } else if n.contains("icon") {
            "📁"
        } else if n.contains("sddm") {
            "🔐"
        } else if n.contains("plymouth") {
            "⚡"
        } else if n.starts_with("ttf-") || n.starts_with("otf-") || n.contains("font") {
            "🔤"
        } else if n.contains("kvantum") {
            "🔲"
        } else if n.contains("wallpaper") {
            "🖼️"
        } else {
            "💎"
        }
    }
}

/// Эмодзи категории по ключу (`CATEGORY_ICONS`). `None` — ключа нет.
pub fn category_icon(key: &str) -> Option<&'static str> {
    Some(match key {
        "global" => "💎",
        "plasma" => "🖥️",
        "colors" => "🌈",
        "aurorae" => "🪟",
        "icons" => "📁",
        "cursor" => "🖱️",
        "kvantum" => "🔲",
        "wallpapers" => "🖼️",
        "wallpaper_plugins" => "🎞️",
        "fonts" => "🔤",
        "sounds" => "🔔",
        "plasmoids" => "🧊",
        "sddm" => "🔐",
        "plymouth" => "⚡",
        // Разделы окружений — значок своего типа, как в боковой панели.
        _ => return crate::categories::category_kind(key).and_then(crate::constants::type_icon),
    })
}

/// Курируемый список (`CURATED`) — имена пакетов для `fetch_curated`.
///
/// Порядок `aerothemeplasma-*` задаёт порядок закрепления наверху подборки:
/// оболочка, значки, звуки. Звуки добавлены 03.10.2026 — без них комплект
/// AeroThemePlasma в подборке был неполным, а сам пакет выпадал из выдачи
/// (`curated_order` берёт в закрепление только то, что перечислено здесь).
/// В КАТЕГОРИЯХ каталога звуки живут в «Звуках», не в «Глобальных темах»:
/// раздел отвечает на вопрос «что это ставит», а пакет ставит темы звуков
/// (см. [`is_relevant`]).
pub const CURATED: &[&str] = &[
    "aerothemeplasma-desktop-git",
    "aerothemeplasma-icons-git",
    "aerothemeplasma-sounds-git",
    "numix-icon-theme-git",
    "numix-circle-icon-theme-git",
    "bibata-cursor-theme",
    "paper-icon-theme",
    "klassy",
    "dracula-gtk-theme",
    "flat-remix",
    "la-capitaine-icon-theme",
    "tela-icon-theme",
    "nordic-theme",
    "vimix-gtk-themes",
    "candy-icons-git",
    "catppuccin-gtk-theme-mocha",
];

/// Раздел каталога, в котором курируемый пакет стоит наверху.
///
/// Подборка показывается не отдельной витриной, а закреплением в СВОЁМ
/// разделе источника AUR (без поискового запроса): оболочка AeroThemePlasma —
/// в «Глобальных темах», её значки — в «Значках», звуки — в «Звуках». Таблица
/// явная, а не по [`is_relevant`]: правило релевантности для части разделов
/// пропускает всё подряд и закрепило бы, скажем, значки в «Стилях Plasma».
/// `None` — своего раздела у пакета в каталоге нет (`klassy` — модуль
/// оформления окон, а не тема Aurorae), такой пакет не закрепляется нигде.
pub fn curated_category(name: &str) -> Option<&'static str> {
    Some(match name {
        "aerothemeplasma-desktop-git" => "global",
        "aerothemeplasma-icons-git"
        | "numix-icon-theme-git"
        | "numix-circle-icon-theme-git"
        | "paper-icon-theme"
        | "flat-remix"
        | "la-capitaine-icon-theme"
        | "tela-icon-theme"
        | "candy-icons-git" => "icons",
        "aerothemeplasma-sounds-git" => "sounds",
        "bibata-cursor-theme" => "cursor",
        "dracula-gtk-theme" | "nordic-theme" | "vimix-gtk-themes" | "catppuccin-gtk-theme-mocha" => "gtk",
        _ => return None,
    })
}

/// Есть ли у раздела `key` курируемые пакеты (иначе за подборкой не ходим).
pub fn has_curated(key: &str) -> bool {
    CURATED.iter().any(|n| curated_category(n) == Some(key))
}

/// Закрепить курируемые пакеты раздела `key` наверху выдачи раздела.
///
/// Порядок закреплённых — порядок [`CURATED`]; пакет, который уже нашёлся
/// в выдаче, переезжает наверх (повтора нет). Пакеты чужих разделов и те,
/// которых в ответе `/info` не оказалось (снят с AUR), не добавляются.
/// Ключ категории у закреплённых — `key`: от него зависит значок плитки.
pub fn pin_curated(key: &str, curated: &[AurPackage], list: Vec<AurPackage>) -> Vec<AurPackage> {
    let mut pinned: Vec<AurPackage> = Vec::new();
    for name in CURATED {
        if curated_category(name) != Some(key) {
            continue;
        }
        let found = list.iter().find(|p| p.name == *name).or_else(|| curated.iter().find(|p| p.name == *name));
        if let Some(p) = found {
            let mut p = p.clone();
            p.category = key.to_string();
            pinned.push(p);
        }
    }
    let rest: Vec<AurPackage> =
        list.into_iter().filter(|p| !pinned.iter().any(|q| q.name == p.name)).collect();
    pinned.extend(rest);
    pinned
}

/// Раздел источника AUR без поискового запроса: [`fetch_for_category`] и
/// сверху курируемые пакеты этого раздела ([`pin_curated`]).
///
/// Подборка — украшение, а не условие: не ответил `/info` — раздел всё равно
/// показывается своей выдачей. Виртуальные `popular`/`new` подборку не
/// закрепляют (своего раздела у них нет).
pub fn fetch_category_with_curated<IO: NetIo + ?Sized>(
    io: &mut IO,
    key: &str,
) -> Result<Vec<AurPackage>, AurError> {
    let list = fetch_for_category(io, key)?;
    if key == "popular" || key == "new" || !has_curated(key) {
        return Ok(list);
    }
    let names: Vec<&str> = CURATED.iter().copied().filter(|n| curated_category(n) == Some(key)).collect();
    match rpc_call(io, &info_url(&names)) {
        Ok(data) => Ok(pin_curated(key, &parse_results(&data), list)),
        Err(_) => Ok(list),
    }
}

/// Категории-агрегаты для виртуальных popular/new (`AGGREGATE_KEYS`).
pub const AGGREGATE_KEYS: &[&str] = &["global", "plasma", "colors", "icons", "cursor"];

/// Поисковые запросы AUR по ключу категории (`CATEGORY_QUERIES`).
/// `(keyword, by)` — `by`=name (подстрока имени) / name-desc (имя+описание).
pub fn category_queries(key: &str) -> &'static [(&'static str, &'static str)] {
    match key {
        "global" => &[
            ("aerothemeplasma", "name"),
            ("plasma-theme", "name"),
            ("look and feel", "name-desc"),
            ("global theme", "name-desc"),
        ],
        "plasma" => &[("plasma-theme", "name")],
        "colors" => &[("colorscheme", "name"), ("color-scheme", "name")],
        "aurorae" => &[("aurorae", "name")],
        "icons" => &[("icon-theme", "name"), ("icons", "name")],
        "cursor" => &[("cursor-theme", "name"), ("cursors", "name"), ("xcursor", "name")],
        "kvantum" => &[("kvantum", "name")],
        // «Стиль приложений»: темы Kvantum и готовые стили Qt. Своего
        // именного соглашения у стилей нет (снимок AUR 17.10.2026: `darkly`,
        // `lightly-qt`, `klassy`, `qt5-styleplugins`, `plastikstyle-qt6`),
        // поэтому запросы по именам семейств; чужое срезает [`is_relevant`].
        "widgetstyle" => &[
            ("kvantum", "name"),
            ("darkly", "name"),
            ("lightly", "name"),
            ("klassy", "name"),
            ("styleplugin", "name"),
            ("plastikstyle", "name"),
        ],
        // Темы GTK. Снимок 04.10.2026: имя `*-gtk-theme(s)*`/`gtk-theme-*` —
        // 236 пакетов, `gtk3-theme-*` — ещё 4. Классические наборы зовутся
        // иначе (`zuki-themes`, `vertex-themes`, `ubuntu-themes`,
        // `xfce-theme-greybird`), но GTK2-часть у них рисует движок Murrine,
        // и пакет зависит от `gtk-engine-murrine` (110 пакетов). Широкое
        // «gtk theme» по описанию добирает `flat-remix-gtk`, `darkly-gtk` и
        // темы без «gtk» в имени; значки, курсоры, стили Fluxbox и
        // программы-переключатели из этих двух запросов срезает [`is_relevant`].
        "gtk" => &[
            ("gtk-theme", "name"),
            ("gtk3-theme", "name"),
            ("gtk-engine-murrine", "depends"),
            ("gtk theme", "name-desc"),
        ],
        "wallpapers" => &[("wallpaper", "name")],
        "wallpaper_plugins" => {
            &[("plasma6-wallpapers", "name"), ("plasma wallpaper plugin", "name-desc")]
        }
        "fonts" => &[("ttf-", "name"), ("otf-", "name")],
        // Имя «*-sound-theme» есть не у всех: наборы вроде
        // `aerothemeplasma-sounds-git` и `xde-sounds` зовутся иначе, а
        // «sound theme» стоит у них в описании. Лишнее из двух широких
        // запросов срезает [`is_relevant`].
        "sounds" => &[
            ("sound-theme", "name"),
            ("sound theme", "name-desc"),
            ("sounds", "name"),
        ],
        "plasmoids" => &[("plasma6-applet", "name"), ("plasmoid", "name")],
        "sddm" => &[("sddm-theme", "name")],
        "plymouth" => &[("plymouth-theme", "name")],
        // Пакеты KWin в AUR зовутся `kwin-script(s)-*` и `kwin-effect(s)-*`
        // (снимок 04.10.2026: 17 и 38 пакетов). У переключателей окон своих
        // пакетов нет — раздел обходится без AUR.
        "kwinscripts" => &[("kwin-script", "name")],
        "kwineffects" => &[("kwin-effect", "name")],
        _ => &[],
    }
}

/// Есть ли у AUR полка для раздела: свои запросы ([`category_queries`])
/// либо сборная витрина «Популярные»/«Новые» ([`AGGREGATE_KEYS`]).
pub fn has_category(key: &str) -> bool {
    key == "popular" || key == "new" || !category_queries(key).is_empty()
}

// ── Фильтр релевантности категории ────────────────────────────────────────
//
// Поиск RPC — подстрока имени или описания, и широкие запросы приносят чужое.
// Живой снимок 03.10.2026: «look and feel» по name-desc вытащил в «Глобальные
// темы» терминал cool-retro-term-git, редактор qemacs-git, оконные менеджеры
// windowmaker-git и labwc-git, клиент Second Life и Java-библиотеку
// jgoodies-looks — у всех «look and feel» стоит в описании, но к теме Plasma
// они отношения не имеют. Такие же хвосты были у цветовых схем (схемы vim,
// Konsole, alacritty), значков (шрифты ttf-ionicons, akonadiconsole), курсоров
// (emacs-multiple-cursors), обоев (программы-установщики фона) и звуков
// (голосовые пакеты FreeSWITCH, плагины VST). Решение одно на все категории:
// пакет обязан быть ТЕМОЙ этого вида, а не просто совпасть по слову.

/// Заведомо не темы, которые приносят широкие запросы: имена целиком.
///
/// Это страховка поверх правил [`is_relevant`]: правило может поменяться, а
/// эти пакеты не должны вернуться в каталог ни при каком его изменении.
pub const NOT_THEMES: &[&str] = &[
    "cool-retro-term-git",
    "qemacs-git",
    "windowmaker-git",
    "labwc",
    "labwc-git",
    "labwc-im",
    "labwc-im-git",
    "labwc-hdr-metadata-git",
    "singularityviewer-alpha",
    "jgoodies-looks",
    "plasma-theme-master",
    "plasmoidviewer-debug",
    "aimp-skin-aurorae",
];

/// Префиксы пакетов-библиотек и дополнений редакторов: темой оформления
/// рабочего стола такой пакет не бывает, в какой бы категории ни всплыл.
const NOT_THEME_PREFIXES: &[&str] = &[
    "vim-", "neovim-", "nvim-", "emacs-", "python-", "python2-", "perl-",
    "ruby-", "nodejs-", "lua-", "haskell-", "rust-", "go-", "lib32-",
    "pidgin-", "geany-", "freeswitch-", "svxlink-",
    // Снимок 04.10.2026: дополнения GNOME Shell («значки на столе», «обои
    // Bing»), сборки библиотек под Android и дополнения конструктора Themix
    // всплывали в «Значках», «Курсорах» и «Обоях».
    "gnome-shell-extension-", "android-", "themix-",
];

/// Слова имени, по которым пакет — программа, а не набор (для обоев и звуков).
const TOOL_NAME_TOKENS: &[&str] = &[
    "cli", "gui", "engine", "wallpaperengine", "switch", "changer", "setter",
    "picker", "manager", "editor", "generator", "plugin", "player", "daemon",
    "lv2", "vst", "vst3", "bin-gui", "viewer", "fetcher", "downloader", "splitter",
    "bing", "importer",
];

/// Слова описания, по которым пакет — программа, а не набор.
const TOOL_DESC_WORDS: &[&str] = &[
    "setter", "setting utility", "wallpaper manager", "utility", "utilities", "a script",
    "bash script", "simple script", "gui for", "renderer", "runtime",
    "integration", "plugin", "daemon", "tool", "application", "client",
    "implementation of", "a program", "manage ", "fetches", "player",
    "editor", "engine", "voice recordings", "language pack", "effects app",
    // Программы смены и скачивания обоев (снимок 04.10.2026).
    "chooser", "change your", "changes your", "changes the desktop",
    "change wallpaper", "downloader", "download of", "fetch", "creator",
    "maker", "split and span", "show up in", "powered by", "wallpaper helper",
    "photo of the day", "use images", "as wallpaper", "live wallpaper",
    "animated", "automatically set", "beautify",
];

/// Слова описания, по которым пакет из раздела GTK — программа или
/// дополнение, а не тема: переключатели тем, мосты настроек, дополнения
/// конструктора тем. Общий [`TOOL_DESC_WORDS`] тут не годится: «engine»
/// стоит в описании настоящих тем («theme for Clearlooks GTK+ engine»).
const GTK_TOOL_DESC_WORDS: &[&str] = &[
    "chooser", "switcher", "theme manager", "gui for", "a gui ", "plugin",
    "syncs ", "a tool", "utility", "extension",
];

/// Части имени через `-`/`_` в нижнем регистре.
fn name_tokens(name: &str) -> Vec<String> {
    name.to_lowercase()
        .split(['-', '_'])
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

/// Подходит ли пакет к категории каталога — см. раздел выше.
///
/// Неизвестная категория (и шрифты, SDDM, Plymouth, Kvantum, виджеты: у них
/// запрос по строгому именному соглашению) проверяется только общими
/// стоп-списками.
pub fn is_relevant(key: &str, pkg: &AurPackage) -> bool {
    let name = pkg.name.to_lowercase();
    let desc = pkg.description.to_lowercase();
    if NOT_THEMES.contains(&name.as_str())
        || NOT_THEME_PREFIXES.iter().any(|p| name.starts_with(p))
    {
        return false;
    }
    let tokens = name_tokens(&name);
    let has_token = |t: &str| tokens.iter().any(|x| x == t);
    let tool_name = TOOL_NAME_TOKENS.iter().any(|t| has_token(t));
    let tool_desc = TOOL_DESC_WORDS.iter().any(|w| desc.contains(w));
    let plasma_ish = desc.contains("plasma") || desc.contains("kde") || name.contains("plasma");
    match key {
        "global" => {
            // Значки и звуки AeroThemePlasma — свои разделы («Значки»,
            // «Звуки»): в «Глобальных темах» карточка значков обещала бы
            // глобальную тему, которой в пакете нет.
            let family = name.starts_with("aerothemeplasma") || name.starts_with("vistathemeplasma");
            if family {
                return !(has_token("icons") || has_token("sounds"));
            }
            const NAME_MARKS: &[&str] =
                &["look-and-feel", "lookandfeel", "global-theme", "plasma-theme"];
            if NAME_MARKS.iter().any(|m| name.contains(m)) {
                return true;
            }
            let lnf = desc.contains("look and feel")
                || desc.contains("look-and-feel")
                || desc.contains("global theme");
            lnf && plasma_ish
        }
        "plasma" => true,
        // Стиль приложений — тема Kvantum либо стиль Qt: в описании стоит
        // «style» или «Kvantum». Тёзки семейств с другим делом (тема GTK
        // `darkly-gtk-git`, редактор `darkly-art-bin`, эффект KWin
        // `lightlyshaders-git`) этого слова не несут.
        "widgetstyle" => {
            let kvantum = name.contains("kvantum") || desc.contains("kvantum");
            let style = desc.contains("style") && !has_token("gtk");
            !tool_name && (kvantum || style)
        }
        // Цветовые схемы каталога — схемы KDE (`.colors`); схемы vim, Konsole,
        // терминалов и TTY к оформлению рабочего стола не относятся.
        "colors" => plasma_ish && !desc.contains("konsole"),
        "aurorae" => {
            name.contains("aurorae-theme") || desc.contains("aurorae theme") || plasma_ish
        }
        // «icons» по подстроке ловит akonadiconsole и шрифты-значки.
        "icons" => {
            let font = ["ttf-", "otf-", "woff-", "woff2-"].iter().any(|p| name.starts_with(p));
            let named = has_token("icons") || has_token("icon") || name.contains("icon-theme");
            let described = desc.contains("icon theme") || desc.contains("icons theme");
            // Дополнения файловых менеджеров, трафареты, значки в терминале.
            const ADDON_WORDS: &[&str] = &[
                "extension that", "nautilus extension", "caja extension", "stencil",
                "in terminal", "file manager", "emoticons", "browser addon",
            ];
            let addon = ADDON_WORDS.iter().any(|w| desc.contains(w));
            !font && !tool_name && !addon && (named || described)
        }
        "cursor" => {
            !name.contains("multiple-cursors") && !desc.contains("selection") && !tool_name
        }
        // Обои — наборы картинок; программы для фона и плагины Plasma
        // живут в «Плагинах обоев» либо не в каталоге вовсе.
        "wallpapers" => {
            !["plasma5-wallpapers-", "plasma6-wallpapers-", "plasma6-applets-", "plasma-wallpaper-"]
                .iter()
                .any(|p| name.starts_with(p))
                // Вариант темы GTK с обоями внутри — место ему в «Темах GTK».
                && !name.contains("gtk-theme")
                && !tool_name
                && !tool_desc
        }
        "sounds" => {
            if tool_name || tool_desc || name.starts_with("lib") {
                return false;
            }
            name.contains("sound-theme")
                || desc.contains("sound theme")
                || desc.contains("desktop sounds")
                || (desc.contains("sounds for") && (plasma_ish || desc.contains("desktop")))
        }
        // Темы GTK. Запросы раздела ([`category_queries`]) приносят и чужое:
        // значки и курсоры «под тему GTK», стили Fluxbox, файл tint2rc,
        // переключатели и настройщики тем, движки.
        "gtk" => {
            let gtk_tool = GTK_TOOL_DESC_WORDS.iter().any(|w| desc.contains(w))
                || tokens.iter().any(|t| t.starts_with("switch") || t == "config" || t == "bridge");
            if tool_name || gtk_tool || name.starts_with("lib") {
                return false;
            }
            const OTHER_PARTS: &[&str] =
                &["icon", "icons", "cursor", "cursors", "folders", "aurorae", "xfwm4", "fluxbox", "tint2rc"];
            if OTHER_PARTS.iter().any(|t| has_token(t)) || desc.contains("fluxbox") {
                return false;
            }
            // Набор для Plasma (стиль, глобальная тема) — не тема GTK, даже
            // если GTK-часть в нём есть; `gtk-theme-plasma` — тема GTK.
            if has_token("plasma") && !name.contains("gtk") {
                return false;
            }
            const GTK_DESKTOPS: &[&str] =
                &["gtk", "gnome", "xfce", "mate", "cinnamon", "budgie", "metacity", "openbox", "unity"];
            // `*-themes` без «gtk» в имени и описании — классические наборы,
            // которые находит запрос по зависимости от Murrine
            // (`ubuntu-themes`, `numix-frost-themes`).
            let gtk_ish = name.contains("gtk")
                || has_token("themes")
                || GTK_DESKTOPS.iter().any(|w| desc.contains(w));
            let theme_ish = name.contains("theme") || desc.contains("theme");
            gtk_ish && theme_ish
        }
        _ => true,
    }
}

/// Разбор одной записи RPC в `AurPackage` — порт `_parse`.
///
/// ⚠️ Поля числовых типов разбираются ТЕРПИМО к чужой схеме (`int_loose`/
/// `float_loose`/`truthy`): RPC отдаёт `NumVotes`/`LastModified` целыми,
/// `Popularity` дробным, `OutOfDate` — `null` или epoch, но у зеркал и
/// прокси-обёрток AUR попадаются числа, пришедшие СТРОКОЙ. Оригинал на этом
/// падал (`int("3")` ещё сработает, а `int("")`/`int("n/a")` — ValueError
/// внутри воркера каталога), поэтому здесь строка разбирается, а мусор даёт
/// дефолт — карточка потеряет счётчик, но источник AUR не отвалится целиком.
pub fn parse_record(rec: &Value) -> AurPackage {
    AurPackage {
        name: str_or_empty(rec, "Name"),
        description: str_or_empty(rec, "Description"),
        version: str_or_empty(rec, "Version"),
        votes: int_loose(rec.get("NumVotes")),
        popularity: float_loose(rec.get("Popularity")),
        maintainer: str_or_empty(rec, "Maintainer"),
        // bool(OutOfDate): null/0/"" → false, ненулевой epoch → true.
        out_of_date: truthy(rec.get("OutOfDate")),
        upstream_url: str_or_empty(rec, "URL"),
        installed: false,
        category: String::new(),
        last_modified: int_loose(rec.get("LastModified")),
    }
}

/// Записи `results` ответа — только объекты.
///
/// Отсутствие `results`, `null`, строка вместо массива, отдельные не-объекты
/// внутри массива — всё это даёт пустую/усечённую выдачу, а не мусорные пакеты
/// с пустым именем. Оригинал на `"results": null` падал TypeError'ом (итерация
/// по None), на строке — AttributeError'ом в `_parse`; починено и в
/// `aur_client.py` (`_results`).
fn results_of(data: &Value) -> impl Iterator<Item = &Value> {
    data.get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|r| r.is_object())
}

/// Все `results` ответа → пакеты. Отсутствие `results` → пустой список.
pub fn parse_results(data: &Value) -> Vec<AurPackage> {
    results_of(data).map(parse_record).collect()
}

/// Проверка RPC-ошибки (`_check_rpc_error`): `type=="error"` или truthy
/// `error` → `Err(текст)`.
///
/// Порт учитывает, что Python проверяет `data.get("error")` на ИСТИННОСТЬ, а не
/// на «непустая строка»: ответ вида `{"error": ["слишком много результатов"]}`
/// (так отвечают некоторые прокси перед AUR) у оригинала тоже был ошибкой.
/// Текстом такой ошибки берём компактный JSON значения — единственное отличие
/// от Python, где `str(RuntimeError(list))` дал бы python-repr списка.
pub fn check_rpc_error(data: &Value) -> Result<(), String> {
    let is_err_type = data.get("type").and_then(Value::as_str) == Some("error");
    let err = data.get("error");
    if is_err_type || truthy(err) {
        let msg = match err {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            Some(v) if truthy(Some(v)) => v.to_string(),
            _ => "AUR RPC error".to_string(),
        };
        return Err(msg);
    }
    Ok(())
}

/// Пост-фетч логика `fetch_curated` над ответом `/rpc/v5/info`: записи в
/// словарь по имени (последняя одноимённая выигрывает, как в Python-словаре),
/// `aerothemeplasma*` наверх в порядке [`CURATED`], остальные — по голосам
/// (убыв., стабильно, в порядке появления в ответе).
///
/// ⚠️ Починен баг оригинала: он строил словарь через `r["Name"]` и падал
/// `KeyError` на записи без `Name` (RPC такие отдаёт, когда обрезает ответ по
/// лимиту). Здесь записи без имени пропускаются.
pub fn curated_order(data: &Value) -> Vec<AurPackage> {
    // Порядок вставки + last-value-wins: как dict-comprehension в Python.
    let mut order: Vec<String> = Vec::new();
    let mut by_name: std::collections::HashMap<String, AurPackage> =
        std::collections::HashMap::new();
    for rec in results_of(data) {
        let name = str_or_empty(rec, "Name");
        if name.is_empty() {
            continue;
        }
        if by_name.insert(name.clone(), parse_record(rec)).is_none() {
            order.push(name);
        }
    }

    let mut pinned: Vec<AurPackage> = Vec::new();
    for name in CURATED {
        if name.starts_with("aerothemeplasma") {
            if let Some(p) = by_name.get(*name) {
                pinned.push(p.clone());
            }
        }
    }
    let mut rest: Vec<AurPackage> = order
        .iter()
        .filter(|n| !n.starts_with("aerothemeplasma"))
        .filter_map(|n| by_name.get(n).cloned())
        .collect();
    // key=lambda p: -p.votes — только голоса, out_of_date здесь НЕ участвует
    // (в отличие от sort_votes); сортировка стабильна, как sorted() в Python.
    rest.sort_by_key(|p| std::cmp::Reverse(p.votes));

    pinned.into_iter().chain(rest).collect()
}

/// Сортировка по голосам (`_sort_votes`): ключ `(out_of_date, -votes)`,
/// стабильная (устаревшие в конец, при равенстве — исходный порядок).
pub fn sort_votes(pkgs: &mut [AurPackage]) {
    pkgs.sort_by(|a, b| {
        a.out_of_date
            .cmp(&b.out_of_date)
            .then_with(|| b.votes.cmp(&a.votes))
    });
}

/// Сортировка «новые» (ветка `key=="new"`): `(out_of_date, -last_modified)`.
pub fn sort_new(pkgs: &mut [AurPackage]) {
    pkgs.sort_by(|a, b| {
        a.out_of_date
            .cmp(&b.out_of_date)
            .then_with(|| b.last_modified.cmp(&a.last_modified))
    });
}

/// Пост-фетч логика `search_aur` над УЖЕ полученным ответом RPC (по самому
/// длинному слову): локальный AND-фильтр остальными словами по имени+описанию,
/// затем `sort_votes`. `data` — ответ RPC (объект с `results`).
///
/// `limit` — как в Python (`None` = весь список).
pub fn search_filter(data: &Value, query: &str, limit: Option<usize>) -> Vec<AurPackage> {
    let terms: Vec<String> = query
        .trim()
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }
    // key = самое длинное слово; Python max() при равной длине берёт ПЕРВОЕ
    // (Rust max_by_key вернул бы последнее), поэтому ищем первое с макс. длиной.
    let max_len = terms.iter().map(|t| t.chars().count()).max().unwrap();
    let key = terms
        .iter()
        .find(|t| t.chars().count() == max_len)
        .cloned()
        .unwrap();
    let rest: Vec<&String> = terms.iter().filter(|t| **t != key).collect();

    let mut out: Vec<AurPackage> = parse_results(data)
        .into_iter()
        .filter(|p| {
            if rest.is_empty() {
                return true;
            }
            let hay = format!("{} {}", p.name, p.description).to_lowercase();
            rest.iter().all(|t| hay.contains(t.as_str()))
        })
        .collect();
    sort_votes(&mut out);
    match limit {
        Some(n) => out.into_iter().take(n).collect(),
        None => out,
    }
}

/// Пост-фетч логика `fetch_for_category` (не-виртуальная ветвь): объединение
/// ответов запросов категории с дедупом по имени (первый выигрывает),
/// отсев чужого ([`is_relevant`]), проставление `category`, `sort_votes` и
/// пин `aerothemeplasma` наверх для `global`. `responses` — ответы RPC в
/// порядке `category_queries(key)`.
pub fn dedup_category(responses: &[&Value], key: &str) -> Vec<AurPackage> {
    let mut seen: Vec<AurPackage> = Vec::new();
    let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for data in responses {
        for rec in results_of(data) {
            let name = str_or_empty(rec, "Name");
            if name.is_empty() || names.contains(&name) {
                continue;
            }
            let mut p = parse_record(rec);
            names.insert(name);
            if !is_relevant(key, &p) {
                continue;
            }
            p.category = key.to_string();
            seen.push(p);
        }
    }
    sort_votes(&mut seen);
    if key == "global" {
        let (pinned, rest): (Vec<_>, Vec<_>) = seen
            .into_iter()
            .partition(|p| p.name.starts_with("aerothemeplasma"));
        seen = pinned.into_iter().chain(rest).collect();
    }
    seen
}

// ── вспомогательное ───────────────────────────────────────────────────────

/// `rec.get(key)` как строка; null/отсутствие/не-строка → "". Повторяет
/// `rec.get(k) or ""` (и `rec.get("Name", "")`).
fn str_or_empty(rec: &Value, key: &str) -> String {
    rec.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Порт `int(v or 0)`, терпимый к чужой схеме: число → целая часть (усечение к
/// нулю, как `int()`), строка с числом → разобранное значение, `true` → 1,
/// всё прочее (null, мусорная строка, объект) → 0. Оригинал на мусорной строке
/// падал ValueError — здесь дефолт (см. `parse_record`).
fn int_loose(v: Option<&Value>) -> i64 {
    match v {
        Some(Value::Number(n)) => n
            .as_i64()
            .or_else(|| n.as_f64().map(|f| f.trunc() as i64))
            .unwrap_or(0),
        Some(Value::String(s)) => {
            let t = s.trim();
            t.parse::<i64>()
                .ok()
                .or_else(|| t.parse::<f64>().ok().map(|f| f.trunc() as i64))
                .unwrap_or(0)
        }
        Some(Value::Bool(b)) => i64::from(*b),
        _ => 0,
    }
}

/// Порт `float(v or 0.0)` с той же терпимостью, что [`int_loose`].
fn float_loose(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse::<f64>().unwrap_or(0.0),
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => 0.0,
    }
}

/// Порт python-истинности значения: null/отсутствие/`false`/0/""/пустой
/// список/пустой объект → `false`, остальное → `true`.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Percent-encode как `urllib.parse.quote(s)` — safe = ALPHA/DIGIT/`_.-~/`.
fn url_quote(s: &str) -> String {
    fn safe(b: u8) -> bool {
        b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~' | b'/')
    }
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if safe(b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

/// Разбор GitHub-upstream в `(owner, repo)` по регэкспу Python
/// `https?://(?:www\.)?github\.com/([\w.-]+)/([\w.-]+?)(?:\.git)?(?:[/#?].*)?$`.
/// Возвращает `None`, если это не github-URL нужной формы.
fn github_owner_repo(url: &str) -> Option<(String, String)> {
    // Класс символов `[\w.-]`: юникод-словосимвол (как Python \w) плюс `.`/`-`.
    fn cls(c: char) -> bool {
        c.is_alphanumeric() || c == '_' || c == '.' || c == '-'
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let rest = rest.strip_prefix("github.com/")?;

    // owner = максимальный префикс из класса до '/'; должен существовать '/'.
    let owner_end = rest.find(|c: char| !cls(c))?;
    if rest.as_bytes().get(owner_end) != Some(&b'/') {
        return None; // символ после owner не '/' → форма не совпала
    }
    let owner = &rest[..owner_end];
    if owner.is_empty() {
        return None;
    }
    let after = &rest[owner_end + 1..];

    // repo = максимальный префикс из класса; далее допустим только конец
    // строки или хвост, начинающийся с '/'/'#'/'?'.
    let seg_end = after.find(|c: char| !cls(c)).unwrap_or(after.len());
    let tail = &after[seg_end..];
    if !tail.is_empty() && !matches!(tail.as_bytes()[0], b'/' | b'#' | b'?') {
        return None;
    }
    let mut seg = &after[..seg_end];
    // Отсечь суффикс `.git` (non-greedy repo его не захватывает).
    if let Some(stripped) = seg.strip_suffix(".git") {
        if !stripped.is_empty() {
            seg = stripped;
        }
    }
    if seg.is_empty() {
        return None;
    }
    Some((owner.to_string(), seg.to_string()))
}

// ── Сетевой слой ──────────────────────────────────────────────────────────
//
// Порт `_http_json` и сетевых функций `fetch_curated`/`fetch_for_category`/
// `search_aur`. Сам вызов — за трейтом [`NetIo`] (`net.rs`).

/// Таймаут запроса RPC, секунды (`TIMEOUT`).
pub const TIMEOUT_SECS: u64 = 25;

/// Сбой обращения к AUR RPC — порт исключений, которые `aur_client` НЕ ловит:
/// они долетают до воркера каталога и показываются покупателю
/// (`_AurWorker.failed.emit(str(e))`).
///
/// Поэтому [`Display`](std::fmt::Display) даёт ровно тот же текст, что
/// `str(e)` в Python.
#[derive(Debug, Clone, PartialEq)]
pub enum AurError {
    /// Сетевой сбой: `_http_json` не ловит НИЧЕГО, исключение urllib уходит
    /// наверх как есть.
    Net(NetError),
    /// Ответ не разобрался как JSON (HTML-страница ошибки, капча прокси).
    NotJson(String),
    /// Ответ разобрался, но это не объект — дальше все читатели зовут
    /// `.get(...)`, и раньше это вылезало невнятным `AttributeError` глубоко
    /// в разборе. Внутри — имя типа по-питоновски (`list`, `str`, …).
    NotObject(String),
    /// RPC вернул ошибку (`type == "error"` либо непустое `error`).
    Rpc(String),
}

impl std::fmt::Display for AurError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AurError::Net(e) => write!(f, "{e}"),
            AurError::NotJson(detail) => {
                f.write_str(&crate::i18n::trf("AUR RPC returned something other than JSON: %1", &[detail]))
            }
            AurError::NotObject(ty) => {
                f.write_str(&crate::i18n::trf("AUR RPC returned a %1 instead of an object", &[ty]))
            }
            AurError::Rpc(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for AurError {}

/// Имя типа значения по-питоновски — для текста [`AurError::NotObject`]
/// (`type(data).__name__`). Целое и дробное различаются так же, как их
/// различает `json.loads`.
fn python_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "int"
            } else {
                "float"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `urllib.parse.quote_plus` — safe = ALPHA/DIGIT/`_.-~`, пробел → `+`.
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

/// URL точных данных по списку имён — порт сборки в `fetch_curated`
/// (`urlencode([("arg[]", n) for n in names])`; ключ `arg[]` тоже
/// percent-энкодится → `arg%5B%5D`).
pub fn info_url(names: &[&str]) -> String {
    let q = names
        .iter()
        .map(|n| format!("arg%5B%5D={}", quote_plus(n)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{RPC_BASE}/info?{q}")
}

/// URL поиска — порт `f"{RPC_BASE}/search/{quote(kw)}?by={by}"`.
/// Ключевое слово экранируется `urllib.parse.quote` (safe по умолчанию `/`),
/// `by` подставляется как есть (значения свои, из [`category_queries`]).
pub fn search_url(keyword: &str, by: &str) -> String {
    format!("{RPC_BASE}/search/{}?by={by}", url_quote(keyword))
}

/// Ответ RPC как объект — порт `_http_json`.
///
/// Проверка типа обязательна: дальше все читатели зовут `.get(...)`, а RPC
/// (или прокси/капча перед ним) умеет ответить массивом или строкой. Не-JSON
/// (HTML-страница ошибки) тоже даёт ошибку наверх, чтобы воркер каталога
/// показал причину, а не «источник AUR пуст».
///
/// Тело декодируется как UTF-8 с заменой битых байт — порт
/// `resp.read().decode("utf-8", "replace")`.
///
/// ⚠️ Расхождение с оригиналом (правило #12): временный сбой ПОВТОРЯЕТСЯ —
/// см. [`crate::net::with_retry`]. Полка AUR — тот же класс отказов, что
/// каталог OCS: один обрыв соединения показывал покупателю ошибку вместо
/// списка пакетов.
pub fn http_json<IO: NetIo + ?Sized>(io: &mut IO, url: &str) -> Result<Value, AurError> {
    let what = format!("AUR RPC {url}");
    let raw = net::with_retry(io, net::Retry::IDEMPOTENT, &what, |io| {
        io.get(url, USER_AGENT, TIMEOUT_SECS)
    })
    .map_err(AurError::Net)?;
    let text = String::from_utf8_lossy(&raw);
    let data: Value =
        serde_json::from_str(&text).map_err(|e| AurError::NotJson(e.to_string()))?;
    if data.is_object() {
        Ok(data)
    } else {
        Err(AurError::NotObject(python_type_name(&data).to_string()))
    }
}

/// Один запрос RPC с проверкой ошибки уровня протокола — общий хвост всех
/// трёх сетевых функций (`_http_json` + `_check_rpc_error`).
fn rpc_call<IO: NetIo + ?Sized>(io: &mut IO, url: &str) -> Result<Value, AurError> {
    let data = http_json(io, url)?;
    check_rpc_error(&data).map_err(AurError::Rpc)?;
    Ok(data)
}

/// Курируемый список одним `/info`-запросом — порт `fetch_curated`.
/// AeroThemePlasma наверху, остальное по голосам (см. [`curated_order`]).
pub fn fetch_curated<IO: NetIo + ?Sized>(io: &mut IO) -> Result<Vec<AurPackage>, AurError> {
    let data = rpc_call(io, &info_url(CURATED))?;
    Ok(curated_order(&data))
}

/// Поиск по AUR (имя+описание) — порт `search_aur`.
///
/// RPC ищет ОДНУ literal-подстроку («aero plasma» → 0 результатов), поэтому
/// многословный запрос идёт в RPC самым длинным словом, а остальные слова —
/// локальный AND-фильтр (см. [`search_filter`]). Пустой запрос сети не
/// касается. `limit = None` — весь список (страницы режет интерфейс).
pub fn search_aur<IO: NetIo + ?Sized>(
    io: &mut IO,
    query: &str,
    limit: Option<usize>,
) -> Result<Vec<AurPackage>, AurError> {
    let lowered = query.trim().to_lowercase();
    let terms: Vec<&str> = lowered.split_whitespace().collect();
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    // max() в Python при равной длине берёт ПЕРВОЕ слово.
    let max_len = terms.iter().map(|t| t.chars().count()).max().unwrap();
    let key = terms.iter().find(|t| t.chars().count() == max_len).unwrap();
    let data = rpc_call(io, &search_url(key, "name-desc"))?;
    Ok(search_filter(&data, query, limit))
}

/// AUR-пакеты категории каталога — порт `fetch_for_category`.
///
/// Обычная категория: объединение поисков по именному соглашению
/// ([`category_queries`]) с дедупом по имени. Виртуальные `popular`/`new`:
/// агрегация [`AGGREGATE_KEYS`] с дедупом (первый выигрывает), пакеты
/// сохраняют СВОЙ ключ категории (от него зависит эмодзи карточки); `new`
/// сортируется по дате изменения, `popular` — по голосам.
/// Категория без запросов → пустой список (честное «в AUR ничего нет»),
/// сеть при этом не трогается.
pub fn fetch_for_category<IO: NetIo + ?Sized>(
    io: &mut IO,
    key: &str,
) -> Result<Vec<AurPackage>, AurError> {
    if key == "popular" || key == "new" {
        let mut seen: Vec<AurPackage> = Vec::new();
        let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for sub in AGGREGATE_KEYS {
            for p in fetch_for_category(io, sub)? {
                if names.insert(p.name.clone()) {
                    seen.push(p);
                }
            }
        }
        if key == "new" {
            sort_new(&mut seen);
        } else {
            sort_votes(&mut seen);
        }
        return Ok(seen);
    }
    let queries = category_queries(key);
    if queries.is_empty() {
        return Ok(Vec::new());
    }
    let mut responses: Vec<Value> = Vec::with_capacity(queries.len());
    for (kw, by) in queries {
        responses.push(rpc_call(io, &search_url(kw, by))?);
    }
    let refs: Vec<&Value> = responses.iter().collect();
    Ok(dedup_category(&refs, key))
}

// ═══════════════════════════════════════════════════════════════════════
//  Карточка пакета: всё, что знает AUR о пакете, — внутри программы
// ═══════════════════════════════════════════════════════════════════════
//
// Плитка AUR открывает подробную карточку, как товар магазина, а не страницу
// в браузере. Данные — из двух мест:
//
// * RPC v5 `info` — паспорт пакета: версия, сопровождающий, голоса,
//   популярность, даты, лицензия, зависимости, ключевые слова, пометка
//   «устарел» (документация RPC: https://wiki.archlinux.org/title/Aurweb_RPC_interface);
// * HTML-страница пакета — то, чего RPC не отдаёт вовсе: соисполнители
//   (co-maintainers) и комментарии. API комментариев у AUR нет, поэтому
//   страница разбирается; если вместо неё пришла проверка «вы не робот»
//   (Anubis), карточка честно говорит «комментарии — на сайте» и даёт кнопку.
//
// Сеть — через тот же [`NetIo`], разбор — чистые функции над записанными
// ответами (`tests/aur_detail.rs`).

/// Паспорт пакета из RPC `info`.
///
/// `out_of_date` — время пометки «устарел» (epoch), 0 — не помечен.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AurInfo {
    pub name: String,
    pub package_base: String,
    pub version: String,
    pub description: String,
    pub url: String,
    pub maintainer: String,
    pub submitter: String,
    pub votes: i64,
    pub popularity: f64,
    pub first_submitted: i64,
    pub last_modified: i64,
    pub out_of_date: i64,
    pub license: Vec<String>,
    pub keywords: Vec<String>,
    pub groups: Vec<String>,
    pub depends: Vec<String>,
    pub make_depends: Vec<String>,
    pub check_depends: Vec<String>,
    pub opt_depends: Vec<String>,
    pub conflicts: Vec<String>,
    pub provides: Vec<String>,
    pub replaces: Vec<String>,
}

impl AurInfo {
    /// Плитка из паспорта — для установки и общего пути показа.
    pub fn as_package(&self) -> AurPackage {
        AurPackage {
            name: self.name.clone(),
            description: self.description.clone(),
            version: self.version.clone(),
            votes: self.votes,
            popularity: self.popularity,
            maintainer: self.maintainer.clone(),
            out_of_date: self.out_of_date != 0,
            upstream_url: self.url.clone(),
            installed: false,
            category: String::new(),
            last_modified: self.last_modified,
        }
    }
}

/// Строки массива поля; не-строки и пустые строки пропускаются (терпимо к
/// чужой схеме, как [`parse_record`]).
fn str_list(rec: &Value, key: &str) -> Vec<String> {
    rec.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Одна запись `info` → паспорт.
pub fn parse_info_record(rec: &Value) -> AurInfo {
    AurInfo {
        name: str_or_empty(rec, "Name"),
        package_base: str_or_empty(rec, "PackageBase"),
        version: str_or_empty(rec, "Version"),
        description: str_or_empty(rec, "Description"),
        url: str_or_empty(rec, "URL"),
        maintainer: str_or_empty(rec, "Maintainer"),
        submitter: str_or_empty(rec, "Submitter"),
        votes: int_loose(rec.get("NumVotes")),
        popularity: float_loose(rec.get("Popularity")),
        first_submitted: int_loose(rec.get("FirstSubmitted")),
        last_modified: int_loose(rec.get("LastModified")),
        out_of_date: int_loose(rec.get("OutOfDate")),
        license: str_list(rec, "License"),
        keywords: str_list(rec, "Keywords"),
        groups: str_list(rec, "Groups"),
        depends: str_list(rec, "Depends"),
        make_depends: str_list(rec, "MakeDepends"),
        check_depends: str_list(rec, "CheckDepends"),
        opt_depends: str_list(rec, "OptDepends"),
        conflicts: str_list(rec, "Conflicts"),
        provides: str_list(rec, "Provides"),
        replaces: str_list(rec, "Replaces"),
    }
}

/// Паспорт пакета `name` из ответа `info`. `None` — такого пакета в ответе
/// нет (удалён из AUR, опечатка).
pub fn parse_info(data: &Value, name: &str) -> Option<AurInfo> {
    results_of(data)
        .find(|r| r.get("Name").and_then(Value::as_str) == Some(name))
        .map(parse_info_record)
}

/// Паспорт одного пакета запросом `info`. `Ok(None)` — AUR о нём не знает.
pub fn fetch_info<IO: NetIo + ?Sized>(io: &mut IO, name: &str) -> Result<Option<AurInfo>, AurError> {
    let data = rpc_call(io, &info_url(&[name]))?;
    Ok(parse_info(&data, name))
}

// ── Страница пакета: соисполнители и комментарии ─────────────────────────

/// Сколько комментариев на одной странице сайта (`?O=` шагает по 10).
pub const PAGE_COMMENTS: i64 = 10;

/// Сколько держать ответ AUR в памяти, прежде чем спросить снова, секунды.
/// Карточку открывают и закрывают подряд — сайт от этого страдать не должен.
pub const DETAIL_CACHE_SECS: u64 = 15 * 60;

/// Пауза между обращениями к HTML-страницам AUR, миллисекунды. Страница
/// тяжелее RPC, и защита сайта от ботов срабатывает именно на частые
/// заходы подряд.
pub const PAGE_PAUSE_MS: u64 = 2_000;

/// Страница пакета с комментариями; `page` — номер страницы комментариев
/// с нуля.
pub fn package_page_url(name: &str, page: i64) -> String {
    let base = format!("{AUR_PKG_URL_PREFIX}{}", url_quote(name));
    if page > 0 {
        format!("{base}?O={}", page * PAGE_COMMENTS)
    } else {
        base
    }
}

/// Сколько ещё ждать до следующего обращения к странице: `since_last_ms` —
/// сколько прошло с прошлого (`None` — обращений не было).
pub fn page_pause_left_ms(since_last_ms: Option<u64>) -> u64 {
    match since_last_ms {
        Some(ms) if ms < PAGE_PAUSE_MS => PAGE_PAUSE_MS - ms,
        _ => 0,
    }
}

/// Комментарий со страницы пакета.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AurComment {
    pub author: String,
    /// Как на сайте: `2023-09-30 14:14 (UTC)`.
    pub date: String,
    /// Текст без разметки, абзацы — переводами строк.
    pub text: String,
    /// Закреплён сопровождающим (показывается первым).
    pub pinned: bool,
}

/// Что удалось взять со страницы пакета.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AurPage {
    pub comaintainers: Vec<String>,
    /// Сначала закреплённые, потом последние — как на сайте.
    pub comments: Vec<AurComment>,
    /// Есть следующая страница комментариев.
    pub has_more: bool,
}

/// Почему страницу не разобрать.
#[derive(Debug, Clone, PartialEq)]
pub enum PageError {
    /// Вместо страницы пакета пришло что-то другое — обычно проверка
    /// «вы не робот». Обходить её нельзя: комментарии — на сайте.
    Blocked,
    /// Сети нет или сервер ответил ошибкой.
    Net(NetError),
}

/// Страница пакета ли это вообще: у настоящей есть таблица сведений.
pub fn is_package_page(html: &str) -> bool {
    html.contains("id=\"pkgdetails\"")
}

/// Разобрать страницу пакета.
pub fn parse_package_page(html: &str) -> Result<AurPage, PageError> {
    if !is_package_page(html) {
        return Err(PageError::Blocked);
    }
    let mut out = AurPage {
        comaintainers: parse_comaintainers(html),
        ..AurPage::default()
    };
    // Разделы комментариев идут подряд: «Pinned Comments», потом «Latest
    // Comments». Граница раздела — начало следующего.
    const SECTION: &str = "<div class=\"comments package-comments\">";
    let starts: Vec<usize> = html.match_indices(SECTION).map(|(i, _)| i).collect();
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(html.len());
        let section = &html[start..end];
        let title = between(section, "<span class=\"text\">", "</span>").unwrap_or("");
        let pinned = title.trim().eq_ignore_ascii_case("Pinned Comments");
        if !pinned && section_has_next(section) {
            out.has_more = true;
        }
        out.comments.extend(parse_comments(section, pinned));
    }
    Ok(out)
}

/// Соисполнители: строка «Maintainer:» несёт их в скобках после главного,
/// через запятую.
fn parse_comaintainers(html: &str) -> Vec<String> {
    let Some(row) = between(html, "<tr class=\"pkgmaint\">", "</tr>") else {
        return Vec::new();
    };
    let cell = between(row, "<td>", "</td>").unwrap_or("");
    let text = strip_tags(cell);
    let Some(open) = text.find('(') else {
        return Vec::new();
    };
    let close = text[open..].find(')').map(|c| open + c).unwrap_or(text.len());
    text[open + 1..close]
        .split(',')
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Есть ли ссылка «Next ›» в навигации раздела.
fn section_has_next(section: &str) -> bool {
    section.contains(">Next ›<") || section.contains(">Next &rsaquo;<")
}

/// Комментарии одного раздела.
fn parse_comments(section: &str, pinned: bool) -> Vec<AurComment> {
    const HEAD: &str = "class=\"comment-header\"";
    let mut out = Vec::new();
    let mut rest = section;
    while let Some(pos) = rest.find(HEAD) {
        let after = &rest[pos..];
        let Some(head_end) = after.find("</h4>") else { break };
        let head = &after[..head_end];
        let body_from = &after[head_end..];
        // Тело — первый блок `article-content` после заголовка.
        let text = match body_from.find("class=\"article-content\"") {
            Some(b) => {
                let block = &body_from[b..];
                let open_end = block.find('>').map(|e| e + 1).unwrap_or(0);
                let inner = div_inner(&block[open_end..]);
                html_to_text(inner)
            }
            None => String::new(),
        };
        // «ник commented on <дата>» — ник до « commented on », дата — текст
        // ссылки с классом `date`. Правка («edited on …») к дате не относится.
        // `head` начинается внутри открывающего тега `<h4 …>` — его хвост
        // до `>` отрезаем.
        let head_body = head.split_once('>').map(|(_, b)| b).unwrap_or(head);
        let head_text =
            strip_tags(head_body.split("<span class=\"edited\">").next().unwrap_or(head_body));
        let author = head_text
            .split(" commented on ")
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let date = between(head, "class=\"date\">", "</a>")
            .map(|d| strip_tags(d).trim().to_string())
            .unwrap_or_default();
        out.push(AurComment { author, date, text, pinned });
        rest = &after[head_end..];
    }
    out
}

/// Содержимое `<div>` до парной закрывающей: внутри тела бывают вложенные
/// `<div>`, первый же `</div>` обрезал бы текст.
fn div_inner(s: &str) -> &str {
    let mut depth = 1i32;
    let mut i = 0;
    let lower = s.to_ascii_lowercase();
    while i < s.len() {
        let Some(rel) = lower[i..].find("<") else { break };
        let at = i + rel;
        if lower[at..].starts_with("</div") {
            depth -= 1;
            if depth == 0 {
                return &s[..at];
            }
        } else if lower[at..].starts_with("<div") {
            depth += 1;
        }
        i = at + 1;
    }
    s
}

/// Подстрока между первым `open` и следующим за ним `close`.
fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let a = s.find(open)? + open.len();
    let b = s[a..].find(close)? + a;
    Some(&s[a..b])
}

/// Теги долой, сущности — в знаки. Пробелы не трогаются.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    crate::translator::html_unescape(&out)
}

/// Разметка комментария (Markdown, отрисованный сайтом) → простой текст:
/// абзацы и строки — переводами строк, пункты списков — «• », код в
/// `<pre>` — как есть, с его переводами строк. Ссылка остаётся своим текстом.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    let mut in_pre = false;
    let bytes = html.as_bytes();
    while i < html.len() {
        if bytes[i] == b'<' {
            let end = html[i..].find('>').map(|e| i + e + 1).unwrap_or(html.len());
            let tag = html[i..end].to_ascii_lowercase();
            let name: String = tag
                .trim_start_matches('<')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '/')
                .collect();
            match name.as_str() {
                "br" | "br/" => out.push('\n'),
                "p" | "/p" | "div" | "/div" | "ul" | "/ul" | "ol" | "/ol" | "blockquote"
                | "/blockquote" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "/h1" | "/h2"
                | "/h3" | "/h4" | "/h5" | "/h6" | "hr" => out.push_str("\n\n"),
                "li" => out.push_str("\n• "),
                "/li" => out.push('\n'),
                "pre" => {
                    out.push_str("\n\n");
                    in_pre = true;
                }
                "/pre" => {
                    out.push_str("\n\n");
                    in_pre = false;
                }
                _ => {}
            }
            i = end;
            continue;
        }
        let next = html[i..].find('<').map(|n| i + n).unwrap_or(html.len());
        let chunk = crate::translator::html_unescape(&html[i..next]);
        if in_pre {
            out.push_str(&chunk);
        } else {
            // Вне `<pre>` перевод строки исходника — просто пробел.
            let flat = chunk.replace(['\n', '\r', '\t'], " ");
            let mut prev_space = out.ends_with([' ', '\n']) || out.is_empty();
            for c in flat.chars() {
                if c == ' ' {
                    if !prev_space {
                        out.push(' ');
                    }
                    prev_space = true;
                } else {
                    out.push(c);
                    prev_space = false;
                }
            }
        }
        i = next;
    }
    // Хвостовые пробелы строк и лишние пустые строки — долой.
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    let mut text = String::new();
    let mut blank = 0;
    for line in lines {
        if line.trim().is_empty() {
            blank += 1;
            continue;
        }
        if !text.is_empty() {
            text.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        text.push_str(line);
    }
    text
}

/// Скачать и разобрать страницу пакета. Пауза между обращениями — забота
/// вызывающего ([`page_pause_left_ms`]).
pub fn fetch_package_page<IO: NetIo + ?Sized>(
    io: &mut IO,
    name: &str,
    page: i64,
) -> Result<AurPage, PageError> {
    let raw = io
        .get(&package_page_url(name, page), USER_AGENT, TIMEOUT_SECS)
        .map_err(PageError::Net)?;
    parse_package_page(&String::from_utf8_lossy(&raw))
}

// ── Картинка карточки ────────────────────────────────────────────────────

/// Метка «картинку надо взять из `og:image` страницы проекта»: её
/// разрешает загрузчик ([`resolve_og_image`]), а не кеш картинок.
pub const OG_IMAGE_SCHEME: &str = "og-image:";

/// Хосты, где страница проекта несёт `og:image` (GitLab и Forgejo).
fn og_image_host(host: &str) -> bool {
    host.contains("gitlab")
        || matches!(
            host,
            "codeberg.org" | "invent.kde.org" | "framagit.org" | "salsa.debian.org"
        )
}

/// Хост адреса (`https://host/...` → `host`), строчными.
fn url_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit('@').next()?.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// Картинки карточки пакета по порядку показа.
///
/// * GitHub — карточка репозитория (та же, что на плитке) и первая картинка
///   README (метка `github-readme:`, её ищет загрузчик);
/// * GitLab/Codeberg — `og:image` страницы проекта (метка `og-image:`);
/// * иначе — ничего: карточка покажет тот же значок, что плитка.
pub fn detail_image_sources(upstream_url: &str) -> Vec<String> {
    if let Some((owner, repo)) = github_owner_repo(upstream_url) {
        return vec![
            format!("https://opengraph.githubassets.com/0/{owner}/{repo}"),
            format!("{}{owner}/{repo}", README_PREVIEW_SCHEME),
        ];
    }
    match url_host(upstream_url) {
        Some(host) if og_image_host(&host) => vec![format!("{OG_IMAGE_SCHEME}{upstream_url}")],
        _ => Vec::new(),
    }
}

/// `og:image` из HTML страницы; относительный адрес — от `page_url`.
pub fn parse_og_image(html: &str, page_url: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find("<meta") {
        let start = from + rel;
        let end = lower[start..].find('>').map(|e| start + e).unwrap_or(lower.len());
        let tag = &html[start..end];
        let tag_l = &lower[start..end];
        let is_og = ["property=\"og:image\"", "property='og:image'", "name=\"og:image\""]
            .iter()
            .any(|m| tag_l.contains(m));
        if is_og {
            if let Some(v) = attr_value(tag, tag_l, "content") {
                let v = crate::translator::html_unescape(v.trim());
                if !v.is_empty() {
                    return Some(absolute_url(&v, page_url));
                }
            }
        }
        from = end;
    }
    None
}

/// Значение атрибута тега (в кавычках любого рода).
fn attr_value<'a>(tag: &'a str, tag_l: &str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=");
    let mut search = 0;
    while let Some(rel) = tag_l[search..].find(&key) {
        let at = search + rel;
        // Начало имени атрибута, а не хвост другого (`data-content=`).
        let ok_start = at == 0 || tag_l.as_bytes()[at - 1].is_ascii_whitespace();
        let val = &tag[at + key.len()..];
        if ok_start {
            return match val.chars().next() {
                Some(q @ ('"' | '\'')) => val[1..].split(q).next(),
                _ => val.split([' ', '>', '/']).next(),
            };
        }
        search = at + key.len();
    }
    None
}

/// Относительный адрес картинки → абсолютный от страницы.
fn absolute_url(link: &str, page_url: &str) -> String {
    if link.starts_with("https://") || link.starts_with("http://") {
        return link.to_string();
    }
    if let Some(rest) = link.strip_prefix("//") {
        return format!("https://{rest}");
    }
    let scheme_host = match url_host(page_url) {
        Some(h) => format!("https://{h}"),
        None => return link.to_string(),
    };
    if link.starts_with('/') {
        format!("{scheme_host}{link}")
    } else {
        format!("{}/{link}", page_url.trim_end_matches('/'))
    }
}

/// Найти картинку для метки `og-image:<адрес страницы>`.
pub fn resolve_og_image(net: &mut dyn NetIo, marker: &str) -> Option<String> {
    let page = marker.strip_prefix(OG_IMAGE_SCHEME)?;
    let body = net.get(page, USER_AGENT, TIMEOUT_SECS).ok()?;
    parse_og_image(&String::from_utf8_lossy(&body), page)
}

/// Метка «картинку надо найти в README репозитория GitHub»
/// (`github-readme:владелец/имя`): её разрешает загрузчик
/// ([`resolve_readme_preview`]), а не кеш картинок.
pub const README_PREVIEW_SCHEME: &str = "github-readme:";

/// Значки-бейджи и прочая мелочь, которая картинкой проекта не является.
const BADGE_MARKERS: &[&str] = &[
    "shields.io", "badge", "badgen", "/workflows/", "travis-ci", "circleci", "codecov",
    "ko-fi.com/img", "buymeacoffee", "liberapay", "paypal", "sponsor", "opencollective",
    "star-history", "contrib.rocks", "visitor", "hits.", "count.getloli", "komarev",
    "avatars.githubusercontent",
];

/// Все ссылки на картинки в тексте README по порядку: `![…](адрес)` и
/// `<img src="адрес">`.
fn image_links(md: &str) -> Vec<String> {
    let mut found: Vec<(usize, String)> = Vec::new();
    let mut i = 0;
    while let Some(p) = md[i..].find("![") {
        let start = i + p;
        let rest = &md[start..];
        if let Some(close) = rest.find("](") {
            let after = &rest[close + 2..];
            if let Some(end) = after.find(')') {
                let inner = after[..end].trim();
                let link = inner.split_whitespace().next().unwrap_or("").trim_matches(['<', '>']);
                if !link.is_empty() {
                    found.push((start, link.to_string()));
                }
            }
        }
        i = start + 2;
    }
    let lower = md.to_lowercase();
    let mut j = 0;
    while let Some(p) = lower[j..].find("<img") {
        let start = j + p;
        let tag_end = lower[start..].find('>').map(|e| start + e).unwrap_or(lower.len());
        let tag = &md[start..tag_end];
        let tag_l = &lower[start..tag_end];
        if let Some(sp) = tag_l.find("src=") {
            let val = &tag[sp + 4..];
            let link = match val.chars().next() {
                Some(q @ ('"' | '\'')) => val[1..].split(q).next().unwrap_or(""),
                _ => val.split([' ', '>']).next().unwrap_or(""),
            };
            if !link.is_empty() {
                found.push((start, link.to_string()));
            }
        }
        j = start + 4;
    }
    found.sort_by_key(|(pos, _)| *pos);
    found.into_iter().map(|(_, l)| l).collect()
}

/// Ссылка из README → прямой адрес картинки: относительная — в
/// `raw.githubusercontent.com` ветки, `github.com/…/blob/…` — в её «сырой»
/// вид. Чужие адреса — как есть.
pub fn absolutize(link: &str, full_name: &str, branch: &str) -> Option<String> {
    let link = link.trim();
    if link.is_empty() || link.starts_with('#') || link.starts_with("data:") {
        return None;
    }
    if let Some(rest) = link.strip_prefix("https://github.com/").or_else(|| link.strip_prefix("http://github.com/")) {
        // owner/repo/blob/ref/path → raw
        let parts: Vec<&str> = rest.splitn(5, '/').collect();
        if parts.len() == 5 && (parts[2] == "blob" || parts[2] == "raw") {
            let path = parts[4].split(['?', '#']).next().unwrap_or("");
            return Some(format!("https://raw.githubusercontent.com/{}/{}/{}/{}", parts[0], parts[1], parts[3], path));
        }
        return Some(link.to_string());
    }
    if link.starts_with("https://") || link.starts_with("http://") {
        return Some(link.to_string());
    }
    if link.starts_with("//") {
        return Some(format!("https:{link}"));
    }
    let path = link.trim_start_matches("./").trim_start_matches('/');
    let path = path.split(['?', '#']).next().unwrap_or("");
    if path.is_empty() || path.starts_with("../") {
        return None;
    }
    Some(format!("https://raw.githubusercontent.com/{full_name}/{branch}/{path}"))
}

/// Первая картинка README, которая не значок: `None` — картинок нет.
/// SVG берётся, только если ничего другого нет (их не всякий загрузчик
/// покажет, а у проектов обычно есть PNG или GIF).
pub fn readme_first_image(md: &str, full_name: &str, branch: &str) -> Option<String> {
    let all: Vec<String> = image_links(md)
        .into_iter()
        .filter(|l| {
            let l = l.to_lowercase();
            !BADGE_MARKERS.iter().any(|b| l.contains(b))
        })
        .filter_map(|l| absolutize(&l, full_name, branch))
        .collect();
    let is_svg = |u: &String| u.to_lowercase().split(['?', '#']).next().unwrap_or("").ends_with(".svg");
    all.iter().find(|u| !is_svg(u)).or_else(|| all.first()).cloned()
}

/// Найти картинку для метки `github-readme:владелец/имя`: прочесть README
/// ветки по умолчанию (`HEAD`) и взять первую картинку.
pub fn resolve_readme_preview(net: &mut dyn NetIo, marker: &str) -> Option<String> {
    let full = marker.strip_prefix(README_PREVIEW_SCHEME)?;
    let branch = "HEAD";
    for name in ["README.md", "readme.md", "README.MD", "Readme.md"] {
        let url = format!("https://raw.githubusercontent.com/{full}/{branch}/{name}");
        if let Ok(body) = net.get(&url, USER_AGENT, TIMEOUT_SECS) {
            return readme_first_image(&String::from_utf8_lossy(&body), full, branch);
        }
    }
    None
}

// ── Строки карточки ──────────────────────────────────────────────────────

/// Строка сведений карточки: подпись, значение, предупреждение, ссылка.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AurFact {
    pub label: String,
    pub value: String,
    /// Красить как предупреждение («помечен устаревшим»).
    pub warn: bool,
    /// Значение — адрес, по нему можно перейти.
    pub link: bool,
}

/// День по epoch, UTC: `2026-06-19`. 0 и меньше — пусто.
pub fn epoch_day(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    crate::engine_update::now_iso(ts).chars().take(10).collect()
}

/// Необязательная зависимость — «пакет: зачем» → «пакет — зачем».
pub fn opt_depend_line(raw: &str) -> String {
    match raw.split_once(':') {
        Some((pkg, why)) if !why.trim().is_empty() => format!("{} — {}", pkg.trim(), why.trim()),
        Some((pkg, _)) => pkg.trim().to_string(),
        None => raw.trim().to_string(),
    }
}

/// Все строки сведений карточки по порядку. Пустые поля не показываются.
pub fn info_facts(info: &AurInfo, comaintainers: &[String]) -> Vec<AurFact> {
    use crate::i18n::{tr, trf};
    let mut out: Vec<AurFact> = Vec::new();
    let mut push = |label: String, value: String| {
        if !value.trim().is_empty() {
            out.push(AurFact { label, value, warn: false, link: false });
        }
    };
    let list = |v: &[String]| v.join(", ");
    if info.out_of_date > 0 {
        // Первой строкой: это главное, что надо знать перед сборкой.
        push(tr("Status"), trf("⚠️ Flagged out-of-date since %1", &[&epoch_day(info.out_of_date)]));
    }
    push(tr("Version"), info.version.clone());
    push(
        tr("Maintainer"),
        if info.maintainer.is_empty() { tr("none (orphaned package)") } else { info.maintainer.clone() },
    );
    push(tr("Co-maintainers"), list(comaintainers));
    push(tr("Votes"), info.votes.to_string());
    push(tr("Popularity"), format!("{:.2}", info.popularity));
    push(tr("First submitted"), epoch_day(info.first_submitted));
    push(tr("Last updated"), epoch_day(info.last_modified));
    push(tr("License"), list(&info.license));
    push(tr("Project page"), info.url.clone());
    push(tr("Depends on"), list(&info.depends));
    push(tr("Build dependencies"), list(&info.make_depends));
    push(tr("Test dependencies"), list(&info.check_depends));
    push(
        tr("Optional"),
        info.opt_depends.iter().map(|d| opt_depend_line(d)).collect::<Vec<_>>().join("\n"),
    );
    push(tr("Conflicts with"), list(&info.conflicts));
    push(tr("Provides"), list(&info.provides));
    push(tr("Replaces"), list(&info.replaces));
    push(tr("Keywords"), list(&info.keywords));
    if !info.package_base.is_empty() && info.package_base != info.name {
        push(tr("Package base"), info.package_base.clone());
    }
    let project = tr("Project page");
    let status = tr("Status");
    for f in &mut out {
        if f.label == project {
            f.link = true;
        }
        if f.label == status && info.out_of_date > 0 {
            f.warn = true;
        }
    }
    out
}

/// Значок закреплённого комментария перед ником.
pub const PINNED_MARK: &str = "📌 ";

/// Подпись вместо комментариев, когда страницу пакета не получить.
pub fn comments_on_site_text() -> String {
    crate::i18n::tr("AUR didn't let us read the comments here (bot protection or no connection) — they are on the website.")
}

/// Кнопка «Подробнее» на плитке пакета.
pub fn details_btn() -> String {
    crate::i18n::tr("📄 Details")
}

/// Подсказка к ней.
pub fn details_tooltip() -> String {
    crate::i18n::tr("Package card: description, dependencies, comments")
}

/// Пакет не найден в AUR (удалён или переименован).
pub fn info_missing_text() -> String {
    crate::i18n::tr("AUR doesn't know this package anymore — it may have been removed or renamed.")
}
