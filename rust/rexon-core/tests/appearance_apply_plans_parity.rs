//! Insurance before merging the installers: golden file of «Применить» and installation
//! in the «Оформление» tab by section and desktop.
//!
//! WHY. `appearance_installer` («Установить» / «Применить» of themes by
//! section) is being merged with the `.rxn` package installer. The checks against Python
//! (`themes_parity.rs`, `appearance_installer_parity.rs`) hold separate
//! plan shapes; here it is for EVERY section in EACH of the eight desktops:
//!
//!   * `available` — whether the section is shown on this desktop;
//!   * `action`    — what «Применить» does (`apply_action_for`);
//!   * `apply`     — the plan or command trace of «Применить» by exactly the
//!     builders that `LiveInstaller::apply` calls (fake
//!     machine and programs — nothing is executed);
//!   * `install`   — the install plan of an unpacked archive (`plan_install`),
//!     the log entry, the removal plan from it and a reinstall on top.
//!
//! Golden file — `golden/appearance_apply_plans.json`. To record a deliberate change:
//! `REXON_UPDATE_GOLDEN=1 cargo test -p rexon-core --test
//! appearance_apply_plans_parity` and review the `git diff` of the golden file.

mod i18n_pin;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use rexon_core::animations::LookIo;
use rexon_core::appearance_installer::{self as ai, ApplyAction, RootEnv};
use rexon_core::ext_manager::ExtStep;
use rexon_core::categories;
use rexon_core::installed_scan::FsSnapshot;
use rexon_core::models::ThemeCategory;
use rexon_core::themes as th;
use rexon_core::utils::{RunOutcome, RunReport};
use serde_json::{json, Map, Value};

const HOME: &str = "/home/t";
const THEME: &str = "MyTheme";

/// Desktop identifiers as `system::detect_current_de` returns them
/// and `LiveInstaller::new` receives them.
const DES: &[&str] = &["kde", "gnome", "xfce", "cinnamon", "mate", "lxqt", "sway", ""];

/// Fake machine: all programs exist, commands "succeed", writes
/// are remembered. The trace is what «Применить» would have done.
struct Machine {
    trace: RefCell<Vec<String>>,
    files: RefCell<HashMap<String, String>>,
}

impl Machine {
    fn new() -> Self {
        Machine { trace: RefCell::new(Vec::new()), files: RefCell::new(HashMap::new()) }
    }
    fn take(&self) -> Vec<Value> {
        self.trace.borrow_mut().drain(..).map(Value::String).collect()
    }
}

impl LookIo for Machine {
    fn run(&self, argv: &[String]) -> (bool, String) {
        self.trace.borrow_mut().push(format!("run {argv:?}"));
        (true, String::new())
    }
    fn have(&self, _: &str) -> bool {
        true
    }
    fn read(&self, path: &str) -> Option<String> {
        self.files.borrow().get(path).cloned()
    }
    fn write(&self, path: &str, text: &str) -> bool {
        self.trace.borrow_mut().push(format!("write {path} = {text:?}"));
        self.files.borrow_mut().insert(path.to_string(), text.to_string());
        true
    }
    fn list_dir(&self, _: &str) -> Vec<String> {
        Vec::new()
    }
}

fn ok_report() -> RunReport {
    RunReport {
        outcome: RunOutcome::Ok,
        returncode: Some(0),
        stdout: Vec::new(),
        stderr: Vec::new(),
        timeout_secs: 0,
        error: String::new(),
    }
}

fn dbg_lines<T: std::fmt::Debug>(items: &[T]) -> Vec<Value> {
    items.iter().map(|s| Value::String(format!("{s:?}"))).collect()
}

fn plan_lines(plan: &th::Plan) -> Value {
    json!({"history": plan.history_kind, "steps": dbg_lines(&plan.steps)})
}

/// The Qt style environment as the live installer's `qt_style_env` builds it
/// on a machine with qt6ct and the Papirus icon theme.
fn kv_env(de: &str) -> th::KvantumEnv<'_> {
    th::KvantumEnv {
        de_id: de,
        home: HOME,
        xdg_config_home: "",
        platform_theme: "qt6ct",
        icon_theme: "Papirus".to_string(),
        qt_desktop_installed: false,
    }
}

/// Where the desktop section buttons look for the user's files.
fn de_ctx(de: &str) -> rexon_core::de_apply::Ctx {
    rexon_core::de_apply::Ctx {
        home: HOME.to_string(),
        xdg_config_home: None,
        wayland: matches!(de, "kde" | "gnome" | "sway"),
        de: de.to_string(),
    }
}

