//! Openbox and labwc window frames and title bar buttons (parity item 5.10):
//! the `<theme>` nodes in `rc.xml`, the button layout via the session's window manager,
//! the frame theme via the section button, the frame sample from `themerc`.
//!
//! The golden is `golden/openbox_labwc_cases.json`. Recapture deliberately:
//! `REXON_UPDATE_GOLDEN=1 cargo test-core --test openbox_labwc`, then
//! read the diff by eye.

use std::cell::RefCell;
use std::collections::BTreeMap;

use rexon_core::animations::{LookCtx, LookIo};
use rexon_core::de_apply;
use rexon_core::qt_settings::{self as qs, TitleWm};
use rexon_core::window_buttons as wb;
use serde_json::{json, Value};

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/openbox_labwc_cases.json");

/// The system Openbox `rc.xml` — an excerpt with the same `<theme>` nodes as in
/// the Openbox 3.6 distribution.
const OPENBOX_SYSTEM: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<openbox_config xmlns=\"http://openbox.org/3.4/rc\" xmlns:xi=\"http://www.w3.org/2001/XInclude\">
<resistance>
  <strength>10</strength>
</resistance>
<theme>
  <name>Clearlooks</name>
  <titleLayout>NLIMC</titleLayout>
  <!--
      available characters are NDSLIMC, each can occur at most once.
      <name>в комментарии не трогается</name>
  -->
  <keepBorder>yes</keepBorder>
  <font place=\"ActiveWindow\">
    <name>sans</name>
    <size>8</size>
  </font>
</theme>
<desktops>
  <names>
    <name>1</name>
  </names>
</desktops>
</openbox_config>
";

/// Files in memory and commands run.
#[derive(Default)]
struct FakeIo {
    files: RefCell<BTreeMap<String, String>>,
    ran: RefCell<Vec<Vec<String>>>,
}

impl FakeIo {
    fn with(files: &[(&str, &str)]) -> Self {
        let io = FakeIo::default();
        for (p, t) in files {
            io.files.borrow_mut().insert(p.to_string(), t.to_string());
        }
        io
    }
}

