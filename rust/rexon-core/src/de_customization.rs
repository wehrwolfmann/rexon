//! Матрица кастомизации рабочих столов: «тип → куда положить → чем включить →
//! что перезапустить» — единый источник правды для универсального `.rxn`.
//!
//! Зачем отдельный модуль. До него в Rexon было ДВЕ параллельные таблицы типов
//! (`constants.rs` — пути `.rxn`, `categories.rs` — каталог «Оформления»), обе
//! заточенные под KDE и под «просто скопировать каталог». Для GNOME, Cinnamon,
//! XFCE, MATE и LXQt копирования НЕДОСТАТОЧНО: установка там значит разное —
//! расширение оболочки GNOME включается `gnome-extensions enable` и требует
//! перезахода на Wayland, апплет Cinnamon дописывается строкой в список
//! `org.cinnamon enabled-applets` со своим счётчиком идентификаторов, тема
//! xfwm4 включается `xfconf-query`, скрипт KWin — ключом в `kwinrc` плюс
//! `reconfigure` по D-Bus. Складывать это ветками `if` в установщик — то же
//! размножение логики, которое запрещает правило #12; поэтому различия описаны
//! ДАННЫМИ здесь, а установщик (`ext_manager`) остаётся одним generic-путём.
//!
//! Таблица покрывает и то, что через `.rxn` установить НЕЛЬЗЯ (апплеты XFCE,
//! MATE, Budgie, LXQt — нативные плагины, их надо компилировать; тема GDM —
//! перепаковка `gresource` от root). Такие строки помечены [`Support`] и в
//! `VALID_TYPES` не попадают: пользователю честно объясняется причина, вместо
//! молчаливого «установлено», после которого ничего не появляется.
//!
//! Человеческие подписи для интерфейса и документации выводятся ИЗ машинных
//! полей ([`Activate::hint`], [`Refresh::hint`]) — чтобы текст не разъезжался
//! с тем, что установщик делает на самом деле.
//!
//! ⚠️ Матрица описывает ВСЕ типы `.rxn` оформления, а не только те, установку
//! которых ведёт сама (`Setup::Matrix`). Раньше «безопасное оформление» (темы
//! GTK, иконки, курсоры, шрифты, обои…) жило отдельным списком в
//! `constants.rs` — две таблицы про одно и то же, ровно то, что запрещает
//! правило #12. Теперь список ОДИН: `VALID_TYPES`, категория «Оформление» и
//! пути установки ВЫВОДЯТСЯ отсюда.
//!
//! Каждая строка отвечает на ДВА разных вопроса, и оба обязательны:
//!   * [`Row::de`] — чьё это хозяйство, то есть куда кладётся файл и чем
//!     включается (у темы GTK каталог общий — [`De::Any`], у плазмоида
//!     KDE-шный);
//!   * [`Row::applies`] — в каких окружениях тип вообще имеет смысл (тема GTK
//!     работает ВЕЗДЕ, включая KDE — под Plasma живут Firefox, GIMP, Inkscape,
//!     а оформление Budgie ею и задаётся; плазмоид — только в KDE).
//!
//! Это разные факты: общий каталог не значит «работает везде» (тема Kvantum
//! лежит в своём `~/.config/Kvantum`, но осмысленна только там, где Qt-стиль
//! выбирается штатно), а KDE-шный каталог не значит «только KDE».
//!
//! ЯЗЫК. Тексты для покупателя ([`De::display_label`], [`Applies::hint`],
//! [`Caveat::text`], [`unavailable_note`], [`condition_note`]) идут через
//! каталог перевода (`crate::i18n`): исходный язык — английский, русский — в
//! `po/ru.po`. Поля строк матрицы (`what`, `note`, подсказки активации и
//! перезапуска, причины отказа) — справочник для разработчика: из них
//! собирается `docs/de_customization_matrix.md`, покупателю они не
//! показываются и потому остаются по-русски.

use crate::i18n::{n_, tr, trf};

// ============================================================================
// Окружения
// ============================================================================

/// Рабочий стол, к которому привязан тип кастомизации.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum De {
    Kde,
    Gnome,
    Cinnamon,
    Xfce,
    Mate,
    Budgie,
    /// Pantheon (elementary OS): Gala на Mutter, оформление — ключами GNOME.
    Pantheon,
    /// Deepin (DDE): оформление — служба DDE на шине сеанса и DConfig
    /// ([`crate::dde`]).
    Deepin,
    Lxqt,
    Cosmic,
    /// Композиторы на wlr-layer-shell (sway, Hyprland, niri, river, labwc…).
    ///
    /// Это канонический ответ на вопрос «что это за композитор». Рядом в
    /// продукте живут ПОХОЖИЕ, но другие наборы — сливать их нельзя, они
    /// отвечают каждый на свой вопрос:
    ///
    /// * [`crate::appdefaults::is_tiling_wm`] — «нет своего набора родных
    ///   программ» (плюс любое НЕизвестное окружение);
    /// * [`crate::wallpaper_backend::DAEMON_LAYER_SHELL_DESKTOPS`] — «демон
    ///   обоев поднимет layer-shell сам» (там законно есть cosmic, которого
    ///   здесь нет, и нет labwc);
    /// * `wallpaper_engine._WLROOTS_DE` в Python — «статичную картинку ставит
    ///   внешний демон swww/hyprpaper/swaybg/wbg» (шире всех: ещё wayfire);
    /// * `qt_desktops._TILING_IDS` — «какие карточки показать в секции
    ///   „Тайловые“ и в каком порядке» (состав сверяется с ядром сторожем).
    Wlroots,
    /// Работает везде (общие каталоги freedesktop: иконки, шрифты, курсоры).
    Any,
}

impl De {
    pub fn key(self) -> &'static str {
        match self {
            De::Kde => "kde",
            De::Gnome => "gnome",
            De::Cinnamon => "cinnamon",
            De::Xfce => "xfce",
            De::Mate => "mate",
            De::Budgie => "budgie",
            De::Pantheon => "pantheon",
            De::Deepin => "deepin",
            De::Lxqt => "lxqt",
            De::Cosmic => "cosmic",
            De::Wlroots => "wlroots",
            De::Any => "any",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            De::Kde => "KDE Plasma",
            De::Gnome => "GNOME",
            De::Cinnamon => "Cinnamon",
            De::Xfce => "XFCE",
            De::Mate => "MATE",
            De::Budgie => "Budgie",
            De::Pantheon => "Pantheon",
            De::Deepin => "Deepin",
            De::Lxqt => "LXQt",
            De::Cosmic => "COSMIC",
            De::Wlroots => "Тайловые и Wayfire (sway/Hyprland/niri/River)",
            De::Any => "Любое окружение",
        }
    }

    /// Подпись на языке интерфейса (правило #13: человеку — на его языке).
    /// [`De::label`] — та же подпись для справочника разработчика.
    pub fn display_label(self) -> String {
        match self {
            De::Wlroots => tr("Tiling and Wayfire (sway/Hyprland/niri/River)"),
            De::Any => tr("Any desktop"),
            // Остальные — имена собственные, они одинаковы на всех языках.
            other => other.label().to_string(),
        }
    }

    /// Все реальные окружения матрицы (без [`De::Any`] — это не окружение, а
    /// «общий каталог»). Порядок — как в перечислении.
    pub const ALL: &'static [De] = &[
        De::Kde,
        De::Gnome,
        De::Cinnamon,
        De::Xfce,
        De::Mate,
        De::Budgie,
        De::Pantheon,
        De::Deepin,
        De::Lxqt,
        De::Cosmic,
        De::Wlroots,
    ];
}

/// Идентификатор рабочего стола Rexon (`system::DE_DATABASE`) → окружение
/// матрицы. Тайловые сведены в [`De::Wlroots`]: оформляются они одинаково, а
/// перечисляются по отдельности только в интерфейсе — покупатель мыслит своим
/// рабочим столом, а не именем протокола.
pub fn de_from_id(de_id: &str) -> Option<De> {
    Some(match de_id {
        "kde" => De::Kde,
        "gnome" => De::Gnome,
        "cinnamon-mint" | "cinnamon" => De::Cinnamon,
        "xfce" => De::Xfce,
        "mate" => De::Mate,
        "budgie" => De::Budgie,
        "pantheon" => De::Pantheon,
        "deepin" => De::Deepin,
        "lxqt" => De::Lxqt,
        "cosmic" => De::Cosmic,
        // wayfire — тоже wlroots-композитор; раньше его знал только движок
        // обоев, и с общим опознаванием окружения (`system`) он бы
        // проваливался в «неизвестное».
        "hyprland" | "sway" | "niri" | "river" | "labwc" | "wayfire" => De::Wlroots,
        _ => return None,
    })
}

// ============================================================================
// Применимость
// ============================================================================

/// Где действует оговорка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaveatAt {
    /// Только в перечисленных окружениях.
    In(&'static [De]),
    /// Во всех, кроме перечисленных.
    Except(&'static [De]),
}

/// Оговорка: тип работает, но при условии — и условие надо назвать ЗАРАНЕЕ.
///
/// Вторая роль рядом с [`unavailable_note`]: та объясняет, почему раздела нет
/// вовсе, эта — на каких условиях он работает. Прятать работающее вместо того,
/// чтобы объяснить условие, мы не будем: человеку нужен звук в тайловом
/// композиторе и тема Kvantum под GNOME, а не пустой каталог.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caveat {
    pub at: CaveatAt,
    /// Текст условия — `msgid` каталога (правило #13: на языке человека);
    /// переводится в [`Caveat::text`].
    pub msgid: &'static str,
}

impl Caveat {
    /// Действует ли оговорка в этом окружении.
    pub fn covers(&self, de: De) -> bool {
        match self.at {
            CaveatAt::In(xs) => xs.contains(&de),
            CaveatAt::Except(xs) => de != De::Any && !xs.contains(&de),
        }
    }

    /// Окружения, где оговорка действует — для интерфейса и документации.
    pub fn list(&self) -> Vec<De> {
        De::ALL.iter().copied().filter(|d| self.covers(*d)).collect()
    }

    /// Текст условия на языке интерфейса.
    pub fn text(&self) -> String {
        tr(self.msgid)
    }
}