/// The plan or trace of «Применить» by the same builder that
/// `LiveInstaller::apply` uses for this action.
fn apply_plan(action: &ApplyAction, cat: &ThemeCategory, de: &str, theme: &str) -> Value {
    match action {
        ApplyAction::KdeGlobal => plan_lines(&th::apply_kde_global_plan(theme)),
        ApplyAction::KdePlasma => plan_lines(&th::apply_kde_plasma_plan(theme)),
        ApplyAction::KdeColors => colors_trace(de, theme),
        ApplyAction::Cursor => json!({
            "sized": th::apply_cursor_plan_sized(theme, Some(32), de, HOME, None, true).map(|p| plan_lines(&p)),
            "unsafe": th::apply_cursor_plan_sized(theme, Some(32), de, HOME, None, false).map(|p| plan_lines(&p)),
            "standard": th::standard_cursor_plan(de, Some(32), HOME, None).map(|p| plan_lines(&p)),
        }),
        ApplyAction::Icons => json!({
            "valid": th::apply_icon_plan_xdg(theme, de, HOME, None, true).map(|p| plan_lines(&p)),
            "invalid": th::apply_icon_plan_xdg(theme, de, HOME, None, false).map(|p| plan_lines(&p)),
        }),
        ApplyAction::Kvantum => plan_lines(&th::apply_kvantum_plan(theme, &kv_env(de))),
        ApplyAction::Gtk => json!({
            "dark": plan_lines(&th::apply_gtk_plan_scheme(theme, de, HOME, None, Some(true))),
            "light": plan_lines(&th::apply_gtk_plan_scheme(theme, de, HOME, None, Some(false))),
        }),
        ApplyAction::Aurorae => Value::String(format!("{:?}", ai::aurorae_plan(theme))),
        ApplyAction::WidgetStyle => plan_lines(&th::apply_qt_style_plan(theme, &kv_env(de))),
        ApplyAction::Splash => Value::String(format!("{:?}", ai::lnf_component_plan("splash", theme))),
        ApplyAction::Lockscreen => Value::String(format!("{:?}", ai::lnf_component_plan("lockscreen", theme))),
        ApplyAction::LockWallpaper => json!({
            "x11": format!("{:?}", ai::lock_wallpaper_plan_in(de, false, &cat.install_dir, theme, true)),
            "wayland": format!("{:?}", ai::lock_wallpaper_plan_in(de, true, &cat.install_dir, theme, true)),
            "missing": format!("{:?}", ai::lock_wallpaper_plan_in(de, false, &cat.install_dir, theme, false)),
        }),
        ApplyAction::SoundTheme => Value::String(format!(
            "{:?}",
            rexon_core::qt_settings::sound_theme_plan(de, theme, "kwriteconfig6", "kreadconfig6", true)
        )),
        ApplyAction::WallpaperPack => {
            let files = vec![
                format!("{}/{theme}/a-1920x1080.jpg", cat.install_dir),
                format!("{}/{theme}/b-3840x2160.png", cat.install_dir),
            ];
            json!({"pick_1080p": ai::pick_pack_image(&files, 1920, 1080)})
        }
        // A picture is set as the background by the common wallpaper executor — its plans per
        // desktop are guarded by `wallpaper_backend_parity.rs`.
        ApplyAction::WallpaperBackground => json!("wallpaper_exec::live_slot"),
        ApplyAction::DeSection => {
            let kind = categories::category_kind(&cat.key).unwrap_or("");
            let io = Machine::new();
            let ctx = de_ctx(de);
            let on = rexon_core::de_apply::enable_steps(&io, &ctx, kind, theme);
            let off = rexon_core::de_apply::disable_steps(&io, &ctx, kind, theme);
            let (ok, _) = rexon_core::de_apply::press(&io, &ctx, kind, theme);
            json!({"enable": dbg_lines(&on), "disable": dbg_lines(&off), "press_ok": ok, "press": io.take()})
        }
        ApplyAction::SystemTheme => {
            let env = RootEnv {
                is_plasmalogin: false,
                has_pkexec: true,
                system_helper_path: "/usr/lib/rexon/rexon-system-helper",
                system_helper_exists: true,
                is_steamos_or_immutable: false,
                is_steamos: false,
                family_is_debian: false,
            };
            let ran = RefCell::new(Vec::new());
            let (ok, _) = ai::system_theme_apply_with(&env, cat, theme, &[], &|_| true, &|argv, t| {
                ran.borrow_mut().push(format!("{argv:?} timeout={t}"));
                ok_report()
            });
            json!({"ok": ok, "ran": ran.into_inner()})
        }
        ApplyAction::Noop => Value::Null,
    }
}

