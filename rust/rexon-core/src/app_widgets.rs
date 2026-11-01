//! «Значки и панели инструментов» вкладки «Настройки»: значки в меню, значки
//! на кнопках и подписи на кнопках панелей инструментов — во ВСЕХ
//! окружениях, родным ключом каждого.
//!
//! Эталон — «Оформление приложений → Настроить значки и панели инструментов»
//! Параметров системы KDE (`kcm_style`, plasma-workspace 6.8,
//! `kcms/style/stylesettings.kcfg`): `kdeglobals [KDE] ShowIconsInMenuItems`,
//! `ShowIconsOnPushButtons` (по умолчанию `true`) и `[Toolbar style]
//! ToolButtonStyle`, `ToolButtonStyleOtherToolbars` (`NoText`, `TextOnly`,
//! `TextBesideIcon` — по умолчанию, `TextUnderIcon`). После записи модуль
//! KDE шлёт `KGlobalSettings.notifyChange(SettingsChanged, SETTINGS_STYLE)` и
//! `org.kde.KToolBar.styleChanged` — шлём то же.
//!
//! Кто что читает (сверено по исходникам и строкам установленных библиотек):
//! * подписи панелей `[Toolbar style]` читает сам `KToolBar` (KXmlGui,
//!   `KToolBarPrivate::loadKDESettings`): главная панель окна —
//!   `ToolButtonStyle`, все прочие — `ToolButtonStyleOtherToolbars`; без
//!   ключа обе — `TextBesideIcon`, регистр не важен, всё незнакомое (и
//!   `NoText`) — только значки. Стиль KDE (`KStyle`, frameworkintegration)
//!   берёт тот же ключ прочих панелей для `QToolBar` со свойством
//!   `otherToolbar`. Значит, программы KDE слушаются обоих ключей в ЛЮБОМ
//!   окружении; поэтому `kdeglobals` пишется всюду, не только в Plasma, и
//!   отдельный выбор для прочих панелей есть везде (вне Plasma — с честной
//!   оговоркой, что он только для программ KDE: у GTK, qt6ct и LXQt
//!   такого различия нет);
//! * значки в меню и на кнопках из `kdeglobals` читает тема платформы KDE
//!   (`KDEPlasmaPlatformTheme6`) — то есть только внутри Plasma;
//! * остальные программы Qt вне Plasma — qt6ct/qt5ct, `[Interface]
//!   menus_have_icons` (`AA_DontShowIconsInMenus`), `toolbutton_style`
//!   (`Qt::ToolButtonStyle` 0–3, 4 — «как у стиля») и
//!   `dialog_buttons_have_icons` (0 — нет, 1 — как у стиля, 2 — да; читает
//!   прокси-стиль qt6ct, который его тема платформы ставит всегда);
//! * LXQt — `lxqt.conf [General] tool_button_style` (`ToolButtonTextBesideIcon`
//!   и т. п., `lxqt-qtplugin`); значков в меню и на кнопках у LXQt нет;
//! * GTK 3: `gtk-menu-images` и `gtk-button-images` работают (проба
//!   30.10.2026 на GTK 3.24.52: значок у `GtkImageMenuItem` и у кнопки с
//!   подписью появляется и пропадает), а `gtk-toolbar-style` `GtkToolbar`
//!   больше не слушает — подписи панелей в GTK 3 не меняются; у GTK 4 этих
//!   настроек нет вовсе. Так и пишем человеку;
//! * MATE (`org.mate.interface`) и Cinnamon (`org.cinnamon.desktop.interface`)
//!   — `menus-have-icons`, `buttons-have-icons`, `toolbar-style`; их службы
//!   xsettings переносят ключи в `Gtk/MenuImages`, `Gtk/ButtonImages`,
//!   `Gtk/ToolbarStyle`; Xfce — сами свойства `xsettings /Gtk/…`;
//! * GNOME, Budgie, Pantheon — у схемы есть только `toolbar-style`; значки
//!   меню и кнопок — `settings.ini` GTK 3.
//!
//! `settings.ini` пишется только у GTK 3: у GTK 4 таких ключей нет, и он
//! ругался бы на каждый из них при запуске каждой программы.
//!
//! Устройство как у `animations` и `window_buttons`: снимок ([`read_state`])
//! и план ([`plan_set`], [`plan_defaults`]) — на подставном [`LookIo`],
//! исполнитель общий ([`run_steps`]).

use serde::Serialize;

use crate::animations::{
    argv, de_label, gsettings_get, gsettings_set, run_steps, xfconf_get, xfconf_set, LookCtx, LookIo, LookStep,
};
use crate::i18n::{n_, tr, trf};
use crate::shell_palette::{ini_apply, ini_get, IniEdit};

/// Вид подписей на кнопках панели инструментов.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toolbar {
    IconsOnly,
    TextOnly,
    TextBeside,
    TextUnder,
}

/// id для страницы, ключ KDE, имя GTK, число Qt, имя LXQt, подпись.
const TOOLBAR: &[(Toolbar, &str, &str, &str, &str, &str, &str)] = &[
    (Toolbar::IconsOnly, "icons", "NoText", "icons", "0", "ToolButtonIconOnly", n_("Icons only")),
    (Toolbar::TextOnly, "text", "TextOnly", "text", "1", "ToolButtonTextOnly", n_("Text only")),
    (Toolbar::TextBeside, "beside", "TextBesideIcon", "both-horiz", "2", "ToolButtonTextBesideIcon", n_("Text beside icons")),
    (Toolbar::TextUnder, "under", "TextUnderIcon", "both", "3", "ToolButtonTextUnderIcon", n_("Text below icons")),
];