/// Окружения, в которых тип оформления вообще имеет смысл.
///
/// Отвечает на вопрос «покажем ли мы это покупателю», а не «куда положим
/// файл»: второе — [`Row::de`]. Заводя новый тип, ответить придётся на оба —
/// поля структуры обязательные, значения по умолчанию не предусмотрены.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applies {
    /// Во всех окружениях, которые Rexon умеет ставить.
    Everywhere,
    /// Везде, кроме перечисленных.
    EverywhereExcept(&'static [De]),
    /// Только в перечисленных.
    Only(&'static [De]),
    /// Везде, но в части окружений — с оговоркой: работает при условии,
    /// которое человеку надо назвать (см. [`Caveat`]).
    EverywhereWith(Caveat),
    /// Как [`Applies::EverywhereWith`], но в перечисленных окружениях типа
    /// нет вовсе (почему — [`unavailable_note`], своя причина в
    /// [`UNAVAILABLE_REASONS`]).
    EverywhereWithExcept(&'static [De], Caveat),
}

impl Applies {
    /// Имеет ли тип смысл в этом окружении.
    pub fn covers(self, de: De) -> bool {
        match self {
            Applies::Everywhere | Applies::EverywhereWith(_) => de != De::Any,
            Applies::EverywhereExcept(xs) | Applies::EverywhereWithExcept(xs, _) => {
                de != De::Any && !xs.contains(&de)
            }
            Applies::Only(xs) => xs.contains(&de),
        }
    }

    /// Оговорка для этого окружения — `None`, когда условий нет.
    pub fn caveat(self, de: De) -> Option<Caveat> {
        match self {
            Applies::EverywhereWith(c) if c.covers(de) => Some(c),
            Applies::EverywhereWithExcept(xs, c) if c.covers(de) && !xs.contains(&de) => Some(c),
            _ => None,
        }
    }

    /// Список окружений применимости — для интерфейса и документации.
    pub fn list(self) -> Vec<De> {
        De::ALL.iter().copied().filter(|d| self.covers(*d)).collect()
    }

    /// Перечисление окружений через запятую на языке интерфейса.
    pub fn hint(self) -> String {
        let names = |xs: Vec<De>| -> String {
            xs.into_iter()
                .map(|d| d.display_label())
                .collect::<Vec<_>>()
                .join(", ")
        };
        match self {
            Applies::Everywhere => tr("in every desktop"),
            Applies::EverywhereWith(c) => trf(
                // TRANSLATORS: %1 is a list of desktops, e.g. “GNOME, XFCE”.
                "in every desktop (with a caveat in %1)",
                &[&names(c.list())],
            ),
            _ => names(self.list()),
        }
    }
}

// ============================================================================
// Кто ставит
// ============================================================================

/// Кем выполняется установка типа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setup {
    /// Generic-путь матрицы: скопировать по [`Payload`] и включить
    /// [`Activate`] — один код на все окружения (правило #12).
    Matrix,
    /// Общий установщик оформления: копирование плюс свои шаги, которые из
    /// таблицы не выражаются (pkexec для экранов входа и загрузки,
    /// `~/.icons/default` у курсоров, разбор комплектов тем).
    Appearance,
}

// ============================================================================
// Поддержка
// ============================================================================

/// Можно ли установить тип из `.rxn` правами пользователя.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Ставится из `.rxn` без root — тип есть в `VALID_TYPES`.
    Rxn,
    /// Технически ставится, но только от root (правка системных каталогов).
    RootOnly(&'static str),
    /// Через `.rxn` не раздаётся: нужна сборка нативного плагина под текущую
    /// версию окружения.
    NeedsBuild(&'static str),
}

impl Support {
    pub fn installable(self) -> bool {
        matches!(self, Support::Rxn)
    }

    /// Причина отказа — текст для интерфейса (правило #13: объяснять, а не
    /// молча ничего не делать).
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Support::Rxn => None,
            Support::RootOnly(r) | Support::NeedsBuild(r) => Some(r),
        }
    }
}

// ============================================================================
// Что лежит внутри data/
// ============================================================================

/// Форма полезной нагрузки в `data/` пакета `.rxn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Payload {
    /// Каждая подпапка `data/` — самостоятельный объект, имя каталога = имя
    /// объекта (темы, look-and-feel, курсоры).
    Dir,
    /// Имя каталога назначения берётся из метаданных, а не из имени папки:
    /// GNOME/Cinnamon требуют, чтобы каталог назывался ровно как `uuid`.
    DirUuid { meta: &'static str, field: &'static str },
    /// KDE-пакет: ставится `kpackagetool6 --type <ptype>`, имя каталога —
    /// `KPlugin.Id` из `metadata.json`. Копирование — запасной путь.
    KPackage { ptype: &'static str, id_field: &'static str },
    /// Тема-«надстройка» над каталогом `~/.themes/<Имя>`: в `.rxn` лежит
    /// готовый каталог темы, значение имеет подкаталог `sub`.
    ThemeSubdir { sub: &'static str },
    /// Отдельные файлы с указанным расширением (действия Nemo).
    Files { ext: &'static str },
}

// ============================================================================
// Чем включить
// ============================================================================

/// Способ активации после копирования.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activate {
    /// Ничего не нужно — объект подхватывается сам.
    Auto,
    /// Применяет сам Rexon — кнопкой «Применить» в каталоге оформления
    /// (правило #13: нажал кнопку — тема встала, никаких команд руками).
    /// Конкретная команда зависит от окружения и живёт в установщике тем.
    Rexon,
    /// Только вручную из настроек окружения (выбор пользователя, не наше
    /// дело решать за него).
    Manual(&'static str),
    /// `kwriteconfig6 --file <file> --group <group> --key <key> <имя>`.
    KdeConfig {
        file: &'static str,
        group: &'static str,
        key: &'static str,
        /// Дополнительная пара ключ=значение того же файла/группы (движок
        /// заставки: `Engine=KSplashQML`).
        extra: Option<(&'static str, &'static str)>,
    },
    /// Ключ `<id>Enabled` в `kwinrc` группы `Plugins` (скрипты и эффекты KWin).
    KwinPlugin,
    /// `gsettings set <schema> <key> '<имя>'`.
    Gsettings { schema: &'static str, key: &'static str },
    /// Дописать элемент в список `gsettings` (апплеты/десклеты/расширения).
    GsettingsList {
        schema: &'static str,
        key: &'static str,
        entry: EntryFmt,
        /// Ключ-счётчик идентификаторов экземпляров (`next-applet-id`).
        counter: Option<&'static str>,
    },
    /// `gnome-extensions enable <uuid>`; запасной путь — список
    /// `org.gnome.shell enabled-extensions`.
    GnomeExtension,
    /// `xfconf-query -c <channel> -p <prop> -s <имя>`.
    Xfconf { channel: &'static str, prop: &'static str },
    /// Ключ в ini-файле конфигурации (`~/.config/lxqt/lxqt.conf`).
    IniKey { file: &'static str, group: &'static str, key: &'static str },
    /// `<theme><name>` в `rc.xml` менеджера окон сеанса — Openbox
    /// (`openbox/rc.xml`) или labwc (`labwc/rc.xml`), затем его
    /// `--reconfigure`. Какой из них, решает сеанс
    /// (`qt_settings::session_title_wm`), правит файл `crate::de_apply`.
    RcXmlTheme,
}

/// Формат строки-элемента списка `gsettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryFmt {
    /// Просто `uuid` (расширения GNOME и Cinnamon).
    Uuid,
    /// `panel1:<зона>:<порядок>:<uuid>:<номер экземпляра>` — апплет Cinnamon.
    CinnamonApplet,
    /// `<uuid>:<номер экземпляра>:<x>:<y>` — десклет Cinnamon.
    CinnamonDesklet,
}

impl Activate {
    /// Человекочитаемое «чем включить» — для интерфейса и документации.
    pub fn hint(self) -> String {
        match self {
            Activate::Auto => "подхватывается автоматически".into(),
            Activate::Rexon => "Rexon применяет сам (кнопка «Применить»)".into(),
            Activate::Manual(where_) => format!("вручную: {where_}"),
            Activate::KdeConfig { file, group, key, extra } => {
                let mut s = format!("kwriteconfig6 --file {file} --group {group} --key {key} <имя>");
                if let Some((k, v)) = extra {
                    s.push_str(&format!(" (+ {k}={v})"));
                }
                s
            }
            Activate::KwinPlugin => {
                "kwriteconfig6 --file kwinrc --group Plugins --key <id>Enabled true".into()
            }
            Activate::Gsettings { schema, key } => {
                format!("gsettings set {schema} {key} '<имя>'")
            }
            Activate::GsettingsList { schema, key, entry, counter } => {
                let e = match entry {
                    EntryFmt::Uuid => "<uuid>",
                    EntryFmt::CinnamonApplet => "panel1:<зона>:<порядок>:<uuid>:<номер>",
                    EntryFmt::CinnamonDesklet => "<uuid>:<номер>:<x>:<y>",
                };
                let c = counter
                    .map(|c| format!(" (+ счётчик {c})"))
                    .unwrap_or_default();
                format!("добавить «{e}» в список {schema} {key}{c}")
            }
            Activate::GnomeExtension => "gnome-extensions enable <uuid>".into(),
            Activate::Xfconf { channel, prop } => {
                format!("xfconf-query -c {channel} -p {prop} -s <имя>")
            }
            Activate::IniKey { file, group, key } => {
                format!("{file}: [{group}] {key}=<имя>")
            }
            Activate::RcXmlTheme => {
                "узел theme/name = <имя> в openbox/rc.xml или labwc/rc.xml + --reconfigure".into()
            }
        }
    }
}

// ============================================================================
// Что перезапустить
// ============================================================================

/// Что нужно перезапустить, чтобы изменение стало видно.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refresh {
    /// Ничего — применяется на лету.
    Nothing,
    /// `qdbus org.kde.KWin /KWin reconfigure`.
    KwinReconfigure,
    /// `plasmashell --replace` (перезапуск оболочки Plasma).
    PlasmashellReplace,
    /// `cinnamon --replace` на X11; на Wayland — только перезаход.
    CinnamonReplace,
    /// GNOME: на X11 `Alt+F2 → r`, на Wayland перезапуск оболочки невозможен —
    /// нужен перезаход в сеанс.
    GnomeShell,
    /// Перезапуск панели LXQt.
    LxqtPanel,
    /// Применяется при следующем входе в систему.
    NextLogin,
    /// Применяется при следующей блокировке экрана.
    NextLock,
}