/// «Применить» of a color scheme: on Plasma — the stock utility, outside Plasma —
/// Rexon layers (`shell_palette::apply_scheme`). The layers need a real
/// directory: the scheme is looked up on disk and the "what was there" note is written next to it —
/// hence a temporary HOME (not the owner's profile), the paths in the trace are `/HOME`.
fn colors_trace(de: &str, theme: &str) -> Value {
    use rexon_core::shell_palette as sp;
    let dir = std::env::temp_dir().join(format!("rexon-apply-golden-{}-{}", std::process::id(), if de.is_empty() { "none" } else { de }));
    let _ = std::fs::remove_dir_all(&dir);
    let schemes = dir.join(".local/share/color-schemes");
    std::fs::create_dir_all(&schemes).unwrap();
    std::fs::write(
        schemes.join(format!("{theme}.colors")),
        "[General]\nName=MyTheme\n[Colors:Window]\nBackgroundNormal=10,20,30\nForegroundNormal=200,200,200\n",
    )
    .unwrap();
    let root = dir.to_string_lossy().into_owned();
    let ctx = sp::SchemeCtx {
        de_id: de.to_string(),
        home: sp::PaletteHome::from_env(|k| if k == "HOME" { root.clone() } else { String::new() }),
        data_dirs: vec![format!("{root}/no-system-data")],
        session_platform_theme: String::new(),
        qt_desktop_installed: false,
    };
    let io = Machine::new();
    let (ok, msg, _) = sp::apply_scheme(&io, &ctx, theme);
    // What landed on disk bypassing `io` (the note, layer files).
    let mut on_disk = Vec::new();
    let mut stack = vec![dir.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                on_disk.push(p.to_string_lossy().replace(&root, "/HOME"));
            }
        }
    }
    on_disk.sort();
    let _ = std::fs::remove_dir_all(&dir);
    let trace: Vec<Value> = io
        .take()
        .into_iter()
        .map(|v| Value::String(v.as_str().unwrap().replace(&root, "/HOME")))
        .collect();
    json!({
        "way": format!("{:?}", sp::scheme_way(de)),
        "ok": ok,
        "message": msg.replace(&root, "/HOME"),
        "trace": trace,
        "files": on_disk,
    })
}

/// An unpacked archive that "looks like anything" (like the package in
/// `rxn_install_plans_parity.rs`): a theme with metadata, contents,
/// cursors, `index.theme`, colors and a picture.
fn archive_snapshot() -> FsSnapshot {
    FsSnapshot::from_json(&json!({
        "dirs": {
            "/x": [THEME],
            "/x/MyTheme": [
                "metadata.json", "metadata.desktop", "contents", "cursors", "index.theme",
                "gtk-3.0", "MyTheme.colors", "kvantum.kvconfig", "xfwm4", "Main.qml", "bg.png",
            ],
            "/x/MyTheme/contents": ["defaults"],
            "/x/MyTheme/cursors": ["left_ptr"],
            "/x/MyTheme/gtk-3.0": ["gtk.css"],
            "/x/MyTheme/xfwm4": ["themerc"],
        },
        "files": {
            "/x/MyTheme/metadata.json": "{\"KPlugin\": {\"Id\": \"org.rexon.mytheme\", \"Name\": \"MyTheme\"}, \"uuid\": \"my@rexon\"}",
            "/x/MyTheme/metadata.desktop": "[Desktop Entry]\nName=MyTheme\nX-KDE-PluginInfo-Name=org.rexon.mytheme\n",
            "/x/MyTheme/contents/defaults": "[kdeglobals]\n",
            "/x/MyTheme/cursors/left_ptr": "",
            "/x/MyTheme/index.theme": "[Icon Theme]\nName=MyTheme\nDirectories=\n",
            "/x/MyTheme/gtk-3.0/gtk.css": "",
            "/x/MyTheme/MyTheme.colors": "[General]\nName=MyTheme\n",
            "/x/MyTheme/kvantum.kvconfig": "",
            "/x/MyTheme/xfwm4/themerc": "",
            "/x/MyTheme/Main.qml": "",
            "/x/MyTheme/bg.png": "",
        },
    }))
}

