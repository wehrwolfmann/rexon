//! «Кнопки заголовка окна» и «Размер рамки» вкладки «Настройки» — во всех
//! окружениях родным ключом.
//!
//! Кнопки. Человек выбирает СТОРОНУ кнопок управления (справа, как в
//! Windows, или слева, как в macOS) и НАБОР — все кнопки модуля «Оформление
//! окон» KDE: меню окна, меню приложения, «на всех рабочих столах»,
//! свернуть, развернуть, закрыть, справка, «под другими», «поверх других»,
//! «скрыть от снимков», промежуток; сверх KDE — «свернуть в заголовок»
//! (у Marco и xfwm4 она есть, у KWin 6.8 буквы для неё нет). Справка и
//! кнопки управления стоят на своей стороне, меню и «булавки» — на
//! противоположной. Кнопки, которых окружение не знает (у GNOME
//! `appmenu`, `icon`), остаются на своей стороне как были — поэтому
//! раскладки по умолчанию переживают разбор и сборку без единого изменения.
//! Чего у окружения нет из набора KDE — раздел называет поимённо
//! ([`unavailable_note`]).
//!
//! Где что лежит (сверено со схемами и бинарями в системе):
//! * KDE — `kwinrc [org.kde.kdecoration2] ButtonsOnLeft/ButtonsOnRight`
//!   (буквы KWin 6.8, `decorations/settings.cpp`: M меню, N меню
//!   приложения, S на всех столах, H справка, I свернуть, A развернуть,
//!   X закрыть, F поверх, B под, E скрыть от снимков, `_` промежуток; по
//!   умолчанию `MSE` / `HIAX`). Там же три флажка модуля — [`FLAGS`];
//! * GNOME, Pantheon — `org.gnome.desktop.wm.preferences button-layout`
//!   (его же GTK берёт для окон с клиентским заголовком);
//! * Budgie — `com.solus-project.budgie-wm button-layout` + ключ GNOME;
//! * Cinnamon — `org.cinnamon.desktop.wm.preferences button-layout`
//!   (Muffin знает и `spacer`);
//! * MATE — `org.mate.Marco.general button-layout` +
//!   `org.mate.interface gtk-decoration-layout` (окна GTK с CSD);
//!   у Marco есть `appmenu`, `stick`, `above`, `shade`, `spacer`, у Muffin
//!   и Mutter `stick`/`above`/`shade` нет;
//! * XFCE — `xfwm4 /general/button_layout` (`O|SHMC`: O меню, H свернуть,
//!   M развернуть, C закрыть, T на всех столах, S свернуть в заголовок,
//!   `|` заголовок; промежутка, справки, «поверх» и «под» нет);
//!   xfsettingsd сам переносит раскладку в `Gtk/DecorationLayout`;
//! * COSMIC — только «свернуть» и «развернуть» вкл/выкл
//!   (`com.system76.CosmicTk/v1/show_minimize`, `show_maximize`);
//! * Deepin (Treeland) — своей раскладки нет, рамки Deepin и программы
//!   DTK кнопки не меняют: общая ветка GTK ниже, раздел честно об этом
//!   говорит ([`note`]);
//! * Hyprland, niri, river — заголовков у композитора нет вовсе; у sway
//!   заголовки есть, но кнопок на них нет (sway(5): `border normal` — рамка и
//!   заголовок, кнопок в описании нет). Кнопки рисуют только сами
//!   GTK-программы: пишем `gtk-decoration-layout` в `settings.ini` и ключ
//!   GNOME (его читает портал GTK). Так и говорим ([`note`]);
//! * Openbox — `<theme><titleLayout>` в `openbox/rc.xml` (буквы из
//!   описания в системном `rc.xml` Openbox 3.6: N значок окна — он же меню
//!   окна, L название, I свернуть, M развернуть, C закрыть, S свернуть в
//!   заголовок, D на всех столах; каждая не больше раза, по умолчанию
//!   `NLIMC`); перечитывает `openbox --reconfigure`;
//! * labwc — `<theme><titlebar><layout>` в `labwc/rc.xml` (labwc-config(5)
//!   0.20: `icon` значок окна, `menu` меню окна, `iconify`, `max`, `close`,
//!   `shade`, `desk` на всех столах; двоеточие обязательно, по умолчанию
//!   `icon:iconify,max,close`); перечитывает `labwc --reconfigure`. Свой
//!   `rc.xml` у обоих заменяет системный целиком, поэтому без своего он
//!   заводится копией системного (так же, как для шрифта заголовка,
//!   `qt_settings::rc_xml_with_title_font`);
//! * LXQt — раскладку пишет тот менеджер окон, что выбран в его сеансе
//!   ([`wm_key`] по `qt_settings::title_wm_for`): Openbox и labwc — как выше,
//!   KWin, xfwm4 и Marco — теми же ключами, что в KDE, XFCE и MATE.
//!   Незнакомый менеджер — подпись говорит, что Rexon его кнопок не пишет.
//! В KDE тот же `gtk-decoration-layout` дублируется в `settings.ini`, чтобы
//! GTK-программы с CSD следовали раскладке и без модуля kde-gtk-config.
//!
//! Рамка. KDE — `BorderSize` (None … Oversized) и `BorderSizeAuto`
//! («как задумано оформлением», по умолчанию); Hyprland —
//! `general.border_size`; niri — ширина кольца фокуса и рамки; sway —
//! `default_border`/`default_floating_border <стиль> N` (sway(5); стиль —
//! с заголовком или без — берём из конфига человека, меняем только
//! толщину); labwc — `border.width` в `themerc-override` (labwc-theme(5)).
//! В MATE и XFCE рамку рисует тема окон Marco/xfwm4 (у них свои разделы
//! «Оформления»); в GNOME, Pantheon и Cinnamon — тема GTK: Mutter и Muffin
//! берут заголовки из CSS темы программ (у Muffin ключ `org.cinnamon.desktop.
//! wm.preferences theme` не читается с версии 5.4 — в `src/core/prefs.c` его
//! больше нет). Отдельной толщины там нет, раздел честно об этом говорит.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::animations::{
    gsettings_get, gsettings_set, kconfig_value, kwin_reconfigure, kwrite, parse_state_line, read_own_wm_file,
    run_steps, state_line, uses_gtk_ini, wm_include, xfconf_get, xfconf_set, LookCtx, LookIo, LookStep,
};
use crate::i18n::{n_, tr, trf};
use crate::themes::WmSyntax;

/// Кнопка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Btn {
    Menu,
    Minimize,
    Maximize,
    Close,
    Sticky,
    Above,
    /// Меню приложения (KDE `N`, Marco `appmenu`).
    AppMenu,
    /// Справка «Что это?» (KDE `H`).
    Help,
    /// Под другими окнами (KDE `B`).
    Below,
    /// Свернуть в заголовок (Marco `shade`, xfwm4 `S`; у KWin буквы нет).
    Shade,
    /// Скрыть от снимков и записи экрана (KDE `E`).
    ExcludeCapture,
    /// Промежуток (KDE `_`, `spacer` у Marco и Muffin).
    Spacer,
    /// Родное имя кнопки, которой мы не управляем — сохраняется как есть.
    Other(String),
}

/// Управляемые кнопки: (id, подпись) — в порядке списка модуля KDE.
pub const MANAGED: &[(&str, &str)] = &[
    ("menu", n_("Window menu")),
    ("appmenu", n_("Application menu")),
    ("sticky", n_("On all desktops")),
    ("minimize", n_("Minimize")),
    ("maximize", n_("Maximize")),
    ("close", n_("Close")),
    ("help", n_("Context help")),
    ("below", n_("Keep below others")),
    ("above", n_("Keep above others")),
    ("shade", n_("Roll up into title bar")),
    ("exclude_capture", n_("Hide from screenshots and screen recordings")),
    ("spacer", n_("Spacer")),
];

/// Набор кнопок модуля «Оформление окон» KDE — мерило «чего нет» у
/// остальных окружений («свернуть в заголовок» в него не входит: у KWin её
/// нет).
const KDE_SET: &[&str] =
    &["menu", "appmenu", "sticky", "minimize", "maximize", "close", "help", "below", "above", "exclude_capture", "spacer"];

impl Btn {
    fn id(&self) -> &str {
        match self {
            Btn::Menu => "menu",
            Btn::Minimize => "minimize",
            Btn::Maximize => "maximize",
            Btn::Close => "close",
            Btn::Sticky => "sticky",
            Btn::Above => "above",
            Btn::AppMenu => "appmenu",
            Btn::Help => "help",
            Btn::Below => "below",
            Btn::Shade => "shade",
            Btn::ExcludeCapture => "exclude_capture",
            Btn::Spacer => "spacer",
            Btn::Other(_) => "",
        }
    }
    fn from_id(id: &str) -> Option<Btn> {
        Some(match id {
            "menu" => Btn::Menu,
            "minimize" => Btn::Minimize,
            "maximize" => Btn::Maximize,
            "close" => Btn::Close,
            "sticky" => Btn::Sticky,
            "above" => Btn::Above,
            "appmenu" => Btn::AppMenu,
            "help" => Btn::Help,
            "below" => Btn::Below,
            "shade" => Btn::Shade,
            "exclude_capture" => Btn::ExcludeCapture,
            "spacer" => Btn::Spacer,
            _ => return None,
        })
    }
}

/// Раскладка: левая и правая стороны заголовка.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    pub left: Vec<Btn>,
    pub right: Vec<Btn>,
}

/// Как окружение пишет раскладку.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// Буквы KWin, две строки.
    Kde,
    /// `menu,minimize:maximize,close` (GNOME, GTK, Marco, Muffin).
    Gnome,
    /// `O|HMC` xfwm4.
    Xfwm,
    /// `NLIMC` Openbox: буквы, `L` — название окна между сторонами.
    Openbox,
    /// `icon:iconify,max,close` labwc.
    Labwc,
}

fn kde_char(b: &Btn) -> String {
    match b {
        Btn::Menu => "M".into(),
        Btn::Minimize => "I".into(),
        Btn::Maximize => "A".into(),
        Btn::Close => "X".into(),
        Btn::Sticky => "S".into(),
        Btn::Above => "F".into(),
        Btn::AppMenu => "N".into(),
        Btn::Help => "H".into(),
        Btn::Below => "B".into(),
        Btn::ExcludeCapture => "E".into(),
        Btn::Spacer => "_".into(),
        // У KWin 6.8 буквы для «свернуть в заголовок» нет.
        Btn::Shade => String::new(),
        Btn::Other(s) => s.clone(),
    }
}

fn kde_btn(c: char) -> Btn {
    match c {
        'M' => Btn::Menu,
        'I' => Btn::Minimize,
        'A' => Btn::Maximize,
        'X' => Btn::Close,
        'S' => Btn::Sticky,
        'F' => Btn::Above,
        'N' => Btn::AppMenu,
        'H' => Btn::Help,
        'B' => Btn::Below,
        'E' => Btn::ExcludeCapture,
        '_' => Btn::Spacer,
        other => Btn::Other(other.to_string()),
    }
}

fn gnome_token(b: &Btn) -> String {
    match b {
        Btn::Menu => "menu".into(),
        Btn::Minimize => "minimize".into(),
        Btn::Maximize => "maximize".into(),
        Btn::Close => "close".into(),
        Btn::Sticky => "stick".into(),
        Btn::Above => "above".into(),
        Btn::AppMenu => "appmenu".into(),
        Btn::Shade => "shade".into(),
        Btn::Spacer => "spacer".into(),
        // Справки, «под» и «скрыть от снимков» в этой записи нет.
        Btn::Help | Btn::Below | Btn::ExcludeCapture => String::new(),
        Btn::Other(s) => s.clone(),
    }
}

fn gnome_btn(t: &str) -> Btn {
    match t {
        "menu" => Btn::Menu,
        "minimize" => Btn::Minimize,
        "maximize" => Btn::Maximize,
        "close" => Btn::Close,
        "stick" => Btn::Sticky,
        "above" => Btn::Above,
        "appmenu" => Btn::AppMenu,
        "shade" => Btn::Shade,
        "spacer" => Btn::Spacer,
        other => Btn::Other(other.to_string()),
    }
}

fn xfwm_char(b: &Btn) -> String {
    match b {
        Btn::Menu => "O".into(),
        Btn::Minimize => "H".into(),
        Btn::Maximize => "M".into(),
        Btn::Close => "C".into(),
        Btn::Sticky => "T".into(),
        Btn::Shade => "S".into(),
        Btn::Above | Btn::AppMenu | Btn::Help | Btn::Below | Btn::ExcludeCapture | Btn::Spacer => String::new(),
        Btn::Other(s) => s.clone(),
    }
}