impl Refresh {
    pub fn hint(self) -> &'static str {
        match self {
            Refresh::Nothing => "ничего, применяется сразу",
            Refresh::KwinReconfigure => "qdbus org.kde.KWin /KWin reconfigure",
            Refresh::PlasmashellReplace => "plasmashell --replace",
            Refresh::CinnamonReplace => "cinnamon --replace (Wayland — перезаход)",
            Refresh::GnomeShell => "Alt+F2 → r на X11; на Wayland — перезаход",
            Refresh::LxqtPanel => "перезапуск lxqt-panel",
            Refresh::NextLogin => "следующий вход в систему",
            Refresh::NextLock => "следующая блокировка экрана",
        }
    }
}

// ============================================================================
// Строка матрицы
// ============================================================================

/// Одна строка матрицы: тип кастомизации в конкретном окружении.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    /// Тип `.rxn` (`VALID_TYPES`). Пустая строка — тип не раздаётся через
    /// `.rxn` (см. [`Row::support`]).
    pub kind: &'static str,
    /// Чьё хозяйство: куда кладётся файл и чем включается. [`De::Any`] —
    /// каталог общий (freedesktop или свой у самой программы). НЕ путать с
    /// применимостью: см. [`Row::applies`].
    pub de: De,
    /// В каких окружениях тип имеет смысл. Отдельный факт от [`Row::de`]:
    /// тема GTK лежит в общем каталоге и работает везде, тема Kvantum лежит
    /// в общем каталоге, но осмысленна только в KDE и LXQt.
    pub applies: Applies,
    /// Что это такое, по-русски (правило #13 — без жаргона в интерфейсе).
    pub what: &'static str,
    /// Куда положить: путь от `$HOME` (без ведущего `~/`) либо абсолютный
    /// системный путь для строк, требующих root. Пусто — устанавливать
    /// нечего, содержимое применяется на месте (обои).
    pub dest: &'static str,
    pub payload: Payload,
    pub activate: Activate,
    pub refresh: Refresh,
    pub support: Support,
    /// Кто ведёт установку: generic-путь матрицы или общий установщик
    /// оформления.
    pub setup: Setup,
    /// Подводные камни, которые иначе всплывут у пользователя.
    pub note: &'static str,
}

impl Row {
    /// Абсолютный путь назначения от переданного `$HOME`.
    pub fn dest_path(&self, home: &str) -> String {
        if self.dest.starts_with('/') {
            self.dest.to_string()
        } else {
            format!("{home}/{}", self.dest)
        }
    }

    /// Имеет ли тип смысл в этом окружении.
    pub fn covers(&self, de: De) -> bool {
        self.applies.covers(de)
    }

    /// Условие, при котором тип работает в этом окружении (`None` — условий
    /// нет).
    pub fn caveat(&self, de: De) -> Option<Caveat> {
        self.applies.caveat(de)
    }
}