/// A path from the log is a file if its last component has a theme file extension
/// (`org.rexon.mytheme` for widgets is a directory).
fn looks_like_file(p: &str) -> bool {
    const EXTS: &[&str] = &["colors", "png", "jpg", "jpeg", "webp", "svg", "ttf", "otf", "qss", "plymouth", "kvconfig"];
    p.rsplit('/').next().and_then(|n| n.rsplit_once('.')).is_some_and(|(_, e)| EXTS.contains(&e))
}

/// Installing an archive, writing the log, removing from it and repeating on top —
/// with the unpack root (`/x`, the theme is a subfolder) and with the theme as the root
/// (`/x/MyTheme`, as with KPackage packages).
fn install_block(cat: &ThemeCategory) -> Value {
    let home = Path::new(HOME);
    let fs = archive_snapshot();
    let mut out = Map::new();
    for root in ["/x", "/x/MyTheme"] {
        let first = ai::plan_install(home, &fs, cat, root);
        let paths = first.record.as_ref().map(|r| r.paths.clone()).unwrap_or_default();
        // After installation everything recorded in the log is in place.
        let uninstall = ai::plan_uninstall(&paths, &|p| !looks_like_file(p), &|p| looks_like_file(p));
        let again = ai::plan_install_with(home, &fs, cat, root, Some(&paths));
        out.insert(
            root.to_string(),
            json!({
                "ok": first.ok,
                "message": first.message,
                "names": first.names,
                "reclassified": first.reclassified,
                "record": first.record,
                "steps": dbg_lines(&first.steps),
                "uninstall": dbg_lines(&uninstall),
                "reinstall_ok": again.ok,
                "reinstall": dbg_lines(&again.steps),
            }),
        );
    }
    Value::Object(out)
}

fn real_categories() -> Vec<ThemeCategory> {
    categories::categories(Path::new(HOME)).into_iter().filter(|c| !c.virtual_).collect()
}

fn collect() -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for cat in real_categories() {
        let install = install_block(&cat);
        for de in DES {
            let theme = if matches!(cat.apply_kind.as_str(), "wallpapers" | "wallpaper_plugins" | "lockwallpaper") {
                "bg.png"
            } else {
                THEME
            };
            let mut entry = Map::new();
            entry.insert("available".into(), json!(categories::category_available(&cat.key, de)));
            for t in [THEME, theme] {
                let action = ai::apply_action_for(Some(&cat), t);
                entry.insert(format!("action {t}"), json!(format!("{action:?}")));
                entry.insert(format!("apply {t}"), apply_plan(&action, &cat, de, t));
            }
            let de_name = if de.is_empty() { "none" } else { de };
            out.insert(format!("{de_name} / {} {}", cat.id, cat.key), Value::Object(entry));
        }
        out.insert(format!("install / {} {}", cat.id, cat.key), install);
    }
    out
}

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/appearance_apply_plans.json");

/// «Применить» and installation over all sections and desktops match the golden file.
#[test]
fn appearance_plans_match_golden() {
    let _lang = i18n_pin::en();
    let now = collect();
    let text = serde_json::to_string_pretty(&now).unwrap() + "\n";
    if std::env::var("REXON_UPDATE_GOLDEN").is_ok() {
        std::fs::write(GOLDEN, &text).unwrap();
        return;
    }
    let want: BTreeMap<String, Value> = serde_json::from_str(
        &std::fs::read_to_string(GOLDEN).expect("эталона нет — снять: REXON_UPDATE_GOLDEN=1"),
    )
    .unwrap();
    let now_keys: Vec<&String> = now.keys().collect();
    let want_keys: Vec<&String> = want.keys().collect();
    assert_eq!(now_keys, want_keys, "состав «раздел × стол» изменился");
    let mut diverged = Vec::new();
    for (k, v) in &now {
        for (part, val) in v.as_object().unwrap() {
            if Some(val) != want[k].get(part) {
                diverged.push(format!("{k} [{part}]\n  было:  {}\n  стало: {val}", want[k].get(part).unwrap_or(&Value::Null)));
            }
        }
    }
    assert!(
        diverged.is_empty(),
        "«Оформление» изменилось ({} мест) — если намеренно, обнови эталон \
         (REXON_UPDATE_GOLDEN=1):\n{}",
        diverged.len(),
        diverged.join("\n")
    );
}