fn xfwm_btn(c: char) -> Btn {
    match c {
        'O' => Btn::Menu,
        'H' => Btn::Minimize,
        'M' => Btn::Maximize,
        'C' => Btn::Close,
        'T' => Btn::Sticky,
        'S' => Btn::Shade,
        other => Btn::Other(other.to_string()),
    }
}

fn openbox_char(b: &Btn) -> String {
    match b {
        Btn::Menu => "N".into(),
        Btn::Minimize => "I".into(),
        Btn::Maximize => "M".into(),
        Btn::Close => "C".into(),
        Btn::Shade => "S".into(),
        Btn::Sticky => "D".into(),
        Btn::Above | Btn::AppMenu | Btn::Help | Btn::Below | Btn::ExcludeCapture | Btn::Spacer => String::new(),
        Btn::Other(s) => s.clone(),
    }
}

fn openbox_btn(c: char) -> Btn {
    match c {
        'N' => Btn::Menu,
        'I' => Btn::Minimize,
        'M' => Btn::Maximize,
        'C' => Btn::Close,
        'S' => Btn::Shade,
        'D' => Btn::Sticky,
        other => Btn::Other(other.to_string()),
    }
}

fn labwc_token(b: &Btn) -> String {
    match b {
        Btn::Menu => "menu".into(),
        Btn::Minimize => "iconify".into(),
        Btn::Maximize => "max".into(),
        Btn::Close => "close".into(),
        Btn::Shade => "shade".into(),
        Btn::Sticky => "desk".into(),
        Btn::Above | Btn::AppMenu | Btn::Help | Btn::Below | Btn::ExcludeCapture | Btn::Spacer => String::new(),
        Btn::Other(s) => s.clone(),
    }
}

fn labwc_btn(t: &str) -> Btn {
    match t {
        "menu" => Btn::Menu,
        "iconify" => Btn::Minimize,
        "max" => Btn::Maximize,
        "close" => Btn::Close,
        "shade" => Btn::Shade,
        "desk" => Btn::Sticky,
        other => Btn::Other(other.to_string()),
    }
}

/// Разбор родной записи. У KDE — `left` и `right` отдельно.
pub fn parse(codec: Codec, left: &str, right: &str) -> Layout {
    match codec {
        Codec::Kde => Layout { left: left.chars().map(kde_btn).collect(), right: right.chars().map(kde_btn).collect() },
        Codec::Gnome => {
            let (l, r) = left.split_once(':').unwrap_or(("", left));
            let side = |s: &str| s.split(',').map(str::trim).filter(|t| !t.is_empty()).map(gnome_btn).collect();
            Layout { left: side(l), right: side(r) }
        }
        Codec::Xfwm => {
            let (l, r) = left.split_once('|').unwrap_or(("", left));
            Layout { left: l.chars().map(xfwm_btn).collect(), right: r.chars().map(xfwm_btn).collect() }
        }
        // Без `L` Openbox названия не рисует, а кнопки ставит подряд слева.
        Codec::Openbox => {
            let (l, r) = left.trim().split_once('L').unwrap_or((left.trim(), ""));
            Layout { left: l.chars().map(openbox_btn).collect(), right: r.chars().map(openbox_btn).collect() }
        }
        Codec::Labwc => {
            let (l, r) = left.split_once(':').unwrap_or(("", left));
            let side = |s: &str| s.split(',').map(str::trim).filter(|t| !t.is_empty()).map(labwc_btn).collect();
            Layout { left: side(l), right: side(r) }
        }
    }
}

/// Сборка родной записи: (левая, правая) у KDE, одна строка в `.0` у прочих.
pub fn format(codec: Codec, layout: &Layout) -> (String, String) {
    match codec {
        Codec::Kde => (
            layout.left.iter().map(kde_char).collect(),
            layout.right.iter().map(kde_char).collect(),
        ),
        Codec::Gnome => {
            let side = |v: &[Btn]| v.iter().map(gnome_token).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(",");
            (format!("{}:{}", side(&layout.left), side(&layout.right)), String::new())
        }
        Codec::Xfwm => {
            let side = |v: &[Btn]| v.iter().map(xfwm_char).collect::<String>();
            (format!("{}|{}", side(&layout.left), side(&layout.right)), String::new())
        }
        Codec::Openbox => {
            let side = |v: &[Btn]| v.iter().map(openbox_char).collect::<String>();
            (format!("{}L{}", side(&layout.left), side(&layout.right)), String::new())
        }
        Codec::Labwc => {
            let side = |v: &[Btn]| v.iter().map(labwc_token).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(",");
            (format!("{}:{}", side(&layout.left), side(&layout.right)), String::new())
        }
    }
}

/// Выбор человека: управляемые кнопки на каждой стороне заголовка, по
/// порядку слева направо. Обе стороны хранятся отдельно — раскладка вроде
/// `close:maximize` (Pantheon) переживает разбор и сборку как есть, а не
/// сваливается на одну сторону.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Choice {
    pub left: Vec<String>,
    pub right: Vec<String>,
}

impl Choice {
    /// Все выбранные кнопки (левая сторона, потом правая).
    pub fn all(&self) -> Vec<String> {
        self.left.iter().chain(self.right.iter()).cloned().collect()
    }
    fn side_mut(&mut self, side: &str) -> &mut Vec<String> {
        if side == "left" {
            &mut self.left
        } else {
            &mut self.right
        }
    }
}

fn is_control(b: &Btn) -> bool {
    matches!(b, Btn::Minimize | Btn::Maximize | Btn::Close)
}

/// Что выбрано в раскладке: управляемые кнопки по сторонам, как стоят.
pub fn choice_of(layout: &Layout) -> Choice {
    let ids = |v: &[Btn]| v.iter().map(Btn::id).filter(|id| !id.is_empty()).map(str::to_string).collect();
    Choice { left: ids(&layout.left), right: ids(&layout.right) }
}

/// На какой стороне кнопки управления: `left` — как в macOS, `right` —
/// как в Windows. По «закрыть», а без него — по остальным кнопкам
/// управления.
pub fn side_of(layout: &Layout) -> &'static str {
    if layout.left.contains(&Btn::Close) {
        "left"
    } else if layout.right.contains(&Btn::Close) {
        "right"
    } else if layout.left.iter().any(is_control) && !layout.right.iter().any(is_control) {
        "left"
    } else {
        "right"
    }
}

/// Порядок кнопок на каждой стороне слева направо: слева — как в macOS
/// («закрыть» у края), справа — как в Windows («закрыть» у края, перед ним
/// «развернуть» и «свернуть»). Справка стоит при кнопках управления, как в
/// раскладке KDE по умолчанию `MSE|HIAX`; меню и «булавки» — по порядку
/// модуля KDE.
const LEFT_ORDER: &[&str] = &[
    "close", "minimize", "maximize", "help", "menu", "appmenu", "sticky", "shade", "below", "above", "exclude_capture", "spacer",
];
const RIGHT_ORDER: &[&str] = &[
    "menu", "appmenu", "sticky", "shade", "below", "above", "exclude_capture", "spacer", "help", "minimize", "maximize", "close",
];

/// Кнопки, которые готовая раскладка ставит на сторону кнопок управления
/// («свернуть в заголовок» — как у xfwm4 по умолчанию `O|SHMC`).
const CONTROL_SIDE: &[&str] = &["shade", "help", "minimize", "maximize", "close"];

fn order_index(side: &str, id: &str) -> usize {
    let order = if side == "left" { LEFT_ORDER } else { RIGHT_ORDER };
    order.iter().position(|x| *x == id).unwrap_or(order.len())
}

/// Готовая раскладка: кнопки управления на стороне `side`, меню и
/// «булавки» — на противоположной.
pub fn preset(side: &str, buttons: &[String]) -> Choice {
    let has = |id: &str| buttons.iter().any(|b| b == id);
    let other = if side == "left" { RIGHT_ORDER } else { LEFT_ORDER };
    let own = if side == "left" { LEFT_ORDER } else { RIGHT_ORDER };
    let controls: Vec<String> =
        own.iter().filter(|i| CONTROL_SIDE.contains(i) && has(i)).map(|i| i.to_string()).collect();
    let extras: Vec<String> =
        other.iter().filter(|i| !CONTROL_SIDE.contains(i) && has(i)).map(|i| i.to_string()).collect();
    if side == "left" {
        Choice { left: controls, right: extras }
    } else {
        Choice { left: extras, right: controls }
    }
}

/// Поставить (`on`) кнопку `id` на сторону `side` или снять её. Кнопка
/// живёт на одной стороне: поставленная сюда уходит с другой. Новая встаёт
/// по порядку стороны ([`LEFT_ORDER`] / [`RIGHT_ORDER`]), уже стоящие не
/// двигаются. Промежуток — не кнопка: он бывает на обеих сторонах сразу, и
/// галочка трогает только свою сторону.
pub fn toggle(choice: &Choice, side: &str, id: &str, on: bool) -> Choice {
    let mut c = choice.clone();
    if id == "spacer" {
        c.side_mut(side).retain(|b| b != id);
    } else {
        c.left.retain(|b| b != id);
        c.right.retain(|b| b != id);
    }
    if on {
        let rank = order_index(side, id);
        let list = c.side_mut(side);
        let at = list.iter().position(|b| order_index(side, b) > rank).unwrap_or(list.len());
        list.insert(at, id.to_string());
    }
    c
}

/// Полка редактора перетаскиванием: место кнопок, которых нет в заголовке.
pub const SHELF: &str = "shelf";

fn is_side(place: &str) -> bool {
    place == "left" || place == "right"
}

/// Перенос кнопки, как перетаскиванием в модуле «Оформление окон» KDE: из
/// `from` (`left` / `right` с позицией `from_index` или полки [`SHELF`]) на
/// сторону `to` в позицию `to_index` — место в стороне уже без переносимой
/// кнопки (больше длины — в конец), — либо на полку (снять). Кнопка стоит в
/// одном месте: поставленная уходит со старого. Промежуток — не кнопка: их
/// сколько угодно, с полки он всегда берётся новым.
pub fn move_button(choice: &Choice, id: &str, from: &str, from_index: usize, to: &str, to_index: usize) -> Choice {
    let mut c = choice.clone();
    if is_side(from) {
        let list = c.side_mut(from);
        // Позиция устарела (раздел перечитан) — берём первую такую кнопку.
        let at = if list.get(from_index).map(String::as_str) == Some(id) {
            Some(from_index)
        } else {
            list.iter().position(|b| b == id)
        };
        if let Some(at) = at {
            list.remove(at);
        }
    }
    if id != "spacer" {
        c.left.retain(|b| b != id);
        c.right.retain(|b| b != id);
    }
    if is_side(to) {
        let list = c.side_mut(to);
        let at = to_index.min(list.len());
        list.insert(at, id.to_string());
    }
    c
}

/// Шаг кнопкой или клавишей (геймпад, клавиатура): `left` / `right` — на
/// место левее или правее по заголовку (крайняя кнопка стороны переходит
/// через название на другую сторону, к названию), [`SHELF`] — снять. Ответ —
/// новый выбор и где кнопка теперь (`None` — на полке или шага нет).
pub fn nudge(choice: &Choice, side: &str, index: usize, dir: &str) -> (Choice, Option<(String, usize)>) {
    let list = if side == "left" { &choice.left } else { &choice.right };
    let Some(id) = list.get(index).cloned() else {
        return (choice.clone(), None);
    };
    let target = match (side, dir) {
        (_, SHELF) => None,
        ("left", "left") | ("right", "left") if index > 0 => Some((side, index - 1)),
        ("right", "left") => Some(("left", choice.left.len())),
        ("left", "right") | ("right", "right") if index + 1 < list.len() => Some((side, index + 1)),
        ("left", "right") => Some(("right", 0)),
        // Дальше края окна шагать некуда.
        _ => return (choice.clone(), Some((side.to_string(), index))),
    };
    match target {
        Some((to, at)) => (move_button(choice, &id, side, index, to, at), Some((to.to_string(), at))),
        None => (move_button(choice, &id, side, index, SHELF, 0), None),
    }
}

/// Кнопка с полки — кнопкой «Добавить слева / справа»: встаёт по порядку
/// стороны, как галочкой ([`toggle`]); промежуток — к названию окна.
pub fn add_button(choice: &Choice, id: &str, side: &str) -> Choice {
    if id != "spacer" {
        return toggle(choice, side, id, true);
    }
    let at = if side == "left" { choice.left.len() } else { 0 };
    move_button(choice, id, SHELF, 0, side, at)
}