/// Полная матрица кастомизации: и то, что мы ставим, и то, что не ставим.
///
/// Порядок — по окружениям (KDE → GNOME → Cinnamon → XFCE → MATE → Budgie →
/// Pantheon → Deepin → LXQt → COSMIC → тайловые), внутри окружения от самого ходового к редкому.
pub const ROWS: &[Row] = &[
    // ── Общее для всех окружений ────────────────────────────────────────
    // Каталоги freedesktop (и свои каталоги самих программ): куда положить —
    // одинаково везде. Применимость всё равно указана явно: общий каталог не
    // значит «работает везде» (Kvantum).
    Row {
        kind: "theme-gtk",
        de: De::Any,
        applies: Applies::EverywhereWith(Caveat {
            at: CaveatAt::In(&[De::Pantheon]),
            msgid: n_("Pantheon's own settings service puts the elementary style back \
                 over the GTK theme: at every login, whenever you change the accent \
                 color and — with the automatic accent — whenever the wallpaper \
                 changes. Until then your theme stays."),
        }),
        what: "Тема GTK (вид окон и виджетов GTK-приложений)",
        dest: ".themes",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Нужна ВЕЗДЕ, включая KDE: под Plasma живут Firefox, GIMP, \
               Inkscape, а в системных настройках есть модуль «Оформление \
               приложений GNOME/GTK» (kde-gtk-config). Оформление Budgie \
               задаётся именно темой GTK. ⚠️ Pantheon: служба \
               io.elementary.settings-daemon (AccentColorManager) при каждом \
               старте, смене акцента и — при акценте «авто» — смене обоев \
               пишет в org.gnome.desktop.interface gtk-theme свой \
               io.elementary.stylesheet.<цвет>; выключателя у неё нет.",
    },
    Row {
        kind: "theme-icons",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Набор значков",
        dest: ".local/share/icons",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Каталог общий с курсорами: и то, и другое — «темы значков» по \
               спецификации freedesktop.",
    },
    Row {
        kind: "theme-cursors",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Указатели мыши",
        dest: ".local/share/icons",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Кроме выбора в настройках окружения пишется ~/.icons/default — \
               иначе приложения на чистом X11 и часть Wayland-клиентов \
               остаются со старым курсором.",
    },
    Row {
        kind: "theme-sounds",
        de: De::Any,
        applies: Applies::EverywhereWith(Caveat {
            at: CaveatAt::In(&[De::Wlroots]),
            msgid: n_("Event sounds are played by a notification daemon (swaync, \
                 mako, dunst) or by the app itself, not by the desktop shell. \
                 On tiling compositors you start that daemon yourself — until \
                 you do, the theme stays silent."),
        }),
        what: "Звуковая тема (звуки событий системы)",
        dest: ".local/share/sounds",
        payload: Payload::Dir,
        // Rexon применяет тему сам — обе вкладки, «Оформление» и
        // «Настройки», пишут родной ключ окружения общим планом
        // `qt_settings::sound_plan` (Д11, проверка окружений 02.10.2026:
        // матрица всё ещё говорила «вручную»).
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Каталог общий по спецификации freedesktop, оболочке звуковая \
               тема не принадлежит: её берут демоны уведомлений и программы \
               на libcanberra — ровно так же, как иконки и курсоры берут \
               GTK- и Qt-приложения.",
    },
    Row {
        kind: "theme-kvantum",
        de: De::Any,
        // Deepin: стиль Qt на весь сеанс отнял бы у программ DTK их Chameleon
        // (`themes::QtStyleWay::Deepin`) — раздела там нет, причина названа.
        applies: Applies::EverywhereWithExcept(&[De::Deepin], Caveat {
            at: CaveatAt::Except(&[De::Kde, De::Lxqt, De::Deepin]),
            msgid: n_("Every Qt app (VLC, qBittorrent, Krita, OBS) will pick \
                 the theme up. This desktop has no Qt style switch of its own, \
                 so Rexon sets one up for you — but the change only shows up \
                 after your next login."),
        }),
        what: "Тема Kvantum (вид Qt-приложений)",
        dest: ".config/Kvantum",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Kvantum — движок стилей Qt (libkvantum.so в каталоге плагинов \
               Qt), он красит ЛЮБОЕ Qt-приложение независимо от оболочки: под \
               GNOME это VLC, qBittorrent, Krita, OBS. Разница между \
               окружениями не в том, работает ли он, а в том, ЧЕМ выбирается \
               стиль: KDE — widgetStyle в kdeglobals, LXQt — [Qt] style в \
               lxqt.conf, остальные — QT_STYLE_OVERRIDE в \
               ~/.config/environment.d. В Deepin раздела нет: переменная на \
               весь сеанс отняла бы у программ DTK их стиль Chameleon.",
    },
    Row {
        kind: "theme-login-screen",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Оформление экрана входа в систему",
        dest: ".local/share/rexon/login-themes",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::NextLogin,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Экран входа рисует менеджер входа, а не рабочий стол: Rexon \
               сам определяет, что стоит (SDDM, GDM или LightDM), и просит \
               пароль только на запись в системный каталог.",
    },
    Row {
        kind: "theme-plymouth",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Заставка загрузки компьютера (Plymouth)",
        dest: ".local/share/rexon/plymouth-themes",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::NextLogin,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Показывается до входа в систему и от рабочего стола не зависит \
               вообще; установка требует пароля и пересборки образа ядра.",
    },
    Row {
        kind: "theme-conky",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Виджет Conky (часы, датчики на столе)",
        dest: ".config/conky",
        payload: Payload::Dir,
        activate: Activate::Manual("запуск conky с этим файлом настроек"),
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Conky — отдельная программа, а не часть рабочего стола, \
               поэтому работает и там, где своих виджетов нет.",
    },
    Row {
        kind: "theme-bundle",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Комплект оформления (несколько тем одним пакетом)",
        dest: ".local/share/rexon/bundles",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Внутри — обычные типы этой же таблицы; каждый ставится своим \
               путём, а негодные для текущего окружения пропускаются.",
    },
    Row {
        kind: "wallpaper-pack",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Набор обоев",
        dest: "",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Своего каталога установки нет: обои применяются прямо из \
               пакета — их ставит движок обоев Rexon, одинаково во всех \
               окружениях.",
    },
    Row {
        kind: "wallpaper-video-adaptive",
        de: De::Any,
        applies: Applies::EverywhereWith(Caveat {
            at: CaveatAt::In(&[De::Pantheon]),
            msgid: n_("Live wallpapers work in the Pantheon “Classic Session” (X11). \
                 In the “Secure Session” (Wayland) the Gala window manager doesn't \
                 let other programs draw the desktop background, so only a still \
                 picture can be set there."),
        }),
        what: "Живые обои (видео под разрешение и язык)",
        dest: ".local/share/rexon/wallpapers",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Рисует движок обоев Rexon, а не рабочий стол, поэтому работает \
               и в GNOME с Cinnamon (через наше расширение оболочки), и в \
               тайловых (слой layer-shell). Pantheon на Wayland — нет: Gala \
               знает только свой протокол pantheon-desktop-shell, \
               wlr-layer-shell у неё нет, а модулей-расширений она не грузит \
               (`wallpaper_backend::pick` → Unsupported).",
    },
    Row {
        kind: "font-pack",
        de: De::Any,
        applies: Applies::Everywhere,
        what: "Набор шрифтов",
        dest: ".local/share/fonts",
        payload: Payload::Dir,
        activate: Activate::Auto,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "",
    },
    // ── KDE Plasma ──────────────────────────────────────────────────────
    Row {
        kind: "theme-kde-global",
        de: De::Kde,
        // Вне Plasma — по частям, теми же путями, что у «Как в
        // дистрибутиве» (`themes::apply_global_choice_plan`): цвета, стиль
        // приложений, значки, курсоры, шрифты, кнопки заголовка; что не
        // переносится, окно «Применить «X»?» называет с причиной.
        applies: Applies::EverywhereWith(Caveat {
            at: CaveatAt::Except(&[De::Kde]),
            msgid: n_("A global theme is a Plasma package. Here Rexon applies its parts one by one — \
                 colors, application style, icons, pointers, fonts and title bar buttons; \
                 the window that opens before applying says what carries over."),
        }),
        what: "Полная тема Plasma (Global Theme)",
        dest: ".local/share/plasma/look-and-feel",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Меняет всё сразу — цвета, стиль Plasma, значки, заставку и \
               экран блокировки; поэтому перед применением Rexon делает \
               снимок панели (правило #13).",
    },
    Row {
        kind: "theme-kde-colors",
        de: De::Kde,
        // Вне Plasma схему применяет слой Rexon (`shell_palette::apply_scheme`):
        // kdeglobals для KDE-программ, палитра qt6ct (в LXQt — его родная
        // палитра) для остальных Qt-программ. В Deepin раздела нет — причина
        // в `UNAVAILABLE_REASONS`.
        applies: Applies::EverywhereWithExcept(&[De::Deepin], Caveat {
            at: CaveatAt::Except(&[De::Kde, De::Deepin]),
            // TRANSLATORS: Outside Plasma: the Color Schemes section note (GTK apps can't see KDE color schemes).
            msgid: n_("Outside Plasma, Rexon passes the scheme to KDE apps (Dolphin, \
                 Kate, Okular) and to the other Qt apps — some of them only pick it up \
                 after your next login. GTK apps (Firefox, GIMP) and most of the \
                 desktop's own apps don't read KDE color schemes and keep their own \
                 colors — for GTK apps, pick a matching theme in “GTK Themes”."),
        }),
        what: "Цветовая схема",
        dest: ".local/share/color-schemes",
        payload: Payload::Files { ext: ".colors" },
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "В пакете лежат отдельные файлы .colors, а не папки. Вне Plasma \
               Rexon вливает цвета схемы в ~/.config/kdeglobals теми же ключами, \
               что Plasma, плюс [UiSettings] ColorScheme (иначе программы KDE с \
               меню «Цветовая схема» сами ставят Breeze), и палитру Qt: LXQt — \
               [Palette] в lxqt.conf, остальные — qt6ct/qt5ct colors/*.conf. \
               GTK-программы схему KDE не читают.",
    },
    Row {
        kind: "theme-kde-plasma",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Стиль Plasma (панель, меню, всплывающие окна)",
        dest: ".local/share/plasma/desktoptheme",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "",
    },
    Row {
        kind: "theme-window-decoration",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Оформление рамок окон (Aurorae)",
        dest: ".local/share/aurorae/themes",
        payload: Payload::Dir,
        activate: Activate::Rexon,
        refresh: Refresh::KwinReconfigure,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Aurorae — движок рамок именно KWin; в других окружениях рамки \
               рисуются своими средствами (xfwm4, Marco, GTK).",
    },
    Row {
        kind: "theme-plasmoid",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Виджет Plasma (плазмоид)",
        dest: ".local/share/plasma/plasmoids",
        payload: Payload::KPackage {
            ptype: "Plasma/Applet",
            id_field: "KPlugin.Id",
        },
        activate: Activate::Manual("добавить виджет на панель или рабочий стол"),
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Appearance,
        note: "Роль плазмоидов в GNOME играют расширения оболочки, в Cinnamon \
               — апплеты и десклеты: это ДРУГИЕ типы, не совместимые между \
               собой.",
    },
    Row {
        kind: "theme-kde-splash",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Заставка загрузки сеанса (KSplash)",
        dest: ".local/share/plasma/look-and-feel",
        payload: Payload::Dir,
        activate: Activate::KdeConfig {
            file: "ksplashrc",
            group: "KSplash",
            key: "Theme",
            extra: Some(("Engine", "KSplashQML")),
        },
        refresh: Refresh::NextLogin,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Отдельного типа пакета у KDE нет: заставка — это look-and-feel \
               с одним каталогом contents/splash.",
    },
    Row {
        kind: "theme-kde-lockscreen",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Экран блокировки",
        dest: ".local/share/plasma/look-and-feel",
        payload: Payload::Dir,
        activate: Activate::KdeConfig {
            file: "kscreenlockerrc",
            group: "Greeter",
            key: "Theme",
            extra: None,
        },
        refresh: Refresh::NextLock,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Своей категории у экрана блокировки в KDE Store тоже нет — \
               он всегда часть look-and-feel (contents/lockscreen).",
    },
    Row {
        kind: "kde-kwin-script",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Скрипт KWin (поведение окон, тайлинг, жесты)",
        dest: ".local/share/kwin/scripts",
        payload: Payload::KPackage { ptype: "KWin/Script", id_field: "KPlugin.Id" },
        activate: Activate::KwinPlugin,
        refresh: Refresh::KwinReconfigure,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Имя ключа в kwinrc — не имя папки, а KPlugin.Id из metadata.json.",
    },
    Row {
        kind: "kde-kwin-effect",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Эффект рабочего стола KWin",
        dest: ".local/share/kwin/effects",
        payload: Payload::KPackage { ptype: "KWin/Effect", id_field: "KPlugin.Id" },
        activate: Activate::KwinPlugin,
        refresh: Refresh::KwinReconfigure,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Эффекты на QML грузятся без сборки; эффекты на C++ — нативные \
               плагины, они через .rxn не раздаются.",
    },
    Row {
        kind: "kde-window-switcher",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Переключатель окон (Alt+Tab)",
        dest: ".local/share/kwin/tabbox",
        payload: Payload::KPackage { ptype: "KWin/WindowSwitcher", id_field: "KPlugin.Id" },
        activate: Activate::KdeConfig {
            file: "kwinrc",
            group: "TabBox",
            key: "LayoutName",
            extra: None,
        },
        refresh: Refresh::KwinReconfigure,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "",
    },
    Row {
        kind: "kde-wallpaper-plugin",
        de: De::Kde,
        applies: Applies::Only(&[De::Kde]),
        what: "Плагин обоев Plasma (свой движок фона)",
        dest: ".local/share/plasma/wallpapers",
        payload: Payload::KPackage { ptype: "Plasma/Wallpaper", id_field: "KPlugin.Id" },
        activate: Activate::Manual(
            "правой кнопкой по столу → «Настроить рабочий стол» → тип обоев",
        ),
        refresh: Refresh::PlasmashellReplace,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Выбор плагина хранится в конфигурации КАЖДОГО контейнера \
               (plasma-org.kde.plasma.desktop-appletsrc), автоматически \
               переключать его небезопасно — можно снести настройку стола.",
    },
    // ── GNOME ───────────────────────────────────────────────────────────
    Row {
        kind: "gnome-shell-extension",
        de: De::Gnome,
        applies: Applies::Only(&[De::Gnome]),
        what: "Расширение оболочки GNOME",
        dest: ".local/share/gnome-shell/extensions",
        payload: Payload::DirUuid { meta: "metadata.json", field: "uuid" },
        activate: Activate::GnomeExtension,
        refresh: Refresh::GnomeShell,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Каталог обязан называться ровно как uuid. Схемы настроек нужно \
               скомпилировать (glib-compile-schemas schemas/). Если версии \
               оболочки нет в shell-version, расширение молча не грузится.",
    },
    Row {
        kind: "theme-gnome-shell",
        de: De::Gnome,
        applies: Applies::Only(&[De::Gnome]),
        what: "Тема оболочки GNOME (верхняя панель, меню)",
        dest: ".themes",
        payload: Payload::ThemeSubdir { sub: "gnome-shell" },
        activate: Activate::Gsettings {
            schema: "org.gnome.shell.extensions.user-theme",
            key: "name",
        },
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Работает только при включённом расширении User Themes \
               (user-theme@gnome-shell-extensions.gcampax.github.com) — без \
               него ключ есть, а тема не применяется.",
    },
    Row {
        kind: "",
        de: De::Gnome,
        applies: Applies::Only(&[De::Gnome]),
        what: "Тема экрана входа GDM",
        dest: "/usr/share/gnome-shell/gnome-shell-theme.gresource",
        payload: Payload::ThemeSubdir { sub: "gnome-shell" },
        activate: Activate::Manual("подмена системного gresource"),
        refresh: Refresh::NextLogin,
        support: Support::RootOnly(
            "тема GDM вшита в бинарный gresource: нужно распаковать системный \
             файл, подменить стили, собрать обратно glib-compile-resources и \
             положить в /usr/share от root",
        ),
        setup: Setup::Matrix,
        note: "Обновление gnome-shell затирает подменённый файл — это ломает \
               экран входа до следующей переустановки темы.",
    },
    // ── Cinnamon ────────────────────────────────────────────────────────
    Row {
        kind: "cinnamon-applet",
        de: De::Cinnamon,
        applies: Applies::Only(&[De::Cinnamon]),
        what: "Апплет панели Cinnamon",
        dest: ".local/share/cinnamon/applets",
        payload: Payload::DirUuid { meta: "metadata.json", field: "uuid" },
        activate: Activate::GsettingsList {
            schema: "org.cinnamon",
            key: "enabled-applets",
            entry: EntryFmt::CinnamonApplet,
            counter: Some("next-applet-id"),
        },
        refresh: Refresh::CinnamonReplace,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "У каждого экземпляра свой номер; счётчик next-applet-id нужно \
               увеличить, иначе следующий апплет получит тот же номер и \
               настройки двух апплетов слипнутся.",
    },
    Row {
        kind: "cinnamon-desklet",
        de: De::Cinnamon,
        applies: Applies::Only(&[De::Cinnamon]),
        what: "Десклет (виджет на рабочем столе)",
        dest: ".local/share/cinnamon/desklets",
        payload: Payload::DirUuid { meta: "metadata.json", field: "uuid" },
        activate: Activate::GsettingsList {
            schema: "org.cinnamon",
            key: "enabled-desklets",
            entry: EntryFmt::CinnamonDesklet,
            counter: Some("next-desklet-id"),
        },
        refresh: Refresh::CinnamonReplace,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "В строке хранятся координаты на столе — ставим в левый верхний \
               угол со смещением, чтобы десклет не оказался под панелью.",
    },
    Row {
        kind: "cinnamon-extension",
        de: De::Cinnamon,
        applies: Applies::Only(&[De::Cinnamon]),
        what: "Расширение Cinnamon",
        dest: ".local/share/cinnamon/extensions",
        payload: Payload::DirUuid { meta: "metadata.json", field: "uuid" },
        activate: Activate::GsettingsList {
            schema: "org.cinnamon",
            key: "enabled-extensions",
            entry: EntryFmt::Uuid,
            counter: None,
        },
        refresh: Refresh::CinnamonReplace,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Апплеты, десклеты и расширения Cinnamon — ТРИ разных типа с \
               разными каталогами и разными списками включения.",
    },
    Row {
        kind: "theme-cinnamon",
        de: De::Cinnamon,
        applies: Applies::Only(&[De::Cinnamon]),
        what: "Тема оболочки Cinnamon",
        dest: ".themes",
        payload: Payload::ThemeSubdir { sub: "cinnamon" },
        activate: Activate::Gsettings { schema: "org.cinnamon.theme", key: "name" },
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "cinnamon.css несовместим с gnome-shell.css: одинаковых имён \
               классов там почти нет, тема GNOME в Cinnamon даёт голый вид.",
    },
    Row {
        kind: "nemo-action",
        de: De::Cinnamon,
        applies: Applies::Only(&[De::Cinnamon]),
        what: "Действие в контекстном меню файлового менеджера Nemo",
        dest: ".local/share/nemo/actions",
        payload: Payload::Files { ext: ".nemo_action" },
        activate: Activate::Auto,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Nemo следит за каталогом — действие появляется без перезапуска.",
    },
    // ── XFCE ────────────────────────────────────────────────────────────
    Row {
        kind: "theme-xfwm4",
        de: De::Xfce,
        applies: Applies::Only(&[De::Xfce]),
        what: "Тема оформления окон xfwm4",
        dest: ".themes",
        payload: Payload::ThemeSubdir { sub: "xfwm4" },
        activate: Activate::Xfconf { channel: "xfwm4", prop: "/general/theme" },
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Тема окон XFCE отдельна от темы GTK: их выбирают двумя разными \
               настройками, и .rxn может нести обе.",
    },
    Row {
        kind: "",
        de: De::Xfce,
        applies: Applies::Only(&[De::Xfce]),
        what: "Плагин панели XFCE",
        dest: "/usr/lib/xfce4/panel/plugins",
        payload: Payload::Dir,
        activate: Activate::Manual("добавить плагин в панель"),
        refresh: Refresh::Nothing,
        support: Support::NeedsBuild(
            "плагин панели XFCE — нативная библиотека .so под конкретную версию \
             xfce4-panel; её нужно компилировать и класть в системный каталог",
        ),
        setup: Setup::Matrix,
        note: "Исключение — внешние плагины-скрипты (genmon, generic monitor): \
               это не плагин, а настройка уже установленного плагина.",
    },
    // ── MATE ────────────────────────────────────────────────────────────
    Row {
        kind: "theme-marco",
        de: De::Mate,
        applies: Applies::Only(&[De::Mate]),
        what: "Тема оформления окон Marco",
        dest: ".themes",
        payload: Payload::ThemeSubdir { sub: "metacity-1" },
        activate: Activate::Gsettings { schema: "org.mate.Marco.general", key: "theme" },
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Marco — форк Metacity, поэтому каталог темы называется \
               metacity-1; те же темы работают в MATE и в старом GNOME 2. \
               Cinnamon их с версии 5.4 не читает: рамки Muffin берёт из \
               темы GTK.",
    },
    Row {
        kind: "",
        de: De::Mate,
        applies: Applies::Only(&[De::Mate]),
        what: "Апплет панели MATE",
        dest: "/usr/share/mate-panel/applets",
        payload: Payload::Dir,
        activate: Activate::Manual("добавить апплет на панель"),
        refresh: Refresh::Nothing,
        support: Support::NeedsBuild(
            "апплету MATE нужны описатель .mate-panel-applet и служба D-Bus в \
             /usr/share/dbus-1/services — оба каталога системные, от root",
        ),
        setup: Setup::Matrix,
        note: "",
    },
    // ── Budgie ──────────────────────────────────────────────────────────
    Row {
        kind: "",
        de: De::Budgie,
        applies: Applies::Only(&[De::Budgie]),
        what: "Апплет панели Budgie",
        dest: "/usr/lib/budgie-desktop/plugins",
        payload: Payload::Dir,
        activate: Activate::Manual("добавить апплет в панель"),
        refresh: Refresh::Nothing,
        support: Support::NeedsBuild(
            "апплеты Budgie — плагины libpeas, собранные под версию \
             budgie-desktop; каталог системный",
        ),
        setup: Setup::Matrix,
        note: "Оформление Budgie задаётся темой GTK — она через .rxn ставится \
               (тип «Тема GTK»).",
    },
    // ── Pantheon ────────────────────────────────────────────────────────
    Row {
        kind: "",
        de: De::Pantheon,
        applies: Applies::Only(&[De::Pantheon]),
        what: "Индикатор панели Wingpanel",
        dest: "/usr/lib/wingpanel",
        payload: Payload::Dir,
        activate: Activate::Manual("индикаторы подхватываются панелью сами"),
        refresh: Refresh::NextLogin,
        support: Support::NeedsBuild(
            "индикаторы Wingpanel — разделяемые библиотеки на Vala, собранные \
             под версию wingpanel; каталог системный",
        ),
        setup: Setup::Matrix,
        note: "Своих тем оболочки у Pantheon нет: вид задают тема GTK \
               (io.elementary.stylesheet.<акцент>), акцент \
               (io.elementary.settings-daemon.interface accent-color, в т.ч. \
               «авто» — по обоям) и тёмный режим с расписанием \
               (io.elementary.settings-daemon.prefers-color-scheme). Док — Plank \
               (net.launchpad.plank.dock.settings, темы в \
               ~/.local/share/plank/themes) — отдельным типом .rxn пока не \
               раздаётся.",
    },
    // ── Deepin ──────────────────────────────────────────────────────────
    Row {
        kind: "",
        de: De::Deepin,
        applies: Applies::Only(&[De::Deepin]),
        what: "Глобальная тема Deepin",
        dest: ".local/share/deepin-themes",
        payload: Payload::Dir,
        activate: Activate::Manual("Центр управления → Персонализация → Тема"),
        refresh: Refresh::Nothing,
        support: Support::NeedsBuild(
            "глобальная тема Deepin (index.theme с разделом [Deepin Theme]) \
             ссылается на темы окон, значков и курсоров по именам; отдельным \
             типом .rxn она пока не раздаётся",
        ),
        setup: Setup::Matrix,
        note: "Rexon поддерживает Deepin только на Treeland (сеанс Wayland, \
               ставит Rexon сам); сеанс X11 с deepin-kwin и прочие сеансы Deepin — \
               честный отказ с путём перехода. В Treeland темы GTK, значков и \
               курсора, шрифты, углы, прозрачность и акцент окон задаёт только \
               Центр управления (свой протокол Treeland, привилегированный) — \
               Rexon называет, где их менять. Обои стола и экрана блокировки — \
               через Центр управления; звуки — ключами DConfig самого Deepin. \
               Демон живых обоев своего слоя фона в Treeland не поднимает (DDE в \
               его списке нет).",
    },
    // ── LXQt ────────────────────────────────────────────────────────────
    Row {
        kind: "theme-lxqt",
        de: De::Lxqt,
        applies: Applies::Only(&[De::Lxqt]),
        what: "Тема LXQt (стили панели и оболочки)",
        dest: ".local/share/lxqt/themes",
        payload: Payload::Dir,
        activate: Activate::IniKey {
            file: ".config/lxqt/lxqt.conf",
            group: "General",
            key: "theme",
        },
        refresh: Refresh::LxqtPanel,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "В каталоге темы обязателен lxqt-panel.qss — без него панель \
               остаётся в старом стиле.",
    },
    // Рамки окон Openbox — их же читает labwc (labwc-theme(5): тема ищется
    // в `<имя>/labwc/`, затем в `<имя>/openbox-3/`). Openbox — менеджер окон
    // LXQt на X11, labwc — на Wayland и отдельный сеанс; среди тайловых
    // раздел виден только в labwc (`categories::category_available`).
    Row {
        kind: "theme-openbox",
        de: De::Lxqt,
        applies: Applies::Only(&[De::Lxqt, De::Wlroots]),
        what: "Тема рамок окон Openbox и labwc",
        dest: ".themes",
        payload: Payload::ThemeSubdir { sub: "openbox-3" },
        activate: Activate::RcXmlTheme,
        refresh: Refresh::Nothing,
        support: Support::Rxn,
        setup: Setup::Matrix,
        note: "Тема рамок Openbox — themerc и рисунки кнопок .xbm в openbox-3; \
               labwc понимает тот же формат. Свой rc.xml заменяет системный \
               целиком, поэтому без своего он заводится копией системного.",
    },
    Row {
        kind: "",
        de: De::Lxqt,
        applies: Applies::Only(&[De::Lxqt]),
        what: "Плагин панели LXQt",
        dest: "/usr/lib/lxqt-panel",
        payload: Payload::Dir,
        activate: Activate::Manual("добавить виджет в панель"),
        refresh: Refresh::LxqtPanel,
        support: Support::NeedsBuild(
            "плагин панели LXQt — библиотека C++ плюс .desktop-описатель, \
             собирается под версию lxqt-panel",
        ),
        setup: Setup::Matrix,
        note: "",
    },
    // ── COSMIC и тайловые ───────────────────────────────────────────────
    Row {
        kind: "",
        de: De::Cosmic,
        applies: Applies::Only(&[De::Cosmic]),
        what: "Тема COSMIC",
        dest: ".config/cosmic",
        payload: Payload::Dir,
        activate: Activate::Manual("настройки внешнего вида COSMIC"),
        refresh: Refresh::Nothing,
        support: Support::NeedsBuild(
            "формат тем COSMIC (файлы RON в ~/.config/cosmic) ещё меняется от \
             выпуска к выпуску — фиксировать его в .rxn рано",
        ),
        setup: Setup::Matrix,
        note: "Обои и значки COSMIC мы ставим уже сейчас — родными ключами: \
               картинку в com.system76.CosmicBackground/v1/all (её рисует \
               cosmic-bg), набор значков в com.system76.CosmicTk/v1/icon_theme.",
    },
    Row {
        kind: "",
        de: De::Wlroots,
        applies: Applies::Only(&[De::Wlroots]),
        what: "Конфигурация тайлового композитора",
        dest: ".config",
        payload: Payload::Dir,
        activate: Activate::Manual("правка личного конфига композитора"),
        refresh: Refresh::NextLogin,
        support: Support::NeedsBuild(
            "у sway/Hyprland/niri оформление задаётся личным файлом настроек, а \
             не устанавливаемым пакетом: подменять его пакетом — значит затирать \
             чужие настройки и оставлять пользователя без рабочего стола",
        ),
        setup: Setup::Matrix,
        note: "Обои и панель здесь ставятся отдельно (layer-shell), это \
               работает независимо от конфига.",
    },
];

// ============================================================================
// Доступ к таблице
// ============================================================================

/// Строка матрицы по типу `.rxn`. Строки без типа (несопровождаемые) не
/// возвращаются никогда.
pub fn row(kind: &str) -> Option<&'static Row> {
    if kind.is_empty() {
        return None;
    }
    ROWS.iter().find(|r| r.kind == kind)
}

