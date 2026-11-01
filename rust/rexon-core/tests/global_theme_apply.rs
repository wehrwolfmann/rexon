//! Окно «Применить «X»?» глобальной темы Plasma: что тема несёт, что окно
//! показывает и что делает каждое сочетание флажков.
//!
//! Машина подставная: каталоги тем и ответы программ — из таблицы, ничего не
//! исполняется. Эталон — `golden/global_theme_apply.json`; намеренную
//! перемену записать: `REXON_UPDATE_GOLDEN=1 cargo test -p rexon-core --test
//! global_theme_apply` и просмотреть `git diff` эталона.

mod i18n_pin;

use std::cell::RefCell;
use std::collections::BTreeMap;

use rexon_core::animations::LookIo;
use rexon_core::appearance_installer::{self as ai, ApplyAction, PressPlan};
use rexon_core::themes::{self as th, GlobalChoice, GlobalPress, GlobalRow};
use serde_json::{json, Value};

const HOME: &str = "/home/t";
const USER: &str = "/home/t/.local/share/plasma/look-and-feel";
const SYSTEM: &str = "/usr/share/plasma/look-and-feel";

/// Подставная машина: файлы — таблица путь → текст, каталоги выводятся из
/// путей файлов; `dbus-send` отвечает заданным `immutable`.
#[derive(Default)]
struct Machine {
    files: BTreeMap<String, String>,
    immutable: Option<&'static str>,
    trace: RefCell<Vec<String>>,
}

impl Machine {
    fn file(mut self, path: &str, text: &str) -> Self {
        self.files.insert(path.to_string(), text.to_string());
        self
    }
}

impl LookIo for Machine {
    fn run(&self, argv: &[String]) -> (bool, String) {
        self.trace.borrow_mut().push(argv.join(" "));
        match self.immutable {
            Some(v) => (true, format!("method return time=1 sender=:1.2\n   boolean {v}\n")),
            None => (false, "org.freedesktop.DBus.Error.ServiceUnknown".into()),
        }
    }
    fn have(&self, tool: &str) -> bool {
        tool == "dbus-send"
    }
    fn read(&self, path: &str) -> Option<String> {
        self.files.get(path).cloned()
    }
    fn write(&self, _: &str, _: &str) -> bool {
        panic!("окно и план ничего не пишут")
    }
    fn list_dir(&self, path: &str) -> Vec<String> {
        let prefix = format!("{path}/");
        let mut out: Vec<String> = self
            .files
            .keys()
            .filter_map(|p| p.strip_prefix(&prefix))
            .map(|rest| rest.split('/').next().unwrap_or_default().to_string())
            .collect();
        out.dedup();
        out
    }
}

const BREEZE_LIKE: &str = "[kdeglobals][General]\nColorScheme=BreezeDark\n\n[kdeglobals][Icons]\nTheme=breeze-dark\n\n[kwinrc][Windows]\nPlacement=Centered\n";

/// Темы стенда: с раскладкой и кнопками заголовка, без раскладки, только
/// раскладка, пустая, и одноимённая своя поверх системной.
fn machine() -> Machine {
    Machine::default()
        .file(&format!("{SYSTEM}/Full/metadata.json"), "{}")
        .file(&format!("{SYSTEM}/Full/contents/defaults"), BREEZE_LIKE)
        .file(&format!("{SYSTEM}/Full/contents/splash/Splash.qml"), "")
        .file(&format!("{SYSTEM}/Full/contents/layouts/org.kde.plasma.desktop-layout.js"), "")
        .file(
            &format!("{SYSTEM}/Full/contents/layouts/defaults"),
            "[kwinrc][org.kde.kdecoration2]\nButtonsOnLeft=MS\nButtonsOnRight=IAX\n\n[kwinrc][Windows]\nBorderlessMaximizedWindows=true\n",
        )
        .file(&format!("{USER}/LooksOnly/metadata.json"), "{}")
        .file(&format!("{USER}/LooksOnly/contents/defaults"), "[plasmarc][Theme]\nname=Sweet\n")
        .file(&format!("{USER}/LooksOnly/contents/previews/preview.png"), "")
        .file(&format!("{USER}/LayoutOnly/metadata.json"), "{}")
        .file(&format!("{USER}/LayoutOnly/contents/layouts/org.kde.plasma.desktop-layout.js"), "")
        .file(&format!("{USER}/Empty/metadata.json"), "{}")
        .file(&format!("{USER}/Empty/contents/defaults"), "[kdeglobals][General]\nColorScheme=\n")
        // Своя копия темы перекрывает системную, как в Plasma.
        .file(&format!("{USER}/Shadowed/metadata.json"), "{}")
        .file(&format!("{USER}/Shadowed/contents/defaults"), "[Wallpaper]\nImage=Next\n")
        .file(&format!("{SYSTEM}/Shadowed/metadata.json"), "{}")
        .file(&format!("{SYSTEM}/Shadowed/contents/layouts/x.js"), "")
}