/// Значок кнопки на макете заголовка и на полке (знак шрифта: значков
/// оформления, как у KDE, вне KWin нет).
pub fn glyph(id: &str) -> &'static str {
    match id {
        "menu" => "≡",
        "appmenu" => "⋮",
        "sticky" => "◉",
        "minimize" => "–",
        "maximize" => "□",
        "close" => "✕",
        "help" => "?",
        "below" => "▾",
        "above" => "▴",
        "shade" => "▔",
        "exclude_capture" => "⊘",
        _ => "",
    }
}

/// Почему кнопки на полке нет у этого окружения (подсказка недоступной).
pub fn missing_reason(de: &str) -> String {
    let wm = match de {
        "kde" => "KWin",
        "mate" => "Marco",
        "xfce" => "xfwm4",
        "cinnamon-mint" => "Muffin",
        "openbox" => "Openbox",
        "labwc" => "labwc",
        "cosmic" => "COSMIC",
        _ => return tr("Title bars drawn by GTK have no such button."),
    };
    trf("%1 has no such title bar button.", &[&wm])
}

/// Одна сторона: управляемые кнопки по выбору; чужие остаются на ней же —
/// стоявшие до первой управляемой встают перед ними, прочие — после.
/// Стороне без управляемых чужие идут к краю окна: слева — перед, справа —
/// после.
fn build_side(chosen: &[String], current: &[Btn], left: bool) -> Vec<Btn> {
    let first_managed = current.iter().position(|b| !matches!(b, Btn::Other(_)));
    let (mut lead, mut trail) = (Vec::new(), Vec::new());
    for (i, b) in current.iter().enumerate() {
        if let Btn::Other(_) = b {
            let before = match first_managed {
                Some(f) => i < f,
                None => left,
            };
            if before {
                lead.push(b.clone());
            } else {
                trail.push(b.clone());
            }
        }
    }
    lead.extend(chosen.iter().filter_map(|id| Btn::from_id(id)));
    lead.extend(trail);
    lead
}

/// Раскладка по выбору; чужие кнопки остаются на своих сторонах.
pub fn build(choice: &Choice, current: &Layout) -> Layout {
    Layout { left: build_side(&choice.left, &current.left, true), right: build_side(&choice.right, &current.right, false) }
}

/// Какие кнопки окружение умеет.
///
/// * KDE — все кнопки модуля «Оформление окон» KWin 6.8 ([`KDE_SET`]);
/// * MATE (Marco) — `appmenu`, `stick`, `above`, `shade`, `spacer` сверх
///   общих (описание ключа в схеме `org.mate.marco` и функции кнопок Marco);
/// * XFCE (xfwm4) — `T` на всех столах и `S` свернуть в заголовок;
/// * Cinnamon (Muffin) — промежуток `spacer` (описание ключа в схеме);
/// * Openbox и labwc — меню окна, на всех столах, свернуть, развернуть,
///   закрыть и свернуть в заголовок (буквы `NDIMCS` / слова labwc-config(5));
/// * GNOME, Budgie, Pantheon и прочие — то, что рисует и GTK у окон со
///   своим заголовком: меню, свернуть, развернуть, закрыть (промежутка GTK
///   не знает, а в GNOME заголовки почти всех окон рисует GTK).
pub fn supported_buttons(de: &str) -> Vec<&'static str> {
    match de {
        "kde" => KDE_SET.to_vec(),
        "mate" => vec!["menu", "appmenu", "sticky", "minimize", "maximize", "close", "above", "shade", "spacer"],
        "xfce" => vec!["menu", "sticky", "minimize", "maximize", "close", "shade"],
        "cinnamon-mint" => vec!["menu", "minimize", "maximize", "close", "spacer"],
        "openbox" | "labwc" => vec!["menu", "sticky", "minimize", "maximize", "close", "shade"],
        "cosmic" => vec!["minimize", "maximize"],
        _ => vec!["menu", "minimize", "maximize", "close"],
    }
}

/// Чего из набора KDE у окружения нет — поимённо, с причиной. Пусто — всё
/// есть (KDE). Там, где кнопки не пишутся вовсе (подпись [`note`] уже это
/// говорит), — тоже пусто.
pub fn unavailable_note(de: &str) -> String {
    let supported = supported_buttons(de);
    let missing: Vec<String> = MANAGED
        .iter()
        .filter(|(id, _)| KDE_SET.contains(id) && !supported.contains(id))
        .map(|(_, label)| tr(label))
        .collect();
    if missing.is_empty() {
        return String::new();
    }
    let list = missing.join(", ");
    match de {
        "mate" => trf("Marco, the MATE window manager, has no such title bar buttons: %1.", &[&list]),
        "xfce" => trf("xfwm4, the Xfce window manager, has no such title bar buttons: %1.", &[&list]),
        "cinnamon-mint" => trf("Muffin, the Cinnamon window manager, has no such title bar buttons: %1.", &[&list]),
        "openbox" => trf("Openbox has no such title bar buttons: %1.", &[&list]),
        "labwc" => trf("labwc has no such title bar buttons: %1.", &[&list]),
        "cosmic" => String::new(),
        _ => trf("Title bars drawn by GTK have no such buttons: %1.", &[&list]),
    }
}

fn codec(de: &str) -> Codec {
    match de {
        "kde" => Codec::Kde,
        "xfce" => Codec::Xfwm,
        "openbox" => Codec::Openbox,
        "labwc" => Codec::Labwc,
        _ => Codec::Gnome,
    }
}

/// Кто пишет раскладку: окружение, а в LXQt — менеджер окон его сеанса
/// (`qt_settings::title_wm_for` по `lxqt/session.conf`): Openbox → `openbox`,
/// labwc → `labwc`, KWin → `kde`, xfwm4 → `xfce`, Marco → `mate` (ключи их
/// родных окружений). Незнакомый или не выбранный — `lxqt` как есть.
pub fn wm_key(io: &dyn LookIo, ctx: &LookCtx) -> String {
    use crate::qt_settings::TitleWm;
    if ctx.de != "lxqt" {
        return ctx.de.clone();
    }
    let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
    let wm = crate::qt_settings::session_title_wm("lxqt", &ctx.config_home, &dirs, ctx.wayland, &|p| io.read(p));
    match wm {
        Some(TitleWm::Openbox) => "openbox",
        Some(TitleWm::Labwc) => "labwc",
        Some(TitleWm::KWin) => "kde",
        Some(TitleWm::Xfwm4) => "xfce",
        Some(TitleWm::Marco) => "mate",
        Some(TitleWm::Sway) => "sway",
        None => "lxqt",
    }
    .to_string()
}

/// Контекст с окружением, заменённым на [`wm_key`].
fn wm_ctx(io: &dyn LookIo, ctx: &LookCtx) -> LookCtx {
    let mut c = ctx.clone();
    c.de = wm_key(io, ctx);
    c
}

/// Менеджер с раскладкой в `rc.xml` и путь узла раскладки в `<theme>`.
fn rc_layout_node(de: &str) -> Option<(crate::qt_settings::TitleWm, &'static [&'static str])> {
    use crate::qt_settings::TitleWm;
    match de {
        "openbox" => Some((TitleWm::Openbox, &["titleLayout"][..])),
        "labwc" => Some((TitleWm::Labwc, &["titlebar", "layout"][..])),
        _ => None,
    }
}

/// Свой `rc.xml` менеджера, его текст (свой либо системный образец).
fn rc_xml_of(io: &dyn LookIo, ctx: &LookCtx, wm: crate::qt_settings::TitleWm) -> (String, Option<String>) {
    let dirs = std::env::var("XDG_CONFIG_DIRS").unwrap_or_default();
    let (own, text, _) = crate::qt_settings::rc_xml_source(wm, &ctx.config_home, &dirs, &|p| io.read(p));
    (own, text)
}

/// Почему раскладку в `rc.xml` записать нельзя (`None` — можно): у Openbox
/// нет ни своего, ни системного файла, либо файл не разбирается.
pub fn rc_refusal(io: &dyn LookIo, ctx: &LookCtx) -> Option<String> {
    let (wm, node) = rc_layout_node(&ctx.de)?;
    let (own, text) = rc_xml_of(io, ctx, wm);
    if crate::qt_settings::rc_xml_with_theme_value(text.as_deref(), wm, node, "x").is_some() {
        return None;
    }
    let label = crate::animations::de_label(&ctx.de);
    Some(if text.is_none() {
        trf("%1 has no settings file to change: %2 doesn't exist, and there is no system one to start from.", &[&label, &own])
    } else {
        trf("The %1 settings file %2 can't be read, so Rexon leaves it as it is.", &[&label, &own])
    })
}

/// Раскладка GTK по умолчанию (`gtk-decoration-layout`).
const GTK_DEFAULT: &str = "menu:minimize,maximize,close";

/// Где окружение хранит раскладку: (схема, ключ) для gsettings.
fn gsettings_key(de: &str) -> (&'static str, &'static str) {
    match de {
        "cinnamon-mint" => ("org.cinnamon.desktop.wm.preferences", "button-layout"),
        "mate" => ("org.mate.Marco.general", "button-layout"),
        "budgie" => ("com.solus-project.budgie-wm", "button-layout"),
        _ => ("org.gnome.desktop.wm.preferences", "button-layout"),
    }
}

fn gtk_ini_layout(io: &dyn LookIo, ctx: &LookCtx) -> Option<String> {
    let path = crate::gtk_sanitizer::gtk_ini_path(&ctx.config_home, "3.0");
    crate::gtk_sanitizer::read_ini_settings(io.read(&path).as_deref()).get("gtk-decoration-layout").cloned()
}

fn cosmic_path(ctx: &LookCtx, key: &str) -> String {
    ctx.cfg(&format!("cosmic/com.system76.CosmicTk/v1/{key}"))
}

/// Раскладка как она стоит сейчас (родная запись).
pub fn read_layout(io: &dyn LookIo, ctx: &LookCtx) -> Layout {
    let de = ctx.de.as_str();
    match de {
        "kde" => parse(
            Codec::Kde,
            &kconfig_value(io, ctx, "kwinrc", "org.kde.kdecoration2", "ButtonsOnLeft").unwrap_or_else(|| "MSE".into()),
            &kconfig_value(io, ctx, "kwinrc", "org.kde.kdecoration2", "ButtonsOnRight").unwrap_or_else(|| "HIAX".into()),
        ),
        "xfce" => parse(Codec::Xfwm, &xfconf_get(io, "xfwm4", "/general/button_layout").unwrap_or_else(|| "O|SHMC".into()), ""),
        "cosmic" => {
            let on = |k: &str| io.read(&cosmic_path(ctx, k)).map(|t| t.trim() != "false").unwrap_or(true);
            let mut right = Vec::new();
            if on("show_minimize") {
                right.push(Btn::Minimize);
            }
            if on("show_maximize") {
                right.push(Btn::Maximize);
            }
            right.push(Btn::Close);
            Layout { left: Vec::new(), right }
        }
        "gnome" | "budgie" | "pantheon" | "cinnamon-mint" | "mate" => {
            let (schema, key) = gsettings_key(de);
            parse(Codec::Gnome, &gsettings_get(io, schema, key).unwrap_or_else(|| GTK_DEFAULT.into()), "")
        }
        "openbox" | "labwc" => {
            let (wm, node) = rc_layout_node(de).expect("у openbox и labwc раскладка в rc.xml");
            let raw = rc_xml_of(io, ctx, wm)
                .1
                .and_then(|t| crate::qt_settings::rc_xml_theme_value(&t, node))
                .filter(|v| !v.trim().is_empty());
            match raw {
                Some(v) => parse(codec(de), &v, ""),
                None => default_layout(de),
            }
        }
        _ => parse(Codec::Gnome, &gtk_ini_layout(io, ctx).unwrap_or_else(|| GTK_DEFAULT.into()), ""),
    }
}

/// Раскладка для GTK (`gtk-decoration-layout`): только то, что GTK знает.
pub fn gtk_layout(layout: &Layout) -> String {
    let side = |v: &[Btn]| {
        v.iter()
            .filter(|b| matches!(b, Btn::Menu | Btn::Minimize | Btn::Maximize | Btn::Close))
            .map(gnome_token)
            .collect::<Vec<_>>()
            .join(",")
    };
    format!("{}:{}", side(&layout.left), side(&layout.right))
}