/// Guard for the 15.11 defect: the golden file does not depend on the desktop the check
/// is run on. The same binary is restarted with `XDG_CURRENT_DESKTOP=X-Cinnamon`
/// (Wayland and X11) and without desktop variables — the golden check is green
/// everywhere. Previously, from a Cinnamon session, an extra `cinnamon --replace` got into
/// the plan of an "unrecognized" desktop. Nothing but the test itself is launched;
/// the child has no display.
#[test]
fn golden_does_not_depend_on_session_env() {
    if std::env::var("REXON_UPDATE_GOLDEN").is_ok() {
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let cases: [(&str, &[(&str, &str)]); 3] = [
        ("Cinnamon Wayland", &[("XDG_CURRENT_DESKTOP", "X-Cinnamon"), ("XDG_SESSION_TYPE", "wayland")]),
        ("Cinnamon X11", &[("XDG_CURRENT_DESKTOP", "X-Cinnamon"), ("XDG_SESSION_TYPE", "x11")]),
        ("без стола", &[]),
    ];
    for (label, vars) in cases {
        let mut cmd = std::process::Command::new(&exe);
        cmd.args(["appearance_plans_match_golden", "--exact", "--test-threads=1"]);
        for k in [
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_TYPE",
            "XDG_SESSION_DESKTOP",
            "DESKTOP_SESSION",
            "WAYLAND_DISPLAY",
            "DISPLAY",
            "REXON_UPDATE_GOLDEN",
        ] {
            cmd.env_remove(k);
        }
        cmd.envs(vars.iter().copied());
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "сверка с эталоном зависит от окружения ({label}):\n{}",
            String::from_utf8_lossy(&out.stdout)
        );
        // The filter must not miss: the test really passed.
        assert!(String::from_utf8_lossy(&out.stdout).contains("1 passed"), "{label}: тест не найден");
    }
}

