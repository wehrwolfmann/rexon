//! «Панель MATE»: готовые раскладки панели (`mate-panel`) одной кнопкой —
//! «Как Windows», классическая, как в Mint, Fedora, Ubuntu MATE и т.д.
//!
//! Логика здесь вся, без привязки к вкладке: список с подписями, отметка
//! текущей, подготовка и применение, возврат прежней панели. Вкладка
//! («Настройки» сейчас, «Оформление» потом) только показывает снимок
//! [`read_state`] и зовёт [`apply`] / [`restore`].
//!
//! Как mate-panel обращается с раскладками (mate-panel 1.28, параметры
//! `--layout` и `--reset` из `mate-panel --help`; поведение — замер на
//! стенде Xvfb 16.10.2026):
//!
//! * раскладка — файл `<имя>.layout` формата key-file: группы
//!   `[Toplevel <id>]` (панели) и `[Object <id>]` (апплеты, значки запуска,
//!   меню). Ищется ТОЛЬКО в каталоге самого mate-panel
//!   (`/usr/share/mate-panel/layouts`); пользовательского каталога у
//!   mate-panel нет. Зато ключ `org.mate.panel default-layout` принимает и
//!   абсолютный путь — им мы и пользуемся для своей исправленной копии;
//! * `mate-panel --layout <имя|путь> --reset` пишет `default-layout` и
//!   опустошает списки панелей и объектов. Работающая панель сама замечает
//!   пустой список, убирает старые панели и строит новые из раскладки —
//!   перезапуск (`--replace`) не нужен. Апплеты, значки и настройки
//!   прежней панели при этом теряются — отсюда снимок до применения;
//! * ⚠️ группа с незнакомым ключом отбрасывается ЦЕЛИКОМ. Раскладки из
//!   ubuntu-mate-settings (redmond, familiar, eleven…) пишут
//!   `relative-to-edge=end`, которого mate-panel 1.28 не знает: без правки
//!   у «Как Windows» пропадали значки, часы и область уведомлений;
//! * ⚠️ апплет, которого нет в системе (меню Brisk, индикаторы), панель
//!   встречает окном «Возникла проблема при загрузке апплета…» — человек
//!   получал панель без меню и окно с ошибкой.
//!
//! Поэтому перед применением раскладка ГОТОВИТСЯ ([`prepare`]):
//! `relative-to-edge=end` → `panel-right-stick=true`, незнакомые ключи и
//! группы — прочь, недостающее меню → встроенная кнопка меню MATE,
//! недостающие индикаторы → часы, прочие недостающие апплеты и значки
//! программ, которых нет, — прочь. Если править нечего, применяется сама
//! системная раскладка по имени; иначе — исправленная копия
//! `~/.local/share/mate-panel/layouts/rexon-<имя>.layout` по пути.
//!
//! Возврат: перед применением Rexon снимает `dconf dump /org/mate/panel/`.
//! Исходная панель человека — снимок до ПЕРВОГО применения, в
//! `~/.local/share/rexon/backups/mate-panel.dconf`; последующие применения
//! его не трогают и кладут свой снимок рядом (`….last`) — правило копии
//! `backup::panel_snapshot_slot`. «Вернуть прежнюю панель» загружает
//! исходную ([`restore`]) — работающая панель подхватывает и это без
//! перезапуска (замер на стенде); после возврата следующее применение
//! снимает исходную заново.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::animations::{argv, gsettings_get, LookIo};
use crate::i18n::{n_, tr, trf};

/// Каталог раскладок самого mate-panel.
pub const SYSTEM_LAYOUTS_DIR: &str = "/usr/share/mate-panel/layouts";
/// Каталог описаний апплетов по умолчанию (переопределяется
/// `MATE_PANEL_APPLETS_DIR`, списком через двоеточие).
pub const SYSTEM_APPLETS_DIR: &str = "/usr/share/mate-panel/applets";
/// Ветка dconf с настройками панели.
pub const PANEL_DCONF_DIR: &str = "/org/mate/panel/";
/// Приставка наших исправленных копий: по ней копия узнаётся и не
/// попадает в список второй раз.
const COPY_PREFIX: &str = "rexon-";

/// Ключи панели, которые mate-panel 1.28 принимает в раскладке.
const TOPLEVEL_KEYS: &[&str] = &[
    "name",
    "screen",
    "monitor",
    "expand",
    "orientation",
    "size",
    "x",
    "y",
    "x-right",
    "y-bottom",
    "x-centered",
    "y-centered",
    "auto-hide",
    "enable-animations",
    "enable-buttons",
    "enable-arrows",
    "hide-delay",
    "unhide-delay",
    "auto-hide-size",
    "animation-speed",
];

/// Ключи объекта панели, которые mate-panel 1.28 принимает в раскладке.
const OBJECT_KEYS: &[&str] = &[
    "object-type",
    "toplevel-id",
    "position",
    "panel-right-stick",
    "locked",
    "applet-iid",
    "attached-toplevel-id",
    "tooltip",
    "use-custom-icon",
    "custom-icon",
    "use-menu-path",
    "menu-path",
    "has-arrow",
    "launcher-location",
    "action-type",
];

/// Часы — ими заменяются недостающие индикаторы (в раскладках Ubuntu MATE
/// часы живут внутри индикаторов).
const CLOCK_IID: &str = "ClockAppletFactory::ClockApplet";

/// Сторонние меню: если их нет, вместо них встаёт встроенная кнопка меню
/// MATE (`object-type=menu`), которая есть всегда.
const MENU_IIDS: &[&str] = &[
    "BriskMenuFactory::BriskMenu",
    "MintMenuAppletFactory::MintMenuApplet",
    "MateMenuAppletFactory::MateMenuApplet",
];

/// Индикаторы (часы, звук, сеть одним апплетом).
const INDICATOR_IIDS: &[&str] = &[
    "IndicatorAppletCompleteFactory::IndicatorAppletComplete",
    "IndicatorAppletFactory::IndicatorApplet",
];

/// Человеческие имена апплетов — для заметок «будет вместо» и «не будет».
/// Незнакомый апплет называется своим именем из `applet-iid`.
const APPLET_LABELS: &[(&str, &str)] = &[
    ("BriskMenuFactory::BriskMenu", n_("Brisk menu")),
    ("MintMenuAppletFactory::MintMenuApplet", n_("Mint menu")),
    ("MateMenuAppletFactory::MateMenuApplet", n_("MATE Menu applet")),
    ("IndicatorAppletCompleteFactory::IndicatorAppletComplete", n_("indicators")),
    ("IndicatorAppletFactory::IndicatorApplet", n_("indicators")),
    ("AppmenuAppletFactory::AppmenuApplet", n_("global app menu")),
    ("WindowButtonsAppletFactory::WindowButtonsApplet", n_("window buttons on the panel")),
    ("MateWindowPickerFactory::MateWindowPicker", n_("window picker")),
    ("StickyNotesAppletFactory::StickyNotesApplet", n_("sticky notes")),
    ("GvcAppletFactory::GvcApplet", n_("volume control")),
    ("TrashAppletFactory::TrashApplet", n_("trash")),
    ("DriveMountAppletFactory::DriveMountApplet", n_("drive mounter")),
    ("WnckletFactory::WorkspaceSwitcherApplet", n_("workspace switcher")),
    ("WnckletFactory::WindowListApplet", n_("window list")),
    ("WnckletFactory::ShowDesktopApplet", n_("show desktop button")),
    ("NotificationAreaAppletFactory::NotificationArea", n_("notification area")),
    (CLOCK_IID, n_("clock")),
];

/// Известные раскладки: имя файла, название и описание. Порядок — порядок
/// показа («Как Windows» первой: за ней сюда и приходят). Описания
/// составлены по содержимому файлов; незнакомая раскладка получает
/// описание из своего состава ([`auto_description`]).
const KNOWN_LAYOUTS: &[(&str, &str, &str)] = &[
    (
        "redmond",
        n_("Like Windows"),
        n_("One panel at the bottom: menu on the left, open windows in the middle, \
            notification area and clock on the right."),
    ),
    (
        "default",
        n_("Classic MATE"),
        n_("Two panels, as in GNOME 2: Applications, Places and System menus and the clock at \
            the top; open windows and workspaces at the bottom."),
    ),
    (
        "linuxmint",
        n_("Like Linux Mint"),
        n_("One panel at the bottom: Mint menu on the left, open windows in the middle, clock \
            on the right."),
    ),
    (
        "opensuse",
        n_("Like openSUSE"),
        n_("One panel at the bottom: menu and sticky notes on the left, open windows in the \
            middle, workspaces and clock on the right."),
    ),
    (
        "familiar",
        n_("Familiar"),
        n_("Two panels: menu, Firefox and indicators at the top; open windows, workspaces and \
            trash at the bottom."),
    ),
    (
        "ubuntu-mate",
        n_("Traditional Ubuntu MATE"),
        n_("Two panels: Applications, Places and System menus and indicators at the top; open \
            windows, workspaces and trash at the bottom."),
    ),
    (
        "ubuntu",
        n_("Like old Ubuntu"),
        n_("Two panels: menus, Firefox, clock and a shutdown button at the top; open windows, \
            workspaces and trash at the bottom."),
    ),
    (
        "fedora",
        n_("Like Fedora"),
        n_("Two panels: menus, file manager, terminal and Firefox buttons, volume and clock at \
            the top; open windows and workspaces at the bottom."),
    ),
    (
        "mageia",
        n_("Like Mageia"),
        n_("Two panels: menus, Mageia Control Center, terminal and Firefox at the top; open \
            windows and workspaces at the bottom."),
    ),
    (
        "contemporary",
        n_("Contemporary"),
        n_("Two panels: menu, the menu of the active app and indicators at the top; open \
            windows, workspaces and trash at the bottom."),
    ),
    (
        "eleven",
        n_("Like macOS"),
        n_("One panel at the top with the menu and the menu of the active app; apps are \
            launched from a dock at the bottom."),
    ),
    (
        "pantheon",
        n_("Like elementary OS"),
        n_("A thin panel at the top with the menu and indicators; apps are launched from a dock \
            at the bottom."),
    ),
    (
        "mutiny",
        n_("Like Unity"),
        n_("A panel at the top with the menu and indicators; apps are launched from a dock on \
            the left."),
    ),
    (
        "netbook",
        n_("Netbook"),
        n_("One panel at the top for small screens: menu, window picker, indicators and \
            window buttons."),
    ),
];