/// План записи раскладки.
pub fn plan_layout(io: &dyn LookIo, ctx: &LookCtx, new: &Layout) -> Vec<LookStep> {
    let old = read_layout(io, ctx);
    let de = ctx.de.as_str();
    let mut steps = Vec::new();
    if &old == new {
        return steps;
    }
    match de {
        "kde" => {
            let (l, r) = format(Codec::Kde, new);
            let (ol, or) = format(Codec::Kde, &old);
            if l != ol {
                steps.push(kwrite("kwinrc", "org.kde.kdecoration2", "ButtonsOnLeft", Some(&l), false));
            }
            if r != or {
                steps.push(kwrite("kwinrc", "org.kde.kdecoration2", "ButtonsOnRight", Some(&r), false));
            }
            steps.push(kwin_reconfigure());
        }
        "xfce" => steps.push(xfconf_set("xfwm4", "/general/button_layout", "string", &format(Codec::Xfwm, new).0)),
        "cosmic" => {
            // У COSMIC есть только два флажка — сторону не сравниваем.
            let has = |l: &Layout, b: &Btn| l.right.contains(b) || l.left.contains(b);
            for (key, b) in [("show_minimize", Btn::Minimize), ("show_maximize", Btn::Maximize)] {
                if has(new, &b) != has(&old, &b) {
                    steps.push(LookStep::WriteFile { path: cosmic_path(ctx, key), content: has(new, &b).to_string() });
                }
            }
        }
        "openbox" | "labwc" => {
            let (wm, node) = rc_layout_node(de).expect("у openbox и labwc раскладка в rc.xml");
            let (own, text) = rc_xml_of(io, ctx, wm);
            let value = format(codec(de), new).0;
            // Файл не разбирается (или у Openbox его нет вовсе) — шагов нет,
            // отказ с причиной говорит [`rc_refusal`].
            let Some(content) = crate::qt_settings::rc_xml_with_theme_value(text.as_deref(), wm, node, &value) else {
                return Vec::new();
            };
            steps.push(LookStep::WriteFile { path: own, content });
            steps.push(LookStep::Notify(vec![crate::animations::argv(&[wm.program(), "--reconfigure"])]));
            // GTK-программы со своим заголовком — тот же общий слой.
            steps.push(LookStep::GtkIni { key: "gtk-decoration-layout".into(), value: gtk_layout(new) });
        }
        "gnome" | "budgie" | "pantheon" | "cinnamon-mint" | "mate" => {
            let (schema, key) = gsettings_key(de);
            let value = format!("'{}'", format(Codec::Gnome, new).0);
            steps.push(gsettings_set(schema, key, &value));
            if de == "budgie" {
                steps.push(gsettings_set("org.gnome.desktop.wm.preferences", "button-layout", &value));
            }
            if de == "mate" {
                steps.push(gsettings_set("org.mate.interface", "gtk-decoration-layout", &format!("'{}'", gtk_layout(new))));
            }
        }
        _ => {
            // Композитор без заголовков: кнопки рисуют только GTK-программы.
            // Ключ GNOME читает портал GTK — пишем, если есть gsettings
            // (неудача не беда: settings.ini уже записан).
            steps.push(LookStep::GtkIni { key: "gtk-decoration-layout".into(), value: gtk_layout(new) });
            if io.have("gsettings") {
                steps.push(LookStep::Notify(vec![crate::animations::argv(&[
                    "gsettings",
                    "set",
                    "org.gnome.desktop.wm.preferences",
                    "button-layout",
                    &format!("'{}'", gtk_layout(new)),
                ])]));
            }
        }
    }
    if de == "kde" {
        // GTK-программы с CSD — тот же общий слой, без kde-gtk-config.
        steps.push(LookStep::GtkIni { key: "gtk-decoration-layout".into(), value: gtk_layout(new) });
    }
    steps
}

// ═══════════════════════════════════════════════════════════════════════════
// Флажки заголовка KDE
// ═══════════════════════════════════════════════════════════════════════════

/// Флажки окна «Кнопки заголовка» модуля «Оформление окон» KDE: (id, ключ
/// `kwinrc [org.kde.kdecoration2]`, значение по умолчанию, подпись). Ключи
/// и умолчания — из `kwindecorationsettings.kcfg` KWin 6.8.
pub const FLAGS: &[(&str, &str, bool, &str)] = &[
    ("close_on_double_click", "CloseOnDoubleClickOnMenu", false, n_("Close windows by double-clicking the window menu button")),
    ("show_tooltips", "ShowToolTips", true, n_("Show title bar button tooltips")),
    (
        "always_show_exclude_capture",
        "AlwaysShowExcludeFromCapture",
        false,
        n_("Always show the “Hide from screenshots and screen recordings” button"),
    ),
];

const DECO_GROUP: &str = "org.kde.kdecoration2";

/// Флажок для раздела.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FlagOption {
    pub id: String,
    pub label: String,
    pub checked: bool,
    /// Можно менять (у «всегда показывать „скрыть от снимков“» — только
    /// когда сама кнопка стоит в заголовке, как у KDE).
    pub enabled: bool,
    /// Пояснение под флажком (пусто — нет).
    pub hint: String,
}

/// Логическое значение KConfig (`true`, `1`, `on`, `yes` — истина).
fn kconfig_bool(raw: Option<String>, default: bool) -> bool {
    match raw {
        Some(v) => matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "1" | "on" | "yes"),
        None => default,
    }
}

fn flag_value(io: &dyn LookIo, ctx: &LookCtx, key: &str, default: bool) -> bool {
    kconfig_bool(kconfig_value(io, ctx, "kwinrc", DECO_GROUP, key), default)
}

fn has_button(layout: &Layout, b: &Btn) -> bool {
    layout.left.contains(b) || layout.right.contains(b)
}

/// Флажки окружения: у KDE — три флажка KWin, у остальных их нет
/// ([`flags_note`]).
pub fn flags(io: &dyn LookIo, ctx: &LookCtx, layout: &Layout) -> Vec<FlagOption> {
    if ctx.de != "kde" {
        return Vec::new();
    }
    FLAGS
        .iter()
        .map(|(id, key, default, label)| {
            let checked = flag_value(io, ctx, key, *default);
            let (enabled, hint) = match *id {
                "close_on_double_click" if checked => (true, tr("Click and hold the window menu button to show the menu.")),
                "always_show_exclude_capture" => {
                    let present = has_button(layout, &Btn::ExcludeCapture);
                    let base = tr("When off, the button appears only while the window is hidden from screenshots and screen recordings.");
                    if present {
                        (true, base)
                    } else {
                        (false, format!("{base} {}", tr("To turn this on, add the “Hide from screenshots and screen recordings” button to the title bar first.")))
                    }
                }
                _ => (true, String::new()),
            };
            FlagOption { id: id.to_string(), label: tr(label), checked, enabled, hint }
        })
        .collect()
}

/// Почему флажков нет: это настройки KWin, у других оконных менеджеров
/// таких нет (Marco, xfwm4, Mutter, Muffin — ни в схемах gsettings, ни в
/// свойствах xfconf).
pub fn flags_note(de: &str) -> String {
    match de {
        "kde" => String::new(),
        other => trf(
            "%1 has no settings for closing windows by double-clicking the menu button, for title bar button tooltips or for the “Hide from screenshots” button: these are options of KWin, the KDE window manager.",
            &[&crate::animations::de_label(other)],
        ),
    }
}

/// План флажка. Значение по умолчанию не пишется, а снимается — как у
/// модуля KDE, чтобы `kwinrc` не обрастал лишними строками.
pub fn plan_flag(io: &dyn LookIo, ctx: &LookCtx, id: &str, on: bool) -> Vec<LookStep> {
    if ctx.de != "kde" {
        return Vec::new();
    }
    let Some((_, key, default, _)) = FLAGS.iter().find(|(fid, ..)| *fid == id) else {
        return Vec::new();
    };
    if id == "always_show_exclude_capture" && !has_button(&read_layout(io, ctx), &Btn::ExcludeCapture) {
        return Vec::new();
    }
    if flag_value(io, ctx, key, *default) == on {
        return Vec::new();
    }
    let value = (on != *default).then_some(if on { "true" } else { "false" });
    vec![kwrite("kwinrc", DECO_GROUP, key, value, true), kwin_reconfigure()]
}

/// Возврат флажков к умолчанию: снять записанные ключи.
fn reset_flags_steps(io: &dyn LookIo, ctx: &LookCtx) -> Vec<LookStep> {
    if ctx.de != "kde" {
        return Vec::new();
    }
    let mut steps: Vec<LookStep> = FLAGS
        .iter()
        .filter(|(_, key, ..)| kconfig_value(io, ctx, "kwinrc", DECO_GROUP, key).is_some())
        .map(|(_, key, ..)| kwrite("kwinrc", DECO_GROUP, key, None, false))
        .collect();
    if !steps.is_empty() {
        steps.push(kwin_reconfigure());
    }
    steps
}

pub fn apply_flag(io: &dyn LookIo, ctx: &LookCtx, id: &str, on: bool) -> String {
    let ctx = &wm_ctx(io, ctx);
    finish(io, ctx, plan_flag(io, ctx, id, on))
}

// ═══════════════════════════════════════════════════════════════════════════
// Рамка
// ═══════════════════════════════════════════════════════════════════════════

/// Размеры рамки KDE (имена KWin) и подписи как в KDE.
const KDE_BORDERS: &[(&str, &str)] = &[
    ("auto", n_("Theme's default")),
    ("None", n_("No borders")),
    ("NoSides", n_("No side borders")),
    ("Tiny", n_("Tiny")),
    ("Normal", n_("Normal")),
    ("Large", n_("Large")),
    ("VeryLarge", n_("Very large")),
    ("Huge", n_("Huge")),
    ("VeryHuge", n_("Very huge")),
    ("Oversized", n_("Oversized")),
];

/// Толщина в точках у тайловых композиторов.
const PX_BORDERS: &[u32] = &[0, 1, 2, 3, 4, 5, 6, 8, 10];

pub const BORDER_TAG: &str = "window borders";
pub const BORDER_STEM: &str = "rexon-borders";

pub fn border_supported(de: &str) -> bool {
    matches!(de, "kde" | "hyprland" | "niri" | "sway" | "labwc")
}

/// Файл поправок темы labwc (labwc-theme(5): `themerc-override` в каталоге
/// настроек labwc перекрывает ключи темы).
pub fn labwc_override_path(ctx: &LookCtx) -> String {
    ctx.cfg("labwc/themerc-override")
}

/// Ключ толщины рамки labwc.
pub const LABWC_BORDER_KEY: &str = "border.width";

/// Значение ключа `key: value` в тексте `themerc` (пусто — ключа нет).
pub fn themerc_value(text: &str, key: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let (k, v) = line.trim().split_once(':')?;
        (k.trim() == key && !line.trim_start().starts_with('#')).then(|| v.trim().to_string())
    })
}

/// Текст `themerc` с ключом `key`, заменённым на `value` (`None` — убрать
/// ключ). Остальные строки человека — как были.
pub fn themerc_with(text: &str, key: &str, value: Option<&str>) -> String {
    let mut out: Vec<String> = text
        .lines()
        .filter(|line| {
            let t = line.trim();
            t.starts_with('#') || t.split_once(':').map(|(k, _)| k.trim() != key).unwrap_or(true)
        })
        .map(str::to_string)
        .collect();
    if let Some(v) = value {
        out.push(format!("{key}: {v}"));
    }
    let mut joined = out.join("\n");
    if !joined.is_empty() {
        joined.push('\n');
    }
    joined
}

/// Стиль рамки sway из конфига человека: последние `default_border` и
/// `default_floating_border` (sway(5): `normal|none|pixel [<n>]`; по
/// умолчанию `normal`). Толщину человек выбирает, значит рамка нужна: `none`
/// становится `pixel` — без заголовка, как и было.
pub fn sway_border_styles(config: Option<&str>) -> (String, String) {
    let mut tiled = "normal".to_string();
    let mut floating = "normal".to_string();
    for line in config.unwrap_or("").lines() {
        let mut words = line.split_whitespace();
        let (Some(cmd), Some(style)) = (words.next(), words.next()) else {
            continue;
        };
        let style = match style {
            "normal" => "normal",
            "pixel" | "none" => "pixel",
            _ => continue,
        };
        match cmd {
            "default_border" => tiled = style.to_string(),
            "default_floating_border" => floating = style.to_string(),
            _ => {}
        }
    }
    (tiled, floating)
}

/// Рамка sway из конфига человека как она есть — для возврата «как в
/// конфиге» командой: стиль и толщина последних `default_border` и
/// `default_floating_border`. Без них — умолчание sway(5) `normal 2`;
/// `none` толщины не несёт.
pub fn sway_border_defaults(config: Option<&str>) -> (String, String) {
    let mut tiled = "normal 2".to_string();
    let mut floating = "normal 2".to_string();
    for line in config.unwrap_or("").lines() {
        let mut words = line.split_whitespace();
        let (Some(cmd), Some(style)) = (words.next(), words.next()) else {
            continue;
        };
        let value = match (style, words.next()) {
            ("none", _) => "none".to_string(),
            ("normal" | "pixel", Some(n)) if n.parse::<u32>().is_ok() => format!("{style} {n}"),
            ("normal" | "pixel", _) => format!("{style} 2"),
            _ => continue,
        };
        match cmd {
            "default_border" => tiled = value,
            "default_floating_border" => floating = value,
            _ => {}
        }
    }
    (tiled, floating)
}