/// Строка для generic-установщика окружений: только типы, установку которых
/// матрица ведёт сама (`Setup::Matrix`). Остальное оформление ставит общий
/// установщик тем — у него свои шаги (пароль на системные каталоги, разбор
/// комплектов), из таблицы они не выражаются.
pub fn de_managed_row(kind: &str) -> Option<&'static Row> {
    row(kind).filter(|r| matches!(r.setup, Setup::Matrix))
}

/// Все типы `.rxn` оформления, которые матрица описывает и Rexon ставит (в
/// порядке объявления). Источник `VALID_TYPES` и категории «Оформление».
pub fn kinds() -> Vec<&'static str> {
    ROWS.iter()
        .filter(|r| r.support.installable() && !r.kind.is_empty())
        .map(|r| r.kind)
        .collect()
}

/// Типы, которые ставит и включает generic-путь матрицы (`DE_CUSTOMIZATION_TYPES`).
pub fn de_managed_kinds() -> Vec<&'static str> {
    ROWS.iter()
        .filter(|r| {
            r.support.installable() && !r.kind.is_empty() && matches!(r.setup, Setup::Matrix)
        })
        .map(|r| r.kind)
        .collect()
}

/// Все строки, применимые в указанном окружении (включая неустанавливаемые —
/// их показывает раздел «что ещё бывает» и объяснение отказа).
///
/// Проверяется именно ПРИМЕНИМОСТЬ, а не «чьё хозяйство»: в KDE попадёт и
/// тема GTK (общий каталог, нужна везде), в GNOME не попадёт плазмоид.
pub fn rows_for(de: De) -> Vec<&'static Row> {
    ROWS.iter().filter(|r| r.applies.covers(de)).collect()
}