// ═══════════════════════════════════════════════════════════════════════════
// Недостающие части раскладки: откуда их поставить
// ═══════════════════════════════════════════════════════════════════════════

/// Откуда ставится часть раскладки в семействе дистрибутивов.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartSource {
    /// Пакет из репозиториев дистрибутива.
    Repo(&'static str),
    /// Только AUR (собирается). `orphan` — у пакета нет сопровождающего.
    Aur { package: &'static str, orphan: bool },
    /// Негде взять.
    Missing,
}

/// Часть раскладки (`applet-iid`) и её пакет по семействам
/// (`system::PkgFamily::as_str`: `arch`, `debian` — и Ubuntu, `fedora`,
/// `suse`).
pub struct PanelPart {
    pub iid: &'static str,
    pub arch: PartSource,
    pub debian: PartSource,
    pub fedora: PartSource,
    pub suse: PartSource,
}

/// Таблица частей раскладок. Сверено 19.10.2026 по самим репозиториям:
/// archlinux.org и AUR RPC (у `mate-indicator-applet` и
/// `mate-window-buttons-applet` в AUR нет сопровождающего), Debian trixie
/// (packages.debian.org), Ubuntu noble (packages.ubuntu.com), Fedora f43
/// (mdapi), openSUSE Factory (api.opensuse.org: имя собранного rpm).
/// Значки запуска программ (Firefox, Evolution) — не части раскладки, их
/// здесь нет. Док Plank раскладка не запускает (Rexon его не включает) —
/// его здесь тоже нет.
pub const PANEL_PARTS: &[PanelPart] = {
    use PartSource::{Aur, Missing, Repo};
    &[
        PanelPart {
            iid: "BriskMenuFactory::BriskMenu",
            arch: Aur { package: "brisk-menu", orphan: false },
            debian: Repo("mate-applet-brisk-menu"),
            fedora: Missing,
            suse: Repo("brisk-menu"),
        },
        PanelPart {
            iid: "MintMenuAppletFactory::MintMenuApplet",
            arch: Aur { package: "mintmenu", orphan: false },
            debian: Missing,
            fedora: Missing,
            suse: Missing,
        },
        PanelPart {
            iid: "MateMenuAppletFactory::MateMenuApplet",
            arch: Aur { package: "mate-menu", orphan: false },
            debian: Repo("mate-menu"),
            fedora: Repo("mate-menu"),
            suse: Repo("mate-menu"),
        },
        PanelPart {
            iid: "IndicatorAppletCompleteFactory::IndicatorAppletComplete",
            arch: Aur { package: "mate-indicator-applet", orphan: true },
            debian: Repo("mate-indicator-applet"),
            fedora: Missing,
            suse: Repo("mate-applet-indicator"),
        },
        PanelPart {
            iid: "IndicatorAppletFactory::IndicatorApplet",
            arch: Aur { package: "mate-indicator-applet", orphan: true },
            debian: Repo("mate-indicator-applet"),
            fedora: Missing,
            suse: Repo("mate-applet-indicator"),
        },
        PanelPart {
            iid: "AppmenuAppletFactory::AppmenuApplet",
            arch: Aur { package: "vala-panel-appmenu-mate", orphan: false },
            debian: Repo("mate-applet-appmenu"),
            fedora: Missing,
            suse: Missing,
        },
        PanelPart {
            iid: "WindowButtonsAppletFactory::WindowButtonsApplet",
            arch: Aur { package: "mate-window-buttons-applet", orphan: true },
            debian: Repo("mate-window-buttons-applet"),
            fedora: Missing,
            suse: Missing,
        },
        PanelPart {
            iid: "MateWindowPickerFactory::MateWindowPicker",
            arch: Repo("mate-netbook"),
            debian: Repo("mate-netbook"),
            fedora: Missing,
            suse: Repo("mate-netbook"),
        },
        PanelPart {
            iid: "DriveMountAppletFactory::DriveMountApplet",
            arch: Repo("mate-applets"),
            debian: Repo("mate-applets"),
            fedora: Repo("mate-applets"),
            suse: Repo("mate-applets"),
        },
        PanelPart {
            iid: "StickyNotesAppletFactory::StickyNotesApplet",
            arch: Repo("mate-applets"),
            debian: Repo("mate-applets"),
            fedora: Repo("mate-applets"),
            suse: Repo("mate-applets"),
        },
        PanelPart {
            iid: "TrashAppletFactory::TrashApplet",
            arch: Repo("mate-applets"),
            debian: Repo("mate-applets"),
            fedora: Repo("mate-applets"),
            suse: Repo("mate-applets"),
        },
        PanelPart {
            iid: "GvcAppletFactory::GvcApplet",
            arch: Repo("mate-media"),
            debian: Repo("mate-media"),
            fedora: Repo("mate-media"),
            suse: Repo("mate-media"),
        },
    ]
};

/// Откуда поставить часть `iid` в семействе `family`.
pub fn part_source(iid: &str, family: &str) -> PartSource {
    let Some(part) = PANEL_PARTS.iter().find(|p| p.iid == iid) else {
        return PartSource::Missing;
    };
    match family {
        "arch" => part.arch,
        "debian" => part.debian,
        "fedora" => part.fedora,
        "suse" => part.suse,
        _ => PartSource::Missing,
    }
}

/// Все пакеты частей раскладок для семейства — и из репозиториев, и
/// помеченные «только AUR» (на CachyOS часть из них лежит в репозитории, и
/// тогда их ставит помощник пакетов). Белый список установки помощника
/// (`system_plan::pkg_helper_install_allowlist`).
pub fn all_part_packages(family: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for part in PANEL_PARTS {
        let pkg = match part_source(part.iid, family) {
            PartSource::Repo(p) => p,
            PartSource::Aur { package, .. } => package,
            PartSource::Missing => continue,
        };
        if !out.contains(&pkg) {
            out.push(pkg);
        }
    }
    out
}

/// Апплеты раскладки `text`, которых нет в системе (`installed` —
/// [`installed_applets`]), без повторов, в порядке файла.
pub fn missing_parts(text: &str, installed: &BTreeSet<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for g in parse_keyfile(text) {
        if !g.is_object() || g.get("object-type") != Some("applet") {
            continue;
        }
        let iid = g.get("applet-iid").unwrap_or("").to_string();
        if !iid.is_empty() && !installed.contains(&iid) && !out.contains(&iid) {
            out.push(iid);
        }
    }
    out
}

/// Что можно поставить из недостающего.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartsOffer {
    /// Пакеты репозиториев (окно пароля администратора).
    pub repo: Vec<String>,
    /// Пакеты AUR: имя и «без сопровождающего».
    pub aur: Vec<(String, bool)>,
    /// Что из раскладки они принесут — человеческие имена.
    pub labels: Vec<String>,
    /// Те из `labels`, что приносят только пакеты AUR.
    pub aur_labels: Vec<String>,
}

impl PartsOffer {
    pub fn is_empty(&self) -> bool {
        self.repo.is_empty() && self.aur.is_empty()
    }
}

/// Что из недостающего `missing` ставится в семействе `family`. Пустое
/// семейство — Rexon здесь пакеты не ставит (SteamOS, неизменяемая
/// система, незнакомый менеджер).
pub fn parts_offer(missing: &[String], family: &str) -> PartsOffer {
    let mut offer = PartsOffer::default();
    for iid in missing {
        let label = applet_label(iid);
        let added = match part_source(iid, family) {
            PartSource::Repo(p) => {
                if !offer.repo.iter().any(|x| x == p) {
                    offer.repo.push(p.to_string());
                }
                true
            }
            PartSource::Aur { package, orphan } => {
                if !offer.aur.iter().any(|(x, _)| x == package) {
                    offer.aur.push((package.to_string(), orphan));
                }
                if !offer.aur_labels.contains(&label) {
                    offer.aur_labels.push(label.clone());
                }
                true
            }
            PartSource::Missing => false,
        };
        if added && !offer.labels.contains(&label) {
            offer.labels.push(label);
        }
    }
    offer
}

impl PartsOffer {
    /// То, что Rexon ставит САМ, без вопроса (решение владельца 24.10;
    /// защита — окно пароля polkit): пакеты официальных репозиториев. Сборка
    /// из AUR без вопроса недопустима — она остаётся за [`parts_question`].
    pub fn repo_only(&self) -> PartsOffer {
        let labels = self.labels.iter().filter(|l| !self.aur_labels.contains(l)).cloned().collect();
        PartsOffer { repo: self.repo.clone(), aur: Vec::new(), labels, aur_labels: Vec::new() }
    }
}