/// Варианты рамки: (id, подпись).
pub fn border_options(de: &str) -> Vec<(String, String)> {
    match de {
        "kde" => KDE_BORDERS.iter().map(|(id, l)| (id.to_string(), tr(l))).collect(),
        "hyprland" | "niri" | "sway" | "labwc" => {
            let mut v = vec![("default".to_string(), tr("As in the config"))];
            v.extend(PX_BORDERS.iter().map(|p| (p.to_string(), trf("%1 px", &[p]))));
            v
        }
        _ => Vec::new(),
    }
}

pub fn read_border(io: &dyn LookIo, ctx: &LookCtx) -> String {
    match ctx.de.as_str() {
        "kde" => {
            let auto = kconfig_value(io, ctx, "kwinrc", "org.kde.kdecoration2", "BorderSizeAuto").map(|v| v != "false").unwrap_or(true);
            if auto {
                "auto".into()
            } else {
                kconfig_value(io, ctx, "kwinrc", "org.kde.kdecoration2", "BorderSize").unwrap_or_else(|| "Normal".into())
            }
        }
        "hyprland" | "niri" | "sway" => read_own_wm_file(io, ctx, &ctx.de, BORDER_STEM)
            .map(|(_, t)| parse_state_line(&t))
            .and_then(|m| m.get("border").cloned())
            .unwrap_or_else(|| "default".into()),
        "labwc" => io
            .read(&labwc_override_path(ctx))
            .and_then(|t| themerc_value(&t, LABWC_BORDER_KEY))
            .unwrap_or_else(|| "default".into()),
        _ => String::new(),
    }
}

/// Наш файл рамки у композитора.
pub fn border_content(de: &str, syntax: WmSyntax, value: &str) -> String {
    border_content_styled(de, syntax, value, &("normal".to_string(), "normal".to_string()))
}

/// То же с готовыми стилями рамки sway ([`sway_border_styles`]).
pub fn border_content_styled(de: &str, syntax: WmSyntax, value: &str, sway: &(String, String)) -> String {
    let c = syntax.comment();
    let mut st = BTreeMap::new();
    st.insert("border".to_string(), value.to_string());
    let mut out = state_line(c, &st);
    out.push_str(&format!("{c} Rexon: window borders. This file is rewritten by Rexon; change borders in Rexon instead.\n"));
    if value == "default" {
        return out;
    }
    out.push_str(&match (de, syntax) {
        (_, WmSyntax::HyprLua) => format!("hl.config({{ general = {{ border_size = {value} }} }})\n"),
        (_, WmSyntax::HyprConf) => format!("general {{\n    border_size = {value}\n}}\n"),
        (_, WmSyntax::Sway) => {
            format!("default_border {} {value}\ndefault_floating_border {} {value}\n", sway.0, sway.1)
        }
        _ => format!("layout {{\n    focus-ring {{\n        width {value}\n    }}\n    border {{\n        width {value}\n    }}\n}}\n"),
    });
    out
}

pub fn plan_border(io: &dyn LookIo, ctx: &LookCtx, value: &str) -> Vec<LookStep> {
    let mut steps = Vec::new();
    if read_border(io, ctx) == value || !border_options(&ctx.de).iter().any(|(id, _)| id == value) {
        return steps;
    }
    match ctx.de.as_str() {
        "kde" => {
            if value == "auto" {
                steps.push(kwrite("kwinrc", "org.kde.kdecoration2", "BorderSizeAuto", None, true));
            } else {
                steps.push(kwrite("kwinrc", "org.kde.kdecoration2", "BorderSizeAuto", Some("false"), true));
                steps.push(kwrite("kwinrc", "org.kde.kdecoration2", "BorderSize", Some(value), false));
            }
            steps.push(kwin_reconfigure());
        }
        "hyprland" | "niri" => {
            let de = ctx.de.clone();
            let v = value.to_string();
            steps.push(LookStep::Wm(wm_include(ctx, &ctx.de, BORDER_TAG, BORDER_STEM, &move |s| border_content(&de, s, &v))));
        }
        "sway" => {
            // Стиль (с заголовком или без) — тот, что у человека в конфиге.
            let config = crate::animations::wm_targets(ctx, "sway", BORDER_STEM, &|_| String::new())
                .into_iter()
                .find_map(|t| io.read(&t.config));
            let styles = sway_border_styles(config.as_deref());
            let v = value.to_string();
            let inc_styles = styles.clone();
            steps.push(LookStep::Wm(wm_include(ctx, "sway", BORDER_TAG, BORDER_STEM, &move |s| {
                border_content_styled("sway", s, &v, &inc_styles)
            })));
            // sway сам конфиг не перечитывает, а `default_border` действует
            // только на новые окна (sway(5)). Работающему sway то же самое
            // говорим командами: новым окнам — `default_border`, открытым —
            // `border`. Не `swaymsg reload`: он перезапускает swaybg самого
            // sway, и фон из его конфига ложится ПОВЕРХ обоев Rexon (сверено
            // на sway 1.12 без экрана). Сеанса нет — молча мимо.
            let notify = |cmd: String| LookStep::Notify(vec![crate::animations::argv(&["swaymsg", &cmd])]);
            if value == "default" {
                let (tiled, floating) = sway_border_defaults(config.as_deref());
                steps.push(notify(format!("default_border {tiled}")));
                steps.push(notify(format!("default_floating_border {floating}")));
            } else {
                steps.push(notify(format!("default_border {} {value}", styles.0)));
                steps.push(notify(format!("default_floating_border {} {value}", styles.1)));
                for (criteria, style) in [("[tiling]", &styles.0), ("[floating]", &styles.1)] {
                    steps.push(notify(format!("{criteria} border {style} {value}")));
                }
            }
        }
        "labwc" => {
            let path = labwc_override_path(ctx);
            let before = io.read(&path).unwrap_or_default();
            let after = themerc_with(&before, LABWC_BORDER_KEY, (value != "default").then_some(value));
            steps.push(LookStep::WriteFile { path, content: after });
            // labwc(1): `--reconfigure` перечитывает конфиг и тему.
            steps.push(LookStep::Notify(vec![crate::animations::argv(&["labwc", "--reconfigure"])]));
        }
        _ => {}
    }
    steps
}

// ═══════════════════════════════════════════════════════════════════════════
// Снимок и применение
// ═══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ButtonOption {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ButtonsState {
    pub de: String,
    /// Сторона выбирается (нет у COSMIC).
    pub has_side: bool,
    /// Где кнопки управления: `right` (как в Windows) | `left` (как в macOS).
    pub side: String,
    pub buttons: Vec<ButtonOption>,
    /// Выбранные кнопки на левой и на правой стороне (по порядку).
    pub left: Vec<String>,
    pub right: Vec<String>,
    /// Все выбранные — для готовых раскладок «как в Windows / macOS».
    pub selected: Vec<String>,
    /// Сейчас стоит раскладка, которую выбор не передаёт дословно.
    pub custom: String,
    pub note: String,
    /// Каких кнопок KDE у окружения нет — поимённо, с причиной.
    pub unavailable: String,
    /// Флажки заголовка (только KDE) и почему их нет у остальных.
    pub flags: Vec<FlagOption>,
    pub flags_note: String,
    pub has_border: bool,
    pub border: String,
    pub border_options: Vec<ButtonOption>,
    pub border_note: String,
    /// Редактор перетаскиванием (макет заголовка + полка) — у окружений со
    /// стороной; у COSMIC только галочки.
    pub drag: bool,
    /// Полка: все кнопки модуля KDE (и сверх них умелые окружения), по
    /// порядку списка KDE.
    pub shelf: Vec<ShelfButton>,
}

/// Кнопка полки редактора перетаскиванием.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ShelfButton {
    pub id: String,
    pub label: String,
    pub glyph: String,
    /// Уже стоит в заголовке (промежуток — никогда: их сколько угодно).
    pub placed: bool,
    /// Окружение такую кнопку умеет; нет — `reason` говорит почему.
    pub available: bool,
    pub reason: String,
}

fn shelf_of(de: &str, choice: &Choice) -> Vec<ShelfButton> {
    let supported = supported_buttons(de);
    MANAGED
        .iter()
        .filter(|(id, _)| KDE_SET.contains(id) || supported.contains(id))
        .map(|(id, label)| {
            let available = supported.contains(id);
            ShelfButton {
                id: id.to_string(),
                label: tr(label),
                glyph: glyph(id).to_string(),
                placed: *id != "spacer" && choice.all().iter().any(|b| b == id),
                available,
                reason: if available { String::new() } else { missing_reason(de) },
            }
        })
        .collect()
}

fn current_text(io: &dyn LookIo, ctx: &LookCtx, layout: &Layout) -> String {
    let _ = io;
    match ctx.de.as_str() {
        "kde" => {
            let (l, r) = format(Codec::Kde, layout);
            format!("{l} | {r}")
        }
        "cosmic" => String::new(),
        "openbox" | "labwc" => format(codec(&ctx.de), layout).0,
        de if !uses_gtk_ini(de) || matches!(de, "gnome" | "budgie" | "pantheon") => format(codec(de), layout).0,
        _ => format(Codec::Gnome, layout).0,
    }
}

pub fn note(de: &str) -> String {
    match de {
        "kde" | "gnome" | "budgie" | "pantheon" | "cinnamon-mint" | "mate" | "xfce" | "openbox" | "labwc" => String::new(),
        "sway" => tr("sway's title bars have no buttons: windows are closed and moved with keyboard shortcuts. The layout applies to applications that draw their title bar themselves (GTK)."),
        // LXQt с менеджером, которого Rexon не знает ([`wm_key`]): с
        // Openbox, labwc, KWin, xfwm4 и Marco сюда не попадаем.
        "lxqt" => tr("In LXQt, title bars are drawn by the window manager chosen in Session Settings. Rexon changes their buttons under Openbox, labwc, KWin, xfwm4 and Marco, and this session uses another one (or none is chosen): the layout applies only to applications that draw their title bar themselves (GTK)."),
        "cosmic" => tr("COSMIC keeps the Close button on the right; only Minimize and Maximize can be turned on or off."),
        // Своей раскладки кнопок у Treeland нет: рамки Deepin и окна
        // программ DTK рисуют кнопки по-своему, раскладка доходит только до
        // GTK-программ с собственным заголовком (общая ветка GTK).
        "deepin" => tr("Deepin's window frames and its own apps keep their buttons as they are: the layout applies only to applications that draw their title bar themselves (GTK)."),
        "hyprland" | "niri" | "river" => trf(
            "%1 draws no title bars of its own: the layout applies to applications that draw their title bar themselves (GTK).",
            &[&crate::animations::de_label(de)],
        ),
        // Незнакомый композитор: про его заголовки ничего не утверждаем.
        other => trf(
            "Rexon can't change the title bar buttons of %1: the layout applies only to applications that draw their title bar themselves (GTK).",
            &[&crate::animations::de_label(other)],
        ),
    }
}

pub fn border_note(de: &str) -> String {
    match de {
        "kde" | "hyprland" => String::new(),
        "niri" => tr("Sets the width of the focus ring and of the window border (if the border is turned on in the config)."),
        "sway" => tr("Only the width changes: whether windows have a title bar stays as set in the sway config."),
        "labwc" => String::new(),
        "budgie" | "mate" | "xfce" | "openbox" => {
            trf("In %1 the window border is drawn by the window theme; choose another window theme to change it.", &[&crate::animations::de_label(de)])
        }
        // Mutter и Muffin рисуют заголовки и рамки по CSS темы GTK.
        "gnome" | "pantheon" | "cinnamon-mint" => trf(
            "In %1 window borders and title bars are drawn by the application theme (GTK); choose another GTK theme to change them.",
            &[&crate::animations::de_label(de)],
        ),
        "lxqt" => tr("In LXQt the window border is drawn by the window manager's theme (Openbox, KWin or labwc)."),
        other => trf("%1 has no window border setting.", &[&crate::animations::de_label(other)]),
    }
}