/// Строки, чьё хозяйство — указанное окружение (`Row::de`). Нужно разбору по
/// окружениям в документации, где важно «куда положить», а не «где имеет
/// смысл».
pub fn rows_hosted_by(de: De) -> Vec<&'static Row> {
    ROWS.iter().filter(|r| r.de == de).collect()
}

/// Каталог установки типа относительно `$HOME` — источник для `INSTALL_PATHS`.
/// `None` — типа нет в матрице ЛИБО каталога установки у него нет вовсе
/// (обои применяются на месте).
pub fn dest_suffix(kind: &str) -> Option<&'static str> {
    row(kind).map(|r| r.dest).filter(|d| !d.is_empty())
}

/// Свои причины, почему типа нет в окружении, — там, где общее «здесь это
/// ничего не делает» было бы неправдой (оно бы сделало, но сломав другое).
pub const UNAVAILABLE_REASONS: &[(&str, De, &str)] = &[
    (
        "theme-kvantum",
        De::Deepin,
        // TRANSLATORS: Why the Kvantum / Application Style sections are missing in Deepin.
        n_("Deepin's own apps (Control Center, file manager and the rest) draw \
            themselves with Deepin's Chameleon style. Switching the Qt style for the \
            whole session would take it away from them and break their look, so in \
            Deepin Rexon leaves the Qt style alone."),
    ),
    (
        "theme-kde-colors",
        De::Deepin,
        // TRANSLATORS: Why the Color Schemes section is missing in Deepin.
        n_("Deepin colors its apps itself, with its Chameleon style and its own \
            accent color in the Control Center. A KDE color scheme would only reach \
            a few KDE apps and leave the rest of the desktop mismatched, so in Deepin \
            Rexon leaves the colors to Deepin."),
    ),
    // Рамки Metacity/Marco и xfwm4 в Cinnamon: общее «такое есть только в
    // MATE» внушало бы, что рамок у Cinnamon нет вовсе. Они есть, но с
    // Cinnamon 5.4 (Muffin пересобран на Mutter 3.36) их рисует тема GTK:
    // Muffin читает `org.cinnamon.desktop.interface gtk-theme`, темы
    // `metacity-1` не загружает, а выбор «Рамки окон» убран из настроек тем
    // (cinnamon bef2930e). Ключ `org.cinnamon.desktop.wm.preferences theme`
    // остался в схеме, но его читает только запасная копия режима высокого
    // контраста.
    ("theme-marco", De::Cinnamon, CINNAMON_BORDERS_FROM_GTK),
    ("theme-xfwm4", De::Cinnamon, CINNAMON_BORDERS_FROM_GTK),
];

// TRANSLATORS: Why the Metacity/Marco and xfwm4 window border sections are missing in Cinnamon. “GTK Themes” is the Rexon section name, “Applications” the item in Cinnamon's own Themes settings.
const CINNAMON_BORDERS_FROM_GTK: &str = n_("In Cinnamon, window borders come from the GTK theme: since Cinnamon 5.4 \
     its window manager Muffin no longer loads separate border themes (Metacity, Marco, xfwm4). \
     To change the borders, pick a theme in “GTK Themes” — it's the same as “Applications” in \
     Cinnamon's own Themes settings.");

/// Почему этого оформления нет в выбранном окружении — текст для человека
/// (правило #13: объяснить, а не показывать пустой экран). `None` — тип в
/// этом окружении работает, объяснять нечего.
///
/// `here` — как рабочий стол зовётся у покупателя («Sway», а не «Тайловые»):
/// он выбирал свой стол, а не группу композиторов, и в объяснении должен
/// увидеть именно его. Пусто — подставится подпись группы.
pub fn unavailable_note(kind: &str, de: De, here: &str) -> Option<String> {
    let row = row(kind)?;
    if row.applies.covers(de) {
        return None;
    }
    if let Some((_, _, why)) = UNAVAILABLE_REASONS.iter().find(|(k, d, _)| *k == kind && *d == de) {
        return Some(tr(why));
    }
    let where_ = row.applies.hint();
    let here = if here.is_empty() { de.display_label() } else { here.to_string() };
    Some(trf(
        // TRANSLATORS: %1 is a list of desktops, %2 the desktop selected now.
        "This kind of customization only exists in: %1.\n\
         Your selected desktop is %2, where it simply does nothing, so we \
         don't offer it.",
        &[&where_, &here],
    ))
}

/// При каком условии этот тип работает в выбранном окружении — текст для
/// человека. `None` — условий нет, говорить нечего.
///
/// Отличие от [`unavailable_note`]: там раздела НЕТ и объясняется почему,
/// здесь раздел ЕСТЬ и объясняется, что нужно, чтобы он сработал. Прятать
/// работающее вместо объяснения условия — та самая ошибка, из-за которой
/// звуковые темы не показывались тайловым, а тема Kvantum — всем, кроме KDE
/// и LXQt.
pub fn condition_note(kind: &str, de: De) -> Option<String> {
    Some(row(kind)?.caveat(de)?.text())
}

/// Композиторы, которым Rexon переменные сеанса передать не может: в их
/// настройках им нет места, а `~/.config/environment.d` они не читают (Д1,
/// проверка окружений 02.10.2026; кто что читает —
/// `compositor_autostart::sync_session_env`).
const NO_SESSION_ENV_IDS: &[&str] = &["sway", "river", "wayfire"];

// TRANSLATORS: Appended to the Qt-style caveat on window managers Rexon can't pass environment variables to. %1 is the window manager (sway, river…).
const NO_SESSION_ENV_NOTE: &str = n_("%1 is the exception: it has no setting Rexon could put this in. \
     Start %1 through uwsm, or add the lines from \
     ~/.config/environment.d/rexon-qt-style.conf to its environment yourself.");

/// [`condition_note`] по идентификатору окружения (`sway`, `xfce`…): там,
/// где переменные сеанса до композитора не доходят, оговорка говорит это
/// прямо, а не обещает «после следующего входа».
pub fn condition_note_id(kind: &str, de_id: &str) -> Option<String> {
    let note = condition_note(kind, de_from_id(de_id)?)?;
    // Цветовая схема вне Plasma тоже ведёт Qt-программы через переменную
    // `QT_QPA_PLATFORMTHEME` в том же файле (`shell_palette::plan_scheme_layer`).
    if matches!(kind, "theme-kvantum" | "theme-kde-colors") && NO_SESSION_ENV_IDS.contains(&de_id) {
        return Some(format!("{note} {}", crate::i18n::trf(NO_SESSION_ENV_NOTE, &[&de_id])));
    }
    Some(note)
}

// ============================================================================
// Скрипты активации
// ============================================================================
//
// Списки gsettings нельзя менять одной командой (нет list-add), а читать
// текущее значение из чистого ядра нечем — поэтому изменение оформляется
// маленьким идемпотентным сценарием оболочки, как это уже сделано для
// Plymouth и SDDM. Сценарии — чистые функции: их вывод сверяется тестами.

/// Экранирование значения для одинарных кавычек оболочки.
fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Общая часть: прочитать список gsettings в переменную `items` по одному
/// элементу на строку (кавычки элементов сохраняются).
fn read_list_snippet(schema: &str, key: &str) -> String {
    format!(
        "cur=$(gsettings get {schema} {key} 2>/dev/null || echo '@as []')\n\
         items=$(printf '%s' \"$cur\" | sed 's/^@as //; s/^\\[//; s/\\]$//' \
         | tr ',' '\\n' | sed 's/^ *//; s/ *$//' | grep -v '^$')\n"
    )
}

/// Сценарий: добавить элемент в список gsettings, если такого ещё нет.
///
/// `dup_marker` — подстрока, по которой ищется уже добавленный элемент (для
/// апплетов Cinnamon это `:uuid:`, иначе сам uuid): повторная установка не
/// должна плодить дубли в панели.
pub fn gsettings_list_add_script(
    schema: &str,
    key: &str,
    entry: &str,
    dup_marker: &str,
) -> String {
    format!(
        "{read}\
         if printf '%s\\n' \"$items\" | grep -qF {marker}; then exit 0; fi\n\
         items=$(printf '%s\\n%s' \"$items\" {entry} | grep -v '^$')\n\
         new=$(printf '%s' \"$items\" | paste -sd, -)\n\
         gsettings set {schema} {key} \"[$new]\"\n",
        read = read_list_snippet(schema, key),
        marker = sq(dup_marker),
        entry = sq(&format!("'{entry}'")),
    )
}