/// Тема со всеми частями Plasma 6.8 и запасной пакет Breeze (своя
/// раскладка, заставка, кнопки заголовка).
const RICH: &str = "[kdeglobals][General]\nColorScheme=SweetDark\nfont=Noto Sans,10,-1,5,50,0,0,0,0,0\nfixed=Hack,9,-1,5,50,0,0,0,0,0\n\n\
[kdeglobals][WM]\nactiveFont=Noto Sans,11,-1,5,75,0,0,0,0,0\n\n\
[kdeglobals][KDE]\nwidgetStyle=kvantum\n\n[kdeglobals][Icons]\nTheme=candy-icons\n\n\
[plasmarc][Theme]\nname=Sweet\n\n[Wallpaper]\nImage=Sweet\n\n\
[kcminputrc][Mouse]\ncursorTheme=Sweet-cursors\n\n[kwinrc][WindowSwitcher]\nLayoutName=thumbnail_grid\n\n\
[kwinrc][org.kde.kdecoration2]\nlibrary=org.kde.kwin.aurorae\ntheme=__aurorae__svg__Sweet\nBorderSize=None\n\n\
[ksplashrc][KSplash]\nTheme=Sweet\n";

fn with_rich(m: Machine) -> Machine {
    m.file(&format!("{USER}/Rich/metadata.json"), "{}")
        .file(&format!("{USER}/Rich/contents/defaults"), RICH)
        .file(&format!("{USER}/Rich/contents/splash/Splash.qml"), "")
        .file(&format!("{SYSTEM}/{}/metadata.json", th::LNF_FALLBACK_PACKAGE), "{}")
        .file(&format!("{SYSTEM}/{}/contents/defaults", th::LNF_FALLBACK_PACKAGE), "[kdeglobals][General]\nColorScheme=BreezeLight\n")
        .file(&format!("{SYSTEM}/{}/contents/splash/Splash.qml", th::LNF_FALLBACK_PACKAGE), "")
        .file(&format!("{SYSTEM}/{}/contents/layouts/org.kde.plasma.desktop-layout.js", th::LNF_FALLBACK_PACKAGE), "")
        .file(
            &format!("{SYSTEM}/{}/contents/layouts/defaults", th::LNF_FALLBACK_PACKAGE),
            "[kwinrc][org.kde.kdecoration2]\nButtonsOnLeft=MS\nButtonsOnRight=HIAX\n",
        )
}

const THEMES: &[&str] = &["Full", "LooksOnly", "LayoutOnly", "Empty", "Shadowed"];

fn contents_of(m: &Machine, theme: &str) -> th::LnfContents {
    let dir = th::lnf_package_dir(m, HOME, theme).expect("тема стенда на диске");
    th::lnf_contents(m, &dir)
}