pub fn read_state(io: &dyn LookIo, ctx: &LookCtx) -> ButtonsState {
    let ctx = &wm_ctx(io, ctx);
    let de = ctx.de.as_str();
    let layout = read_layout(io, ctx);
    let choice = choice_of(&layout);
    let rebuilt = build(&choice, &layout);
    let custom = if rebuilt != layout { current_text(io, ctx, &layout) } else { String::new() };
    let supported = supported_buttons(de);
    let known = |v: &[String]| v.iter().filter(|b| supported.contains(&b.as_str())).cloned().collect::<Vec<_>>();
    ButtonsState {
        de: ctx.de.clone(),
        has_side: de != "cosmic",
        side: side_of(&layout).to_string(),
        buttons: MANAGED
            .iter()
            .filter(|(id, _)| supported.contains(id))
            .map(|(id, l)| ButtonOption { id: id.to_string(), label: tr(l) })
            .collect(),
        left: known(&choice.left),
        right: known(&choice.right),
        selected: known(&choice.all()),
        custom,
        note: note(de),
        unavailable: unavailable_note(de),
        flags: flags(io, ctx, &layout),
        flags_note: flags_note(de),
        has_border: border_supported(de),
        border: read_border(io, ctx),
        border_options: border_options(de).into_iter().map(|(id, label)| ButtonOption { id, label }).collect(),
        border_note: border_note(de),
        drag: de != "cosmic",
        shelf: shelf_of(de, &Choice { left: known(&choice.left), right: known(&choice.right) }),
    }
}

/// Применить выбор кнопок по сторонам.
pub fn apply_choice(io: &dyn LookIo, ctx: &LookCtx, choice: &Choice) -> String {
    let ctx = &wm_ctx(io, ctx);
    if let Some(why) = rc_refusal(io, ctx) {
        return why;
    }
    let current = read_layout(io, ctx);
    let mut choice = choice.clone();
    // Неумелые кнопки этого окружения не трогаем: как стояли, так стоят —
    // на своей стороне.
    // Стоявшие до первой умелой встают перед выбранными, прочие — после
    // (у GNOME `appmenu:close` → «как в macOS» даёт `appmenu,close,…:`).
    let supported = supported_buttons(&ctx.de);
    let cur = choice_of(&current);
    for (side, ids) in [("left", &cur.left), ("right", &cur.right)] {
        let first_known = ids.iter().position(|b| supported.contains(&b.as_str()));
        let mut lead = Vec::new();
        for (i, b) in ids.iter().enumerate() {
            if !supported.contains(&b.as_str()) && !choice.all().contains(b) {
                let before = match first_known {
                    Some(f) => i < f,
                    None => side == "left",
                };
                if before {
                    lead.push(b.clone());
                } else {
                    choice.side_mut(side).push(b.clone());
                }
            }
        }
        let list = choice.side_mut(side);
        lead.append(list);
        *list = lead;
    }
    let new = build(&choice, &current);
    finish(io, ctx, plan_layout(io, ctx, &new))
}

/// Готовая раскладка «как в Windows» (`right`) или «как в macOS» (`left`)
/// с набором `buttons`.
pub fn apply_preset(io: &dyn LookIo, ctx: &LookCtx, side: &str, buttons: &[String]) -> String {
    apply_choice(io, ctx, &preset(side, buttons))
}

/// Поставить или снять одну кнопку на стороне `side`; остальное как стоит.
pub fn apply_toggle(io: &dyn LookIo, ctx: &LookCtx, side: &str, id: &str, on: bool) -> String {
    let ctx = &wm_ctx(io, ctx);
    let current = choice_of(&read_layout(io, ctx));
    apply_choice(io, ctx, &toggle(&current, side, id, on))
}

/// Выбор, как его видит раздел: только умелые кнопки окружения (позиции
/// редактора перетаскиванием считаются по нему; прочее [`apply_choice`]
/// оставляет на месте само).
fn shown_choice(io: &dyn LookIo, ctx: &LookCtx) -> Choice {
    let supported = supported_buttons(&ctx.de);
    let c = choice_of(&read_layout(io, ctx));
    let known = |v: &[String]| v.iter().filter(|b| supported.contains(&b.as_str())).cloned().collect();
    Choice { left: known(&c.left), right: known(&c.right) }
}

/// Перетаскивание ([`move_button`]); кнопку, которой у окружения нет,
/// ставить отказывается с причиной.
pub fn apply_move(
    io: &dyn LookIo,
    ctx: &LookCtx,
    id: &str,
    from: &str,
    from_index: usize,
    to: &str,
    to_index: usize,
) -> String {
    let ctx = &wm_ctx(io, ctx);
    if is_side(to) && !supported_buttons(&ctx.de).contains(&id) {
        return missing_reason(&ctx.de);
    }
    let current = shown_choice(io, ctx);
    apply_choice(io, ctx, &move_button(&current, id, from, from_index, to, to_index))
}

/// Шаг кнопкой «влево / вправо / на полку» ([`nudge`]): сообщение и где
/// кнопка теперь (чтобы выделение шло за ней).
pub fn apply_nudge(io: &dyn LookIo, ctx: &LookCtx, side: &str, index: usize, dir: &str) -> (String, Option<(String, usize)>) {
    let ctx = &wm_ctx(io, ctx);
    let (next, at) = nudge(&shown_choice(io, ctx), side, index, dir);
    (apply_choice(io, ctx, &next), at)
}

/// «Добавить слева / справа» с полки ([`add_button`]).
pub fn apply_add(io: &dyn LookIo, ctx: &LookCtx, id: &str, side: &str) -> String {
    let ctx = &wm_ctx(io, ctx);
    if !supported_buttons(&ctx.de).contains(&id) {
        return missing_reason(&ctx.de);
    }
    apply_choice(io, ctx, &add_button(&shown_choice(io, ctx), id, side))
}

pub fn apply_border(io: &dyn LookIo, ctx: &LookCtx, value: &str) -> String {
    let ctx = &wm_ctx(io, ctx);
    finish(io, ctx, plan_border(io, ctx, value))
}

/// Вернуть раскладку по умолчанию окружения.
pub fn default_layout(de: &str) -> Layout {
    match de {
        "kde" => parse(Codec::Kde, "MSE", "HIAX"),
        "xfce" => parse(Codec::Xfwm, "O|SHMC", ""),
        "gnome" | "pantheon" => parse(Codec::Gnome, "appmenu:close", ""),
        "cinnamon-mint" => parse(Codec::Gnome, ":minimize,maximize,close", ""),
        "mate" => parse(Codec::Gnome, "menu:minimize,maximize,close", ""),
        "budgie" => parse(Codec::Gnome, "appmenu:minimize,maximize,close", ""),
        "cosmic" => Layout { left: Vec::new(), right: vec![Btn::Minimize, Btn::Maximize, Btn::Close] },
        // Умолчания самих менеджеров: Openbox — `NLIMC` (его системный
        // rc.xml), labwc — `icon:iconify,max,close` (labwc-config(5)).
        "openbox" => parse(Codec::Openbox, "NLIMC", ""),
        "labwc" => parse(Codec::Labwc, "icon:iconify,max,close", ""),
        _ => parse(Codec::Gnome, GTK_DEFAULT, ""),
    }
}

/// Возврат раскладки к умолчанию: у gsettings и xfconf — сбросом ключа
/// (умолчание схемы с поправками дистрибутива, а не наша догадка о нём), у
/// KDE — удалением ключей, у остальных — записью раскладки по умолчанию.
pub fn reset_layout_steps(io: &dyn LookIo, ctx: &LookCtx) -> Vec<LookStep> {
    let de = ctx.de.as_str();
    let reset = |schema: &str, key: &str| LookStep::Cmd(crate::animations::argv(&["gsettings", "reset", schema, key]));
    let default_gtk = gtk_layout(&default_layout(de));
    match de {
        "kde" => {
            let mut steps = Vec::new();
            for key in ["ButtonsOnLeft", "ButtonsOnRight"] {
                if kconfig_value(io, ctx, "kwinrc", "org.kde.kdecoration2", key).is_some() {
                    steps.push(kwrite("kwinrc", "org.kde.kdecoration2", key, None, false));
                }
            }
            if steps.is_empty() {
                return steps;
            }
            steps.push(kwin_reconfigure());
            steps.push(LookStep::GtkIni { key: "gtk-decoration-layout".into(), value: default_gtk });
            steps
        }
        "gnome" | "budgie" | "pantheon" | "cinnamon-mint" | "mate" => {
            let (schema, key) = gsettings_key(de);
            let mut steps = vec![reset(schema, key)];
            if de == "budgie" {
                steps.push(reset("org.gnome.desktop.wm.preferences", "button-layout"));
            }
            if de == "mate" {
                steps.push(reset("org.mate.interface", "gtk-decoration-layout"));
            }
            steps
        }
        "xfce" => {
            if xfconf_get(io, "xfwm4", "/general/button_layout").is_none() {
                return Vec::new();
            }
            vec![LookStep::Cmd(crate::animations::argv(&["xfconf-query", "-c", "xfwm4", "-p", "/general/button_layout", "-r"]))]
        }
        _ => plan_layout(io, ctx, &default_layout(de)),
    }
}

pub fn apply_defaults(io: &dyn LookIo, ctx: &LookCtx) -> String {
    let ctx = &wm_ctx(io, ctx);
    if let Some(why) = rc_refusal(io, ctx) {
        return why;
    }
    let mut steps = reset_layout_steps(io, ctx);
    steps.extend(reset_flags_steps(io, ctx));
    let border = if ctx.de == "kde" { "auto" } else { "default" };
    if border_supported(&ctx.de) {
        steps.extend(plan_border(io, ctx, border));
    }
    finish(io, ctx, steps)
}