/// Вопрос перед применением раскладки `id`, которой не хватает частей:
/// (заголовок, текст). Задаётся только о сборке из AUR: пакеты из
/// репозиториев ставятся и без него. `None` — спрашивать не о чем.
pub fn parts_question(id: &str, offer: &PartsOffer) -> Option<(String, String)> {
    if offer.aur.is_empty() {
        return None;
    }
    let mut parts = vec![trf(
        "The “%1” layout needs %2. Install them together with the layout?",
        &[&title_of(id), &offer.labels.join(", ")],
    )];
    if !offer.repo.is_empty() {
        parts.push(trf(
            "Packages: %1 (an administrator password may be requested).",
            &[&offer.repo.join(", ")],
        ));
    }
    if !offer.aur.is_empty() {
        let names: Vec<&str> = offer.aur.iter().map(|(n, _)| n.as_str()).collect();
        parts.push(trf(
            "Built from AUR: %1 — this takes a few minutes.",
            &[&names.join(", ")],
        ));
        let orphans: Vec<&str> = offer.aur.iter().filter(|(_, o)| *o).map(|(n, _)| n.as_str()).collect();
        if !orphans.is_empty() {
            parts.push(trf(
                "%1 has no maintainer in AUR: nobody keeps the package up to date, and it may fail to build.",
                &[&orphans.join(", ")],
            ));
        }
    }
    parts.push(tr("“No” — the parts from AUR are replaced, as described under the layout."));
    Some((tr("Install the missing parts?"), parts.join("\n\n")))
}

/// Предложение для живой системы: семейство и запреты — как у пакета
/// Conky (`de_apply::live_conky_package`).
pub fn live_parts_offer(io: &dyn LookIo, ctx: &PanelCtx, id: &str) -> PartsOffer {
    let Some((_, path)) = list_layouts(io, ctx).into_iter().find(|(name, _)| name == id) else {
        return PartsOffer::default();
    };
    let Some(text) = io.read(&path) else {
        return PartsOffer::default();
    };
    let detector = crate::system::DistroDetector::detect_live();
    let family = if detector.is_steamos() || detector.is_immutable() {
        String::new()
    } else {
        detector.family_live().as_str().to_string()
    };
    let installed = installed_applets(io, &ctx.applet_dirs);
    parts_offer(&missing_parts(&text, &installed), &family)
}

/// Поставить пакеты частей из официальных репозиториев (окно пароля) — тем
/// же путём, что значки и курсор GTK-темы; из AUR этот путь не собирает.
pub fn live_install_repo_parts(packages: &[String]) -> Result<(), String> {
    crate::appearance_installer::live_install_repo_packages(packages, crate::privilege::op::PkgSource::MatePanel)
}

/// Отказ применения: части, которые ставили, не встали («всё или
/// ничего»). Окно пароля закрыто — «передумал»: совет применить ещё раз и
/// ввести пароль; иначе — причина.
pub fn parts_refusal(id: &str, offer: &PartsOffer, why: &str) -> String {
    if why == crate::appearance_installer::password_not_confirmed() {
        return crate::i18n::fill(
            // TRANSLATORS: %1 is the layout name, %2 a list of panel parts.
            &crate::i18n::ntr(
                "The “%1” layout wasn't applied: it needs %2, which isn't installed. Apply the layout \
                 again and enter the password to install it.",
                "The “%1” layout wasn't applied: it needs %2, which aren't installed. Apply the layout \
                 again and enter the password to install them.",
                offer.labels.len() as u64,
            ),
            &[&title_of(id), &offer.labels.join(", ")],
        );
    }
    trf(
        // TRANSLATORS: %1 is the layout name, %2 a list of panel parts, %3 the reason.
        "The “%1” layout wasn't applied: couldn't install %2: %3.",
        &[&title_of(id), &offer.labels.join(", "), &why],
    )
}

/// Поставить части (`install`), затем применить раскладку — она подхватит
/// всё, что встало (mate-panel следит за каталогом апплетов сам). «Всё или
/// ничего»: не встало — раскладка НЕ применяется, и сказано почему и что
/// сделать. Части, которых взять негде (или из AUR, от сборки которых
/// человек отказался), раскладка заменяет, как описано под ней.
pub fn apply_with_parts(
    io: &dyn LookIo,
    ctx: &PanelCtx,
    id: &str,
    offer: &PartsOffer,
    install: &mut dyn FnMut(&PartsOffer) -> Result<(), String>,
) -> String {
    if !offer.is_empty() {
        if let Err(why) = install(offer) {
            return parts_refusal(id, offer, &why);
        }
    }
    apply(io, ctx, id)
}

// ═══════════════════════════════════════════════════════════════════════════
// Где искать
// ═══════════════════════════════════════════════════════════════════════════

/// Где лежат раскладки, апплеты и наши файлы.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelCtx {
    /// Окружение как его зовёт ядро (`system::detect_current_de`).
    pub de: String,
    /// `$XDG_DATA_HOME` либо `~/.local/share`.
    pub data_home: String,
    /// Каталоги раскладок в порядке поиска: первый найденный файл с именем
    /// побеждает.
    pub layout_dirs: Vec<String>,
    /// Каталоги описаний апплетов (`*.mate-panel-applet`).
    pub applet_dirs: Vec<String>,
}

impl PanelCtx {
    /// `xdg_data_dirs` и `applets_env` — значения `XDG_DATA_DIRS` и
    /// `MATE_PANEL_APPLETS_DIR` (пусто — не заданы).
    pub fn new(de: &str, home: &str, xdg_data_home: Option<&str>, xdg_data_dirs: &str, applets_env: &str) -> Self {
        let data_home = crate::utils::data_home(xdg_data_home, home);
        // Каталог самого mate-panel — первым: по имени панель ищет только
        // там. Остальные системные каталоги данных — для сборок с другой
        // приставкой (/usr/local, NixOS); их раскладки применяются по пути.
        let mut layout_dirs = vec![SYSTEM_LAYOUTS_DIR.to_string()];
        let data_dirs = if xdg_data_dirs.trim().is_empty() { "/usr/local/share:/usr/share" } else { xdg_data_dirs };
        for dir in data_dirs.split(':').filter(|d| !d.is_empty()) {
            let candidate = format!("{}/mate-panel/layouts", dir.trim_end_matches('/'));
            if !layout_dirs.contains(&candidate) {
                layout_dirs.push(candidate);
            }
        }
        // Свои раскладки человека — последними: имя системной раскладки
        // они не перекрывают.
        let own = copies_dir_of(&data_home);
        if !layout_dirs.contains(&own) {
            layout_dirs.push(own);
        }
        let applet_dirs: Vec<String> = applets_env.split(':').filter(|d| !d.is_empty()).map(str::to_string).collect();
        let applet_dirs = if applet_dirs.is_empty() { vec![SYSTEM_APPLETS_DIR.to_string()] } else { applet_dirs };
        PanelCtx { de: de.to_string(), data_home, layout_dirs, applet_dirs }
    }

    /// Из окружения процесса (окно, стенд).
    pub fn from_env() -> Self {
        let env = |n: &str| std::env::var(n).unwrap_or_default();
        let de = crate::animations::LookCtx::from_env().de;
        let xdg = env("XDG_DATA_HOME");
        PanelCtx::new(
            &de,
            &env("HOME"),
            if xdg.is_empty() { None } else { Some(&xdg) },
            &env("XDG_DATA_DIRS"),
            &env("MATE_PANEL_APPLETS_DIR"),
        )
    }

    /// Каталог исправленных копий.
    pub fn copies_dir(&self) -> String {
        copies_dir_of(&self.data_home)
    }

    /// Снимок панели перед последним применением.
    pub fn backup_path(&self) -> String {
        format!("{}/rexon/backups/mate-panel.dconf", self.data_home.trim_end_matches('/'))
    }
}

fn copies_dir_of(data_home: &str) -> String {
    format!("{}/mate-panel/layouts", data_home.trim_end_matches('/'))
}

// ═══════════════════════════════════════════════════════════════════════════
// Разбор key-file
// ═══════════════════════════════════════════════════════════════════════════

/// Группа key-file с ключами в порядке файла.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub keys: Vec<(String, String)>,
}

impl Group {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.keys.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    fn set(&mut self, key: &str, value: &str) {
        match self.keys.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.to_string(),
            None => self.keys.push((key.to_string(), value.to_string())),
        }
    }

    fn remove(&mut self, key: &str) {
        self.keys.retain(|(k, _)| k != key);
    }

    /// Имя после приставки группы (`[Object briskmenu]` → `briskmenu`), если
    /// группа этого вида.
    fn suffix(&self, prefix: &str) -> Option<&str> {
        let rest = self.name.strip_prefix(prefix)?;
        (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim())
    }

    fn is_toplevel(&self) -> bool {
        self.suffix("Toplevel").is_some()
    }

    fn is_object(&self) -> bool {
        self.suffix("Object").is_some()
    }
}

/// Разобрать key-file (формат GKeyFile): группы и ключи в порядке файла,
/// комментарии и пустые строки пропускаются.
pub fn parse_keyfile(text: &str) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            groups.push(Group { name: name.trim().to_string(), keys: Vec::new() });
            continue;
        }
        let (Some(group), Some((key, value))) = (groups.last_mut(), line.split_once('=')) else {
            continue;
        };
        group.keys.push((key.trim().to_string(), value.trim().to_string()));
    }
    groups
}

fn write_keyfile(groups: &[Group]) -> String {
    let mut out = String::new();
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("[{}]\n", group.name));
        for (k, v) in &group.keys {
            out.push_str(&format!("{k}={v}\n"));
        }
    }
    out
}

// ═══════════════════════════════════════════════════════════════════════════
// Апплеты в системе
// ═══════════════════════════════════════════════════════════════════════════

/// Апплеты, которые есть в системе, в виде `applet-iid`
/// (`Фабрика::Апплет`): в файле `*.mate-panel-applet` группа
/// `[Applet Factory]` называет фабрику (`Id=`), остальные группы — апплеты.
pub fn installed_applets(io: &dyn LookIo, dirs: &[String]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for dir in dirs {
        for name in io.list_dir(dir) {
            if !name.ends_with(".mate-panel-applet") {
                continue;
            }
            let Some(text) = io.read(&format!("{}/{name}", dir.trim_end_matches('/'))) else {
                continue;
            };
            let groups = parse_keyfile(&text);
            let Some(factory) = groups.iter().find(|g| g.name == "Applet Factory").and_then(|g| g.get("Id")) else {
                continue;
            };
            for group in groups.iter().filter(|g| g.name != "Applet Factory") {
                out.insert(format!("{factory}::{}", group.name));
            }
        }
    }
    out
}