/// Сценарий: убрать из списка gsettings все элементы, содержащие подстроку.
pub fn gsettings_list_remove_script(schema: &str, key: &str, marker: &str) -> String {
    format!(
        "{read}\
         items=$(printf '%s\\n' \"$items\" | grep -vF {marker} | grep -v '^$')\n\
         new=$(printf '%s' \"$items\" | paste -sd, -)\n\
         gsettings set {schema} {key} \"[$new]\"\n",
        read = read_list_snippet(schema, key),
        marker = sq(marker),
    )
}

/// Сценарий включения апплета Cinnamon — в НАЧАЛО правой зоны первой
/// панели, как ставит окно настроек Cinnamon (решение Wehrwolfmann
/// 20.10.2026): апплеты той же зоны сдвигаются на одно место, новый встаёт
/// на нулевое. Номер экземпляра — из счётчика `next-applet-id`, но не
/// меньше «наибольший занятый + 1» (у штатных записей счётчик бывает 0 при
/// занятых номерах — настройки двух апплетов слиплись бы); счётчик
/// увеличивается. Те же правила, что у `cinnamon_spices::applet_enable_front`.
pub fn cinnamon_applet_enable_script(uuid: &str) -> String {
    format!(
        "{read}\
         if printf '%s\\n' \"$items\" | grep -qF {marker}; then exit 0; fi\n\
         id=$(gsettings get org.cinnamon next-applet-id 2>/dev/null || echo 0)\n\
         case \"$id\" in ''|*[!0-9]*) id=0;; esac\n\
         max=$(printf '%s\\n' \"$items\" | tr -d \"'\" | awk -F: 'BEGIN{{m=-1}} $5 ~ /^[0-9]+$/ {{if ($5+0>m) m=$5+0}} END{{print m}}')\n\
         if [ \"$id\" -le \"$max\" ]; then id=$((max + 1)); fi\n\
         items=$(printf '%s\\n' \"$items\" | awk -F: 'BEGIN{{OFS=\":\"}} $1==\"\\047panel1\" && $2==\"right\" && $3 ~ /^[0-9]+$/ {{$3=$3+1}} {{print}}')\n\
         entry=\"'panel1:right:0:{uuid}:$id'\"\n\
         items=$(printf '%s\\n%s' \"$items\" \"$entry\" | grep -v '^$')\n\
         new=$(printf '%s' \"$items\" | paste -sd, -)\n\
         gsettings set org.cinnamon enabled-applets \"[$new]\"\n\
         gsettings set org.cinnamon next-applet-id $((id + 1))\n",
        read = read_list_snippet("org.cinnamon", "enabled-applets"),
        marker = sq(&format!(":{uuid}:")),
    )
}

/// Сценарий включения десклета Cinnamon (координаты — левый верхний угол со
/// смещением, чтобы виджет не встал под панель).
pub fn cinnamon_desklet_enable_script(uuid: &str) -> String {
    format!(
        "{read}\
         if printf '%s\\n' \"$items\" | grep -qF {marker}; then exit 0; fi\n\
         id=$(gsettings get org.cinnamon next-desklet-id 2>/dev/null || echo 0)\n\
         entry=\"'{uuid}:$id:100:100'\"\n\
         items=$(printf '%s\\n%s' \"$items\" \"$entry\" | grep -v '^$')\n\
         new=$(printf '%s' \"$items\" | paste -sd, -)\n\
         gsettings set org.cinnamon enabled-desklets \"[$new]\"\n\
         gsettings set org.cinnamon next-desklet-id $((id + 1))\n",
        read = read_list_snippet("org.cinnamon", "enabled-desklets"),
        marker = sq(&format!("'{uuid}:")),
    )
}

/// Сценарий включения расширения GNOME: сначала штатная утилита, при её
/// отсутствии — правка списка `enabled-extensions`. Заодно предупреждает,
/// если версии текущей оболочки нет в `shell-version` (иначе расширение
/// молча не грузится, и человек считает, что установка не сработала).
pub fn gnome_extension_enable_script(uuid: &str, dest_dir: &str) -> String {
    // Предупреждение видит человек (вывод шага установки), поэтому оно из
    // каталога перевода. В сценарии оно стоит в двойных кавычках оболочки:
    // перевод экранируется, а на место `%1` встаёт переменная `$ver`.
    let warning = tr(
        // TRANSLATORS: %1 is a GNOME version number.
        "WARNING: the extension doesn't declare support for GNOME %1 — it \
         may not start",
    )
    .replace('\\', "\\\\")
    .replace('"', "\\\"")
    .replace('`', "\\`")
    .replace('$', "\\$")
    .replace("%1", "$ver");
    format!(
        "meta={meta}\n\
         if command -v gnome-shell >/dev/null 2>&1 && [ -f \"$meta\" ]; then\n\
         \x20 ver=$(gnome-shell --version 2>/dev/null | sed 's/[^0-9.]//g' | cut -d. -f1)\n\
         \x20 if [ -n \"$ver\" ] && ! grep -q \"\\\"$ver\" \"$meta\"; then\n\
         \x20   echo \"{warning}\" >&2\n\
         \x20 fi\n\
         fi\n\
         if command -v glib-compile-schemas >/dev/null 2>&1 && [ -d {schemas} ]; then\n\
         \x20 glib-compile-schemas {schemas} >/dev/null 2>&1 || true\n\
         fi\n\
         if command -v gnome-extensions >/dev/null 2>&1; then\n\
         \x20 gnome-extensions enable {uuid_q} && exit 0\n\
         fi\n\
         {fallback}",
        meta = sq(&format!("{dest_dir}/metadata.json")),
        schemas = sq(&format!("{dest_dir}/schemas")),
        uuid_q = sq(uuid),
        fallback =
            gsettings_list_add_script("org.gnome.shell", "enabled-extensions", uuid, uuid),
    )
}

/// Сценарий отключения расширения GNOME (обратное к предыдущему).
pub fn gnome_extension_disable_script(uuid: &str) -> String {
    format!(
        "if command -v gnome-extensions >/dev/null 2>&1; then\n\
         \x20 gnome-extensions disable {uuid_q} 2>/dev/null || true\n\
         fi\n\
         {fallback}",
        uuid_q = sq(uuid),
        fallback =
            gsettings_list_remove_script("org.gnome.shell", "enabled-extensions", uuid),
    )
}

/// Сценарий записи ключа в ini-файл конфигурации (LXQt): секция создаётся,
/// существующий ключ заменяется, файл создаётся при отсутствии.
pub fn ini_set_script(path: &str, group: &str, key: &str, value: &str) -> String {
    format!(
        "f={path}\n\
         mkdir -p \"$(dirname \"$f\")\"\n\
         [ -f \"$f\" ] || printf '[%s]\\n' {group} > \"$f\"\n\
         grep -q \"^\\[{group}\\]\" \"$f\" || printf '\\n[%s]\\n' {group} >> \"$f\"\n\
         if grep -q \"^{key}=\" \"$f\"; then\n\
         \x20 sed -i \"s|^{key}=.*|{key}={value}|\" \"$f\"\n\
         else\n\
         \x20 sed -i \"0,/^\\[{group}\\]/s||[{group}]\\n{key}={value}|\" \"$f\"\n\
         fi\n",
        path = sq(path),
        group = group,
        key = key,
        value = value,
    )
}

/// Сценарий сброса ключа KDE, если он указывает на удаляемый объект.
///
/// Это не косметика, а защита от дурака (правило #13): если удалить пакет
/// экрана блокировки, оставив `kscreenlockerrc` указывающим на него, экран
/// блокировки не соберётся — человек окажется заперт перед чёрным экраном.
pub fn kde_config_reset_if_script(
    file: &str,
    group: &str,
    key: &str,
    value: &str,
) -> String {
    format!(
        "cur=$(kreadconfig6 --file {file} --group {group} --key {key} 2>/dev/null)\n\
         if [ \"$cur\" = {value} ]; then\n\
         \x20 kwriteconfig6 --file {file} --group {group} --key {key} --delete\n\
         fi\n",
        file = sq(file),
        group = sq(group),
        key = sq(key),
        value = sq(value),
    )
}