impl LookIo for FakeIo {
    fn run(&self, argv: &[String]) -> (bool, String) {
        self.ran.borrow_mut().push(argv.to_vec());
        (true, String::new())
    }
    fn have(&self, tool: &str) -> bool {
        tool != "gsettings"
    }
    fn read(&self, path: &str) -> Option<String> {
        self.files.borrow().get(path).cloned()
    }
    fn write(&self, path: &str, text: &str) -> bool {
        self.files.borrow_mut().insert(path.to_string(), text.to_string());
        true
    }
    fn list_dir(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}

fn look(de: &str, wayland: bool) -> LookCtx {
    let mut ctx = LookCtx::new(de, "/h", Some("/c"));
    ctx.wayland = wayland;
    ctx
}

const LXQT_SESSION: &str = "[General]\nwindow_manager=openbox\ncompositor=labwc\n";

/// `<theme>` nodes: what changes and what stays byte for byte.
fn rc_xml_cases() -> Value {
    let cases: Vec<(&str, TitleWm, Option<&str>, &[&str], &str)> = vec![
        ("openbox: тема — меняется только <theme><name>, не <name> шрифта и не в комментарии", TitleWm::Openbox, Some(OPENBOX_SYSTEM), &["name"], "Onyx-Citrus"),
        ("openbox: раскладка кнопок <titleLayout>", TitleWm::Openbox, Some(OPENBOX_SYSTEM), &["titleLayout"], "CIMLN"),
        ("openbox: файла нет — не заводим (свой rc.xml заменил бы системный целиком)", TitleWm::Openbox, None, &["name"], "Onyx"),
        ("labwc: файла нет — заводится с одной <theme>", TitleWm::Labwc, None, &["name"], "Bear2"),
        (
            "labwc: <titlebar> есть, <layout> нет — встаёт в <titlebar>",
            TitleWm::Labwc,
            Some("<?xml version=\"1.0\"?>\n<labwc_config>\n  <theme>\n    <name>Onyx</name>\n    <titlebar>\n      <showTitle>yes</showTitle>\n    </titlebar>\n  </theme>\n</labwc_config>\n"),
            &["titlebar", "layout"],
            "close,max,iconify:menu",
        ),
        (
            "labwc: нет <theme> — встаёт в корень вместе с <titlebar>",
            TitleWm::Labwc,
            Some("<labwc_config>\n  <core><gap>4</gap></core>\n</labwc_config>\n"),
            &["titlebar", "layout"],
            "icon:iconify,max,close",
        ),
        (
            "labwc: самозакрытая <theme/> и имя с & экранируется",
            TitleWm::Labwc,
            Some("<labwc_config>\n  <theme/>\n</labwc_config>\n"),
            &["name"],
            "Black & White",
        ),
        (
            "labwc: <theme> в одну строку",
            TitleWm::Labwc,
            Some("<labwc_config><theme></theme></labwc_config>"),
            &["name"],
            "Natura",
        ),
        ("битый файл — не трогаем", TitleWm::Labwc, Some("<labwc_config>\n  <theme>\n</labwc_config>\n"), &["name"], "X"),
    ];
    let mut out = Vec::new();
    for (name, wm, input, path, value) in cases {
        let got = qs::rc_xml_with_theme_value(input, wm, path, value);
        if let Some(text) = &got {
            assert_eq!(qs::rc_xml_theme_value(text, path).as_deref(), Some(value), "{name}: читается обратно");
            assert_eq!(qs::rc_xml_with_theme_value(Some(text), wm, path, value).as_ref(), Some(text), "{name}: повтор ничего не меняет");
        }
        out.push(json!({"name": name, "path": path, "value": value, "input": input, "expected": got}));
    }
    // Reading: the theme name is a direct child of `<theme>`, not the font's `<name>`.
    assert_eq!(qs::rc_xml_theme_value(OPENBOX_SYSTEM, &["name"]).as_deref(), Some("Clearlooks"));
    assert_eq!(qs::rc_xml_theme_value(OPENBOX_SYSTEM, &["titleLayout"]).as_deref(), Some("NLIMC"));
    assert_eq!(qs::rc_xml_theme_value(OPENBOX_SYSTEM, &["titlebar", "layout"]), None);
    Value::Array(out)
}

/// A snapshot of what the button did: files, commands, message, state.
fn outcome(io: &FakeIo, message: &str, ctx: &LookCtx) -> Value {
    let state = wb::read_state(io, ctx);
    json!({
        "message": message,
        "files": io.files.borrow().clone(),
        "ran": io.ran.borrow().clone(),
        "state": {"de": state.de, "side": state.side, "left": state.left, "right": state.right,
                  "buttons": state.buttons.iter().map(|b| b.id.clone()).collect::<Vec<_>>(),
                  "custom": state.custom, "note": state.note, "unavailable": state.unavailable,
                  "has_border": state.has_border, "border_note": state.border_note},
    })
}

fn button_cases() -> Value {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let mut out = serde_json::Map::new();
    let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    // LXQt on X11 with Openbox: no rc.xml of its own — created as a copy of the system one.
    let io = FakeIo::with(&[("/c/lxqt/session.conf", LXQT_SESSION), ("/etc/xdg/openbox/rc.xml", OPENBOX_SYSTEM)]);
    let ctx = look("lxqt", false);
    let before = wb::read_state(&io, &ctx);
    assert_eq!(before.de, "openbox");
    assert_eq!((before.left.clone(), before.right.clone()), (ids(&["menu"]), ids(&["minimize", "maximize", "close"])));
    let msg = wb::apply_preset(&io, &ctx, "left", &ids(&["menu", "minimize", "maximize", "close"]));
    assert_eq!(qs::rc_xml_theme_value(&io.read("/c/openbox/rc.xml").unwrap(), &["titleLayout"]).as_deref(), Some("CIMLN"));
    assert!(io.ran.borrow().iter().any(|a| a == &ids(&["openbox", "--reconfigure"])));
    out.insert("lxqt X11 + Openbox: «как в macOS» из системного rc.xml".into(), outcome(&io, &msg, &ctx));

    // LXQt on Wayland with labwc: its own rc.xml, «shade» on the right.
    let own = "<?xml version=\"1.0\"?>\n<labwc_config>\n  <theme>\n    <name>Onyx</name>\n  </theme>\n</labwc_config>\n";
    let io = FakeIo::with(&[("/c/lxqt/session.conf", LXQT_SESSION), ("/c/labwc/rc.xml", own)]);
    let ctx = look("lxqt", true);
    let msg = wb::apply_toggle(&io, &ctx, "right", "shade", true);
    assert!(io.ran.borrow().iter().any(|a| a == &ids(&["labwc", "--reconfigure"])));
    out.insert("lxqt Wayland + labwc: поставить «свернуть в заголовок»".into(), outcome(&io, &msg, &ctx));

    // A labwc session without rc.xml: created with a single <theme>.
    let io = FakeIo::default();
    let ctx = look("labwc", true);
    let msg = wb::apply_toggle(&io, &ctx, "left", "sticky", true);
    out.insert("labwc без rc.xml: «на всех рабочих столах» слева".into(), outcome(&io, &msg, &ctx));

    // The reset to default for Openbox is `NLIMC`.
    let io = FakeIo::with(&[("/c/lxqt/session.conf", LXQT_SESSION), ("/c/openbox/rc.xml", &OPENBOX_SYSTEM.replace("NLIMC", "CMIL"))]);
    let ctx = look("lxqt", false);
    let msg = wb::apply_defaults(&io, &ctx);
    out.insert("Openbox: «по умолчанию» возвращает NLIMC".into(), outcome(&io, &msg, &ctx));

    // Openbox without a single rc.xml: a refusal with a reason, nothing written.
    let io = FakeIo::with(&[("/c/lxqt/session.conf", LXQT_SESSION)]);
    let ctx = look("lxqt", false);
    let msg = wb::apply_toggle(&io, &ctx, "right", "shade", true);
    assert!(io.files.borrow().keys().all(|k| k == "/c/lxqt/session.conf"));
    out.insert("Openbox без rc.xml: отказ".into(), outcome(&io, &msg, &ctx));

    // LXQt with KWin: buttons are written with KDE keys.
    let io = FakeIo::with(&[("/c/lxqt/session.conf", "[General]\nwindow_manager=kwin_x11\n")]);
    let ctx = look("lxqt", false);
    assert_eq!(wb::read_state(&io, &ctx).de, "kde");

    // LXQt with an unknown manager — a label, not a write.
    let io = FakeIo::with(&[("/c/lxqt/session.conf", "[General]\nwindow_manager=icewm\n")]);
    let ctx = look("lxqt", false);
    out.insert("LXQt с незнакомым менеджером".into(), outcome(&io, "", &ctx));
    Value::Object(out)
}

fn theme_cases() -> Value {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let ctx = |de: &str, wayland: bool| de_apply::Ctx {
        home: "/h".into(),
        xdg_config_home: Some("/c".into()),
        wayland,
        de: de.into(),
    };
    let plan = |io: &FakeIo, c: &de_apply::Ctx| format!("{:#?}", de_apply::press_plan(io, c, de_apply::OPENBOX_KIND, "Onyx-Citrus"));
    let mut out = serde_json::Map::new();

    let io = FakeIo::with(&[("/c/lxqt/session.conf", LXQT_SESSION), ("/etc/xdg/openbox/rc.xml", OPENBOX_SYSTEM)]);
    let c = ctx("lxqt", false);
    assert_eq!(de_apply::active_names(&io, &c, de_apply::OPENBOX_KIND), vec!["Clearlooks".to_string()]);
    out.insert("lxqt X11 + Openbox".into(), json!(plan(&io, &c)));

    let io = FakeIo::with(&[("/c/lxqt/session.conf", LXQT_SESSION)]);
    let c = ctx("lxqt", true);
    assert!(de_apply::active_names(&io, &c, de_apply::OPENBOX_KIND).is_empty());
    out.insert("lxqt Wayland + labwc без rc.xml".into(), json!(plan(&io, &c)));

    let io = FakeIo::with(&[("/c/labwc/rc.xml", "<labwc_config>\n  <theme>\n    <name>Bear2</name>\n  </theme>\n</labwc_config>\n")]);
    let c = ctx("labwc", true);
    assert_eq!(de_apply::active_names(&io, &c, de_apply::OPENBOX_KIND), vec!["Bear2".to_string()]);
    out.insert("сеанс labwc".into(), json!(plan(&io, &c)));

    let io = FakeIo::with(&[("/c/lxqt/session.conf", "[General]\nwindow_manager=kwin_x11\n")]);
    out.insert("lxqt + KWin: отказ".into(), json!(plan(&io, &ctx("lxqt", false))));

    let io = FakeIo::with(&[("/c/lxqt/session.conf", LXQT_SESSION)]);
    out.insert("Openbox без rc.xml: отказ".into(), json!(plan(&io, &ctx("lxqt", false))));
    Value::Object(out)
}

#[test]
fn openbox_labwc_golden() {
    let got = json!({
        "_note": "Рамки и кнопки заголовка Openbox и labwc (rc.xml). Переснимать осознанно: REXON_UPDATE_GOLDEN=1.",
        "rc_xml": rc_xml_cases(),
        "buttons": button_cases(),
        "theme": theme_cases(),
    });
    let text = serde_json::to_string_pretty(&got).unwrap() + "\n";
    if std::env::var("REXON_UPDATE_GOLDEN").is_ok() {
        std::fs::write(GOLDEN, &text).unwrap();
        return;
    }
    let want = std::fs::read_to_string(GOLDEN).expect("эталона нет — снять: REXON_UPDATE_GOLDEN=1");
    assert_eq!(text, want, "разошлось с эталоном {GOLDEN}");
}

/// The Openbox frames section: visible in LXQt and labwc, not in other tiling ones, with
/// an explanation; the section key leads to the matrix type.
#[test]
fn openbox_section_visibility() {
    use rexon_core::categories as c;
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    assert_eq!(c::category_kind(c::OPENBOX_KEY), Some(de_apply::OPENBOX_KIND));
    for de in ["lxqt", "labwc"] {
        assert!(c::category_available(c::OPENBOX_KEY, de), "{de}");
    }
    for de in ["kde", "xfce", "sway", "hyprland", "niri", "gnome"] {
        assert!(!c::category_available(c::OPENBOX_KEY, de), "{de}");
        let note = c::category_note(std::path::Path::new("/h"), c::OPENBOX_KEY, de).unwrap_or_default();
        assert!(note.contains("Openbox and labwc"), "{de}: {note}");
    }
    assert!(c::WINDOW_BORDER_SECTIONS.contains(&c::OPENBOX_KEY));
}

/// Frame sample: a theme without its own picture is drawn from the `themerc` colors and
/// the `.xbm` button images; an empty `themerc` gives a placeholder.
#[test]
fn openbox_sample_from_themerc() {
    use rexon_core::local_preview::{card_picture, DrawOp, PreviewSource, SampleInput};
    let root = std::env::temp_dir().join(format!("rexon-openbox-sample-{}", std::process::id()));
    let dir = root.join("Citrus/openbox-3");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("themerc"),
        "! комментарий\nwindow.active.title.bg: raised gradient vertical\nwindow.active.title.bg.color: #414141\n\
         window.active.title.bg.colorTo: #202020\nwindow.*.button.*.image.color: rgb:e0/e0/e0\nborder.width: 2\n\
         window.inactive.title.bg.color: grey85\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("close.xbm"),
        "#define close_width 6\n#define close_height 6\nstatic unsigned char close_bits[] = {\n 0x21, 0x12, 0x0c, 0x0c, 0x12, 0x21 };\n",
    )
    .unwrap();
    let path = root.join("Citrus").to_string_lossy().into_owned();
    let pic = card_picture(&SampleInput { kind: "openbox", install_path: &path, icon_themes: &[] }, "🪟");
    assert_eq!(pic.source, PreviewSource::Sample);
    assert!(pic.ops.iter().any(|op| matches!(op, DrawOp::Shade { .. })), "переход цвета заголовка");
    let pixels = pic.ops.iter().filter(|op| matches!(op, DrawOp::Pixels { width: 12, height: 12, .. })).count();
    assert_eq!(pixels, 2, "кнопка «закрыть» из .xbm у обоих окон, вдвое крупнее");
    assert!(pic.ops.contains(&DrawOp::Fill { rect: (14.0, 12.0, 196.0, 96.0), color: (0, 0, 0, 255) }), "рамка неактивного окна");
    std::fs::write(dir.join("themerc"), "! пусто\n").unwrap();
    let pic = card_picture(&SampleInput { kind: "openbox", install_path: &path, icon_themes: &[] }, "🪟");
    assert_eq!(pic.source, PreviewSource::Placeholder);
    let _ = std::fs::remove_dir_all(root);
}