/// Все четыре сочетания флажков → план «Применить» тем же путём, что у
/// живого установщика (`press_plan_with`).
fn plans(theme: &str, contents: &th::LnfContents) -> Value {
    let mut out = serde_json::Map::new();
    for (appearance, layout) in [(true, false), (true, true), (false, true), (false, false)] {
        let facts = ai::PressFacts {
            de: "kde".into(),
            home: HOME.into(),
            global_theme_ok: true,
            global: Some(GlobalPress {
                choice: GlobalChoice { appearance, layout, parts: None },
                contents: contents.clone(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let key = format!("appearance={appearance} layout={layout}");
        let got = match ai::press_plan_with(&ApplyAction::KdeGlobal, theme, &facts) {
            PressPlan::Steps { steps, .. } => json!(steps.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>()),
            PressPlan::Refused(why) => json!({ "refused": why }),
            other => json!(format!("{other:?}")),
        };
        out.insert(key, got);
    }
    Value::Object(out)
}

#[test]
fn golden() {
    let _lang = i18n_pin::en();
    let m = machine();
    let mut doc = serde_json::Map::new();
    for theme in THEMES {
        let contents = contents_of(&m, theme);
        let question = th::global_question(theme, &contents, false);
        let locked = th::global_question(theme, &contents, true);
        doc.insert(
            theme.to_string(),
            json!({
                "dir": th::lnf_package_dir(&m, HOME, theme),
                "contents": contents,
                "badge": th::lnf_has_desktop_layout(&m, &th::lnf_package_dir(&m, HOME, theme).unwrap()),
                "question": question,
                "question_plasma_locked": locked,
                "plans": plans(theme, &contents),
            }),
        );
    }
    doc.insert("Rich".into(), rich_golden());
    let text = serde_json::to_string_pretty(&Value::Object(doc)).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/global_theme_apply.json");
    if std::env::var("REXON_UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        std::fs::write(&path, &text).unwrap();
    } else {
        let want = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (снять: REXON_UPDATE_GOLDEN=1)", path.display()));
        assert!(want == text, "эталон {} разошёлся:\n{text}", path.display());
    }
}

/// Поведение Plasma 6.8, которое эталон держит, словами — чтобы перемена не
/// прошла пересъёмкой незамеченной.
#[test]
fn kde_rules() {
    let _lang = i18n_pin::en();
    let m = machine();
    // Оба вида — два флажка, расположение по умолчанию НЕ отмечено.
    let q = th::global_question("Full", &contents_of(&m, "Full"), false);
    assert_eq!((q.appearance_row, q.layout_row), (GlobalRow::Checkbox, GlobalRow::Checkbox));
    assert!(q.appearance_checked && !q.layout_checked);
    // Закреплённые виджеты: флажка расположения нет.
    let q = th::global_question("Full", &contents_of(&m, "Full"), true);
    assert_eq!(q.layout_row, GlobalRow::Hidden);
    // Своей раскладки нет — расположение всё равно предложено, как в KDE
    // (Plasma возьмёт раскладку Breeze), подпись говорит это прямо, флажок
    // не отмечен; отметки плитки нет.
    let q = th::global_question("LooksOnly", &contents_of(&m, "LooksOnly"), false);
    assert_eq!((q.appearance_row, q.layout_row), (GlobalRow::Checkbox, GlobalRow::Checkbox));
    assert!(q.appearance_checked && !q.layout_checked);
    assert_eq!(q.layout_text, "Desktop and widget layout (from Breeze — the theme has none of its own)");
    assert!(!th::lnf_has_desktop_layout(&m, &th::lnf_package_dir(&m, HOME, "LooksOnly").unwrap()));
    // Расположение из Breeze — тот же вызов plasmashell с именем темы:
    // запасной пакет подставляет сама Plasma.
    assert_eq!(th::LNF_FALLBACK_PACKAGE, "org.kde.breeze.desktop");
    let press = GlobalPress {
        choice: GlobalChoice { appearance: false, layout: true, parts: None },
        contents: contents_of(&m, "LooksOnly"),
        ..Default::default()
    };
    let plan = format!("{:?}", th::apply_kde_global_choice_plan("LooksOnly", &press).unwrap().steps);
    assert!(plan.contains("loadLookAndFeelDefaultLayout") && plan.contains("string:LooksOnly"), "{plan}");
    // Одна часть — строка без флажка, и она отмечена.
    let q = th::global_question("LayoutOnly", &contents_of(&m, "LayoutOnly"), false);
    assert_eq!((q.appearance_row, q.layout_row), (GlobalRow::Hidden, GlobalRow::Bullet));
    assert!(q.layout_checked && !q.appearance_checked);
    // Пустая тема — объяснение вместо флажков.
    let q = th::global_question("Empty", &contents_of(&m, "Empty"), false);
    assert!(!q.empty_text.is_empty());
    // Своя копия темы перекрывает системную.
    assert_eq!(th::lnf_package_dir(&m, HOME, "Shadowed").unwrap(), format!("{USER}/Shadowed"));
    assert!(!contents_of(&m, "Shadowed").layout.desktop);
    // Чужое имя с «/» каталогом темы не станет.
    assert_eq!(th::lnf_package_dir(&m, HOME, "../Full"), None);
}

/// Окно спрашивает у plasmashell, закреплены ли виджеты; нет оболочки — не
/// закреплены (флажок расположения на месте).
#[test]
fn live_question_asks_plasmashell() {
    let _lang = i18n_pin::en();
    let mut m = machine();
    m.immutable = Some("true");
    let q = th::global_question_live(&m, HOME, "Full", "Full", "kde").unwrap();
    assert_eq!(q.layout_row, GlobalRow::Hidden);
    assert_eq!(m.trace.borrow().as_slice(), [th::plasma_immutable_argv().join(" ")]);
    m.immutable = None;
    let q = th::global_question_live(&m, HOME, "Full", "Full", "kde").unwrap();
    assert_eq!(q.layout_row, GlobalRow::Checkbox);
    assert!(th::global_question_live(&m, HOME, "Missing", "Missing", "kde").is_none());
}

/// Без окна (`global: None`) «Применить» — прежний `-a` без расположения.
#[test]
fn without_question_plan_is_unchanged() {
    let facts = ai::PressFacts { de: "kde".into(), home: HOME.into(), global_theme_ok: true, ..Default::default() };
    let PressPlan::Steps { steps, .. } = ai::press_plan_with(&ApplyAction::KdeGlobal, "Full", &facts) else {
        panic!("план");
    };
    assert_eq!(steps.len(), 1);
    let line = format!("{:?}", steps[0]);
    assert!(line.contains("\"plasma-apply-lookandfeel\", \"-a\", \"Full\"]"), "{line}");
}

/// Русские подписи окна — из настоящего `po/ru.po`.
#[test]
fn russian_texts() {
    let _lang = i18n_pin::ru();
    let m = machine();
    let q = th::global_question("Full", &contents_of(&m, "Full"), false);
    assert_eq!(q.title, "Применить «Full»?");
    assert_eq!(q.layout_text, "Расположение рабочего стола и виджетов");
    assert_eq!(q.appearance_text, "Оформление");
    let q = th::global_question("LooksOnly", &contents_of(&m, "LooksOnly"), false);
    assert_eq!(q.layout_text, "Расположение рабочего стола и виджетов (из Breeze — своего в теме нет)");
    assert_eq!(th::lnf_layout_badge(), "Содержит расположение виджетов");
}

// ── Части темы: «Выбрать, что применить…» и применение вне Plasma ─────────

fn rich() -> (Machine, th::LnfContents) {
    let m = with_rich(machine());
    let c = th::lnf_contents_live(&m, HOME, "Rich").expect("Rich на диске");
    (m, c)
}

/// Планы разделов, которые живой установщик снимает пробами
/// (`LiveInstaller::global_sections`): здесь — те же построители кнопок
/// «Значки» и «Курсоры» с подставными пробами, цвета и стиль — условные
/// шаги, кнопки заголовка — план раздела «Кнопки заголовка» на стенде.
fn sections(m: &Machine, c: &th::LnfContents, de: &str, choice: GlobalChoice) -> GlobalPress {
    use rexon_core::themes::{ApplyStep, Cmd, LnfPart};
    let mut press = GlobalPress { choice, contents: c.clone(), ..Default::default() };
    let sel = th::global_selection(&press, de);
    let facts = ai::PressFacts {
        de: de.into(),
        home: HOME.into(),
        icons_valid: true,
        cursor_safe: true,
        ..Default::default()
    };
    for &part in th::lnf_section_parts(de) {
        if sel & part.bit() == 0 {
            continue;
        }
        let name = th::lnf_section_value(c, part).unwrap_or_default();
        let got = match part {
            LnfPart::Icons | LnfPart::Cursors => {
                let action = if part == LnfPart::Icons { ApplyAction::Icons } else { ApplyAction::Cursor };
                match ai::press_plan_with(&action, &name, &facts) {
                    PressPlan::Steps { steps, .. } => Ok(steps
                        .into_iter()
                        .filter_map(|s| match s {
                            rexon_core::install_plan::Step::Apply(a) => Some(a),
                            _ => None,
                        })
                        .collect()),
                    other => Err(format!("{other:?}")),
                }
            }
            LnfPart::TitlebarLayout => {
                th::lnf_buttons_steps(m, &rexon_core::animations::LookCtx::new(de, HOME, None), c)
            }
            _ => Ok(vec![ApplyStep::Primary(Cmd { argv: vec![format!("section-{part:?}"), name], timeout: None })]),
        };
        press.sections.insert(part, got);
    }
    press
}

fn plan_lines(theme: &str, de: &str, press: GlobalPress) -> Value {
    let facts = ai::PressFacts {
        de: de.into(),
        home: HOME.into(),
        global_theme_ok: true,
        global: Some(press),
        ..Default::default()
    };
    match ai::press_plan_with(&ApplyAction::KdeGlobal, theme, &facts) {
        PressPlan::Steps { steps, say, .. } => json!({
            "steps": steps.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>(),
            "say": format!("{say:?}"),
        }),
        PressPlan::Refused(why) => json!({ "refused": why }),
        other => json!(format!("{other:?}")),
    }
}

fn rich_golden() -> Value {
    let (m, c) = rich();
    let mut kde_parts = serde_json::Map::new();
    for part in th::lnf_parts_of(c.mask()) {
        let choice = GlobalChoice { appearance: true, layout: false, parts: Some(part.bit()) };
        kde_parts.insert(format!("{part:?}"), plan_lines("Rich", "kde", sections(&m, &c, "kde", choice)));
    }
    let mut outside = serde_json::Map::new();
    for de in ["mate", "xfce", "gnome", "hyprland", "deepin"] {
        let q = th::global_question_in("Rich", &c, false, de);
        let all = th::lnf_mask(&th::lnf_parts_of(u32::MAX));
        let simple = GlobalChoice { appearance: true, layout: false, parts: None };
        let every = GlobalChoice { appearance: true, layout: true, parts: Some(all) };
        outside.insert(
            de.into(),
            json!({
                "question": q,
                "plan_simple": plan_lines("Rich", de, sections(&m, &c, de, simple)),
                "plan_every_part": plan_lines("Rich", de, sections(&m, &c, de, every)),
            }),
        );
    }
    json!({
        "contents": c,
        "question": th::global_question("Rich", &c, false),
        "question_plasma_locked": th::global_question("Rich", &c, true),
        "kde_each_part": kde_parts,
        "outside": outside,
    })
}

/// Части и их признаки — как `KLookAndFeelManager::packageContents`
/// Plasma 6.8; недостающее у темы (раскладка, кнопки) — из Breeze.
#[test]
fn parts_like_plasma() {
    use rexon_core::themes::LnfPart::*;
    let (_m, c) = rich();
    assert_eq!(
        c.parts,
        vec![
            Colors, WidgetStyle, WindowDecoration, BorderSize, Icons, PlasmaTheme, Cursors, Fonts, WindowSwitcher,
            SplashScreen, Wallpaper, DesktopLayout, TitlebarLayout
        ]
    );
    // Своей раскладки у темы нет — подпись «из Breeze», но часть есть.
    assert!(!c.layout.desktop);
    assert_eq!(c.layout.buttons, Some(("MS".into(), "HIAX".into())));
    // Без запасного пакета — только своё.
    let m = with_rich(machine());
    let own = th::lnf_contents(&m, &format!("{USER}/Rich"));
    assert!(!own.parts.contains(&DesktopLayout) && !own.parts.contains(&TitlebarLayout));
    // Флаги — значения Plasma.
    assert_eq!(Colors.bit(), 1);
    assert_eq!(BorderSize.bit(), 1 << 11);
    assert_eq!(DesktopLayout.bit(), 1 << 16);
    assert_eq!(th::LNF_APPEARANCE_MASK, 0xfff);
}

/// Подробный вид в Plasma: флажок на часть, расположение не отмечено,
/// при закреплённых виджетах флажка «Расположение рабочего стола» нет.
#[test]
fn detailed_view_in_plasma() {
    let _lang = i18n_pin::en();
    let (_m, c) = rich();
    let q = th::global_question("Rich", &c, false);
    let texts: Vec<&str> = q.parts.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Desktop layout", "Titlebar Buttons layout", "Colors", "Application Style", "Window Decoration Style",
            "Window Decoration Size", "Icons", "Plasma Style", "Pointers", "Fonts", "Task Switcher", "Splash Screen"
        ]
    );
    assert!(q.parts.iter().all(|p| p.enabled && p.checked != p.layout));
    assert!(!q.detailed && q.more_enabled);
    let q = th::global_question("Rich", &c, true);
    assert!(q.parts.iter().all(|p| p.text != "Desktop layout"));
}

/// Plasma: всё оформление — `plasma-apply-lookandfeel -a`; одна часть —
/// её ключи, тема записывается выбранной; значки — план раздела.
#[test]
fn kde_partial_plans() {
    use rexon_core::themes::LnfPart;
    let (m, c) = rich();
    let all_app = th::LNF_APPEARANCE_MASK;
    let plan = plan_lines("Rich", "kde", sections(&m, &c, "kde", GlobalChoice { appearance: true, layout: false, parts: Some(all_app) }));
    assert!(plan.to_string().contains(r#"\"plasma-apply-lookandfeel\", \"-a\", \"Rich\"]"#), "{plan}");
    let only = |p: LnfPart| {
        plan_lines("Rich", "kde", sections(&m, &c, "kde", GlobalChoice { appearance: true, layout: false, parts: Some(p.bit()) }))
            .to_string()
    };
    let colors = only(LnfPart::Colors);
    assert!(colors.contains("LookAndFeelPackage") && colors.contains("plasma-apply-colorscheme") && !colors.contains("plasma-apply-lookandfeel"), "{colors}");
    assert!(only(LnfPart::Fonts).contains("refreshFonts"));
    assert!(only(LnfPart::WindowDecoration).contains("reloadConfig"));
    assert!(only(LnfPart::Icons).contains("candy-icons"));
    // Обои Plasma 6.8 не применяет — одни они ничего не дают.
    assert!(!only(LnfPart::PlasmaTheme).contains("plasma-apply-lookandfeel"));
}

/// Вне Plasma: окно сразу подробное, непереносимые части выключены с
/// причиной, план — шаги разделов этого стола, шрифты — родные ключи.
#[test]
fn outside_plasma() {
    use rexon_core::themes::LnfPart;
    let _lang = i18n_pin::en();
    let (m, c) = rich();
    let q = th::global_question_in("Rich", &c, false, "mate");
    assert!(q.detailed && !q.outside_note.is_empty());
    let row = |id: LnfPart| q.parts.iter().find(|p| p.id == id).unwrap().clone();
    for id in [LnfPart::Colors, LnfPart::WidgetStyle, LnfPart::Icons, LnfPart::Cursors, LnfPart::Fonts, LnfPart::TitlebarLayout] {
        assert!(row(id).enabled && row(id).reason.is_empty(), "{id:?}");
    }
    for id in [LnfPart::PlasmaTheme, LnfPart::WindowDecoration, LnfPart::SplashScreen, LnfPart::DesktopLayout] {
        assert!(!row(id).enabled && !row(id).checked && !row(id).reason.is_empty(), "{id:?}");
    }
    let every = GlobalChoice { appearance: true, layout: true, parts: Some(u32::MAX) };
    let plan = plan_lines("Rich", "mate", sections(&m, &c, "mate", every)).to_string();
    assert!(!plan.contains("plasma-apply") && !plan.contains("kwriteconfig6"), "{plan}");
    assert!(plan.contains("org.mate.interface") && plan.contains("Noto Sans 10"), "{plan}");
    assert!(plan.contains("org.mate.Marco.general"), "кнопки заголовка: {plan}");
    // Deepin: цвета и стиль — Chameleon, шрифты — Центр управления.
    let q = th::global_question_in("Rich", &c, false, "deepin");
    assert!(q.parts.iter().filter(|p| matches!(p.id, LnfPart::Colors | LnfPart::WidgetStyle | LnfPart::Fonts)).all(|p| !p.enabled));
}