/// Step 5a: «Применить» produces a plan ([`ai::press_plan_with`]) and executes
/// it with the common executor. The press plan is THE SAME steps of the same builders
/// as in the golden file above (order and argv), without a single launch; actions
/// that still go their own way are `Own`, "nothing" is `Nothing`.
#[test]
fn press_plan_is_the_golden_plan() {
    use rexon_core::install_plan::Step;
    let _lang = i18n_pin::en();
    let as_steps = |p: &th::Plan| p.steps.iter().cloned().map(Step::from).collect::<Vec<_>>();
    let colors_press = rexon_core::shell_palette::SchemePress {
        steps: vec![th::ApplyStep::ForceOk, th::ApplyStep::WriteSetsOk {
            dir: "/h/.config".into(),
            path: "/h/.config/kdeglobals".into(),
            content: "[UiSettings]\nColorScheme=MyTheme\n".into(),
        }],
        outcome: Default::default(),
        lxqt: false,
    };
    for cat in real_categories() {
        for de in DES {
            let action = ai::apply_action_for(Some(&cat), THEME);
            let kv = kv_env(de);
            for flag in [true, false] {
                let facts = ai::PressFacts {
                    de: de.to_string(),
                    home: HOME.to_string(),
                    xdg_config_home: None,
                    global_theme_ok: flag,
                    icons_valid: flag,
                    gtk_dark: flag,
                    gtk4: None,
                    qt_platform_theme: kv.platform_theme.to_string(),
                    qt_icon_theme: kv.icon_theme.clone(),
                    qt_desktop_installed: kv.qt_desktop_installed,
                    kwin_reconfigure: flag.then(|| vec!["dbus-send".to_string(), "reconfigure".to_string()]),
                    aurorae_library: if flag { "org.kde.kwin.aurorae.v2".into() } else { String::new() },
                    cursor_safe: flag,
                    cursor_standard: !flag,
                    cursor_size: flag.then_some(48),
                    colors: Some(if flag { Ok(colors_press.clone()) } else { Err("нет схемы".to_string()) }),
                    sound: Some(ai::SoundFacts {
                        exists: flag,
                        kwrite: "kwriteconfig6".into(),
                        kread: "kreadconfig6".into(),
                        has_gsettings: true,
                    }),
                    lock: Some(ai::LockFacts { install_dir: cat.install_dir.clone(), is_file: flag, treeland: false }),
                    desktop: (action == ApplyAction::DeSection).then(|| {
                        rexon_core::de_apply::press_plan(&Machine::new(), &de_ctx(de), categories::category_kind(&cat.key).unwrap_or(""), THEME)
                    }),
                    global: None,
                };
                let got = ai::press_plan_with(&action, THEME, &facts);
                let steps = |plan: &th::Plan, de: &str| ai::PressPlan::Steps {
                    steps: as_steps(plan),
                    history: Some((plan.history_kind, de.to_string())),
                    say: ai::PressSay::Plain,
                };
                let primary = |argv: Vec<String>| Step::from(th::ApplyStep::Primary(th::Cmd { argv, timeout: None }));
                let want = match action {
                    ApplyAction::KdeGlobal if !flag => {
                        assert!(matches!(got, ai::PressPlan::Refused(Some(ref m)) if m.contains("Plasma 5")), "{got:?}");
                        continue;
                    }
                    ApplyAction::KdeGlobal => steps(&th::apply_kde_global_plan(THEME), "kde"),
                    ApplyAction::KdePlasma => steps(&th::apply_kde_plasma_plan(THEME), "kde"),
                    ApplyAction::Icons => match th::apply_icon_plan_xdg(THEME, de, HOME, None, flag) {
                        Some(p) => steps(&p, de),
                        None => ai::PressPlan::Refused(None),
                    },
                    ApplyAction::Kvantum => steps(&th::apply_kvantum_plan(THEME, &kv), "kde"),
                    ApplyAction::Gtk => steps(&th::apply_gtk_plan_scheme(THEME, de, HOME, None, Some(flag)), de),
                    // First the frame module (`library`), then the same theme write.
                    ApplyAction::Aurorae => {
                        let library = if flag { "org.kde.kwin.aurorae.v2" } else { "org.kde.kwin.aurorae" };
                        let theme_argv = ai::aurorae_plan(THEME).primary.argv;
                        let mut library_argv = theme_argv.clone();
                        let n = library_argv.len();
                        library_argv[n - 2] = "library".into();
                        library_argv[n - 1] = library.into();
                        let mut plan = vec![
                            primary(library_argv),
                            Step::from(th::ApplyStep::IfOk(th::Cmd { argv: theme_argv, timeout: None })),
                        ];
                        if let Some(argv) = facts.kwin_reconfigure.clone() {
                            plan.push(Step::from(th::ApplyStep::IfOk(th::Cmd { argv, timeout: Some(10) })));
                        }
                        ai::PressPlan::Steps { steps: plan, history: None, say: ai::PressSay::Plain }
                    }
                    ApplyAction::WidgetStyle => ai::PressPlan::Steps {
                        steps: as_steps(&th::apply_qt_style_plan(THEME, &kv)),
                        history: None,
                        say: ai::PressSay::Plain,
                    },
                    // The theme and right away `Engine=KSplashQML` (like the KDE module).
                    ApplyAction::Splash => ai::PressPlan::Steps {
                        steps: vec![
                            primary(ai::lnf_component_plan("splash", THEME).argv),
                            Step::from(th::ApplyStep::IfOk(th::Cmd { argv: ai::splash_engine_plan(THEME).argv, timeout: None })),
                        ],
                        history: None,
                        say: ai::PressSay::Plain,
                    },
                    ApplyAction::Lockscreen => ai::PressPlan::Steps {
                        steps: vec![primary(ai::lnf_component_plan("lockscreen", THEME).argv)],
                        history: None,
                        say: ai::PressSay::Plain,
                    },
                    ApplyAction::Noop => ai::PressPlan::Nothing,
                    // Cursor: the same builders the old
                    // `apply_cursor` called (a safe one — theme with size;
                    // an unsafe one, but the desktop's default cursor — reset of
                    // the native key; otherwise refusal without writing).
                    ApplyAction::Cursor => {
                        let size = flag.then_some(48);
                        let plan = if flag {
                            th::apply_cursor_plan_sized(THEME, size, de, HOME, None, true)
                        } else {
                            th::standard_cursor_plan(de, size, HOME, None)
                                .or_else(|| th::apply_cursor_plan_sized(THEME, size, de, HOME, None, false))
                        };
                        match plan {
                            Some(p) => ai::PressPlan::Steps {
                                steps: as_steps(&p),
                                history: Some((p.history_kind, de.to_string())),
                                say: ai::PressSay::Cursor,
                            },
                            None => ai::PressPlan::Refused(None),
                        }
                    }
                    // Colors outside Plasma — `shell_palette` layer steps, in
                    // Plasma — its own way (a command, then a fix-up based on the result).
                    ApplyAction::KdeColors => match rexon_core::shell_palette::scheme_way(de) {
                        rexon_core::shell_palette::SchemeWay::Layer { .. } if flag => ai::PressPlan::Steps {
                            steps: colors_press.steps.iter().cloned().map(Step::from).collect(),
                            history: Some(("kde-colors", de.to_string())),
                            say: ai::PressSay::Done(colors_press.applied_msg()),
                        },
                        rexon_core::shell_palette::SchemeWay::Layer { .. } => {
                            ai::PressPlan::Refused(Some("нет схемы".to_string()))
                        }
                        _ => ai::PressPlan::Own,
                    },
                    // Sound — the same sound engine plan as in the golden file above:
                    // writes (they do not change `ok`), then the name read back.
                    // Before them — "what was there" for kdeglobals: it will be rewritten by
                    // `kwriteconfig6`, bypassing the rollback.
                    ApplyAction::SoundTheme if !flag => ai::PressPlan::Refused(Some(
                        rexon_core::qt_settings::sound_theme_missing_message(THEME),
                    )),
                    ApplyAction::SoundTheme => {
                        let p = rexon_core::qt_settings::sound_theme_plan(de, THEME, "kwriteconfig6", "kreadconfig6", true);
                        let mut v = vec![Step::from(th::ApplyStep::Guard { paths: vec![format!("{HOME}/.config/kdeglobals")] })];
                        v.extend(p.write.iter().map(|a| Step::from(th::ApplyStep::BestEffort(th::Cmd { argv: a.clone(), timeout: None }))));
                        v.push(Step::from(th::ApplyStep::ReadBackSetsOk {
                            read: th::Cmd { argv: p.verify.clone().unwrap(), timeout: None },
                            expect: THEME.to_string(),
                        }));
                        ai::PressPlan::Steps { steps: v, history: None, say: ai::PressSay::Plain }
                    }
                    // Lock screen wallpaper — the same plan as in the golden file above
                    // (`x11`/`missing`): the picture decides `ok`, the keys are
                    // auxiliary; before them, "what was there" for kscreenlockerrc.
                    ApplyAction::LockWallpaper => match ai::lock_wallpaper_plan_in(de, false, &cat.install_dir, THEME, flag) {
                        Err(e) => ai::PressPlan::Refused(Some(e)),
                        Ok(p) => {
                            let mut v = vec![
                                Step::from(th::ApplyStep::Guard { paths: vec![format!("{HOME}/.config/kscreenlockerrc")] }),
                                primary(p.image.argv.clone()),
                            ];
                            v.extend(p.extra.iter().map(|c| Step::from(th::ApplyStep::BestEffort(th::Cmd { argv: c.argv.clone(), timeout: None }))));
                            ai::PressPlan::Steps {
                                steps: v,
                                history: None,
                                say: ai::PressSay::Pair { done: p.success_message, failed: p.failure_message },
                            }
                        }
                    },
                    // Desktop section: theme and extension are steps of the common plan
                    // (exactly the golden file's enable steps, preceded by "what was there" for
                    // the files the commands will rewrite), Conky goes its own way.
                    ApplyAction::DeSection => {
                        let kind = categories::category_kind(&cat.key).unwrap_or("");
                        match facts.desktop.clone().unwrap() {
                            rexon_core::de_apply::Plan::Steps { steps, check } => {
                                let on = rexon_core::de_apply::enable_steps(&Machine::new(), &de_ctx(de), kind, THEME);
                                let files: Vec<ExtStep> = steps
                                    .iter()
                                    .filter_map(|s| match s {
                                        Step::File(f) => Some(f.clone()),
                                        _ => None,
                                    })
                                    .collect();
                                assert_eq!(files, on, "{} на {de:?}: шаги кнопки разошлись с шагами включения", cat.key);
                                for (i, s) in steps.iter().enumerate() {
                                    if let Step::Apply(a) = s {
                                        assert!(i == 0 && matches!(a, th::ApplyStep::Guard { .. }), "{} на {de:?}: {steps:?}", cat.key);
                                    }
                                }
                                ai::PressPlan::Desktop { steps, check }
                            }
                            rexon_core::de_apply::Plan::Refused(m) => ai::PressPlan::Refused(Some(m)),
                            rexon_core::de_apply::Plan::Own => ai::PressPlan::Own,
                        }
                    }
                    _ => ai::PressPlan::Own,
                };
                assert_eq!(got, want, "{} на {de:?}: план нажатия разошёлся с построителями эталона", cat.key);
            }
        }
    }
}