fn finish(io: &dyn LookIo, ctx: &LookCtx, steps: Vec<LookStep>) -> String {
    if steps.is_empty() {
        return String::new();
    }
    match run_steps(io, ctx, &steps) {
        Ok(()) => tr("Title bar settings applied."),
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn defaults_roundtrip_unchanged() {
        for (codec, l, r) in [
            (Codec::Kde, "MSE", "HIAX"),
            (Codec::Kde, "", "IAX"),
            (Codec::Kde, "XA", "MI"),
            (Codec::Gnome, "appmenu:close", ""),
            (Codec::Gnome, ":minimize,maximize,close", ""),
            (Codec::Gnome, "menu:minimize,maximize,close", ""),
            (Codec::Gnome, "close,minimize,maximize:", ""),
            (Codec::Gnome, "close:maximize", ""),
            (Codec::Gnome, "close,minimize:maximize,menu", ""),
            (Codec::Xfwm, "O|SHMC", ""),
            (Codec::Xfwm, "CH|MO", ""),
        ] {
            let layout = parse(codec, l, r);
            let rebuilt = build(&choice_of(&layout), &layout);
            assert_eq!(rebuilt, layout, "{l} {r}");
            assert_eq!(format(codec, &rebuilt), if codec == Codec::Kde { (l.into(), r.into()) } else { (l.into(), String::new()) });
        }
    }

    /// Pantheon `close:maximize`: обе стороны читаются как стоят, и меню,
    /// поставленное слева, остаётся слева (раньше уезжало направо).
    #[test]
    fn two_sided_layout_is_kept() {
        let p = parse(Codec::Gnome, "close:maximize", "");
        assert_eq!(choice_of(&p), Choice { left: ids(&["close"]), right: ids(&["maximize"]) });
        let wanted = Choice { left: ids(&["close", "minimize", "maximize", "menu"]), right: vec![] };
        assert_eq!(format(Codec::Gnome, &build(&wanted, &p)).0, "close,minimize,maximize,menu:");
        // Кнопки на обеих сторонах в любом сочетании.
        let both = Choice { left: ids(&["close", "minimize"]), right: ids(&["maximize", "menu"]) };
        assert_eq!(format(Codec::Gnome, &build(&both, &p)).0, "close,minimize:maximize,menu");
        assert_eq!(gtk_layout(&build(&both, &p)), "close,minimize:maximize,menu");
    }

    #[test]
    fn two_sided_layout_in_every_codec() {
        let both = Choice { left: ids(&["close", "minimize"]), right: ids(&["maximize", "menu"]) };
        // KDE: ButtonsOnLeft / ButtonsOnRight; S, E и H теперь управляемые —
        // не выбраны, значит сняты.
        let kde = build(&both, &parse(Codec::Kde, "MSE", "HIAX"));
        assert_eq!(format(Codec::Kde, &kde), ("XI".into(), "AM".into()));
        // GNOME / Cinnamon / MATE / Budgie / Pantheon: «a,b:c,d».
        let g = build(&both, &parse(Codec::Gnome, ":minimize,maximize,close", ""));
        assert_eq!(format(Codec::Gnome, &g).0, "close,minimize:maximize,menu");
        let mate = build(&Choice { left: ids(&["close", "sticky"]), right: ids(&["above"]) }, &parse(Codec::Gnome, "menu:minimize,maximize,close", ""));
        assert_eq!(format(Codec::Gnome, &mate).0, "close,stick:above");
        // XFCE: «ab|cd»; S («свернуть в заголовок») управляемая — не выбрана.
        let x = build(&both, &parse(Codec::Xfwm, "O|SHMC", ""));
        assert_eq!(format(Codec::Xfwm, &x).0, "CH|MO");
    }

    #[test]
    fn presets_and_toggle() {
        let all = ids(&["menu", "minimize", "maximize", "close"]);
        assert_eq!(preset("right", &all), Choice { left: ids(&["menu"]), right: ids(&["minimize", "maximize", "close"]) });
        assert_eq!(preset("left", &all), Choice { left: ids(&["close", "minimize", "maximize"]), right: ids(&["menu"]) });
        // Поставить меню слева у Pantheon: справа не появляется.
        let c = Choice { left: ids(&["close"]), right: ids(&["maximize"]) };
        let c = toggle(&c, "left", "menu", true);
        assert_eq!(c, Choice { left: ids(&["close", "menu"]), right: ids(&["maximize"]) });
        // «Свернуть» слева встаёт между «закрыть» и меню, по порядку.
        let c = toggle(&c, "left", "minimize", true);
        assert_eq!(c.left, ids(&["close", "minimize", "menu"]));
        // Перенос на другую сторону: слева уходит, справа встаёт у края.
        let c = toggle(&c, "right", "close", true);
        assert_eq!(c, Choice { left: ids(&["minimize", "menu"]), right: ids(&["maximize", "close"]) });
        // Снятие.
        let c = toggle(&c, "right", "maximize", false);
        assert_eq!(c.right, ids(&["close"]));
    }

    /// Машина с одним ключом gsettings — раскладкой кнопок.
    struct GsIo(std::cell::RefCell<String>);

    impl LookIo for GsIo {
        fn run(&self, a: &[String]) -> (bool, String) {
            match (a[0].as_str(), a[1].as_str()) {
                ("gsettings", "get") => (true, format!("'{}'\n", self.0.borrow())),
                ("gsettings", "set") => {
                    *self.0.borrow_mut() = a[4].trim_matches('\'').to_string();
                    (true, String::new())
                }
                _ => (true, String::new()),
            }
        }
        fn have(&self, _: &str) -> bool {
            true
        }
        fn read(&self, _: &str) -> Option<String> {
            None
        }
        fn write(&self, _: &str, _: &str) -> bool {
            true
        }
        fn list_dir(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }

    /// Живой путь Pantheon: снимок показывает обе стороны, а галочка «меню
    /// слева» пишет `close,menu:maximize`, не трогая правую сторону.
    #[test]
    fn pantheon_state_and_toggle_keep_both_sides() {
        let io = GsIo(std::cell::RefCell::new("close:maximize".into()));
        let ctx = LookCtx::new("pantheon", "/home/t", None);
        let st = read_state(&io, &ctx);
        assert_eq!((st.left.clone(), st.right.clone()), (ids(&["close"]), ids(&["maximize"])));
        assert_eq!(st.side, "left");
        assert_eq!(st.custom, "", "раскладка передаётся выбором дословно");
        apply_toggle(&io, &ctx, "left", "menu", true);
        assert_eq!(*io.0.borrow(), "close,menu:maximize");
        apply_toggle(&io, &ctx, "right", "minimize", true);
        assert_eq!(*io.0.borrow(), "close,menu:minimize,maximize");
        // Готовая раскладка «как в Windows» по-прежнему собирает всё справа.
        apply_preset(&io, &ctx, "right", &st.selected);
        assert_eq!(*io.0.borrow(), ":maximize,close");
    }

    #[test]
    fn mac_style_keeps_foreign_buttons() {
        // KDE по умолчанию: справка при кнопках управления, меню, «на всех
        // столах» и «скрыть от снимков» — напротив; туда и обратно без потерь.
        let kde = parse(Codec::Kde, "MSE", "HIAX");
        let selected = choice_of(&kde).all();
        let mac = build(&preset("left", &selected), &kde);
        assert_eq!(format(Codec::Kde, &mac), ("XIAH".into(), "MSE".into()));
        assert_eq!(gtk_layout(&mac), "close,minimize,maximize:menu");
        assert_eq!(format(Codec::Kde, &build(&preset("right", &selected), &mac)), ("MSE".into(), "HIAX".into()));
        // Чужая кнопка (GTK `icon`) остаётся на своей стороне.
        let gnome = parse(Codec::Gnome, "icon:close", "");
        assert_eq!(format(Codec::Gnome, &build(&preset("left", &ids(&["close", "minimize"])), &gnome)).0, "icon,close,minimize:");
        // `appmenu` GNOME в своём наборе не держит — и снимком она не
        // теряется: остаётся впереди, где стояла.
        let io = GsIo(std::cell::RefCell::new("appmenu:close".into()));
        let ctx = LookCtx::new("gnome", "/home/t", None);
        apply_preset(&io, &ctx, "left", &ids(&["close", "minimize"]));
        assert_eq!(*io.0.borrow(), "appmenu,close,minimize:");
        // xfwm4: «свернуть в заголовок» — при кнопках управления, как в `O|SHMC`.
        let x = parse(Codec::Xfwm, "O|SHMC", "");
        let all = choice_of(&x).all();
        assert_eq!(format(Codec::Xfwm, &build(&preset("right", &all), &x)).0, "O|SHMC");
        assert_eq!(format(Codec::Xfwm, &build(&preset("left", &all), &x)).0, "CHMS|O");
    }

    /// Все кнопки модуля KDE (11, с промежутком) переживают разбор и сборку;
    /// промежуток бывает на обеих сторонах, и галочка трогает только свою.
    #[test]
    fn all_kde_buttons_and_spacer() {
        let kde = parse(Codec::Kde, "MN_SE", "HB_FIAX");
        let c = choice_of(&kde);
        assert_eq!(c.left, ids(&["menu", "appmenu", "spacer", "sticky", "exclude_capture"]));
        assert_eq!(c.right, ids(&["help", "below", "spacer", "above", "minimize", "maximize", "close"]));
        assert_eq!(build(&c, &kde), kde);
        assert_eq!(format(Codec::Kde, &build(&c, &kde)), ("MN_SE".into(), "HB_FIAX".into()));
        let off = toggle(&c, "left", "spacer", false);
        assert_eq!(off.left, ids(&["menu", "appmenu", "sticky", "exclude_capture"]));
        assert!(off.right.contains(&"spacer".to_string()), "справа промежуток остался");
        let on = toggle(&off, "left", "spacer", true);
        assert_eq!(on.left.last().map(String::as_str), Some("spacer"));
        // Справка встаёт при кнопках управления, «под» — среди «булавок».
        let c = toggle(&Choice { left: vec![], right: ids(&["minimize", "maximize", "close"]) }, "right", "help", true);
        assert_eq!(c.right, ids(&["help", "minimize", "maximize", "close"]));
        let c = toggle(&Choice { left: ids(&["menu", "sticky"]), right: vec![] }, "left", "below", true);
        assert_eq!(c.left, ids(&["menu", "sticky", "below"]));
    }

    /// Кнопки, которых в записи окружения нет, в неё не попадают: у GNOME —
    /// справка, «под», «скрыть от снимков»; у xfwm4 — ещё и промежуток; у
    /// KDE — «свернуть в заголовок» (у KWin для неё нет буквы).
    #[test]
    fn codecs_skip_buttons_they_lack() {
        let l = Layout { left: vec![Btn::Help, Btn::Menu, Btn::Shade], right: vec![Btn::Below, Btn::Spacer, Btn::ExcludeCapture, Btn::Close] };
        assert_eq!(format(Codec::Gnome, &l).0, "menu,shade:spacer,close");
        assert_eq!(format(Codec::Xfwm, &l).0, "OS|C");
        assert_eq!(format(Codec::Kde, &l), ("HM".into(), "B_EX".into()));
        assert_eq!(parse(Codec::Gnome, "appmenu,spacer:shade,above,stick", "").left, vec![Btn::AppMenu, Btn::Spacer]);
        assert_eq!(parse(Codec::Xfwm, "OS|T", "").left, vec![Btn::Menu, Btn::Shade]);
        assert_eq!(format(Codec::Openbox, &l).0, "NSLC");
        assert_eq!(format(Codec::Labwc, &l).0, "menu,shade:close");
    }

    /// Openbox: буквы `NDSLIMC` (L — название между сторонами), labwc —
    /// слова labwc-config(5) через `:`; умолчания переживают разбор и
    /// сборку без изменений, чужое (`icon`) остаётся на месте.
    #[test]
    fn openbox_and_labwc_codecs() {
        for (codec, raw) in [
            (Codec::Openbox, "NLIMC"),
            (Codec::Openbox, "CMIL"),
            (Codec::Openbox, "NDSLIMC"),
            (Codec::Labwc, "icon:iconify,max,close"),
            (Codec::Labwc, "close,max,iconify:menu,desk,shade"),
        ] {
            let l = parse(codec, raw, "");
            assert_eq!(format(codec, &l).0, raw, "{raw}");
            assert_eq!(build(&choice_of(&l), &l), l, "{raw}");
        }
        let ob = parse(Codec::Openbox, "NDLSIMC", "");
        assert_eq!(ob.left, vec![Btn::Menu, Btn::Sticky]);
        assert_eq!(ob.right, vec![Btn::Shade, Btn::Minimize, Btn::Maximize, Btn::Close]);
        // Без `L` Openbox ставит всё слева.
        assert_eq!(parse(Codec::Openbox, "NIMC", "").right, Vec::<Btn>::new());
        let lw = parse(Codec::Labwc, "icon:iconify,max,close", "");
        assert_eq!(lw.left, vec![Btn::Other("icon".into())]);
        // «Как в macOS»: кнопки управления слева, меню — справа.
        let mac = build(&preset("left", &ids(&["menu", "minimize", "maximize", "close"])), &parse(Codec::Openbox, "NLIMC", ""));
        assert_eq!(format(Codec::Openbox, &mac).0, "CIMLN");
        let mac = build(&preset("left", &ids(&["minimize", "maximize", "close"])), &lw);
        assert_eq!(format(Codec::Labwc, &mac).0, "icon,close,iconify,max:");
        assert_eq!(default_layout("openbox"), parse(Codec::Openbox, "NLIMC", ""));
        assert_eq!(default_layout("labwc"), lw);
    }

    /// Чего нет — названо поимённо и с оконным менеджером; у KDE — пусто.
    #[test]
    fn unavailable_buttons_are_named() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        assert_eq!(unavailable_note("kde"), "");
        let mate = unavailable_note("mate");
        assert!(mate.contains("Marco") && mate.contains("Context help") && mate.contains("Keep below others"), "{mate}");
        assert!(!mate.contains("Spacer") && !mate.contains("Application menu"), "{mate}");
        let x = unavailable_note("xfce");
        assert!(x.contains("xfwm4") && x.contains("Spacer") && x.contains("Keep above others"), "{x}");
        assert!(unavailable_note("cinnamon-mint").contains("Muffin"));
        assert!(unavailable_note("gnome").contains("GTK"));
        // У каждого окружения — только кнопки, которые его запись умеет.
        let ob = unavailable_note("openbox");
        assert!(ob.contains("Openbox") && ob.contains("Keep above others") && !ob.contains("Roll up"), "{ob}");
        assert!(unavailable_note("labwc").starts_with("labwc"));
        for de in ["kde", "mate", "xfce", "cinnamon-mint", "gnome", "cosmic", "sway", "openbox", "labwc"] {
            for id in supported_buttons(de) {
                assert!(MANAGED.iter().any(|(m, _)| *m == id), "{de}: {id}");
            }
        }
    }

    /// Deepin: рамки deepin-kwin кнопки не меняют — подпись раздела так и
    /// говорит, а не уверяет, что заголовков у Deepin нет вовсе.
    #[test]
    fn deepin_note_is_honest() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let n = note("deepin");
        assert!(n.contains("Deepin's window frames") && !n.contains("draws no title bars"), "{n}");
    }

    /// Файловый `LookIo`: что записано, то и читается; команды — в журнал.
    #[derive(Default)]
    struct FileIo {
        files: std::cell::RefCell<BTreeMap<String, String>>,
        ran: std::cell::RefCell<Vec<Vec<String>>>,
    }

    impl LookIo for FileIo {
        fn run(&self, a: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(a.to_vec());
            (true, String::new())
        }
        fn have(&self, _: &str) -> bool {
            true
        }
        fn read(&self, p: &str) -> Option<String> {
            self.files.borrow().get(p).cloned()
        }
        fn write(&self, p: &str, t: &str) -> bool {
            self.files.borrow_mut().insert(p.to_string(), t.to_string());
            true
        }
        fn list_dir(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }

    /// sway: рамка есть (`default_border <стиль> N`, sway(5)); стиль из
    /// конфига человека сохраняется, открытые окна получают её командой.
    #[test]
    fn sway_border_keeps_the_title_bar_style() {
        assert!(border_supported("sway"));
        assert_eq!(sway_border_styles(None), ("normal".into(), "normal".into()));
        assert_eq!(
            sway_border_styles(Some("# default_border pixel 1\ndefault_border none\ndefault_floating_border normal 3\n")),
            ("pixel".into(), "normal".into())
        );
        let io = FileIo::default();
        let ctx = LookCtx::new("sway", "/h", Some("/c"));
        io.write("/c/sway/config", "default_border pixel 2\n");
        assert_eq!(apply_border(&io, &ctx, "4"), tr("Title bar settings applied."));
        let own = io.read("/c/sway/rexon-borders.conf").unwrap();
        assert!(own.contains("default_border pixel 4\ndefault_floating_border normal 4\n"), "{own}");
        assert!(io.read("/c/sway/config").unwrap().contains("include /c/sway/rexon-borders.conf"));
        assert_eq!(read_border(&io, &ctx), "4");
        {
            let ran = io.ran.borrow();
            let has = |c: &str| ran.contains(&vec!["swaymsg".to_string(), c.to_string()]);
            // `swaymsg reload` перезапускает swaybg самого sway, и его фон
            // ложится ПОВЕРХ обоев Rexon (сверено на sway 1.12 без экрана):
            // новые окна получают рамку командами, без перечитывания.
            assert!(!has("reload"), "{ran:?}");
            assert!(has("default_border pixel 4"), "{ran:?}");
            assert!(has("default_floating_border normal 4"), "{ran:?}");
            assert!(has("[tiling] border pixel 4"), "{ran:?}");
        }
        // «Как в конфиге» возвращает значения из конфига человека (у sway по
        // умолчанию — `normal 2`), тоже без перечитывания.
        io.ran.borrow_mut().clear();
        assert_eq!(apply_border(&io, &ctx, "default"), tr("Title bar settings applied."));
        let ran = io.ran.borrow();
        let has = |c: &str| ran.contains(&vec!["swaymsg".to_string(), c.to_string()]);
        assert!(!has("reload"), "{ran:?}");
        assert!(has("default_border pixel 2"), "{ran:?}");
        assert!(has("default_floating_border normal 2"), "{ran:?}");
        assert_eq!(
            sway_border_defaults(Some("default_border none\ndefault_floating_border pixel 3\n")),
            ("none".into(), "pixel 3".into())
        );
        assert_eq!(border_note("sway"), tr("Only the width changes: whether windows have a title bar stays as set in the sway config."));
    }

    /// sway своего конфига НЕ заводит (в отличие от niri и Hyprland): без
    /// личного он читает `/etc/sway/config`. Прежде рамка отвечала «запустите
    /// sway один раз, чтобы он создал свой конфиг» — совет, после которого
    /// ничего не меняется. Теперь системный копируется на место личного (так
    /// советует sway(5)), и рамка ставится. Конфиг i3 sway тоже читает — его
    /// не заслоняем.
    #[test]
    fn sway_without_own_config_seeds_it_from_system() {
        let io = FileIo::default();
        let ctx = LookCtx::new("sway", "/h", Some("/c"));
        io.write("/etc/sway/config", "set $mod Mod4\nbindsym $mod+q kill\n");
        assert_eq!(apply_border(&io, &ctx, "4"), tr("Title bar settings applied."));
        let cfg = io.read("/c/sway/config").unwrap();
        assert!(cfg.starts_with("set $mod Mod4\nbindsym $mod+q kill\n"), "{cfg}");
        assert!(cfg.contains("include /c/sway/rexon-borders.conf"), "{cfg}");
        assert_eq!(io.read("/etc/sway/config").unwrap(), "set $mod Mod4\nbindsym $mod+q kill\n");

        let io = FileIo::default();
        io.write("/etc/sway/config", "set $mod Mod4\n");
        io.write("/c/i3/config", "bindsym Mod4+q kill\n");
        assert_ne!(apply_border(&io, &ctx, "4"), tr("Title bar settings applied."));
        assert!(io.read("/c/sway/config").is_none(), "конфиг i3 заслонён");
    }

    /// labwc: `border.width` в `themerc-override` (labwc-theme(5)); чужие
    /// строки файла не трогаются, «как в конфиге» убирает только наш ключ.
    #[test]
    fn labwc_border_edits_themerc_override() {
        let io = FileIo::default();
        let ctx = LookCtx::new("labwc", "/h", Some("/c"));
        let path = "/c/labwc/themerc-override";
        io.write(path, "# мои правки\nwindow.active.title.bg.color: #222222\nborder.width: 1\n");
        assert_eq!(read_border(&io, &ctx), "1");
        apply_border(&io, &ctx, "3");
        assert_eq!(io.read(path).unwrap(), "# мои правки\nwindow.active.title.bg.color: #222222\nborder.width: 3\n");
        assert!(io.ran.borrow().contains(&vec!["labwc".to_string(), "--reconfigure".to_string()]));
        apply_border(&io, &ctx, "default");
        assert_eq!(io.read(path).unwrap(), "# мои правки\nwindow.active.title.bg.color: #222222\n");
        assert_eq!(read_border(&io, &ctx), "default");
    }

    /// Подписи без неправды: у sway заголовки есть (без кнопок), labwc и
    /// LXQt рисуют заголовки с кнопками; «нет заголовков» — только там, где
    /// их действительно нет. В GNOME/Cinnamon рамку рисует тема GTK.
    #[test]
    fn title_bar_notes_are_honest() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        for de in ["sway", "labwc", "lxqt", "wayfire"] {
            assert!(!note(de).contains("draws no title bars"), "{de}: {}", note(de));
        }
        assert!(note("sway").contains("no buttons"));
        // labwc и Openbox свои кнопки пишут — оговорки нет; LXQt с
        // незнакомым менеджером называет, под какими пишет.
        assert_eq!(note("labwc"), "");
        assert_eq!(note("openbox"), "");
        assert!(note("lxqt").contains("Openbox") && note("lxqt").contains("labwc"));
        for de in ["hyprland", "niri", "river"] {
            assert!(note(de).contains("draws no title bars"), "{de}");
        }
        for de in ["gnome", "pantheon", "cinnamon-mint"] {
            assert!(border_note(de).contains("GTK"), "{de}");
        }
        assert!(border_note("lxqt").contains("window manager"));
        assert!(!border_note("labwc").contains("has no window border setting"));
    }

    fn ch(l: &[&str], r: &[&str]) -> Choice {
        Choice { left: ids(l), right: ids(r) }
    }

    /// Перетаскивание как в модуле KDE: внутри стороны, между сторонами, с
    /// полки и на полку; позиция — место в стороне уже без кнопки.
    #[test]
    fn move_button_like_kde_drag() {
        let kde = ch(&["menu", "sticky", "exclude_capture"], &["help", "minimize", "maximize", "close"]);
        // Внутри стороны: «закрыть» в начало правой.
        assert_eq!(move_button(&kde, "close", "right", 3, "right", 0).right, ids(&["close", "help", "minimize", "maximize"]));
        // Вперёд по стороне: «меню» за «на всех столах».
        assert_eq!(move_button(&kde, "menu", "left", 0, "left", 1).left, ids(&["sticky", "menu", "exclude_capture"]));
        // Между сторонами: «справка» в конец левой.
        let c = move_button(&kde, "help", "right", 0, "left", 9);
        assert_eq!(c, ch(&["menu", "sticky", "exclude_capture", "help"], &["minimize", "maximize", "close"]));
        // На полку — снять.
        assert_eq!(move_button(&kde, "sticky", "left", 1, SHELF, 0).left, ids(&["menu", "exclude_capture"]));
        // С полки — новая кнопка в позицию.
        assert_eq!(move_button(&kde, "above", SHELF, 0, "right", 1).right, ids(&["help", "above", "minimize", "maximize", "close"]));
        // С полки уже стоящая — переносится, а не двоится.
        let c = move_button(&kde, "close", SHELF, 0, "left", 0);
        assert_eq!(c, ch(&["close", "menu", "sticky", "exclude_capture"], &["help", "minimize", "maximize"]));
        // Устаревшая позиция — берётся первая такая кнопка.
        assert_eq!(move_button(&kde, "close", "right", 0, SHELF, 0).right, ids(&["help", "minimize", "maximize"]));
    }

    /// Промежуток — не кнопка: с полки всегда новый, на полку уходит тот,
    /// что тащили.
    #[test]
    fn spacers_are_many() {
        let c = ch(&["menu", "spacer"], &["spacer", "close"]);
        let two = move_button(&c, "spacer", SHELF, 0, "right", 2);
        assert_eq!(two.right, ids(&["spacer", "close", "spacer"]));
        assert_eq!(two.left, ids(&["menu", "spacer"]));
        assert_eq!(move_button(&two, "spacer", "right", 2, SHELF, 0), c);
        assert_eq!(move_button(&c, "spacer", "left", 1, "right", 2), ch(&["menu"], &["spacer", "close", "spacer"]));
        assert_eq!(add_button(&c, "spacer", "left").left, ids(&["menu", "spacer", "spacer"]));
        assert_eq!(add_button(&c, "spacer", "right").right, ids(&["spacer", "spacer", "close"]));
    }

    /// Шаги кнопками: через название на другую сторону, у края окна — стоп.
    #[test]
    fn nudge_steps_across_the_title() {
        let c = ch(&["menu", "sticky"], &["minimize", "close"]);
        let (n, at) = nudge(&c, "left", 1, "right");
        assert_eq!(n, ch(&["menu"], &["sticky", "minimize", "close"]));
        assert_eq!(at, Some(("right".into(), 0)));
        let (n, at) = nudge(&c, "right", 0, "left");
        assert_eq!(n, ch(&["menu", "sticky", "minimize"], &["close"]));
        assert_eq!(at, Some(("left".into(), 2)));
        let (n, at) = nudge(&c, "left", 0, "right");
        assert_eq!(n, ch(&["sticky", "menu"], &["minimize", "close"]));
        assert_eq!(at, Some(("left".into(), 1)));
        // Край окна.
        assert_eq!(nudge(&c, "left", 0, "left"), (c.clone(), Some(("left".into(), 0))));
        assert_eq!(nudge(&c, "right", 1, "right"), (c.clone(), Some(("right".into(), 1))));
        // На полку и мимо списка.
        assert_eq!(nudge(&c, "right", 1, SHELF), (ch(&["menu", "sticky"], &["minimize"]), None));
        assert_eq!(nudge(&c, "right", 7, "left"), (c.clone(), None));
    }

    /// Перенос сводится в родные строки каждого менеджера окон.
    #[test]
    fn moves_format_for_every_wm() {
        let c = move_button(&ch(&["menu"], &["minimize", "maximize", "close"]), "close", "right", 2, "left", 0);
        let layout = build(&c, &Layout::default());
        assert_eq!(format(Codec::Kde, &layout), ("XM".to_string(), "IA".to_string()));
        assert_eq!(format(Codec::Gnome, &layout).0, "close,menu:minimize,maximize");
        assert_eq!(format(Codec::Xfwm, &layout).0, "CO|HM");
        assert_eq!(format(Codec::Openbox, &layout).0, "CNLIM");
        assert_eq!(format(Codec::Labwc, &layout).0, "close,menu:iconify,max");
    }

    /// Полка: все 11 кнопок KDE; чего у окружения нет — недоступно с
    /// причиной; стоящие помечены, промежуток — никогда.
    #[test]
    fn shelf_marks_missing_and_placed() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let c = ch(&["menu", "spacer"], &["close"]);
        let kde = shelf_of("kde", &c);
        assert_eq!(kde.len(), 11);
        assert!(kde.iter().all(|b| b.available && !b.glyph.is_empty() || b.id == "spacer"));
        assert!(kde.iter().find(|b| b.id == "menu").unwrap().placed);
        assert!(!kde.iter().find(|b| b.id == "spacer").unwrap().placed);
        let xfce = shelf_of("xfce", &c);
        assert_eq!(xfce.len(), 12, "сверх KDE — «свернуть в заголовок»");
        let help = xfce.iter().find(|b| b.id == "help").unwrap();
        assert!(!help.available && help.reason.contains("xfwm4"), "{help:?}");
        assert!(shelf_of("gnome", &c).iter().find(|b| b.id == "sticky").unwrap().reason.contains("GTK"));
    }
}