/// Человеческое имя апплета.
pub fn applet_label(iid: &str) -> String {
    match APPLET_LABELS.iter().find(|(id, _)| *id == iid) {
        Some((_, label)) => tr(label),
        None => iid.rsplit("::").next().unwrap_or(iid).to_string(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Подготовка раскладки
// ═══════════════════════════════════════════════════════════════════════════

/// Раскладка, готовая к применению.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// Текст раскладки, который примет mate-panel этой системы.
    pub text: String,
    /// Правилось ли что-нибудь (нет — можно применять исходный файл).
    pub changed: bool,
    /// Сколько в ней панелей.
    pub panels: usize,
    /// Заметки для человека: что заменено и чего не будет.
    pub notes: Vec<String>,
}

/// Подготовить раскладку к применению в ЭТОЙ системе (см. описание модуля):
/// `installed` — апплеты в системе ([`installed_applets`]), `file_exists`
/// — есть ли файл значка запуска.
pub fn prepare(text: &str, installed: &BTreeSet<String>, file_exists: &dyn Fn(&str) -> bool) -> Prepared {
    let groups = parse_keyfile(text);
    let mut changed = false;
    let mut notes = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    let clock_available = installed.contains(CLOCK_IID);
    let mut has_clock = groups.iter().any(|g| g.is_object() && g.get("applet-iid") == Some(CLOCK_IID));
    let mut out: Vec<Group> = Vec::new();

    for mut group in groups {
        let allowed = if group.is_toplevel() {
            TOPLEVEL_KEYS
        } else if group.is_object() {
            OBJECT_KEYS
        } else {
            // `[Customsetting …]` и прочее, чего mate-panel не читает.
            changed = true;
            continue;
        };
        if group.is_object() {
            // Ключ GNOME Panel: mate-panel о нём не знает и выбросил бы
            // объект целиком. Смысл тот же, что у `panel-right-stick`.
            if let Some(edge) = group.get("relative-to-edge").map(str::to_string) {
                if edge == "end" && group.get("panel-right-stick").is_none() {
                    group.set("panel-right-stick", "true");
                }
                group.remove("relative-to-edge");
                changed = true;
            }
        }
        let before = group.keys.len();
        group.keys.retain(|(k, _)| allowed.contains(&k.as_str()));
        changed |= group.keys.len() != before;

        if group.is_object() {
            match group.get("object-type").unwrap_or("") {
                "applet" => {
                    let iid = group.get("applet-iid").unwrap_or("").to_string();
                    if !installed.contains(&iid) {
                        changed = true;
                        if MENU_IIDS.contains(&iid.as_str()) {
                            group.set("object-type", "menu");
                            group.remove("applet-iid");
                            notes.push(trf(
                                "%1 is not installed — the standard MATE menu button will be used instead.",
                                &[&capitalize(&applet_label(&iid))],
                            ));
                        } else if INDICATOR_IIDS.contains(&iid.as_str()) && clock_available && !has_clock {
                            group.set("applet-iid", CLOCK_IID);
                            has_clock = true;
                            notes.push(tr("Indicators are not installed — a clock will be shown instead."));
                        } else {
                            let label = applet_label(&iid);
                            if !dropped.contains(&label) {
                                dropped.push(label);
                            }
                            continue;
                        }
                    }
                }
                "launcher" => {
                    // Значок программы, которой нет: панель всё равно его
                    // пропустит (с жалобой в журнал) — убираем сами.
                    let location = group.get("launcher-location").unwrap_or("");
                    if location.starts_with('/') && !file_exists(location) {
                        changed = true;
                        continue;
                    }
                }
                _ => {}
            }
        }
        out.push(group);
    }
    if !dropped.is_empty() {
        notes.push(trf("Not installed and will be left out: %1.", &[&dropped.join(", ")]));
    }
    let panels = out.iter().filter(|g| g.is_toplevel()).count();
    Prepared { text: write_keyfile(&out), changed, panels, notes }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Подписи
// ═══════════════════════════════════════════════════════════════════════════

/// Описание незнакомой раскладки по её составу: где стоят панели.
pub fn auto_description(text: &str) -> String {
    let mut sides: Vec<String> = Vec::new();
    for group in parse_keyfile(text).iter().filter(|g| g.is_toplevel()) {
        let side = match group.get("orientation").unwrap_or("top") {
            "bottom" => tr("at the bottom"),
            "left" => tr("on the left"),
            "right" => tr("on the right"),
            _ => tr("at the top"),
        };
        if !sides.contains(&side) {
            sides.push(side);
        }
    }
    if sides.is_empty() {
        return tr("A panel layout without panels.");
    }
    trf("Panels: %1.", &[&sides.join(", ")])
}

fn title_of(id: &str) -> String {
    match KNOWN_LAYOUTS.iter().find(|(name, _, _)| *name == id) {
        Some((_, title, _)) => tr(title),
        None => capitalize(&id.replace(['-', '_'], " ")),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Снимок раздела
// ═══════════════════════════════════════════════════════════════════════════

/// Одна раскладка в списке.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LayoutEntry {
    /// Имя файла без `.layout`.
    pub id: String,
    pub title: String,
    pub description: String,
    /// Что будет иначе, чем задумано в раскладке (заменено, пропущено).
    pub notes: Vec<String>,
    /// Выбрана сейчас (`default-layout`).
    pub current: bool,
    /// Полный путь исходного файла.
    #[serde(skip)]
    pub path: String,
}

/// Снимок раздела «Панель MATE».
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PanelState {
    /// Показывать раздел: сеанс MATE и mate-panel есть.
    pub available: bool,
    pub layouts: Vec<LayoutEntry>,
    /// Название текущей раскладки (пусто — не знаем).
    pub current_title: String,
    /// Есть снимок прежней панели — можно вернуть.
    pub can_restore: bool,
}

/// Раскладки в системе: известные — в порядке [`KNOWN_LAYOUTS`], остальные
/// — по алфавиту. Наши копии (`rexon-*`) в список не входят.
pub fn list_layouts(io: &dyn LookIo, ctx: &PanelCtx) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for dir in &ctx.layout_dirs {
        for name in io.list_dir(dir) {
            let Some(id) = name.strip_suffix(".layout") else {
                continue;
            };
            if id.is_empty() || id.starts_with(COPY_PREFIX) || found.iter().any(|(f, _)| f == id) {
                continue;
            }
            found.push((id.to_string(), format!("{}/{name}", dir.trim_end_matches('/'))));
        }
    }
    let rank = |id: &str| KNOWN_LAYOUTS.iter().position(|(n, _, _)| *n == id).unwrap_or(usize::MAX);
    found.sort_by(|a, b| rank(&a.0).cmp(&rank(&b.0)).then_with(|| a.0.cmp(&b.0)));
    found
}

/// Имя раскладки из значения `default-layout` (имя, путь к файлу или путь
/// к нашей копии).
pub fn layout_id_from_setting(value: &str) -> String {
    let value = value.trim();
    if !value.starts_with('/') {
        return value.to_string();
    }
    let file = value.rsplit('/').next().unwrap_or(value);
    let stem = file.strip_suffix(".layout").unwrap_or(file);
    stem.strip_prefix(COPY_PREFIX).unwrap_or(stem).to_string()
}

/// Показывать ли раздел.
pub fn is_available(io: &dyn LookIo, ctx: &PanelCtx) -> bool {
    ctx.de == "mate" && io.have("mate-panel")
}

/// Снимок раздела.
pub fn read_state(io: &dyn LookIo, ctx: &PanelCtx) -> PanelState {
    let mut st = PanelState { available: false, layouts: Vec::new(), current_title: String::new(), can_restore: false };
    if !is_available(io, ctx) {
        return st;
    }
    st.available = true;
    let current = gsettings_get(io, "org.mate.panel", "default-layout")
        .map(|v| layout_id_from_setting(&v))
        .unwrap_or_default();
    let installed = installed_applets(io, &ctx.applet_dirs);
    let exists = |path: &str| io.read(path).is_some();
    for (id, path) in list_layouts(io, ctx) {
        let Some(text) = io.read(&path) else {
            continue;
        };
        let description = match KNOWN_LAYOUTS.iter().find(|(name, _, _)| *name == id) {
            Some((_, _, desc)) => tr(desc),
            None => auto_description(&text),
        };
        let mut notes = prepare(&text, &installed, &exists).notes;
        let dock = path.strip_suffix(".layout").map(|p| format!("{p}.dock")).unwrap_or_default();
        if io.read(&dock).is_some() {
            notes.push(tr("Designed together with the Plank dock — Rexon doesn't turn the dock on."));
        }
        if id == current {
            st.current_title = title_of(&id);
        }
        st.layouts.push(LayoutEntry { title: title_of(&id), current: id == current, description, notes, path, id });
    }
    st.can_restore = io.read(&ctx.backup_path()).is_some();
    st
}

/// Текст подтверждения перед применением: (заголовок, вопрос).
pub fn confirm_texts(id: &str) -> (String, String) {
    (
        tr("Replace the MATE panel?"),
        trf(
            "The current panel will be replaced with the “%1” layout. Its applets, launchers and \
             panel settings will be reset to what this layout has. Rexon saves the current panel \
             first, so you can bring it back with “Restore previous panel”.",
            &[&title_of(id)],
        ),
    )
}

/// Текст подтверждения перед возвратом прежней панели: (заголовок, вопрос).
pub fn restore_confirm_texts() -> (String, String) {
    (
        tr("Restore the previous MATE panel?"),
        tr("The panel will go back to how it was before Rexon first changed its layout. \
            Changes made to the panel since then will be lost."),
    )
}

// ═══════════════════════════════════════════════════════════════════════════
// Применение и возврат
// ═══════════════════════════════════════════════════════════════════════════

/// Применить раскладку `id` → сообщение для человека.
pub fn apply(io: &dyn LookIo, ctx: &PanelCtx, id: &str) -> String {
    if !is_available(io, ctx) {
        return tr("⚠️ The MATE panel is not available in this session.");
    }
    let title = title_of(id);
    let Some((_, path)) = list_layouts(io, ctx).into_iter().find(|(name, _)| name == id) else {
        return trf("⚠️ Panel layout “%1” was not found.", &[&title]);
    };
    let Some(text) = io.read(&path) else {
        return trf("⚠️ Could not read panel layout “%1”.", &[&title]);
    };
    let installed = installed_applets(io, &ctx.applet_dirs);
    let prepared = prepare(&text, &installed, &|p: &str| io.read(p).is_some());
    if prepared.panels == 0 {
        return trf("⚠️ Panel layout “%1” has no panels.", &[&title]);
    }

    // Снимок прежней панели — до любых изменений. Не вышло — не трогаем.
    // Исходную панель человека (снимок до первого применения) следующие
    // применения не перезаписывают: их снимок ложится рядом.
    let backup = ctx.backup_path();
    let mark = crate::backup::panel_original_returned_mark(&backup);
    let slot = crate::backup::panel_snapshot_slot(io.read(&backup).is_some(), io.read(&mark).as_deref());
    let snapshot = match slot {
        crate::backup::PanelSnapshotSlot::Original => backup.clone(),
        crate::backup::PanelSnapshotSlot::Last => crate::backup::panel_last_snapshot_path(&backup),
    };
    let (ok, dump) = io.run(&argv(&["dconf", "dump", PANEL_DCONF_DIR]));
    if !ok || !io.write(&snapshot, &dump) {
        let why = if ok { snapshot } else { dump };
        return trf("⚠️ Could not save the current panel, so nothing was changed: %1", &[&why]);
    }
    if slot == crate::backup::PanelSnapshotSlot::Original && io.read(&mark).is_some() {
        io.write(&mark, "");
    }

    let target = if !prepared.changed && path == format!("{SYSTEM_LAYOUTS_DIR}/{id}.layout") {
        id.to_string()
    } else {
        let copy = format!("{}/{COPY_PREFIX}{id}.layout", ctx.copies_dir());
        if !io.write(&copy, &prepared.text) {
            return trf("⚠️ Could not write file %1", &[&copy]);
        }
        copy
    };
    let (ok, out) = io.run(&argv(&["mate-panel", "--layout", &target, "--reset"]));
    if !ok {
        return trf("⚠️ Could not apply panel layout “%1”: %2", &[&title, &out]);
    }
    trf("✅ MATE panel: layout “%1” applied.", &[&title])
}

/// «Сохранить панель» вкладки «Рабочие столы»: снять копию панели в тот же
/// файл, что перед применением раскладки ([`PanelCtx::backup_path`]) —
/// копия у MATE одна, и «Вернуть панель» и «Вернуть прежнюю панель»
/// возвращают одно и то же. `(удалось, путь либо причина)`.
///
/// Копию сохранил сам человек — она и есть его панель: следующие применения
/// раскладок её не перезаписывают (`backup::panel_snapshot_slot`).
pub fn save_snapshot(io: &dyn LookIo, ctx: &PanelCtx) -> (bool, String) {
    let (ok, dump) = io.run(&argv(&["dconf", "dump", PANEL_DCONF_DIR]));
    if !ok {
        return (false, dump);
    }
    let path = ctx.backup_path();
    if io.write(&path, &dump) {
        let mark = crate::backup::panel_original_returned_mark(&path);
        if io.read(&mark).is_some() {
            io.write(&mark, "");
        }
        (true, path)
    } else {
        (false, path)
    }
}

/// Вернуть исходную панель — какой она была до первого применения
/// раскладки (или сохранённую «Сохранить панель»). После возврата следующее
/// применение снимает исходную заново.
pub fn restore(io: &dyn LookIo, ctx: &PanelCtx) -> String {
    if !is_available(io, ctx) {
        return tr("⚠️ The MATE panel is not available in this session.");
    }
    let backup = ctx.backup_path();
    if io.read(&backup).is_none() {
        return tr("⚠️ There is no saved panel to restore.");
    }
    // Страховка: то, что стоит сейчас. Шаги ниже разрушают панель до
    // загрузки снимка, и испорченный снимок оставлял бы её пустой и
    // сброшенной.
    let undo = format!("{backup}.before-restore");
    let (ok, now) = io.run(&argv(&["dconf", "dump", PANEL_DCONF_DIR]));
    if !ok || !io.write(&undo, &now) {
        let why = if ok { undo } else { now };
        return trf("⚠️ Could not save the current panel, so nothing was changed: %1", &[&why]);
    }
    // Сначала — пустой список объектов: работающая панель снимает все
    // апплеты и стирает их ветки. Без этого объект с тем же именем в обеих
    // раскладках (`notification-area` есть и в «Как Windows», и в
    // классической) панель не перезагружает, и он оставался на чужой
    // панели (замер на стенде). Ждём, пока панель управится (ветка
    // объектов опустела); панели нет — ждать некого, через 3 с идём дальше.
    let (ok, out) = io.run(&argv(&["dconf", "write", "/org/mate/panel/general/object-id-list", "@as []"]));
    if !ok {
        return trf("⚠️ Could not restore the previous panel: %1", &[&out]);
    }
    for _ in 0..30 {
        let (ok, out) = io.run(&argv(&["dconf", "list", "/org/mate/panel/objects/"]));
        if !ok || out.trim().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // Панели и объекты — начисто: иначе ключи, которых в снимке нет (он
    // хранит только отличия от умолчаний), остались бы от раскладки.
    // `default-layout` — туда же: снимок с ним по умолчанию его не несёт.
    let steps = [
        argv(&["dconf", "reset", "-f", "/org/mate/panel/toplevels/"]),
        argv(&["dconf", "reset", "-f", "/org/mate/panel/objects/"]),
        argv(&["dconf", "reset", "/org/mate/panel/general/default-layout"]),
        argv(&["sh", "-c", "dconf load /org/mate/panel/ < \"$1\"", "sh", &backup]),
    ];
    for step in &steps {
        let (ok, out) = io.run(step);
        if !ok {
            // Вернуть панель, какой она была до нажатия.
            for back in [
                argv(&["dconf", "reset", "-f", "/org/mate/panel/toplevels/"]),
                argv(&["dconf", "reset", "-f", "/org/mate/panel/objects/"]),
                argv(&["sh", "-c", "dconf load /org/mate/panel/ < \"$1\"", "sh", &undo]),
            ] {
                io.run(&back);
            }
            return trf("⚠️ Could not restore the previous panel: %1", &[&out]);
        }
    }
    // Исходная вернулась: следующее применение снимает её заново. Отметка не
    // записалась — исходная просто остаётся прежней (это безопасная сторона).
    io.write(&crate::backup::panel_original_returned_mark(&backup), crate::backup::PANEL_ORIGINAL_RETURNED);
    tr("✅ MATE panel restored to how it was before.")
}

// ═══════════════════════════════════════════════════════════════════════════
// Своя раскладка (образы «Как в дистрибутиве»)
// ═══════════════════════════════════════════════════════════════════════════
//
// Образ описывает панели ролями (`native/desktop-model.json`: меню, окна,
// часы, лоток, значки запуска), а раскладку mate-panel из них собирает
// [`own_layout_text`] — только из того, что есть в самом mate-panel
// (кнопка меню, список окон, часы, область уведомлений, значки запуска),
// поэтому окна «не удалось загрузить апплет» не бывает. Применение и возврат
// — те же `mate-panel --layout <путь> --reset` и снимок `dconf dump`, что у
// готовых раскладок выше, но одной командой оболочки на шаг: их исполняет
// общий прогон образа, а не [`LookIo`].

/// Объект своей раскладки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnObject {
    /// Встроенная кнопка главного меню MATE.
    MainMenu,
    WindowList,
    Clock,
    NotificationArea,
    /// Значок запуска: полный путь к `.desktop`.
    Launcher(String),
}

/// Панель своей раскладки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnPanel {
    /// Имя панели в раскладке (`top`, `dock`).
    pub id: String,
    pub top: bool,
    pub size: u32,
    /// Во всю ширину; иначе — по содержимому.
    pub expand: bool,
    /// Посередине края (док).
    pub centered: bool,
    /// Слева направо: объекты у начала края.
    pub start: Vec<OwnObject>,
    /// Справа налево от конца края (`panel-right-stick`).
    pub end: Vec<OwnObject>,
}

/// Текст раскладки mate-panel из панелей `panels`.
pub fn own_layout_text(panels: &[OwnPanel]) -> String {
    let mut groups: Vec<Group> = Vec::new();
    let mut objects: Vec<Group> = Vec::new();
    for p in panels {
        let mut keys = vec![
            ("expand".to_string(), p.expand.to_string()),
            ("orientation".to_string(), if p.top { "top" } else { "bottom" }.to_string()),
            ("size".to_string(), p.size.to_string()),
        ];
        if p.centered {
            keys.push(("x-centered".to_string(), "true".to_string()));
        }
        // Панель «по содержимому» mate-panel ставит по координатам и край
        // выводит из них же: без `y-bottom` док уезжал наверх (стенд Xvfb
        // 31.10.2026, mate-panel 1.28.7: `orientation='top'` после применения).
        if !p.expand {
            keys.push((if p.top { "y" } else { "y-bottom" }.to_string(), "0".to_string()));
        }
        groups.push(Group { name: format!("Toplevel {}", p.id), keys });
        for (stick, list) in [(false, &p.start), (true, &p.end)] {
            for (i, o) in list.iter().enumerate() {
                let (name, mut keys) = match o {
                    OwnObject::MainMenu => ("menu".to_string(), vec![("object-type", "menu".to_string())]),
                    OwnObject::WindowList => (
                        "window-list".to_string(),
                        vec![("object-type", "applet".to_string()), ("applet-iid", "WnckletFactory::WindowListApplet".to_string())],
                    ),
                    OwnObject::Clock => (
                        "clock".to_string(),
                        vec![("object-type", "applet".to_string()), ("applet-iid", CLOCK_IID.to_string())],
                    ),
                    OwnObject::NotificationArea => (
                        "notification-area".to_string(),
                        vec![
                            ("object-type", "applet".to_string()),
                            ("applet-iid", "NotificationAreaAppletFactory::NotificationArea".to_string()),
                        ],
                    ),
                    OwnObject::Launcher(path) => {
                        let stem = std::path::Path::new(path)
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        (
                            format!("launcher-{stem}"),
                            vec![("object-type", "launcher".to_string()), ("launcher-location", path.clone())],
                        )
                    }
                };
                keys.push(("toplevel-id", p.id.clone()));
                keys.push(("position", (i * 10).to_string()));
                if stick {
                    keys.push(("panel-right-stick", "true".to_string()));
                }
                keys.push(("locked", "true".to_string()));
                objects.push(Group {
                    name: format!("Object {}-{name}", p.id),
                    keys: keys.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                });
            }
        }
    }
    groups.extend(objects);
    write_keyfile(&groups)
}

/// Снять панель в `dump` и применить раскладку `layout` — одной командой:
/// без снимка раскладка не применяется (как у [`apply`]).
pub fn own_apply_argv(layout: &str, dump: &str) -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        "mkdir -p \"$(dirname \"$2\")\" && dconf dump /org/mate/panel/ > \"$2\" && mate-panel --layout \"$1\" --reset",
        "sh",
        layout,
        dump,
    ])
}