/// Sound without programs: nothing to write with — refusal before the first step; without
/// `kreadconfig6` in Plasma — the writes go through, but the result is "written, nothing
/// to verify with" (as with the old own path).
#[test]
fn sound_press_without_tools() {
    let _lang = i18n_pin::en();
    let facts = |kwrite: &str, kread: &str, gs: bool| ai::PressFacts {
        de: "kde".into(),
        home: HOME.into(),
        sound: Some(ai::SoundFacts { exists: true, kwrite: kwrite.into(), kread: kread.into(), has_gsettings: gs }),
        ..Default::default()
    };
    let got = ai::press_plan_with(&ApplyAction::SoundTheme, THEME, &facts("", "", false));
    assert!(matches!(got, ai::PressPlan::Refused(Some(ref m)) if m.contains("Nothing to write")), "{got:?}");
    let got = ai::press_plan_with(&ApplyAction::SoundTheme, THEME, &facts("kwriteconfig6", "", false));
    let ai::PressPlan::Steps { steps, say, .. } = got else { panic!("{got:?}") };
    assert!(matches!(say, ai::PressSay::Pair { ref failed, .. } if failed.contains("nothing to check")), "{say:?}");
    assert!(!steps.iter().any(|s| matches!(s, rexon_core::install_plan::Step::Apply(th::ApplyStep::ReadBackSetsOk { .. }))));
}