fn row(t: Toolbar) -> &'static (Toolbar, &'static str, &'static str, &'static str, &'static str, &'static str, &'static str) {
    TOOLBAR.iter().find(|r| r.0 == t).expect("все виды в таблице")
}

impl Toolbar {
    pub fn id(self) -> &'static str {
        row(self).1
    }
    pub fn kde(self) -> &'static str {
        row(self).2
    }
    pub fn gtk(self) -> &'static str {
        row(self).3
    }
    pub fn qt(self) -> &'static str {
        row(self).4
    }
    pub fn lxqt(self) -> &'static str {
        row(self).5
    }
    pub fn from_id(v: &str) -> Option<Toolbar> {
        TOOLBAR.iter().find(|r| r.1 == v).map(|r| r.0)
    }
    pub fn from_kde(v: &str) -> Option<Toolbar> {
        TOOLBAR.iter().find(|r| r.2 == v.trim()).map(|r| r.0)
    }
    /// GTK: имя (`both-horiz`), имя перечисления (`GTK_TOOLBAR_BOTH_HORIZ`)
    /// или число — `settings.ini` принимает все три.
    pub fn from_gtk(v: &str) -> Option<Toolbar> {
        let v = v.trim().trim_matches('\'').to_ascii_lowercase();
        let v = v.strip_prefix("gtk_toolbar_").unwrap_or(&v).replace('_', "-");
        match v.as_str() {
            "icons" | "0" => Some(Toolbar::IconsOnly),
            "both" | "2" => Some(Toolbar::TextUnder),
            "both-horiz" | "3" => Some(Toolbar::TextBeside),
            "text" | "1" => Some(Toolbar::TextOnly),
            _ => None,
        }
    }
    /// Qt: 0–3; 4 («как у стиля») и мусор — `None`.
    pub fn from_qt(v: &str) -> Option<Toolbar> {
        TOOLBAR.iter().find(|r| r.4 == v.trim()).map(|r| r.0)
    }
    pub fn from_lxqt(v: &str) -> Option<Toolbar> {
        TOOLBAR.iter().find(|r| r.5 == v.trim()).map(|r| r.0)
    }
}

/// Настройки раздела.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Widgets {
    pub menu_icons: bool,
    pub button_icons: bool,
    pub toolbar: Toolbar,
    /// Прочие панели (не главная) — родное различие KDE; вне Plasma его
    /// слушаются только программы KDE, остальные берут `toolbar`.
    pub other_toolbars: Toolbar,
}

/// Умолчания KDE (`stylesettings.kcfg`).
pub const KDE_DEFAULTS: Widgets =
    Widgets { menu_icons: true, button_icons: true, toolbar: Toolbar::TextBeside, other_toolbars: Toolbar::TextBeside };

// ── Где что лежит ──────────────────────────────────────────────────────────

const KDEGLOBALS: &str = "kdeglobals";
const G_KDE: &str = "KDE";
const G_TOOLBAR: &str = "Toolbar style";
const K_MENU: &str = "ShowIconsInMenuItems";
const K_BUTTONS: &str = "ShowIconsOnPushButtons";
const K_TOOLBAR: &str = "ToolButtonStyle";
const K_OTHER: &str = "ToolButtonStyleOtherToolbars";

const GTK_MENU: &str = "gtk-menu-images";
const GTK_BUTTONS: &str = "gtk-button-images";
const GTK_TOOLBAR: &str = "gtk-toolbar-style";

const QT_MENU: &str = "menus_have_icons";
const QT_TOOLBAR: &str = "toolbutton_style";
const QT_BUTTONS: &str = "dialog_buttons_have_icons";

/// Схема gsettings с тремя ключами (`menus-have-icons`, `buttons-have-icons`,
/// `toolbar-style`) и её служба xsettings.
fn full_schema(de: &str) -> Option<&'static str> {
    match de {
        "mate" => Some("org.mate.interface"),
        "cinnamon-mint" => Some("org.cinnamon.desktop.interface"),
        _ => None,
    }
}

/// GNOME-семейство: у схемы только `toolbar-style`.
fn gnome_family(de: &str) -> bool {
    matches!(de, "gnome" | "budgie" | "pantheon")
}

/// Раздел есть? В Deepin оформление программ задаёт только его Центр
/// управления (как и у остальных разделов, `dde::refusal`).
pub fn available(de: &str) -> bool {
    de != "deepin"
}

/// Пишутся ли настройки qt6ct/qt5ct: вне Plasma (там тема платформы KDE) и
/// вне LXQt (там своя тема платформы).
fn uses_qtct(de: &str) -> bool {
    !matches!(de, "kde" | "lxqt" | "deepin")
}

fn qtct_conf(ctx: &LookCtx, tool: &str) -> String {
    ctx.cfg(&format!("{tool}/{tool}.conf"))
}