/// A bench, not for an ordinary run: in an isolated session (Xvfb + Openbox,
/// labwc without a screen) do the same as the Rexon buttons, with real files and
/// commands. What to do is `REXON_STAND_ACTION`: `theme=<name>` (the frames section
/// button), `buttons=left|right` (a ready layout «as in macOS /
/// Windows» with all of the manager's buttons), `defaults`, `state`. The environment is
/// the process variables, as for the Rexon window (`HOME`, `XDG_CONFIG_HOME`,
/// `XDG_CURRENT_DESKTOP`, `DISPLAY`, `LABWC_PID`). Run the built
/// test executable ONLY with the bench environment.
#[test]
#[ignore]
fn stand_apply() {
    let Ok(action) = std::env::var("REXON_STAND_ACTION") else { return };
    let io = rexon_core::animations::SystemIo;
    let look = LookCtx::from_env();
    let msg = if let Some(name) = action.strip_prefix("theme=") {
        let (ok, m) = de_apply::press(&io, &de_apply::Ctx::live(), de_apply::OPENBOX_KIND, name);
        format!("{ok} {m}")
    } else if let Some(side) = action.strip_prefix("buttons=") {
        let all: Vec<String> = ["menu", "sticky", "shade", "minimize", "maximize", "close"].iter().map(|s| s.to_string()).collect();
        wb::apply_preset(&io, &look, side, &all)
    } else if action == "defaults" {
        wb::apply_defaults(&io, &look)
    } else {
        let s = wb::read_state(&io, &look);
        format!("de={} left={:?} right={:?} custom={} note={} active={:?}", s.de, s.left, s.right, s.custom, s.note,
            de_apply::active_names(&io, &de_apply::Ctx::live(), de_apply::OPENBOX_KIND))
    };
    println!("STAND: {msg}");
}