/// Вернуть панель из снимка `dump` — та же последовательность, что у
/// [`restore`] (пустой список объектов, ждать, пока панель их снимет,
/// панели и объекты начисто, загрузить снимок); снимок после загрузки
/// удаляется.
pub fn own_restore_argv(dump: &str) -> Vec<String> {
    argv(&[
        "sh",
        "-c",
        "[ -f \"$1\" ] || exit 0; dconf write /org/mate/panel/general/object-id-list '@as []'; i=0; \
         while [ $i -lt 30 ] && [ -n \"$(dconf list /org/mate/panel/objects/)\" ]; do sleep 0.1; i=$((i+1)); done; \
         dconf reset -f /org/mate/panel/toplevels/; dconf reset -f /org/mate/panel/objects/; \
         dconf reset /org/mate/panel/general/default-layout; dconf load /org/mate/panel/ < \"$1\" && rm -f \"$1\"",
        "sh",
        dump,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// Копия `/usr/share/mate-panel/layouts/redmond.layout` (ubuntu-mate-settings).
    const REDMOND: &str = "[Toplevel bottom]
expand=true
orientation=bottom
size=28

[Object briskmenu]
object-type=applet
applet-iid=BriskMenuFactory::BriskMenu
toplevel-id=bottom
position=0
locked=true

[Object showdesktopapplet]
locked=true
position=10
toplevel-id=bottom
applet-iid=WnckletFactory::ShowDesktopApplet
object-type=applet

[Object firefox]
locked=true
position=20
launcher-location=/usr/share/applications/firefox.desktop
toplevel-id=bottom
object-type=launcher
menu-path=applications:/

[Object evolution]
locked=true
position=30
launcher-location=/usr/share/applications/org.gnome.Evolution.desktop
toplevel-id=bottom
object-type=launcher
menu-path=applications:/

[Object window-list]
object-type=applet
applet-iid=WnckletFactory::WindowListApplet
toplevel-id=bottom
position=40
locked=true

[Object drivemountapplet]
object-type=applet
applet-iid=DriveMountAppletFactory::DriveMountApplet
toplevel-id=bottom
position=20
relative-to-edge=end
locked=true

[Object notification-area]
object-type=applet
applet-iid=NotificationAreaAppletFactory::NotificationArea
toplevel-id=bottom
position=10
relative-to-edge=end
locked=true

[Object indicatorappletcomplete]
object-type=applet
applet-iid=IndicatorAppletCompleteFactory::IndicatorAppletComplete
toplevel-id=bottom
position=0
relative-to-edge=end
locked=true
";

    /// Копия `/usr/share/mate-panel/layouts/linuxmint.layout` (mate-panel 1.28.7).
    const LINUXMINT: &str = "[Toplevel bottom]
expand=true
orientation=bottom
size=24

[Object menu-bar]
object-type=applet
applet-iid=MintMenuAppletFactory::MintMenuApplet
toplevel-id=bottom
position=0
locked=true

[Object show-desktop]
object-type=applet
applet-iid=WnckletFactory::ShowDesktopApplet
toplevel-id=bottom
position=1
locked=true

[Object window-list]
object-type=applet
applet-iid=WnckletFactory::WindowListApplet
toplevel-id=bottom
position=20
locked=true

[Object notification-area]
object-type=applet
applet-iid=NotificationAreaAppletFactory::NotificationArea
toplevel-id=bottom
position=10
panel-right-stick=true
locked=true

[Object clock]
object-type=applet
applet-iid=ClockAppletFactory::ClockApplet
toplevel-id=bottom
position=0
panel-right-stick=true
locked=true
";

    /// Копия `/usr/share/mate-panel/layouts/default.layout` (mate-panel 1.28.7).
    const DEFAULT: &str = "[Toplevel top]
expand=true
orientation=top
size=24

[Toplevel bottom]
expand=true
orientation=bottom
size=24

[Object menu-bar]
object-type=menu-bar
toplevel-id=top
position=0
locked=true

[Object notification-area]
object-type=applet
applet-iid=NotificationAreaAppletFactory::NotificationArea
toplevel-id=top
position=10
panel-right-stick=true
locked=true

[Object clock]
object-type=applet
applet-iid=ClockAppletFactory::ClockApplet
toplevel-id=top
position=0
panel-right-stick=true
locked=true

[Object show-desktop]
object-type=applet
applet-iid=WnckletFactory::ShowDesktopApplet
toplevel-id=bottom
position=0
locked=true

[Object window-list]
object-type=applet
applet-iid=WnckletFactory::WindowListApplet
toplevel-id=bottom
position=20
locked=true

[Object workspace-switcher]
object-type=applet
applet-iid=WnckletFactory::WorkspaceSwitcherApplet
toplevel-id=bottom
position=0
panel-right-stick=true
locked=true
";

    /// Сокращённая копия `org.mate.panel.Wncklet.mate-panel-applet`.
    const WNCKLET: &str = "[Applet Factory]
Id=WnckletFactory
InProcess=false
Location=/usr/lib/mate-panel/wnck-applet
Name=Window Navigation Applet Factory

[WindowMenuApplet]
Name=Window Selector

[WorkspaceSwitcherApplet]
Name=Workspace Switcher

[WindowListApplet]
Name=Window List

[ShowDesktopApplet]
Name=Show Desktop
";

    /// Апплеты этой машины (MATE без brisk-menu, mintmenu и индикаторов).
    fn plain_mate() -> BTreeSet<String> {
        [
            "WnckletFactory::WindowMenuApplet",
            "WnckletFactory::WorkspaceSwitcherApplet",
            "WnckletFactory::WindowListApplet",
            "WnckletFactory::ShowDesktopApplet",
            "ClockAppletFactory::ClockApplet",
            "NotificationAreaAppletFactory::NotificationArea",
            "DriveMountAppletFactory::DriveMountApplet",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn firefox_only(path: &str) -> bool {
        path == "/usr/share/applications/firefox.desktop"
    }

    fn object<'a>(groups: &'a [Group], id: &str) -> Option<&'a Group> {
        groups.iter().find(|g| g.name == format!("Object {id}"))
    }

    #[test]
    fn keyfile_keeps_groups_and_key_order() {
        let groups = parse_keyfile(REDMOND);
        assert_eq!(groups.len(), 9);
        assert_eq!(groups[0].name, "Toplevel bottom");
        assert_eq!(groups[0].get("orientation"), Some("bottom"));
        assert_eq!(groups[2].keys[0], ("locked".to_string(), "true".to_string()));
        // Сборка и разбор сходятся.
        assert_eq!(parse_keyfile(&write_keyfile(&groups)), groups);
    }

    #[test]
    fn applets_are_read_as_factory_and_applet() {
        struct Dir;
        impl LookIo for Dir {
            fn run(&self, _: &[String]) -> (bool, String) {
                (false, String::new())
            }
            fn have(&self, _: &str) -> bool {
                false
            }
            fn read(&self, path: &str) -> Option<String> {
                (path == "/a/org.mate.panel.Wncklet.mate-panel-applet").then(|| WNCKLET.to_string())
            }
            fn write(&self, _: &str, _: &str) -> bool {
                false
            }
            fn list_dir(&self, path: &str) -> Vec<String> {
                if path == "/a" {
                    vec!["org.mate.panel.Wncklet.mate-panel-applet".into(), "README".into()]
                } else {
                    Vec::new()
                }
            }
        }
        let got = installed_applets(&Dir, &["/a".to_string()]);
        assert!(got.contains("WnckletFactory::WindowListApplet"));
        assert!(got.contains("WnckletFactory::ShowDesktopApplet"));
        assert_eq!(got.len(), 4);
    }

    #[test]
    fn redmond_is_fixed_for_plain_mate() {
        let p = prepare(REDMOND, &plain_mate(), &firefox_only);
        assert!(p.changed);
        assert_eq!(p.panels, 1);
        let groups = parse_keyfile(&p.text);

        // Ни одного ключа, которого не знает mate-panel 1.28.
        assert!(!p.text.contains("relative-to-edge"));
        // Правый край сохранён смыслом.
        let tray = object(&groups, "notification-area").expect("область уведомлений осталась");
        assert_eq!(tray.get("panel-right-stick"), Some("true"));
        assert_eq!(object(&groups, "drivemountapplet").unwrap().get("panel-right-stick"), Some("true"));

        // Меню Brisk → встроенная кнопка меню.
        let menu = object(&groups, "briskmenu").expect("меню осталось");
        assert_eq!(menu.get("object-type"), Some("menu"));
        assert_eq!(menu.get("applet-iid"), None);

        // Индикаторы → часы, справа.
        let clock = object(&groups, "indicatorappletcomplete").expect("вместо индикаторов — часы");
        assert_eq!(clock.get("applet-iid"), Some(CLOCK_IID));
        assert_eq!(clock.get("panel-right-stick"), Some("true"));

        // Evolution нет — значка нет; Firefox есть — значок есть.
        assert!(object(&groups, "evolution").is_none());
        assert!(object(&groups, "firefox").is_some());
        assert_eq!(p.notes.len(), 2, "{:?}", p.notes);
    }

    #[test]
    fn redmond_with_everything_installed_only_loses_the_gnome_key() {
        let mut all = plain_mate();
        all.insert("BriskMenuFactory::BriskMenu".into());
        all.insert("IndicatorAppletCompleteFactory::IndicatorAppletComplete".into());
        let p = prepare(REDMOND, &all, &|_: &str| true);
        assert!(p.changed, "relative-to-edge всё равно правится");
        assert!(p.notes.is_empty());
        let groups = parse_keyfile(&p.text);
        assert_eq!(object(&groups, "briskmenu").unwrap().get("applet-iid"), Some("BriskMenuFactory::BriskMenu"));
        assert_eq!(groups.len(), 9);
    }

    #[test]
    fn default_layout_needs_no_changes() {
        let p = prepare(DEFAULT, &plain_mate(), &|_: &str| true);
        assert!(!p.changed);
        assert!(p.notes.is_empty());
        assert_eq!(p.panels, 2);
    }

    #[test]
    fn linuxmint_without_mintmenu_gets_the_menu_button_and_keeps_its_clock() {
        let p = prepare(LINUXMINT, &plain_mate(), &|_: &str| true);
        let groups = parse_keyfile(&p.text);
        assert_eq!(object(&groups, "menu-bar").unwrap().get("object-type"), Some("menu"));
        assert_eq!(groups.iter().filter(|g| g.get("applet-iid") == Some(CLOCK_IID)).count(), 1);
    }

    #[test]
    fn missing_indicators_are_dropped_when_the_layout_has_a_clock() {
        let text = format!(
            "{LINUXMINT}\n[Object ind]\nobject-type=applet\napplet-iid=IndicatorAppletCompleteFactory::IndicatorAppletComplete\ntoplevel-id=bottom\nposition=0\n\n[Customsetting windowcontrollayout]\nmate-general=menu:minimize\n"
        );
        let p = prepare(&text, &plain_mate(), &|_: &str| true);
        let groups = parse_keyfile(&p.text);
        assert!(object(&groups, "ind").is_none());
        assert!(!p.text.contains("Customsetting"));
        assert_eq!(groups.iter().filter(|g| g.get("applet-iid") == Some(CLOCK_IID)).count(), 1);
    }

    #[test]
    fn setting_value_maps_back_to_the_layout_name() {
        assert_eq!(layout_id_from_setting("redmond"), "redmond");
        assert_eq!(layout_id_from_setting("/usr/share/mate-panel/layouts/fedora.layout"), "fedora");
        assert_eq!(layout_id_from_setting("/home/w/.local/share/mate-panel/layouts/rexon-redmond.layout"), "redmond");
    }

    #[test]
    fn auto_description_names_panel_sides() {
        assert!(auto_description(DEFAULT).contains(", "));
        assert_eq!(auto_description("[Object x]\nobject-type=menu\n"), tr("A panel layout without panels."));
    }

    // ── Снимок, применение, возврат на подставной машине ────────────────

    #[derive(Default)]
    struct Machine {
        files: RefCell<BTreeMap<String, String>>,
        runs: RefCell<Vec<Vec<String>>>,
        dconf_fails: bool,
        /// `dconf load` снимка падает (снимок испорчен).
        snapshot_load_fails: bool,
    }

    impl LookIo for Machine {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.runs.borrow_mut().push(argv.to_vec());
            if self.snapshot_load_fails
                && argv.first().map(String::as_str) == Some("sh")
                && argv.last().is_some_and(|p| p.ends_with("mate-panel.dconf"))
            {
                return (false, "error: invalid key file".into());
            }
            match argv.first().map(String::as_str) {
                Some("gsettings") => (true, "'default'\n".into()),
                Some("dconf") if self.dconf_fails => (false, "dconf: no bus".into()),
                // Панель уже сняла апплеты: ветка объектов пуста.
                Some("dconf") if argv[1] != "dump" => (true, String::new()),
                Some("dconf") => (true, "[general]\ntoplevel-id-list=['top']\n".into()),
                _ => (true, String::new()),
            }
        }
        fn have(&self, tool: &str) -> bool {
            matches!(tool, "mate-panel" | "gsettings")
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            self.files.borrow_mut().insert(path.into(), text.into());
            true
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            let prefix = format!("{}/", path.trim_end_matches('/'));
            self.files
                .borrow()
                .keys()
                .filter_map(|k| k.strip_prefix(&prefix))
                .filter(|rest| !rest.contains('/'))
                .map(str::to_string)
                .collect()
        }
    }

    fn machine() -> (Machine, PanelCtx) {
        let m = Machine::default();
        for (name, text) in [("redmond.layout", REDMOND), ("default.layout", DEFAULT), ("zeta.layout", DEFAULT)] {
            m.write(&format!("{SYSTEM_LAYOUTS_DIR}/{name}"), text);
        }
        m.write(&format!("{SYSTEM_APPLETS_DIR}/org.mate.panel.Wncklet.mate-panel-applet"), WNCKLET);
        m.write(
            &format!("{SYSTEM_APPLETS_DIR}/org.mate.panel.ClockApplet.mate-panel-applet"),
            "[Applet Factory]\nId=ClockAppletFactory\n\n[ClockApplet]\nName=Clock\n",
        );
        m.write(
            &format!("{SYSTEM_APPLETS_DIR}/org.mate.panel.NotificationAreaApplet.mate-panel-applet"),
            "[Applet Factory]\nId=NotificationAreaAppletFactory\n\n[NotificationArea]\nName=Notification Area\n",
        );
        let ctx = PanelCtx::new("mate", "/home/w", None, "/usr/share", "");
        // Наша прежняя копия не должна попасть в список второй раз.
        m.write(&format!("{}/rexon-redmond.layout", ctx.copies_dir()), "[Toplevel x]\n");
        (m, ctx)
    }

    #[test]
    fn state_lists_layouts_known_first_and_marks_current() {
        let (m, ctx) = machine();
        let st = read_state(&m, &ctx);
        assert!(st.available);
        let ids: Vec<&str> = st.layouts.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["redmond", "default", "zeta"]);
        assert!(st.layouts[1].current);
        assert_eq!(st.current_title, tr("Classic MATE"));
        assert_eq!(st.layouts[0].title, tr("Like Windows"));
        assert!(!st.layouts[0].notes.is_empty(), "без Brisk и индикаторов есть что сказать");
        assert!(!st.can_restore);
    }

    #[test]
    fn section_is_hidden_outside_mate() {
        let (m, mut ctx) = machine();
        ctx.de = "xfce".into();
        assert!(!read_state(&m, &ctx).available);
    }

    #[test]
    fn apply_saves_the_panel_and_uses_a_fixed_copy() {
        let (m, ctx) = machine();
        let msg = apply(&m, &ctx, "redmond");
        assert!(msg.starts_with('✅'), "{msg}");
        assert!(m.read(&ctx.backup_path()).unwrap().contains("toplevel-id-list"));
        let copy = format!("{}/rexon-redmond.layout", ctx.copies_dir());
        assert!(!m.read(&copy).unwrap().contains("relative-to-edge"));
        let runs = m.runs.borrow();
        assert_eq!(runs.last().unwrap(), &argv(&["mate-panel", "--layout", &copy, "--reset"]));
        // Снимок — раньше применения.
        let dump = runs.iter().position(|r| r[0] == "dconf").unwrap();
        assert!(dump < runs.len() - 1);
        drop(runs);
        assert!(read_state(&m, &ctx).can_restore);
    }

    #[test]
    fn apply_of_a_clean_system_layout_goes_by_name() {
        let (m, ctx) = machine();
        apply(&m, &ctx, "default");
        assert_eq!(m.runs.borrow().last().unwrap(), &argv(&["mate-panel", "--layout", "default", "--reset"]));
    }

    #[test]
    fn apply_refuses_without_a_snapshot() {
        let (mut m, ctx) = machine();
        m.dconf_fails = true;
        let msg = apply(&m, &ctx, "redmond");
        assert!(msg.starts_with("⚠️"), "{msg}");
        assert!(!m.runs.borrow().iter().any(|r| r[0] == "mate-panel"));
    }

    #[test]
    fn restore_resets_then_loads_the_snapshot() {
        let (m, ctx) = machine();
        assert!(restore(&m, &ctx).starts_with("⚠️"), "снимка ещё нет");
        m.write(&ctx.backup_path(), "[general]\n");
        assert!(restore(&m, &ctx).starts_with('✅'));
        let runs = m.runs.borrow();
        let load = runs.last().unwrap();
        assert_eq!(load[0], "sh");
        assert_eq!(load.last().unwrap(), &ctx.backup_path());
        let pos = |want: Vec<String>| runs.iter().position(|r| *r == want).expect("шаг есть");
        // Сначала панель снимает апплеты, потом ветки стираются и грузится снимок.
        let emptied = pos(argv(&["dconf", "write", "/org/mate/panel/general/object-id-list", "@as []"]));
        let reset = pos(argv(&["dconf", "reset", "-f", "/org/mate/panel/objects/"]));
        assert!(emptied < reset);
    }

    /// Снимок не загрузился — панель возвращается к тому, что было до
    /// нажатия, а не остаётся пустой и сброшенной.
    #[test]
    fn failed_restore_puts_the_panel_back() {
        let (mut m, ctx) = machine();
        m.snapshot_load_fails = true;
        m.write(&ctx.backup_path(), "[general]\nbroken");
        assert!(restore(&m, &ctx).starts_with("⚠️"));
        let runs = m.runs.borrow();
        let loads: Vec<&Vec<String>> = runs.iter().filter(|r| r[0] == "sh").collect();
        assert_eq!(loads.len(), 2, "после неудачи нужна загрузка страховки: {runs:?}");
        let undo = loads[1].last().unwrap();
        assert_ne!(undo, &ctx.backup_path());
        // Страховка — то, что было до нажатия (вывод `dconf dump`).
        assert_eq!(m.read(undo).as_deref(), Some("[general]\ntoplevel-id-list=['top']\n"));
    }

    // ── Недостающие части раскладки ───────────────────────────────────────

    /// Таблица «часть раскладки → пакет по семейству → источник», сверено
    /// с репозиториями 19.10.2026 (archlinux.org и AUR RPC,
    /// packages.debian.org trixie, packages.ubuntu.com noble, Fedora mdapi
    /// f43, openSUSE Factory).
    #[test]
    fn parts_table_matches_the_repositories() {
        use PartSource::{Aur, Missing, Repo};
        let brisk = "BriskMenuFactory::BriskMenu";
        let ind = "IndicatorAppletCompleteFactory::IndicatorAppletComplete";
        let mint = "MintMenuAppletFactory::MintMenuApplet";
        assert_eq!(part_source(brisk, "arch"), Aur { package: "brisk-menu", orphan: false });
        assert_eq!(part_source(brisk, "debian"), Repo("mate-applet-brisk-menu"));
        assert_eq!(part_source(brisk, "fedora"), Missing);
        assert_eq!(part_source(brisk, "suse"), Repo("brisk-menu"));
        assert_eq!(part_source(ind, "arch"), Aur { package: "mate-indicator-applet", orphan: true });
        assert_eq!(part_source(ind, "debian"), Repo("mate-indicator-applet"));
        assert_eq!(part_source(ind, "suse"), Repo("mate-applet-indicator"));
        assert_eq!(part_source(mint, "arch"), Aur { package: "mintmenu", orphan: false });
        for f in ["debian", "fedora", "suse"] {
            assert_eq!(part_source(mint, f), Missing, "{f}");
        }
        assert_eq!(part_source("DriveMountAppletFactory::DriveMountApplet", "arch"), Repo("mate-applets"));
        assert_eq!(part_source("MateWindowPickerFactory::MateWindowPicker", "arch"), Repo("mate-netbook"));
        assert_eq!(part_source("Unknown::Thing", "arch"), Missing);
        // Каждая часть, которую `prepare` заменяет или выбрасывает, есть в таблице.
        for iid in MENU_IIDS.iter().chain(INDICATOR_IIDS) {
            assert!(PANEL_PARTS.iter().any(|p| p.iid == *iid), "{iid}");
        }
    }

    #[test]
    fn missing_parts_of_like_windows_are_offered_by_family() {
        let (m, ctx) = machine();
        let installed = installed_applets(&m, &ctx.applet_dirs);
        let missing = missing_parts(REDMOND, &installed);
        assert!(missing.contains(&"BriskMenuFactory::BriskMenu".to_string()), "{missing:?}");
        assert!(missing.contains(&"IndicatorAppletCompleteFactory::IndicatorAppletComplete".to_string()));
        assert!(!missing.iter().any(|i| i.starts_with("Wncklet")), "стоящее не просится: {missing:?}");

        let arch = parts_offer(&missing, "arch");
        assert!(arch.aur.contains(&("brisk-menu".to_string(), false)), "{arch:?}");
        assert!(arch.aur.contains(&("mate-indicator-applet".to_string(), true)));
        assert!(arch.repo.contains(&"mate-applets".to_string()), "{arch:?}");
        let (title, text) = parts_question("redmond", &arch).expect("есть что собирать из AUR");
        assert!(!title.is_empty());
        assert!(text.contains("AUR"), "{text}");
        assert!(text.contains("mate-indicator-applet"), "сирота названа: {text}");
        // Сам, без вопроса, ставится только то, что из репозиториев.
        let auto = arch.repo_only();
        assert!(auto.aur.is_empty() && auto.repo == arch.repo, "{auto:?}");
        assert!(!auto.labels.is_empty() && auto.labels.iter().all(|l| !arch.aur_labels.contains(l)), "{auto:?}");

        let debian = parts_offer(&missing, "debian");
        assert!(debian.aur.is_empty() && debian.repo.contains(&"mate-applet-brisk-menu".to_string()));
        // Всё есть в репозиториях — вопроса нет, части ставятся сами.
        assert!(parts_question("redmond", &debian).is_none());
        // Fedora: меню Brisk и индикаторов нет — вопрос только о том, что есть.
        let fedora = parts_offer(&["BriskMenuFactory::BriskMenu".to_string()], "fedora");
        assert!(fedora.is_empty() && parts_question("redmond", &fedora).is_none());
        // Неизменяемая система / незнакомый менеджер — не предлагать.
        assert!(parts_offer(&missing, "").is_empty());
    }

    /// Части ставятся ДО применения, и раскладка встаёт с ними; не встали —
    /// раскладка НЕ применяется («всё или ничего»), и сказано почему.
    #[test]
    fn apply_with_parts_installs_first_all_or_nothing() {
        let (m, ctx) = machine();
        let offer = parts_offer(&["BriskMenuFactory::BriskMenu".to_string()], "debian");
        let mut calls = Vec::new();
        let out = apply_with_parts(&m, &ctx, "redmond", &offer, &mut |o: &PartsOffer| {
            calls.push(o.repo.clone());
            Ok(())
        });
        assert_eq!(calls, [vec!["mate-applet-brisk-menu".to_string()]]);
        assert!(out.starts_with('✅'), "{out}");
        let (files, runs) = (m.files.borrow().clone(), m.runs.borrow().len());
        let out = apply_with_parts(&m, &ctx, "redmond", &offer, &mut |_| Err("no network".into()));
        assert!(out.contains("no network") && !out.contains('✅'), "{out}");
        assert_eq!((m.files.borrow().clone(), m.runs.borrow().len()), (files, runs), "раскладка не применялась");
        // Окно пароля закрыто — совет повторить, причина не повторяется.
        let pw = crate::appearance_installer::password_not_confirmed();
        let out = apply_with_parts(&m, &ctx, "redmond", &offer, &mut |_| Err(pw.clone()));
        assert!(!out.contains(&pw) && !out.contains('✅') && out.contains(&offer.labels[0]), "{out}");
        // Ставить нечего — установщик не зовётся.
        let mut called = false;
        apply_with_parts(&m, &ctx, "redmond", &PartsOffer::default(), &mut |_| {
            called = true;
            Ok(())
        });
        assert!(!called);
    }

    /// Исходная панель (снимок до первого применения) не перезаписывается
    /// следующими применениями; после возврата снимается заново. Снимок,
    /// снятый прежним выпуском (без отметки), — исходный.
    #[test]
    fn original_panel_survives_later_layouts_until_restored() {
        let (m, ctx) = machine();
        let backup = ctx.backup_path();
        let last = crate::backup::panel_last_snapshot_path(&backup);
        m.write(&backup, "[general]\nисходная\n");
        assert!(apply(&m, &ctx, "redmond").starts_with('✅'));
        assert!(apply(&m, &ctx, "default").starts_with('✅'));
        assert_eq!(m.read(&backup).as_deref(), Some("[general]\nисходная\n"), "исходная цела");
        assert_eq!(m.read(&last).as_deref(), Some("[general]\ntoplevel-id-list=['top']\n"));
        // «Вернуть прежнюю панель» грузит исходную.
        assert!(restore(&m, &ctx).starts_with('✅'));
        assert_eq!(m.runs.borrow().last().unwrap().last().unwrap(), &backup);
        // После возврата первое применение снимает исходную заново…
        assert!(apply(&m, &ctx, "redmond").starts_with('✅'));
        assert_eq!(m.read(&backup).as_deref(), Some("[general]\ntoplevel-id-list=['top']\n"));
        // …а следующее её уже не трогает.
        m.write(&backup, "[general]\nвторая исходная\n");
        assert!(apply(&m, &ctx, "default").starts_with('✅'));
        assert_eq!(m.read(&backup).as_deref(), Some("[general]\nвторая исходная\n"));
    }

    /// Копия «Сохранить панель» после возврата снова защищена от применений.
    #[test]
    fn manual_save_after_restore_is_kept_by_layouts() {
        let (m, ctx) = machine();
        let backup = ctx.backup_path();
        m.write(&backup, "[general]\n");
        assert!(restore(&m, &ctx).starts_with('✅'));
        assert!(save_snapshot(&m, &ctx).0);
        m.write(&backup, "[general]\nсохранённая\n");
        assert!(apply(&m, &ctx, "redmond").starts_with('✅'));
        assert_eq!(m.read(&backup).as_deref(), Some("[general]\nсохранённая\n"));
    }

    /// «Сохранить панель» и «Вернуть прежнюю панель» видят одну копию.
    #[test]
    fn saved_panel_is_the_same_copy_restore_uses() {
        let (m, ctx) = machine();
        let (ok, path) = save_snapshot(&m, &ctx);
        assert!(ok);
        assert_eq!(path, ctx.backup_path());
        assert_eq!(m.read(&path).as_deref(), Some("[general]\ntoplevel-id-list=['top']\n"));
        assert!(read_state(&m, &ctx).can_restore);
    }

}