/// Common sections: «Применить» on their desktops does something rather than staying silent.
#[test]
fn common_sections_apply_something() {
    let cats = real_categories();
    let by_id = |id: i64| cats.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("нет раздела {id}"));
    for (id, des) in [
        (722, &["kde"][..]),
        (104, &["kde"][..]),
        (114, &["kde"][..]),
        (123, &["kde", "gnome", "xfce", "lxqt"][..]),
        (132, &["kde", "gnome", "xfce", "cinnamon", "mate", "lxqt", "sway"][..]),
        (107, &["kde", "gnome", "xfce", "cinnamon", "mate", "lxqt", "sway"][..]),
        (135, &["kde", "gnome", "xfce", "cinnamon", "mate", "lxqt", "sway"][..]),
    ] {
        let cat = by_id(id);
        for de in des {
            let action = ai::apply_action_for(Some(cat), THEME);
            assert_ne!(action, ApplyAction::Noop, "{} на {de}: «Применить» ничего не делает", cat.key);
            let plan = apply_plan(&action, cat, de, THEME);
            assert!(!plan.is_null(), "{} на {de}: плана нет", cat.key);
            let s = plan.to_string();
            assert!(!s.contains("\"steps\":[]"), "{} на {de}: пустой план: {s}", cat.key);
        }
    }
}

/// Removal by the log takes away exactly the recorded paths — for every section
/// whose installation succeeded.
#[test]
fn uninstall_by_record_removes_every_recorded_path() {
    let home = Path::new(HOME);
    let fs = archive_snapshot();
    for cat in real_categories() {
        for root in ["/x", "/x/MyTheme"] {
        let out = ai::plan_install(home, &fs, &cat, root);
        let Some(rec) = out.record else { continue };
        let steps = ai::plan_uninstall(&rec.paths, &|_| true, &|_| false);
        let removed: Vec<&String> = steps
            .iter()
            .filter_map(|s| match s {
                ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => Some(path),
                _ => None,
            })
            .collect();
        for p in &rec.paths {
            assert!(removed.contains(&p), "{}: путь журнала не снимается: {p}", cat.key);
        }
        // Removal does not go beyond what was recorded.
        assert_eq!(removed.len(), rec.paths.len(), "{}: лишнее удаление {steps:?}", cat.key);
        }
    }
}

/// Reinstalling one's own theme (paths from the log of the same purchase)
/// succeeds and puts down the same paths.
#[test]
fn reinstall_over_own_record_keeps_paths() {
    let home = Path::new(HOME);
    let fs = archive_snapshot();
    for cat in real_categories() {
        for root in ["/x", "/x/MyTheme"] {
        let first = ai::plan_install(home, &fs, &cat, root);
        let Some(rec) = first.record.clone() else { continue };
        let again = ai::plan_install_with(home, &fs, &cat, root, Some(&rec.paths));
        assert!(again.ok, "{} ({root}): повтор отказал: {}", cat.key, again.message);
        assert_eq!(again.record.map(|r| r.paths), Some(rec.paths), "{} ({root}): повтор кладёт в другое место", cat.key);
        }
    }
}