/// Сценарий сброса скалярного ключа gsettings, если он указывает на нашу
/// тему: слепой сброс затёр бы тему, которую пользователь выбрал сам.
pub fn gsettings_reset_if_script(schema: &str, key: &str, value: &str) -> String {
    format!(
        "cur=$(gsettings get {schema} {key} 2>/dev/null | sed \"s/^'//; s/'$//\")\n\
         if [ \"$cur\" = {value} ]; then gsettings reset {schema} {key}; fi\n",
        value = sq(value),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_installable_row_has_kind_and_dest() {
        for r in ROWS {
            if r.support.installable() {
                assert!(!r.kind.is_empty(), "устанавливаемая строка без типа: {}", r.what);
                assert!(!r.dest.starts_with('/'), "{}: пользовательский тип не может ставиться в системный каталог", r.kind);
            } else {
                assert!(r.kind.is_empty(), "неустанавливаемая строка с типом: {}", r.kind);
                assert!(r.support.reason().is_some());
            }
        }
    }

    /// Каждая строка отвечает на ОБА вопроса: куда класть и где это имеет
    /// смысл. Забыть второй нельзя — поле обязательное; здесь проверяется,
    /// что ответ ещё и осмысленный.
    #[test]
    fn every_row_answers_both_questions() {
        for r in ROWS {
            let list = r.applies.list();
            assert!(
                !list.is_empty(),
                "{}: применимость пуста — тип не нужен нигде?",
                if r.kind.is_empty() { r.what } else { r.kind }
            );
            if r.de != De::Any {
                assert!(
                    r.applies.covers(r.de),
                    "{}: лежит в каталоге {}, но там же и не применяется",
                    r.what,
                    r.de.label()
                );
            }
        }
    }

    /// Тема GTK нужна ВЕЗДЕ, включая KDE (Firefox, GIMP, Inkscape под Plasma
    /// плюс штатный модуль kde-gtk-config), а оформление Budgie ею и
    /// задаётся. Убрать её из KDE — регрессия, поэтому проверяем явно.
    #[test]
    fn gtk_theme_applies_everywhere() {
        let gtk = row("theme-gtk").unwrap();
        for de in De::ALL {
            assert!(gtk.covers(*de), "тема GTK пропала из {}", de.label());
        }
        assert!(row("theme-plasmoid").unwrap().covers(De::Kde));
        assert!(!row("theme-plasmoid").unwrap().covers(De::Gnome));
    }

    #[test]
    fn rows_for_uses_applicability() {
        let kde: Vec<&str> = rows_for(De::Kde).iter().map(|r| r.kind).collect();
        assert!(kde.contains(&"theme-gtk"));
        assert!(kde.contains(&"theme-plasmoid"));
        let gnome: Vec<&str> = rows_for(De::Gnome).iter().map(|r| r.kind).collect();
        assert!(gnome.contains(&"theme-gtk"));
        assert!(!gnome.contains(&"theme-plasmoid"));
        // Тайловые: своего конфига не раздаём, но обои, шрифты и звуки —
        // пожалуйста (звуковую тему играет демон уведомлений, не оболочка).
        let tiling: Vec<&str> = rows_for(De::Wlroots).iter().map(|r| r.kind).collect();
        assert!(tiling.contains(&"wallpaper-pack"));
        assert!(tiling.contains(&"font-pack"));
        assert!(tiling.contains(&"theme-sounds"));
    }

    /// Kvantum — движок стилей Qt, а не часть оболочки: Qt-приложения (VLC,
    /// qBittorrent, Krita, OBS) живут в любом рабочем столе. Прятать его от
    /// GNOME — та же ошибка, что прятать тему GTK от KDE.
    ///
    /// Единственное исключение — Deepin: там стиль Qt на весь сеанс отнял бы
    /// у программ DTK их Chameleon (`themes::QtStyleWay::Deepin`), и раздел
    /// скрыт со своей причиной, а не с общим «здесь это ничего не делает».
    #[test]
    fn kvantum_applies_everywhere() {
        let kv = row("theme-kvantum").unwrap();
        for de in De::ALL {
            if *de == De::Deepin {
                assert!(!kv.covers(*de), "Kvantum в Deepin сломал бы Chameleon");
                assert!(UNAVAILABLE_REASONS.iter().any(|(k, d, _)| *k == "theme-kvantum" && *d == De::Deepin));
                continue;
            }
            assert!(kv.covers(*de), "Kvantum пропал из {}", de.label());
        }
        // Где стиль Qt выбирается штатно — оговорки нет; где нет — есть.
        assert!(kv.caveat(De::Kde).is_none());
        assert!(kv.caveat(De::Lxqt).is_none());
        for de in [De::Gnome, De::Xfce, De::Wlroots, De::Cosmic] {
            assert!(kv.caveat(de).is_some(), "{} без оговорки", de.label());
        }
    }

    /// Звуковая тема лежит в общем каталоге freedesktop и оболочке не
    /// принадлежит — значит применима везде, но в тайловых с оговоркой.
    #[test]
    fn sounds_apply_everywhere_with_caveat_in_tiling() {
        let s = row("theme-sounds").unwrap();
        for de in De::ALL {
            assert!(s.covers(*de), "звуки пропали из {}", de.label());
        }
        // Язык — исходный: русский сверяет `tests/categories_parity.rs`
        // на настоящем `po/ru.po`.
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let note = condition_note("theme-sounds", De::Wlroots).unwrap();
        assert!(note.contains("swaync"), "{note}");
        assert!(condition_note("theme-sounds", De::Kde).is_none());
        assert!(condition_note("theme-icons", De::Wlroots).is_none());
        // Применяет Rexon, а не человек вручную (Д11).
        assert_eq!(s.activate, Activate::Rexon);
    }

    #[test]
    fn unavailable_note_explains_and_stays_quiet_when_fine() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        assert!(unavailable_note("theme-gtk", De::Kde, "").is_none());
        let note = unavailable_note("theme-plasmoid", De::Gnome, "").unwrap();
        assert!(note.contains("KDE Plasma") && note.contains("GNOME"), "{note}");
        // Человек выбрал Sway — про «тайловые» ему говорить незачем.
        let sway = unavailable_note("theme-cinnamon", De::Wlroots, "Sway").unwrap();
        assert!(sway.contains("Sway") && !sway.contains("Tiling"), "{sway}");
    }

    /// Рамки Marco и xfwm4 в Cinnamon: не «такое есть только в MATE/XFCE»,
    /// а куда идти — рамки Cinnamon рисует тема GTK.
    #[test]
    fn cinnamon_borders_point_to_gtk_theme() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        for kind in ["theme-marco", "theme-xfwm4"] {
            assert!(!row(kind).unwrap().covers(De::Cinnamon), "{kind}");
            let note = unavailable_note(kind, De::Cinnamon, "Cinnamon").unwrap();
            assert!(note.contains("GTK Themes") && note.contains("Muffin"), "{kind}: {note}");
            assert!(!note.contains("only exists in"), "{kind}: {note}");
        }
        // Так же — на экране раздела боковой панели.
        for key in ["marco", "xfwm4"] {
            assert!(!crate::categories::category_available(key, "cinnamon"), "{key}");
            let note = crate::categories::category_note(std::path::Path::new("/h"), key, "cinnamon").unwrap();
            assert!(note.contains("GTK Themes") && !note.contains("only exists in"), "{key}: {note}");
        }
        // В остальных окружениях — прежнее общее объяснение.
        let gnome = unavailable_note("theme-marco", De::Gnome, "").unwrap();
        assert!(gnome.contains("only exists in") && gnome.contains("MATE"), "{gnome}");
    }

    #[test]
    fn de_ids_map_to_matrix() {
        assert_eq!(de_from_id("cinnamon-mint"), Some(De::Cinnamon));
        for id in ["hyprland", "sway", "niri", "river", "labwc", "wayfire"] {
            assert_eq!(de_from_id(id), Some(De::Wlroots), "{id}");
        }
        assert_eq!(de_from_id("unknown-wm"), None);
    }

    #[test]
    fn kinds_are_unique() {
        let k = kinds();
        let mut sorted = k.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(k.len(), sorted.len(), "дублирующиеся типы в матрице");
    }

    #[test]
    fn lookup_ignores_empty_kind() {
        assert!(row("").is_none());
        assert!(row("kde-kwin-script").is_some());
        assert_eq!(dest_suffix("cinnamon-applet"), Some(".local/share/cinnamon/applets"));
    }

    #[test]
    fn dest_path_expands_home() {
        let r = row("theme-lxqt").unwrap();
        assert_eq!(r.dest_path("/home/t"), "/home/t/.local/share/lxqt/themes");
    }

    #[test]
    fn hints_describe_real_commands() {
        let r = row("theme-xfwm4").unwrap();
        assert_eq!(
            r.activate.hint(),
            "xfconf-query -c xfwm4 -p /general/theme -s <имя>"
        );
        assert_eq!(r.refresh.hint(), "ничего, применяется сразу");
    }

    #[test]
    fn applet_script_is_idempotent_and_bumps_counter() {
        let s = cinnamon_applet_enable_script("weather@mockturtl");
        assert!(s.contains("':weather@mockturtl:'"));
        assert!(s.contains("next-applet-id $((id + 1))"));
        assert!(s.contains("panel1:right:0:weather@mockturtl:$id"), "в начало правой зоны");
        assert!(s.contains("{$3=$3+1}"), "соседи по зоне сдвигаются");
    }

    /// Сценарий апплета настоящим bash с поддельным `gsettings`: новый — в
    /// начало правой зоны, соседи сдвинуты, номер не занят, счётчик вырос.
    #[test]
    fn applet_script_runs_and_puts_new_applet_first() {
        use std::os::unix::fs::PermissionsExt;
        let have = |c: &str| std::process::Command::new("sh").arg("-c").arg(format!("command -v {c}")).output().is_ok_and(|o| o.status.success());
        if !(have("bash") && have("awk") && have("paste")) {
            return;
        }
        let dir = std::env::temp_dir().join(format!("rexon-applet-script-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("gsettings");
        std::fs::write(
            &fake,
            "#!/bin/sh\n\
             if [ \"$1\" = get ] && [ \"$3\" = enabled-applets ]; then\n\
             echo \"['panel1:left:0:menu@cinnamon.org', 'panel1:right:0:systray@cinnamon.org:3', 'panel1:right:1:calendar@cinnamon.org:7', 'panel2:right:0:other@x:2']\"; exit 0; fi\n\
             if [ \"$1\" = get ]; then echo 0; exit 0; fi\n\
             echo \"$@\" >> \"$(dirname \"$0\")/log\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default());
        let out = std::process::Command::new("bash")
            .arg("-c")
            .arg(cinnamon_applet_enable_script("weather@mockturtl"))
            .env("PATH", path)
            .output()
            .unwrap();
        let log = std::fs::read_to_string(dir.join("log")).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(
            log.contains(
                "set org.cinnamon enabled-applets ['panel1:left:0:menu@cinnamon.org','panel1:right:1:systray@cinnamon.org:3',\
                 'panel1:right:2:calendar@cinnamon.org:7','panel2:right:0:other@x:2','panel1:right:0:weather@mockturtl:8']"
            ),
            "{log}"
        );
        assert!(log.contains("set org.cinnamon next-applet-id 9"), "{log}");
    }

    #[test]
    fn list_scripts_quote_values() {
        let add = gsettings_list_add_script("org.cinnamon", "enabled-extensions", "a'b", "a'b");
        assert!(add.contains("'a'\\''b'"));
        let rm = gsettings_list_remove_script("org.cinnamon", "enabled-extensions", "x@y");
        assert!(rm.contains("grep -vF 'x@y'"));
    }

    #[test]
    fn gnome_script_compiles_schemas_and_falls_back() {
        let s = gnome_extension_enable_script("e@x", "/home/t/.local/share/gnome-shell/extensions/e@x");
        assert!(s.contains("glib-compile-schemas"));
        assert!(s.contains("gnome-extensions enable 'e@x'"));
        assert!(s.contains("gsettings set org.gnome.shell enabled-extensions"));
    }

    #[test]
    fn reset_only_touches_our_theme() {
        let s = gsettings_reset_if_script("org.cinnamon.theme", "name", "Mint-Y");
        assert!(s.contains("if [ \"$cur\" = 'Mint-Y' ]"));
        assert!(s.contains("gsettings reset org.cinnamon.theme name"));
    }
}