/// qt6ct/qt5ct, которыми человек может пользоваться: программа есть или её
/// конфиг уже заведён (как у слоя цветов `shell_palette`).
fn qtct_tools(io: &dyn LookIo, ctx: &LookCtx) -> Vec<&'static str> {
    crate::qt_settings::QTCT_TOOLS
        .iter()
        .copied()
        .filter(|t| io.have(t) || io.read(&qtct_conf(ctx, t)).is_some())
        .collect()
}

fn gtk3_ini(ctx: &LookCtx) -> String {
    crate::gtk_sanitizer::gtk_ini_path(&ctx.config_home, "3.0")
}

fn lxqt_conf(ctx: &LookCtx) -> String {
    ctx.cfg("lxqt/lxqt.conf")
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().trim_matches('\'').to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

fn bool_s(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

// ── Снимок ─────────────────────────────────────────────────────────────────

fn ini_value(io: &dyn LookIo, path: &str, group: &str, key: &str) -> Option<String> {
    ini_get(&io.read(path)?, group, key).filter(|v| !v.is_empty())
}

/// Настройки как они стоят сейчас — по главному для окружения источнику.
pub fn read_settings(io: &dyn LookIo, ctx: &LookCtx) -> Widgets {
    let de = ctx.de.as_str();
    let kde = |g: &str, k: &str| ini_value(io, &ctx.cfg(KDEGLOBALS), g, k);
    let kde_toolbar = |k: &str| kde(G_TOOLBAR, k).and_then(|v| Toolbar::from_kde(&v));
    let gtk = |k: &str| ini_value(io, &gtk3_ini(ctx), "Settings", k);
    // У GTK 3 без ключа значков в меню и на кнопках нет.
    let gtk_bool = |k: &str| gtk(k).and_then(|v| parse_bool(&v)).unwrap_or(false);
    let qtct_toolbar = || {
        qtct_tools(io, ctx)
            .into_iter()
            .find_map(|t| ini_value(io, &qtct_conf(ctx, t), "Interface", QT_TOOLBAR).and_then(|v| Toolbar::from_qt(&v)))
    };
    let mut w = KDE_DEFAULTS;
    match de {
        "kde" => {
            w.menu_icons = kde(G_KDE, K_MENU).and_then(|v| parse_bool(&v)).unwrap_or(true);
            w.button_icons = kde(G_KDE, K_BUTTONS).and_then(|v| parse_bool(&v)).unwrap_or(true);
            w.toolbar = kde_toolbar(K_TOOLBAR).unwrap_or(Toolbar::TextBeside);
        }
        _ if full_schema(de).is_some() => {
            let schema = full_schema(de).unwrap_or_default();
            let b = |k: &str, d: bool| gsettings_get(io, schema, k).and_then(|v| parse_bool(&v)).unwrap_or(d);
            w.menu_icons = b("menus-have-icons", true);
            w.button_icons = b("buttons-have-icons", de == "mate");
            w.toolbar = gsettings_get(io, schema, "toolbar-style").and_then(|v| Toolbar::from_gtk(&v)).unwrap_or(Toolbar::TextBeside);
        }
        "xfce" => {
            let b = |p: &str| xfconf_get(io, "xsettings", p).and_then(|v| parse_bool(&v)).unwrap_or(true);
            w.menu_icons = b("/Gtk/MenuImages");
            w.button_icons = b("/Gtk/ButtonImages");
            w.toolbar = xfconf_get(io, "xsettings", "/Gtk/ToolbarStyle")
                .and_then(|v| Toolbar::from_gtk(&v))
                .unwrap_or(Toolbar::TextBeside);
        }
        _ if gnome_family(de) => {
            w.menu_icons = gtk_bool(GTK_MENU);
            w.button_icons = gtk_bool(GTK_BUTTONS);
            w.toolbar = gsettings_get(io, "org.gnome.desktop.interface", "toolbar-style")
                .and_then(|v| Toolbar::from_gtk(&v))
                .unwrap_or(Toolbar::TextBeside);
        }
        "lxqt" => {
            w.menu_icons = gtk_bool(GTK_MENU);
            w.button_icons = gtk_bool(GTK_BUTTONS);
            w.toolbar = ini_value(io, &lxqt_conf(ctx), "General", "tool_button_style")
                .and_then(|v| Toolbar::from_lxqt(&v))
                .unwrap_or(Toolbar::TextBeside);
        }
        _ => {
            // Тайловые, COSMIC, labwc…: значки — как их видит GTK 3, подписи —
            // как у qt6ct, иначе как у программ KDE.
            w.menu_icons = gtk_bool(GTK_MENU);
            w.button_icons = gtk_bool(GTK_BUTTONS);
            w.toolbar = qtct_toolbar().or_else(|| kde_toolbar(K_TOOLBAR)).unwrap_or(Toolbar::TextBeside);
        }
    }
    // Прочие панели — только ключ KDE, в любом окружении (умолчание
    // `KToolBar` — подписи рядом со значками).
    if de != "deepin" {
        w.other_toolbars = kde_toolbar(K_OTHER).unwrap_or(Toolbar::TextBeside);
    }
    w
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WidgetsOption {
    pub id: String,
    pub label: String,
}

/// Снимок раздела для страницы.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WidgetsState {
    pub de: String,
    /// Раздел есть в этом окружении.
    pub available: bool,
    pub menu_icons: bool,
    pub button_icons: bool,
    pub toolbar: String,
    pub other_toolbars: String,
    /// Отдельный выбор для прочих панелей — везде, где есть раздел
    /// (ключ KDE слушают программы KDE в любом окружении).
    pub has_other_toolbars: bool,
    /// Оговорка под выбором прочих панелей: вне Plasma он только для
    /// программ KDE. В Plasma пусто.
    pub other_toolbars_note: String,
    pub toolbar_options: Vec<WidgetsOption>,
    /// Честное пояснение: где настройка не доходит.
    pub note: String,
}

/// Пояснение под заголовком. `qt_platform_theme` — `$QT_QPA_PLATFORMTHEME`
/// сеанса, `has_qtct` — qt6ct или qt5ct есть.
pub fn note(de: &str, qt_platform_theme: &str, has_qtct: bool) -> String {
    if de == "deepin" {
        return tr("In Deepin, application icons and toolbars are set in its Control Center.");
    }
    let gtk = tr(
        "GTK applications: icons in menus and on buttons change only in GTK 3 applications with classic menus; toolbar labels and GTK 4 applications stay as they are.",
    );
    let qt = match de {
        "kde" => return gtk,
        "lxqt" => tr("Qt applications: LXQt has a toolbar setting only — icons in menus and on buttons stay as the style draws them."),
        _ if !has_qtct => tr("Other Qt applications keep their own look: install qt6ct for them to follow these settings. KDE applications follow the toolbar labels anyway."),
        _ if !matches!(qt_platform_theme, "qt6ct" | "qt5ct") => tr(
            "Other Qt applications follow these settings after you pick a style in \u{ab}Application Style\u{bb} and sign in again. KDE applications follow the toolbar labels anyway.",
        ),
        _ => String::new(),
    };
    if qt.is_empty() {
        gtk
    } else {
        format!("{qt}\n{gtk}")
    }
}

/// Оговорка к выбору прочих панелей: у GTK, qt6ct и LXQt главной и прочих
/// панелей не различают — там всё по первому выбору.
pub fn other_toolbars_note(de: &str) -> String {
    if de == "kde" || !available(de) {
        return String::new();
    }
    tr("Only KDE applications tell secondary toolbars apart; in other applications all toolbars follow \u{ab}Toolbar labels\u{bb}.")
}

pub fn read_state(io: &dyn LookIo, ctx: &LookCtx, qt_platform_theme: &str) -> WidgetsState {
    let de = ctx.de.as_str();
    let w = read_settings(io, ctx);
    WidgetsState {
        de: de.to_string(),
        available: available(de),
        menu_icons: w.menu_icons,
        button_icons: w.button_icons,
        toolbar: w.toolbar.id().into(),
        other_toolbars: w.other_toolbars.id().into(),
        has_other_toolbars: available(de),
        other_toolbars_note: other_toolbars_note(de),
        toolbar_options: TOOLBAR.iter().map(|r| WidgetsOption { id: r.1.into(), label: tr(r.6) }).collect(),
        note: note(de, qt_platform_theme, uses_qtct(de) && !qtct_tools(io, ctx).is_empty()),
    }
}

// ── План ───────────────────────────────────────────────────────────────────

/// Правка ini-файла целиком нашим текстом; `None` — менять нечего (или
/// файла нет, а правка только снимает ключи).
fn ini_step(io: &dyn LookIo, path: &str, edits: &[IniEdit]) -> Option<LookStep> {
    let before = io.read(path);
    if before.is_none() && edits.iter().all(|e| e.value.is_none()) {
        return None;
    }
    let after = ini_apply(before.as_deref(), edits);
    (before.as_deref() != Some(after.as_str())).then(|| LookStep::WriteFile { path: path.to_string(), content: after })
}

fn set(group: &str, key: &str, value: &str) -> IniEdit {
    IniEdit { group: group.into(), key: key.into(), value: Some(value.into()) }
}

fn unset(group: &str, key: &str) -> IniEdit {
    IniEdit { group: group.into(), key: key.into(), value: None }
}

/// Сигналы, которые шлёт модуль KDE: тема платформы перечитывает значки
/// кнопок и меню, `KToolBar` — подписи панелей. Вне сеанса — молча мимо.
fn kde_notify() -> Vec<LookStep> {
    let signal = |path: &str, member: &str, args: &[&str]| {
        let mut a = argv(&["dbus-send", "--session", "--type=signal", path, member]);
        a.extend(args.iter().map(|s| s.to_string()));
        LookStep::Notify(vec![a])
    };
    vec![
        // SettingsChanged (3), SETTINGS_STYLE (7).
        signal("/KGlobalSettings", "org.kde.KGlobalSettings.notifyChange", &["int32:3", "int32:7"]),
        // ToolbarStyleChanged (6).
        signal("/KGlobalSettings", "org.kde.KGlobalSettings.notifyChange", &["int32:6", "int32:0"]),
        signal("/KToolBar", "org.kde.KToolBar.styleChanged", &[]),
    ]
}

/// Записи под `w` (`defaults` — снять наши ключи, вернуть умолчания
/// окружения).
fn plan(io: &dyn LookIo, ctx: &LookCtx, w: Option<&Widgets>) -> Vec<LookStep> {
    let de = ctx.de.as_str();
    if !available(de) {
        return Vec::new();
    }
    let mut steps = Vec::new();
    let pick = |g: &str, k: &str, v: &dyn Fn(&Widgets) -> String| match w {
        Some(w) => set(g, k, &v(w)),
        None => unset(g, k),
    };

    // 1. kdeglobals — Plasma целиком, программы KDE вне Plasma — подписи панелей.
    let mut edits = vec![
        pick(G_TOOLBAR, K_TOOLBAR, &|w| w.toolbar.kde().into()),
        pick(G_TOOLBAR, K_OTHER, &|w| w.other_toolbars.kde().into()),
    ];
    if de == "kde" {
        edits.push(pick(G_KDE, K_MENU, &|w| bool_s(w.menu_icons).into()));
        edits.push(pick(G_KDE, K_BUTTONS, &|w| bool_s(w.button_icons).into()));
    }
    let kde_changed = ini_step(io, &ctx.cfg(KDEGLOBALS), &edits).map(|s| steps.push(s)).is_some();

    // 2. Родные ключи окружения для GTK.
    let gtk_ini = |keys: &[&str]| {
        let edits: Vec<IniEdit> = keys
            .iter()
            .map(|k| {
                pick("Settings", k, &|w| match *k {
                    GTK_MENU => bool_s(w.menu_icons).into(),
                    GTK_BUTTONS => bool_s(w.button_icons).into(),
                    _ => w.toolbar.gtk().into(),
                })
            })
            .collect();
        ini_step(io, &gtk3_ini(ctx), &edits)
    };
    if let Some(schema) = full_schema(de) {
        for (key, value) in [
            ("menus-have-icons", w.map(|w| bool_s(w.menu_icons).to_string())),
            ("buttons-have-icons", w.map(|w| bool_s(w.button_icons).to_string())),
            ("toolbar-style", w.map(|w| format!("'{}'", w.toolbar.gtk()))),
        ] {
            steps.push(match value {
                Some(v) => gsettings_set(schema, key, &v),
                None => LookStep::Cmd(argv(&["gsettings", "reset", schema, key])),
            });
        }
    } else if de == "xfce" {
        for (prop, kind, value) in [
            ("/Gtk/MenuImages", "bool", w.map(|w| bool_s(w.menu_icons).to_string())),
            ("/Gtk/ButtonImages", "bool", w.map(|w| bool_s(w.button_icons).to_string())),
            ("/Gtk/ToolbarStyle", "string", w.map(|w| w.toolbar.gtk().to_string())),
        ] {
            match value {
                Some(v) => steps.push(xfconf_set("xsettings", prop, kind, &v)),
                None if xfconf_get(io, "xsettings", prop).is_some() => {
                    steps.push(LookStep::Cmd(argv(&["xfconf-query", "-c", "xsettings", "-p", prop, "-r"])))
                }
                None => {}
            }
        }
    } else if gnome_family(de) {
        steps.push(match w {
            Some(w) => gsettings_set("org.gnome.desktop.interface", "toolbar-style", &format!("'{}'", w.toolbar.gtk())),
            None => LookStep::Cmd(argv(&["gsettings", "reset", "org.gnome.desktop.interface", "toolbar-style"])),
        });
        steps.extend(gtk_ini(&[GTK_MENU, GTK_BUTTONS]));
    } else {
        steps.extend(gtk_ini(&[GTK_MENU, GTK_BUTTONS, GTK_TOOLBAR]));
    }

    // 3. Остальные программы Qt.
    if de == "lxqt" {
        steps.extend(ini_step(io, &lxqt_conf(ctx), &[pick("General", "tool_button_style", &|w| w.toolbar.lxqt().into())]));
    } else if uses_qtct(de) {
        for tool in qtct_tools(io, ctx) {
            let edits = [
                pick("Interface", QT_MENU, &|w| bool_s(w.menu_icons).into()),
                pick("Interface", QT_BUTTONS, &|w| if w.button_icons { "2" } else { "0" }.into()),
                pick("Interface", QT_TOOLBAR, &|w| w.toolbar.qt().into()),
            ];
            steps.extend(ini_step(io, &qtct_conf(ctx, tool), &edits));
        }
    }

    if kde_changed {
        steps.extend(kde_notify());
    }
    steps
}

/// Записи под новые настройки.
pub fn plan_set(io: &dyn LookIo, ctx: &LookCtx, w: &Widgets) -> Vec<LookStep> {
    plan(io, ctx, Some(w))
}

/// «По умолчанию»: наши ключи снимаются, у gsettings и xfconf — сбросом
/// (умолчание схемы с поправками дистрибутива, а не наша догадка о нём).
pub fn plan_defaults(io: &dyn LookIo, ctx: &LookCtx) -> Vec<LookStep> {
    plan(io, ctx, None)
}

/// Что поменять.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WidgetsChange {
    MenuIcons(bool),
    ButtonIcons(bool),
    /// Подписи главной панели (вне KDE — всех панелей не-KDE программ).
    Toolbar(Toolbar),
    /// Прочие панели программ KDE (в любом окружении).
    OtherToolbars(Toolbar),
}

pub fn changed(current: &Widgets, change: &WidgetsChange) -> Widgets {
    let mut w = *current;
    match *change {
        WidgetsChange::MenuIcons(b) => w.menu_icons = b,
        WidgetsChange::ButtonIcons(b) => w.button_icons = b,
        WidgetsChange::Toolbar(t) => w.toolbar = t,
        WidgetsChange::OtherToolbars(t) => w.other_toolbars = t,
    }
    w
}

/// Применить одну правку. Ответ — фраза для человека (пусто — менять нечего).
pub fn apply(io: &dyn LookIo, ctx: &LookCtx, change: &WidgetsChange) -> String {
    let new = changed(&read_settings(io, ctx), change);
    finish(io, ctx, plan_set(io, ctx, &new))
}

pub fn apply_defaults(io: &dyn LookIo, ctx: &LookCtx) -> String {
    finish(io, ctx, plan_defaults(io, ctx))
}

fn finish(io: &dyn LookIo, ctx: &LookCtx, steps: Vec<LookStep>) -> String {
    if steps.is_empty() {
        return String::new();
    }
    match run_steps(io, ctx, &steps) {
        Ok(()) => tr("Icon and toolbar settings applied. Applications that are already open pick them up after a restart."),
        Err(e) => trf("Could not apply the icon and toolbar settings in %1: %2", &[&de_label(&ctx.de), &e]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// Подставная машина: файлы, ключи gsettings/xfconf и журнал команд.
    #[derive(Default)]
    struct FakeIo {
        files: RefCell<BTreeMap<String, String>>,
        keys: RefCell<BTreeMap<String, String>>,
        tools: Vec<&'static str>,
        log: RefCell<Vec<Vec<String>>>,
    }

    impl LookIo for FakeIo {
        fn run(&self, a: &[String]) -> (bool, String) {
            self.log.borrow_mut().push(a.to_vec());
            let mut keys = self.keys.borrow_mut();
            match a[0].as_str() {
                "gsettings" => {
                    let k = format!("{} {}", a[2], a[3]);
                    match a[1].as_str() {
                        "get" => keys.get(&k).map(|v| (true, format!("{v}\n"))).unwrap_or((false, String::new())),
                        "set" => {
                            keys.insert(k, a[4].clone());
                            (true, String::new())
                        }
                        _ => {
                            keys.remove(&k);
                            (true, String::new())
                        }
                    }
                }
                "xfconf-query" => {
                    let k = format!("{} {}", a[2], a[4]);
                    if a.iter().any(|x| x == "-r") {
                        keys.remove(&k);
                        (true, String::new())
                    } else if let Some(i) = a.iter().position(|x| x == "-s") {
                        keys.insert(k, a[i + 1].clone());
                        (true, String::new())
                    } else {
                        keys.get(&k).map(|v| (true, format!("{v}\n"))).unwrap_or((false, String::new()))
                    }
                }
                _ => (true, String::new()),
            }
        }
        fn have(&self, tool: &str) -> bool {
            matches!(tool, "gsettings" | "xfconf-query" | "dbus-send") || self.tools.contains(&tool)
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            self.files.borrow_mut().insert(path.into(), text.into());
            true
        }
        fn list_dir(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }

    fn ctx(de: &str) -> LookCtx {
        LookCtx::new(de, "/h", None)
    }

    fn file(io: &FakeIo, rel: &str) -> String {
        io.read(&format!("/h/.config/{rel}")).unwrap_or_default()
    }

    #[test]
    fn toolbar_names_roundtrip() {
        for r in TOOLBAR {
            let t = r.0;
            assert_eq!(Toolbar::from_id(t.id()), Some(t));
            assert_eq!(Toolbar::from_kde(t.kde()), Some(t));
            assert_eq!(Toolbar::from_gtk(t.gtk()), Some(t));
            assert_eq!(Toolbar::from_qt(t.qt()), Some(t));
            assert_eq!(Toolbar::from_lxqt(t.lxqt()), Some(t));
        }
        // settings.ini GTK: число и имя перечисления тоже.
        assert_eq!(Toolbar::from_gtk("3"), Some(Toolbar::TextBeside));
        assert_eq!(Toolbar::from_gtk("GTK_TOOLBAR_BOTH_HORIZ"), Some(Toolbar::TextBeside));
        assert_eq!(Toolbar::from_gtk("'icons'"), Some(Toolbar::IconsOnly));
        // qt6ct 4 — «как у стиля»: своего вида нет.
        assert_eq!(Toolbar::from_qt("4"), None);
    }

    #[test]
    fn kde_writes_kdeglobals_and_notifies() {
        let io = FakeIo::default();
        io.write("/h/.config/kdeglobals", "[General]\nColorScheme=BreezeDark\n");
        let c = ctx("kde");
        assert_eq!(read_settings(&io, &c), KDE_DEFAULTS);
        let msg = apply(&io, &c, &WidgetsChange::MenuIcons(false));
        assert!(!msg.is_empty());
        apply(&io, &c, &WidgetsChange::OtherToolbars(Toolbar::IconsOnly));
        let kg = file(&io, "kdeglobals");
        assert!(kg.contains("ColorScheme=BreezeDark"), "{kg}");
        assert!(kg.contains("[KDE]") && kg.contains("ShowIconsInMenuItems=false"), "{kg}");
        assert!(kg.contains("[Toolbar style]") && kg.contains("ToolButtonStyleOtherToolbars=NoText"), "{kg}");
        let w = read_settings(&io, &c);
        assert!(!w.menu_icons && w.button_icons);
        assert_eq!((w.toolbar, w.other_toolbars), (Toolbar::TextBeside, Toolbar::IconsOnly));
        // Сигналы модуля KDE ушли.
        let log = io.log.borrow();
        assert!(log.iter().any(|a| a.contains(&"org.kde.KToolBar.styleChanged".to_string())));
        assert!(log.iter().any(|a| a.contains(&"int32:7".to_string())));
        drop(log);
        // GTK 3 тоже получает значки; GTK 4 — не трогаем.
        assert!(file(&io, "gtk-3.0/settings.ini").contains("gtk-menu-images=false"));
        assert!(io.read("/h/.config/gtk-4.0/settings.ini").is_none());
        // Состояние для страницы.
        let st = read_state(&io, &c, "kde");
        assert!(st.has_other_toolbars && st.available && st.toolbar_options.len() == 4);
        // «По умолчанию» снимает наши ключи, чужие остаются.
        apply_defaults(&io, &c);
        let kg = file(&io, "kdeglobals");
        assert!(!kg.contains("ShowIconsInMenuItems") && !kg.contains("ToolButtonStyle"), "{kg}");
        assert!(kg.contains("ColorScheme=BreezeDark"));
        assert_eq!(read_settings(&io, &c), KDE_DEFAULTS);
        // Повтор «По умолчанию» — менять нечего.
        assert!(plan_defaults(&io, &c).iter().all(|s| !matches!(s, LookStep::WriteFile { .. })));
    }

    #[test]
    fn mate_and_cinnamon_use_their_schema() {
        for (de, schema) in [("mate", "org.mate.interface"), ("cinnamon-mint", "org.cinnamon.desktop.interface")] {
            let io = FakeIo::default();
            let c = ctx(de);
            io.keys.borrow_mut().insert(format!("{schema} toolbar-style"), "'both'".into());
            assert_eq!(read_settings(&io, &c).toolbar, Toolbar::TextUnder);
            apply(&io, &c, &WidgetsChange::ButtonIcons(false));
            apply(&io, &c, &WidgetsChange::Toolbar(Toolbar::IconsOnly));
            let keys = io.keys.borrow().clone();
            assert_eq!(keys.get(&format!("{schema} buttons-have-icons")).map(String::as_str), Some("false"), "{de}");
            assert_eq!(keys.get(&format!("{schema} toolbar-style")).map(String::as_str), Some("'icons'"), "{de}");
            // Программы KDE в этом окружении: главная панель — тот же выбор,
            // прочие — свой (по умолчанию подписи рядом со значками).
            let kg = file(&io, "kdeglobals");
            assert!(kg.contains("ToolButtonStyle=NoText") && kg.contains("ToolButtonStyleOtherToolbars=TextBesideIcon"), "{kg}");
            assert!(!kg.contains("ShowIconsInMenuItems"), "{kg}");
            // settings.ini не трогаем: GTK берёт ключи у службы xsettings.
            assert!(io.read("/h/.config/gtk-3.0/settings.ini").is_none());
            apply_defaults(&io, &c);
            assert!(!io.keys.borrow().keys().any(|k| k.starts_with(schema)), "{de}");
        }
    }

    #[test]
    fn xfce_uses_xsettings() {
        let io = FakeIo::default();
        let c = ctx("xfce");
        apply(&io, &c, &WidgetsChange::MenuIcons(false));
        apply(&io, &c, &WidgetsChange::Toolbar(Toolbar::TextOnly));
        let w = read_settings(&io, &c);
        assert!(!w.menu_icons && w.button_icons);
        assert_eq!(w.toolbar, Toolbar::TextOnly);
        assert_eq!(io.keys.borrow().get("xsettings /Gtk/ToolbarStyle").map(String::as_str), Some("text"));
        apply_defaults(&io, &c);
        assert!(io.keys.borrow().is_empty());
        // Повторный сброс — без `-r` у отсутствующих свойств.
        assert!(plan_defaults(&io, &c).iter().all(|s| !matches!(s, LookStep::Cmd(a) if a[0] == "xfconf-query")));
    }

    #[test]
    fn gnome_family_toolbar_key_and_gtk3_ini() {
        let io = FakeIo::default();
        let c = ctx("gnome");
        // У GTK 3 без ключа значков нет.
        let w = read_settings(&io, &c);
        assert!(!w.menu_icons && !w.button_icons);
        apply(&io, &c, &WidgetsChange::MenuIcons(true));
        apply(&io, &c, &WidgetsChange::Toolbar(Toolbar::TextUnder));
        let ini = file(&io, "gtk-3.0/settings.ini");
        assert!(ini.contains("gtk-menu-images=true") && !ini.contains(GTK_TOOLBAR), "{ini}");
        assert_eq!(io.keys.borrow().get("org.gnome.desktop.interface toolbar-style").map(String::as_str), Some("'both'"));
        assert!(read_settings(&io, &c).menu_icons);
    }

    #[test]
    fn tiling_writes_qtct_and_gtk3() {
        let io = FakeIo { tools: vec!["qt6ct"], ..Default::default() };
        io.write("/h/.config/qt6ct/qt6ct.conf", "[Appearance]\nicon_theme=Papirus\n");
        let c = ctx("hyprland");
        apply(&io, &c, &WidgetsChange::ButtonIcons(true));
        apply(&io, &c, &WidgetsChange::Toolbar(Toolbar::TextUnder));
        let q = file(&io, "qt6ct/qt6ct.conf");
        assert!(q.contains("icon_theme=Papirus"), "{q}");
        assert!(q.contains("[Interface]") && q.contains("dialog_buttons_have_icons=2") && q.contains("toolbutton_style=3"), "{q}");
        let ini = file(&io, "gtk-3.0/settings.ini");
        assert!(ini.contains("gtk-button-images=true") && ini.contains("gtk-toolbar-style=both"), "{ini}");
        // qt5ct нет — его конфиг не заводим.
        assert!(io.read("/h/.config/qt5ct/qt5ct.conf").is_none());
        assert_eq!(read_settings(&io, &c).toolbar, Toolbar::TextUnder);
        // Пояснение: qt6ct есть, но сеанс его не включил.
        assert!(read_state(&io, &c, "").note.contains("Application Style"));
        assert!(!read_state(&io, &c, "qt6ct").note.contains("Application Style"));
        apply_defaults(&io, &c);
        let q = file(&io, "qt6ct/qt6ct.conf");
        assert!(!q.contains("toolbutton_style") && q.contains("icon_theme=Papirus"), "{q}");
    }

    #[test]
    fn lxqt_toolbar_in_lxqt_conf() {
        let io = FakeIo { tools: vec!["qt6ct"], ..Default::default() };
        let c = ctx("lxqt");
        apply(&io, &c, &WidgetsChange::Toolbar(Toolbar::IconsOnly));
        assert!(file(&io, "lxqt/lxqt.conf").contains("tool_button_style=ToolButtonIconOnly"));
        assert_eq!(read_settings(&io, &c).toolbar, Toolbar::IconsOnly);
        // qt6ct в LXQt не действует — не пишем.
        assert!(io.read("/h/.config/qt6ct/qt6ct.conf").is_none());
    }

    /// Прочие панели вне Plasma: отдельный выбор есть, пишет только ключ KDE
    /// и не задевается выбором главной панели.
    #[test]
    fn other_toolbars_outside_kde_write_kdeglobals_only() {
        for de in ["mate", "xfce", "gnome", "lxqt", "hyprland", "cinnamon-mint"] {
            let io = FakeIo { tools: vec!["qt6ct"], ..Default::default() };
            let c = ctx(de);
            let st = read_state(&io, &c, "");
            assert!(st.has_other_toolbars, "{de}");
            assert!(st.other_toolbars_note.contains("KDE"), "{de}: {}", st.other_toolbars_note);
            assert_eq!(st.other_toolbars, "beside", "{de}: умолчание KToolBar");

            apply(&io, &c, &WidgetsChange::Toolbar(Toolbar::TextUnder));
            apply(&io, &c, &WidgetsChange::OtherToolbars(Toolbar::TextOnly));
            let kg = file(&io, "kdeglobals");
            assert!(kg.contains("ToolButtonStyle=TextUnderIcon"), "{de}: {kg}");
            assert!(kg.contains("ToolButtonStyleOtherToolbars=TextOnly"), "{de}: {kg}");
            assert!(!kg.contains("[KDE]"), "{de}: {kg}");
            let w = read_settings(&io, &c);
            assert_eq!((w.toolbar, w.other_toolbars), (Toolbar::TextUnder, Toolbar::TextOnly), "{de}");

            // Выбор главной панели прочие не сбрасывает.
            apply(&io, &c, &WidgetsChange::Toolbar(Toolbar::IconsOnly));
            assert_eq!(read_settings(&io, &c).other_toolbars, Toolbar::TextOnly, "{de}");

            // Родные ключи окружения прочих панелей не знают: qt6ct, LXQt,
            // GTK берут только главный выбор.
            for rel in ["qt6ct/qt6ct.conf", "lxqt/lxqt.conf", "gtk-3.0/settings.ini"] {
                let t = file(&io, rel);
                assert!(!t.contains("TextOnly") && !t.contains("toolbutton_style=1") && !t.contains("=text\n"), "{de} {rel}: {t}");
            }
            assert!(!io.keys.borrow().values().any(|v| v.contains("text") && !v.contains("both")), "{de}");

            // «По умолчанию» снимает и ключ прочих панелей.
            apply_defaults(&io, &c);
            assert!(!file(&io, "kdeglobals").contains("ToolButtonStyleOtherToolbars"), "{de}");
            assert_eq!(read_settings(&io, &c).other_toolbars, Toolbar::TextBeside, "{de}");
        }
    }

    #[test]
    fn other_toolbars_note_only_outside_kde() {
        assert!(other_toolbars_note("kde").is_empty());
        assert!(other_toolbars_note("deepin").is_empty());
        assert!(!other_toolbars_note("mate").is_empty());
        let io = FakeIo::default();
        let st = read_state(&io, &ctx("kde"), "kde");
        assert!(st.has_other_toolbars && st.other_toolbars_note.is_empty());
        assert!(!read_state(&io, &ctx("deepin"), "").has_other_toolbars);
    }

    #[test]
    fn deepin_has_no_section() {
        let io = FakeIo::default();
        let c = ctx("deepin");
        assert!(!read_state(&io, &c, "").available);
        assert!(plan_set(&io, &c, &KDE_DEFAULTS).is_empty());
        assert!(apply(&io, &c, &WidgetsChange::MenuIcons(false)).is_empty());
    }
}
