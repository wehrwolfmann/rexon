//! Desktop customization matrix: consistency of the tables and install plans.
//!
//! The module has no Python golden file (this is new functionality, not a port),
//! so what is checked is not a byte match but two properties:
//!   * all parallel type tables are updated (rule #12) — a type from the matrix
//!     must be in `VALID_TYPES`, in the «Appearance» category, and have a path,
//!     a label, an icon and a gradient, with the path THE SAME as in the matrix;
//!   * the install/removal plan does exactly what the matrix row promises:
//!     puts things into the right directory under the right name, enables with the right command and
//!     refreshes the desktop only where that is safe.

mod i18n_pin;

use std::collections::HashMap;

use rexon_core::constants;
use rexon_core::de_customization::{self as dec, De, Support};
use rexon_core::ext_manager::{
    safe_install_plan, safe_uninstall_plan, ExtEnv, ExtInfo, ExtStep, FsSnap, TmpAlloc,
};
use serde_json::{json, Map, Value};

// ============================================================================
// Table consistency
// ============================================================================

#[test]
fn every_matrix_kind_is_registered_everywhere() {
    for kind in dec::kinds() {
        assert!(constants::is_valid_type(kind), "{kind} is missing from VALID_TYPES");
        assert!(constants::is_appearance(kind), "{kind} is missing from the «Appearance» category");
        assert!(!constants::is_scripted(kind), "{kind} must not be a script type");
        assert!(constants::type_label(kind).is_some(), "{kind} has no label");
        assert!(constants::type_icon(kind).is_some(), "{kind} has no icon");
        assert!(constants::type_gradient(kind).is_some(), "{kind} has no gradient");
    }
}

/// `VALID_TYPES` is now DERIVED from the matrix. Its contents must stay the same:
/// the list below is a snapshot of what existed before the two tables were merged (16 "safe"
/// decoration types + adaptive wallpapers + 16 desktop customization types + 2
/// script ones). A mismatch = a lost or an arbitrarily added type.
#[test]
fn valid_types_derived_from_matrix_keeps_composition() {
    const BEFORE: &[&str] = &[
        "theme-gtk", "theme-icons", "theme-cursors", "theme-sounds",
        "theme-kde-global", "theme-kde-colors", "theme-kde-plasma",
        "theme-kvantum", "theme-window-decoration", "theme-login-screen",
        "theme-plymouth", "theme-conky", "theme-bundle", "theme-plasmoid",
        "wallpaper-pack", "font-pack", "wallpaper-video-adaptive",
        "kde-kwin-script", "kde-kwin-effect", "kde-window-switcher",
        "kde-wallpaper-plugin", "theme-kde-splash", "theme-kde-lockscreen",
        "gnome-shell-extension", "theme-gnome-shell", "cinnamon-applet",
        "cinnamon-desklet", "cinnamon-extension", "theme-cinnamon",
        "nemo-action", "theme-xfwm4", "theme-marco", "theme-lxqt",
        // 01.11.2026: Openbox and labwc frames (parity item 5.10).
        "theme-openbox",
        "desktop-tool", "plugin",
    ];
    let mut got = constants::valid_types();
    let mut want = BEFORE.to_vec();
    got.sort_unstable();
    want.sort_unstable();
    assert_eq!(got, want, "the set of VALID_TYPES changed");

    // The «Appearance» category = the whole matrix, script types are not part of it.
    let mut appearance = constants::category_appearance();
    appearance.sort_unstable();
    let mut want_app: Vec<&str> =
        BEFORE.iter().copied().filter(|t| !constants::is_scripted(t)).collect();
    want_app.sort_unstable();
    assert_eq!(appearance, want_app);
}

#[test]
fn install_paths_match_matrix() {
    for kind in dec::kinds() {
        let row = dec::row(kind).unwrap();
        let expected = if row.dest.is_empty() { None } else { Some(row.dest) };
        assert_eq!(
            constants::install_path_suffix(kind),
            expected,
            "the path of {kind} diverged from the matrix"
        );
    }
    // A wallpaper set is applied straight from the package — it has no install directory
    // (that was so in the original INSTALL_PATHS too).
    assert_eq!(constants::install_path_suffix("wallpaper-pack"), None);
    assert_eq!(constants::install_path_suffix("plugin"), None);
}

/// Catalog filter: the buyer is not shown what physically does not work in their
/// desktop — but what does work is not taken away either.
#[test]
fn matrix_filters_by_desktop() {
    let kinds_in = |de: De| -> Vec<&'static str> {
        dec::rows_for(de).iter().map(|r| r.kind).collect()
    };
    // GNOME has no plasmoids, but it has shell extensions.
    let gnome = kinds_in(De::Gnome);
    assert!(!gnome.contains(&"theme-plasmoid"));
    assert!(gnome.contains(&"gnome-shell-extension"));
    // KDE has both of its own, and the GTK theme (Firefox, GIMP, kde-gtk-config).
    let kde = kinds_in(De::Kde);
    assert!(kde.contains(&"theme-plasmoid"));
    assert!(kde.contains(&"theme-gtk"));
    assert!(!kde.contains(&"gnome-shell-extension"));
    // Budgie is themed with a GTK theme — it must be there.
    assert!(kinds_in(De::Budgie).contains(&"theme-gtk"));
    // Tiling ones: wallpapers and fonts yes, KDE widgets no.
    let tiling = kinds_in(De::Wlroots);
    assert!(tiling.contains(&"wallpaper-pack") && tiling.contains(&"font-pack"));
    assert!(!tiling.contains(&"theme-plasmoid"));
}

#[test]
fn unsupported_rows_explain_themselves() {
    let blocked: Vec<&dec::Row> = dec::ROWS
        .iter()
        .filter(|r| !r.support.installable())
        .collect();
    // Native panel plugins of XFCE/MATE/Budgie/LXQt, the GDM theme, tiling configs,
    // COSMIC themes — all seven rows must carry a reason.
    assert!(blocked.len() >= 6);
    for r in blocked {
        let reason = r.support.reason().expect("refusal reason");
        assert!(reason.len() > 20, "the explanation is too short: {reason}");
    }
}

#[test]
fn each_desktop_is_covered() {
    for de in [De::Kde, De::Gnome, De::Cinnamon, De::Xfce, De::Mate, De::Lxqt] {
        assert!(!dec::rows_hosted_by(de).is_empty(), "{} has no rows", de.label());
    }
    // Cinnamon — three DIFFERENT kinds of add-ons plus the theme and Nemo actions.
    assert_eq!(dec::rows_hosted_by(De::Cinnamon).len(), 5);
}

// ============================================================================
// Documentation — from the same table
// ============================================================================

/// Build `docs/de_customization_matrix.md` from the matrix.
fn render_doc() -> String {
    let mut s = String::new();
    s.push_str(
        "# Customizing Linux desktops: a per-environment breakdown\n\n\
         The full map of what each desktop is customized with, where it is \
         put, what enables it and what has to be restarted. The table is the working \
         basis of the universal `.rxn`: Rexon installs and enables the types from the \
         \"`.rxn` type\" column itself.\n\n\
         > This file is GENERATED from `rust/rexon-core/src/de_customization.rs`\n\
         > (test `matrix_doc_is_up_to_date`). Edit the matrix, not this\n\
         > text: to update — `REXON_UPDATE_DOC=1 cargo test -p rexon-core\n\
         > --test de_customization`.\n",
    );

    // Common directories: where to put things is the same everywhere. The "Works in" column
    // answers the SECOND question of the matrix: a common directory does not mean "in all
    // desktops" (the Kvantum theme).
    s.push_str("\n## Common directory (installed the same way everywhere)\n\n");
    s.push_str(
        "| `.rxn` type | What it is | Where to put it | Works in | How to enable |\n\
         |---|---|---|---|---|\n",
    );
    for r in dec::rows_hosted_by(De::Any) {
        let path = if r.dest.is_empty() {
            "— (applied in place)".to_string()
        } else {
            format!("`~/{}`", r.dest)
        };
        s.push_str(&format!(
            "| `{}` | {} | {path} | {} | {} |\n",
            r.kind,
            r.what,
            squash(&r.applies.hint()),
            r.activate.hint(),
        ));
    }
    let common_notes: Vec<&dec::Row> = dec::rows_hosted_by(De::Any)
        .into_iter()
        .filter(|r| !r.note.is_empty())
        .collect();
    if !common_notes.is_empty() {
        s.push_str("\nPitfalls:\n\n");
        for r in common_notes {
            s.push_str(&format!(
                "* **{}** — {}\n",
                constants::type_label(r.kind).unwrap_or_else(|| r.kind.to_string()),
                squash(r.note)
            ));
        }
    }

    // Caveats: the type works, but under a condition. That is NOT the same as "does not work"
    // — the catalog section stays in place, and the condition is named to the person.
    let caveats: Vec<(&dec::Row, dec::Caveat)> = dec::ROWS
        .iter()
        .filter_map(|r| match r.applies {
            dec::Applies::EverywhereWith(c) | dec::Applies::EverywhereWithExcept(_, c) => Some((r, c)),
            _ => None,
        })
        .collect();
    if !caveats.is_empty() {
        s.push_str("\nWorks, but with a caveat:\n\n");
        for (r, c) in caveats {
            s.push_str(&format!(
                "* **{}** ({}) — {}\n",
                constants::type_label(r.kind).unwrap_or_else(|| r.kind.to_string()),
                c.list()
                    .into_iter()
                    .map(|d| d.display_label())
                    .collect::<Vec<_>>()
                    .join(", "),
                squash(&c.text())
            ));
        }
    }

    for de in [
        De::Kde,
        De::Gnome,
        De::Cinnamon,
        De::Xfce,
        De::Mate,
        De::Budgie,
        De::Lxqt,
        De::Cosmic,
        De::Wlroots,
    ] {
        let rows = dec::rows_hosted_by(de);
        if rows.is_empty() {
            continue;
        }
        s.push_str(&format!("\n## {}\n", de.label()));

        let supported: Vec<&dec::Row> =
            rows.iter().copied().filter(|r| r.support.installable()).collect();
        if !supported.is_empty() {
            s.push('\n');
            s.push_str(
                "| `.rxn` type | What it is | Where to put it | How to enable | What to restart |\n\
                 |---|---|---|---|---|\n",
            );
            for r in &supported {
                s.push_str(&format!(
                    "| `{}` | {} | `~/{}` | {} | {} |\n",
                    r.kind,
                    r.what,
                    r.dest,
                    r.activate.hint(),
                    r.refresh.hint(),
                ));
            }
            let notes: Vec<&&dec::Row> =
                supported.iter().filter(|r| !r.note.is_empty()).collect();
            if !notes.is_empty() {
                s.push_str("\nPitfalls:\n\n");
                for r in notes {
                    s.push_str(&format!(
                        "* **{}** — {}\n",
                        constants::type_label(r.kind).unwrap_or_else(|| r.kind.to_string()),
                        squash(r.note)
                    ));
                }
            }
        }

        let blocked: Vec<&dec::Row> =
            rows.iter().copied().filter(|r| !r.support.installable()).collect();
        if !blocked.is_empty() {
            s.push_str("\nNot distributed via `.rxn`:\n\n");
            for r in blocked {
                s.push_str(&format!(
                    "* **{}** (`{}`) — {}.{}\n",
                    r.what,
                    r.dest,
                    squash(r.support.reason().unwrap()),
                    if r.note.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", squash(r.note))
                    }
                ));
            }
        }
    }
    s
}

/// Collapse a source line break into one space (in Rust the table strings are
/// wrapped by width, in markdown they must be whole).
fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The documentation must not lag behind the table: it is updated by the same test with
/// `REXON_UPDATE_DOC=1`.
#[test]
fn matrix_doc_is_up_to_date() {
    // The document is for developers and is written in English (the source language of the
    // catalog), so it needs no compiled `po/ru.po`. The matrix row fields (`what`, `note`,
    // activation/restart hints, refusal reasons) are not catalog strings and come out as
    // written in `src/de_customization.rs`.
    let _lang = i18n_pin::en();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/de_customization_matrix.md");
    let doc = render_doc();
    if std::env::var("REXON_UPDATE_DOC").is_ok() {
        std::fs::write(path, &doc).expect("writing the document");
        return;
    }
    let on_disk = std::fs::read_to_string(path).expect("docs/de_customization_matrix.md");
    assert_eq!(
        on_disk, doc,
        "the document is behind the matrix — update it: REXON_UPDATE_DOC=1 cargo test \
         -p rexon-core --test de_customization"
    );
}

// ============================================================================
// Stand for the plans
// ============================================================================

struct Case {
    fs: FsSnap,
    which: HashMap<String, String>,
    ok: bool,
    wayland: bool,
}

impl Case {
    fn new(snapshot: Value, tools: &[&str], ok: bool, wayland: bool) -> Self {
        Case {
            fs: FsSnap::from_json(&snapshot),
            which: tools
                .iter()
                .map(|t| ((*t).to_string(), format!("/usr/bin/{t}")))
                .collect(),
            ok,
            wayland,
        }
    }

    /// The stand's running desktop: Cinnamon if `cinnamon` is among the programs
    /// (restarting the shell — only in its own session), otherwise unrecognized.
    fn xdg(&self) -> &'static str {
        if self.which.contains_key("cinnamon") { "X-Cinnamon" } else { "" }
    }

    fn plan(&self, manifest: &Map<String, Value>, uninstall: bool) -> Vec<ExtStep> {
        let tmp = TmpAlloc::new("/tmp/t");
        let which = |n: &str| self.which.get(n).cloned();
        let run_ok = |_: &[String]| self.ok;
        let env = ExtEnv {
            home: "/home/t",
            fs: &self.fs,
            which: &which,
            run_ok: &run_ok,
            xdg_current_desktop: self.xdg(),
            xdg_session_type: if self.wayland { "wayland" } else { "x11" },
            wayland_display: self.wayland,
            stage_helper_available: false,
            system_helper_path: "/usr/lib/rexon/helper",
            system_helper_exists: false,
            helper_family: "arch",
            tmp: &tmp,
        };
        let ext = ExtInfo { dir: "/ext", manifest };
        if uninstall {
            safe_uninstall_plan(&env, &ext)
        } else {
            safe_install_plan(&env, &ext)
        }
    }

    /// Steps of the desktop section's «Apply»/«Turn on» button — what
    /// the installation did before 04.10.2026.
    fn enable(&self, kind: &str, name: &str) -> Vec<ExtStep> {
        let tmp = TmpAlloc::new("/tmp/t");
        let which = |n: &str| self.which.get(n).cloned();
        let run_ok = |_: &[String]| self.ok;
        let env = ExtEnv {
            home: "/home/t",
            fs: &self.fs,
            which: &which,
            run_ok: &run_ok,
            xdg_current_desktop: self.xdg(),
            xdg_session_type: if self.wayland { "wayland" } else { "x11" },
            wayland_display: self.wayland,
            stage_helper_available: false,
            system_helper_path: "/usr/lib/rexon/helper",
            system_helper_exists: false,
            helper_family: "arch",
            tmp: &tmp,
        };
        rexon_core::ext_manager::de_enable_steps(&env, kind, name)
    }
}

fn manifest(id: &str, t: &str) -> Map<String, Value> {
    json!({"id": id, "type": t}).as_object().unwrap().clone()
}

/// Snapshot: `data/<sub>` with a metadata file of the given contents.
fn snap_with_meta(sub: &str, meta_name: &str, meta_text: &str, extra_dirs: &[&str]) -> Value {
    let mut dirs = json!({
        "/ext": ["data"],
        "/ext/data": [sub],
        format!("/ext/data/{sub}"): [meta_name],
    });
    for d in extra_dirs {
        dirs[format!("/ext/data/{sub}/{d}")] = json!([]);
    }
    json!({
        "dirs": dirs,
        "files": { format!("/ext/data/{sub}/{meta_name}"): {"size": 1, "text": meta_text} },
    })
}

fn argvs(steps: &[ExtStep]) -> Vec<Vec<String>> {
    steps
        .iter()
        .filter_map(|s| match s {
            ExtStep::Run { argv, .. } | ExtStep::Spawn { argv } => Some(argv.clone()),
            _ => None,
        })
        .collect()
}

fn copies(steps: &[ExtStep]) -> Vec<(String, String)> {
    steps
        .iter()
        .filter_map(|s| match s {
            ExtStep::CopyTree { src, dest } | ExtStep::CopyFile { src, dest } => {
                Some((src.clone(), dest.clone()))
            }
            _ => None,
        })
        .collect()
}

// ============================================================================
// Install plans
// ============================================================================

/// GNOME: the destination directory is named by the uuid from metadata.json, NOT by the
/// folder name in the archive — otherwise the shell will not find the extension.
#[test]
fn gnome_extension_installs_under_uuid_and_enables() {
    let case = Case::new(
        snap_with_meta(
            "my-extension-v3",
            "metadata.json",
            r#"{"uuid": "my@ext.example.com", "shell-version": ["47", "48"]}"#,
            &["schemas"],
        ),
        &["gnome-extensions", "glib-compile-schemas"],
        true,
        true,
    );
    let steps = case.plan(&manifest("my-ext", "gnome-shell-extension"), false);

    assert_eq!(
        copies(&steps),
        vec![(
            "/ext/data/my-extension-v3".to_string(),
            "/home/t/.local/share/gnome-shell/extensions/my@ext.example.com".to_string(),
        )]
    );
    // Installation only puts files (decision of 04.10.2026), the button enables.
    assert!(argvs(&steps).is_empty(), "{steps:?}");
    let on = case.enable("gnome-shell-extension", "my@ext.example.com");
    let script = &argvs(&on)[0][2];
    assert!(script.contains("gnome-extensions enable 'my@ext.example.com'"));
    assert!(script.contains("glib-compile-schemas"));
    assert!(script.contains("/home/t/.local/share/gnome-shell/extensions/my@ext.example.com/schemas"));
    // We do not restart the GNOME shell — that is impossible on Wayland.
    assert!(!argvs(&on).iter().any(|a| a.iter().any(|s| s.contains("gnome-shell --replace"))));
}

/// Cinnamon: an applet is appended as a line to the list with an instance counter,
/// and on X11 the shell restarts by itself.
#[test]
fn cinnamon_applet_enables_and_restarts_on_x11() {
    let case = Case::new(
        snap_with_meta("weather", "metadata.json", r#"{"uuid": "weather@mockturtl"}"#, &[]),
        &["cinnamon"],
        true,
        false,
    );
    let steps = case.plan(&manifest("weather", "cinnamon-applet"), false);

    assert_eq!(
        copies(&steps)[0].1,
        "/home/t/.local/share/cinnamon/applets/weather@mockturtl"
    );
    assert!(argvs(&steps).is_empty() && !steps.iter().any(|s| matches!(s, ExtStep::Spawn { .. })));
    let on = case.enable("cinnamon-applet", "weather@mockturtl");
    let a = argvs(&on);
    assert!(a[0][2].contains("next-applet-id $((id + 1))"));
    // Like the Cinnamon settings window (Spices.enable_extension): a new applet goes to
    // position zero of the first panel's right zone, the zone neighbours shift.
    assert!(a[0][2].contains("panel1:right:0:weather@mockturtl:$id"), "at the start of the right zone");
    assert!(a[0][2].contains("$1==\"\\047panel1\" && $2==\"right\""), "only the right zone of the first panel is shifted");
    assert!(a[0][2].contains("{$3=$3+1}"), "zone neighbours are shifted by one");
    // The instance number is not less than "the largest taken + 1".
    assert!(a[0][2].contains("if [ \"$id\" -le \"$max\" ]; then id=$((max + 1)); fi"), "the number is not taken");
    assert_eq!(a.last().unwrap(), &vec!["/usr/bin/cinnamon".to_string(), "--replace".to_string()]);
}

/// On Wayland restarting Cinnamon is impossible — there must be no such step.
#[test]
fn cinnamon_applet_does_not_restart_on_wayland() {
    let case = Case::new(
        snap_with_meta("weather", "metadata.json", r#"{"uuid": "weather@mockturtl"}"#, &[]),
        &["cinnamon"],
        true,
        true,
    );
    let steps = case.plan(&manifest("weather", "cinnamon-applet"), false);
    assert!(!steps.iter().any(|s| matches!(s, ExtStep::Spawn { .. })));
    let on = case.enable("cinnamon-applet", "weather@mockturtl");
    assert!(!on.iter().any(|s| matches!(s, ExtStep::Spawn { .. })));
}

/// KDE: the package is installed with its own tool, enabled with a key by KPlugin.Id
/// (not by the folder name) and applied through reconfigure.
#[test]
fn kwin_script_uses_kpackagetool_and_plugin_id() {
    let case = Case::new(
        snap_with_meta(
            "krohnkite-src",
            "metadata.json",
            r#"{"KPlugin": {"Id": "krohnkite"}}"#,
            &[],
        ),
        &["kpackagetool6", "qdbus6"],
        true,
        true,
    );
    let steps = case.plan(&manifest("krohnkite", "kde-kwin-script"), false);
    let a = argvs(&steps);

    assert_eq!(a[0][..4], ["/usr/bin/kpackagetool6", "--type", "KWin/Script", "-u"]);
    assert_eq!(a[1][..4], ["/usr/bin/kpackagetool6", "--type", "KWin/Script", "-i"]);
    // The tool succeeded — there must be no copying; no enabling either.
    assert!(copies(&steps).is_empty());
    assert_eq!(a.len(), 2, "{a:?}");
    // The owner mark also on a package installed by the tool: removal
    // recognizes its own by it.
    assert!(steps.iter().any(|s| matches!(s, ExtStep::WriteFile { path, content }
        if path == "/home/t/.local/share/kwin/scripts/krohnkite/.rexon-owner" && content == "krohnkite\n")), "{steps:?}");
    let on = argvs(&case.enable("kde-kwin-script", "krohnkite"));
    assert_eq!(
        on[0],
        vec![
            "kwriteconfig6", "--file", "kwinrc", "--group", "Plugins",
            "--key", "krohnkiteEnabled", "true",
        ]
    );
    assert_eq!(on[1], vec!["/usr/bin/qdbus6", "org.kde.KWin", "/KWin", "reconfigure"]);
}

/// A KWin script with the same KPlugin.Id is already installed and was not put there by this package
/// (no mark — by hand or by an earlier Rexon): no `kpackagetool -u/-i` on
/// install, no `-r` on removal, no copy — only a note with the reason.
#[test]
fn kwin_script_never_replaces_foreign_package() {
    english();
    let mut snapshot =
        snap_with_meta("src", "metadata.json", r#"{"KPlugin": {"Id": "myscript"}}"#, &[]);
    snapshot["dirs"]["/home/t/.local/share/kwin/scripts/myscript"] = json!(["main.js"]);
    snapshot["files"]["/home/t/.local/share/kwin/scripts/myscript/main.js"] = json!({"size": 4, "text": "own"});
    let case = Case::new(snapshot, &["kpackagetool6", "qdbus6"], true, true);
    let m = manifest("myscript", "kde-kwin-script");
    let install = case.plan(&m, false);
    let kpt = |steps: &[ExtStep]| {
        steps.iter().any(|s| matches!(s, ExtStep::Run { argv, .. } if argv[0].ends_with("kpackagetool6")))
    };
    assert!(!kpt(&install) && copies(&install).is_empty(), "the installation touched a foreign script: {install:?}");
    // The package has one script, and it is skipped — the installation refuses entirely
    // (a refusal step with the reason), not a "success" without a single file put down.
    assert!(
        matches!(install.as_slice(), [ExtStep::Refuse { text }] if text.contains("skipped") && text.contains("myscript")),
        "{install:?}"
    );
    let uninstall = case.plan(&m, true);
    assert!(!kpt(&uninstall), "the removal removed a foreign script: {uninstall:?}");
    assert!(!uninstall.iter().any(|s| matches!(s, ExtStep::RemoveTree { .. })), "{uninstall:?}");
}

/// If kpackagetool finished with an error — we fall back to copying rather than silently
/// "installing" nothing.
#[test]
fn kwin_script_falls_back_to_copy() {
    let case = Case::new(
        snap_with_meta("src", "metadata.json", r#"{"KPlugin": {"Id": "myscript"}}"#, &[]),
        &["kpackagetool6"],
        false,
        true,
    );
    let steps = case.plan(&manifest("myscript", "kde-kwin-script"), false);
    assert_eq!(
        copies(&steps)[0].1,
        "/home/t/.local/share/kwin/scripts/myscript"
    );
}

/// XFCE: the window theme goes into ~/.themes, the xfconf-query button enables it.
#[test]
fn xfwm4_theme_copies_to_themes_and_switches() {
    let snapshot = json!({
        "dirs": {
            "/ext": ["data"],
            "/ext/data": ["Arc-Dark"],
            "/ext/data/Arc-Dark": ["xfwm4"],
            "/ext/data/Arc-Dark/xfwm4": ["themerc"],
        },
        "files": {"/ext/data/Arc-Dark/xfwm4/themerc": {"size": 1, "text": ""}},
    });
    let case = Case::new(snapshot, &["xfconf-query"], true, false);
    let steps = case.plan(&manifest("arc", "theme-xfwm4"), false);

    assert_eq!(copies(&steps)[0].1, "/home/t/.themes/Arc-Dark");
    assert!(argvs(&steps).is_empty(), "{steps:?}");
    assert_eq!(
        argvs(&case.enable("theme-xfwm4", "Arc-Dark"))[0],
        vec!["xfconf-query", "-c", "xfwm4", "-p", "/general/theme", "-n", "-t", "string", "-s", "Arc-Dark"]
    );
}

/// A Nemo action is a separate file, nothing to enable.
#[test]
fn nemo_action_copies_file_without_activation() {
    let snapshot = json!({
        "dirs": {"/ext": ["data"], "/ext/data": ["open-here.nemo_action", "readme.txt"]},
        "files": {
            "/ext/data/open-here.nemo_action": {"size": 1, "text": ""},
            "/ext/data/readme.txt": {"size": 1, "text": ""},
        },
    });
    let case = Case::new(snapshot, &[], true, false);
    let steps = case.plan(&manifest("open-here", "nemo-action"), false);

    assert_eq!(
        copies(&steps),
        vec![(
            "/ext/data/open-here.nemo_action".to_string(),
            "/home/t/.local/share/nemo/actions/open-here.nemo_action".to_string(),
        )]
    );
    assert!(argvs(&steps).is_empty());
}

/// LXQt: the theme is enabled with a key in the personal lxqt.conf, we do not touch the panel.
#[test]
fn lxqt_theme_writes_config_key() {
    let snapshot = json!({
        "dirs": {
            "/ext": ["data"],
            "/ext/data": ["Frost"],
            "/ext/data/Frost": ["lxqt-panel.qss"],
        },
        "files": {"/ext/data/Frost/lxqt-panel.qss": {"size": 1, "text": ""}},
    });
    let case = Case::new(snapshot, &[], true, false);
    let steps = case.plan(&manifest("frost", "theme-lxqt"), false);

    assert_eq!(copies(&steps)[0].1, "/home/t/.local/share/lxqt/themes/Frost");
    assert!(argvs(&steps).is_empty(), "{steps:?}");
    let on = case.enable("theme-lxqt", "Frost");
    let script = &argvs(&on)[0][2];
    assert!(script.contains("/home/t/.config/lxqt/lxqt.conf"));
    assert!(script.contains("theme=Frost"));
}

// ============================================================================
// Removal plans
// ============================================================================

/// Lock screen: the key must be reset BEFORE removing the directory, otherwise
/// KDE is left with a link to nowhere and the lock screen will not build.
#[test]
fn lockscreen_uninstall_resets_key_before_removing() {
    let snapshot = json!({
        "dirs": {
            "/ext": ["data"],
            "/ext/data": ["org.rexon.lock"],
            "/ext/data/org.rexon.lock": ["contents"],
            "/ext/data/org.rexon.lock/contents": [],
            "/home/t/.local/share/plasma/look-and-feel/org.rexon.lock": [".rexon-owner"],
        },
        // The owner mark: the directory was put by this package (removal does not take away a foreign one).
        "files": {
            "/home/t/.local/share/plasma/look-and-feel/org.rexon.lock/.rexon-owner": {"size": 5, "text": "lock\n"},
        },
    });
    let case = Case::new(snapshot, &["kwriteconfig6"], true, true);
    let steps = case.plan(&manifest("lock", "theme-kde-lockscreen"), true);

    let reset_at = steps
        .iter()
        .position(|s| matches!(s, ExtStep::Run { argv, .. } if argv[2].contains("kscreenlockerrc")))
        .expect("lock screen key reset");
    let remove_at = steps
        .iter()
        .position(|s| matches!(s, ExtStep::RemoveTree { .. }))
        .expect("directory removal");
    assert!(reset_at < remove_at, "the directory was removed before the key reset");
}

/// Removing a GNOME extension: disable and remove from the list, then delete.
#[test]
fn gnome_extension_uninstall_disables_and_removes() {
    let mut snapshot = snap_with_meta(
        "my-extension-v3",
        "metadata.json",
        r#"{"uuid": "my@ext.example.com"}"#,
        &[],
    );
    let dest = "/home/t/.local/share/gnome-shell/extensions/my@ext.example.com";
    snapshot["dirs"][dest] = json!([".rexon-owner"]);
    snapshot["files"][format!("{dest}/.rexon-owner")] = json!({"size": 7, "text": "my-ext\n"});
    let case = Case::new(snapshot, &["gnome-extensions"], true, true);
    let steps = case.plan(&manifest("my-ext", "gnome-shell-extension"), true);

    let script = &argvs(&steps)[0][2];
    assert!(script.contains("gnome-extensions disable 'my@ext.example.com'"));
    assert!(script.contains("gsettings set org.gnome.shell enabled-extensions"));
    assert!(steps.iter().any(|s| matches!(
        s,
        ExtStep::RemoveTree { path, .. }
            if path == "/home/t/.local/share/gnome-shell/extensions/my@ext.example.com"
    )));
}

/// A KWin script must be unloaded explicitly on removal: checked live —
/// `reconfigure` does not unload a removed script, it keeps running until the end of the session.
#[test]
fn kwin_script_uninstall_unloads_script() {
    let mut snapshot =
        snap_with_meta("src", "metadata.json", r#"{"KPlugin": {"Id": "myscript"}}"#, &[]);
    let dest = "/home/t/.local/share/kwin/scripts/myscript";
    snapshot["dirs"][dest] = json!([".rexon-owner"]);
    snapshot["files"][format!("{dest}/.rexon-owner")] = json!({"size": 9, "text": "myscript\n"});
    let case = Case::new(snapshot, &["kpackagetool6", "qdbus6"], true, true);
    let steps = case.plan(&manifest("myscript", "kde-kwin-script"), true);
    let a = argvs(&steps);

    assert!(a.iter().any(|x| x
        == &vec![
            "/usr/bin/qdbus6", "org.kde.KWin", "/Scripting", "unloadScript", "myscript"
        ]));
    // Order: disable → unload → remove the package → delete the files.
    let unload = a.iter().position(|x| x.contains(&"unloadScript".to_string())).unwrap();
    let remove = a.iter().position(|x| x.contains(&"-r".to_string())).unwrap();
    assert!(unload < remove);
}

/// The Cinnamon theme is reset only if exactly it is selected — removing our package
/// must not break the user's other choice.
#[test]
fn cinnamon_theme_uninstall_resets_only_own_theme() {
    let snapshot = json!({
        "dirs": {
            "/ext": ["data"],
            "/ext/data": ["Mint-Y-Dark"],
            "/ext/data/Mint-Y-Dark": ["cinnamon"],
            "/ext/data/Mint-Y-Dark/cinnamon": [],
            "/home/t/.themes/Mint-Y-Dark": [],
        },
        "files": {},
    });
    let case = Case::new(snapshot, &[], true, false);
    let steps = case.plan(&manifest("mint-y", "theme-cinnamon"), true);
    let script = &argvs(&steps)[0][2];
    assert!(script.contains("if [ \"$cur\" = 'Mint-Y-Dark' ]"));
    assert!(script.contains("gsettings reset org.cinnamon.theme name"));
}

/// Rows that are not installed through .rxn do not get into the installer at all.
#[test]
fn blocked_kinds_have_no_plan() {
    for r in dec::ROWS {
        if let Support::Rxn = r.support {
            continue;
        }
        assert!(dec::row(r.kind).is_none());
    }
}

// ============================================================================
// Installation ≠ applying
// ============================================================================

/// ⛔ Defect of 03.10.2026: installing an `.rxn` with a cursor theme silently wrote
/// `~/.icons/default/index.theme` (`Inherits=<theme>`) — the cursor of X11 programs
/// changed right away, before «Apply». In the same way, colors,
/// the Plasma style, the global theme, Kvantum, KDE window frames, the splash and the lock
/// screen were applied on install. The guard is common: for EVERY type enabled by the
/// «Apply» button of the «Appearance» tab, and in EVERY desktop, the install plan
/// only puts files into the type's directory — not a single settings write, not a single
/// enable command. A new type with a button falls under the guard by itself.
#[test]
fn rxn_install_never_applies_what_the_button_applies() {
    use rexon_core::appearance_installer::applied_by_button;

    let button_kinds: Vec<&str> =
        dec::kinds().into_iter().filter(|k| applied_by_button(k)).collect();
    // The composition — so that the guard does not silently become empty if the link between sections and types
    // breaks.
    for must in [
        "theme-cursors", "theme-icons", "theme-gtk", "theme-sounds",
        "theme-kvantum", "theme-kde-global", "theme-kde-colors",
        "theme-kde-plasma", "theme-window-decoration", "theme-kde-splash",
        "theme-kde-lockscreen",
        // Decision of 04.10.2026: all types that the installation itself used to enable
        // have their own button — «Apply», «Turn on» or «▶️ Start».
        "theme-xfwm4", "theme-marco", "theme-gnome-shell", "theme-cinnamon",
        "theme-lxqt", "theme-openbox", "kde-window-switcher", "kde-kwin-script", "kde-kwin-effect",
        "gnome-shell-extension", "cinnamon-applet", "cinnamon-desklet",
        "cinnamon-extension", "nemo-action", "theme-conky", "theme-login-screen",
        "theme-plymouth",
    ] {
        assert!(button_kinds.contains(&must), "{must}: no «Apply» button?");
    }
    // Plasma widgets, fonts and the wallpaper plugin have nothing to enable with (the choice is in the
    // desktop itself) — they have no button.
    for not in ["theme-plasmoid", "font-pack", "kde-wallpaper-plugin"] {
        assert!(!applied_by_button(not), "{not}: unexpected «Apply» button");
    }

    // A package that looks like a theme of any kind: a subfolder with cursors, look-and-feel
    // contents and metadata plus a color scheme file.
    // For desktop types — subdirectories of frames and shells as well, a uuid in
    // the metadata, an LXQt panel, a Conky file and a Nemo action; the Conky directory is already
    // "copied" — the former installation wrote autostart for it and launched it.
    let snapshot = json!({
        "dirs": {
            "/ext": ["data", "manifest.json"],
            "/ext/data": ["MyTheme", "MyTheme.colors", "my.nemo_action"],
            "/ext/data/MyTheme": [
                "cursors", "contents", "index.theme", "metadata.json", "xfwm4",
                "metacity-1", "gnome-shell", "cinnamon", "lxqt-panel.qss", "conky.conf", "openbox-3",
            ],
            "/ext/data/MyTheme/cursors": [],
            "/ext/data/MyTheme/contents": [],
            "/ext/data/MyTheme/xfwm4": [],
            "/ext/data/MyTheme/metacity-1": [],
            "/ext/data/MyTheme/gnome-shell": [],
            "/ext/data/MyTheme/cinnamon": [],
            "/ext/data/MyTheme/openbox-3": [],
            "/home/t/.config/conky/my-theme": ["conky.conf"],
        },
        "files": {
            "/ext/data/MyTheme.colors": {"size": 10, "text": "[General]\n"},
            "/ext/data/my.nemo_action": {"size": 10, "text": "[Nemo Action]\n"},
            "/ext/data/MyTheme/index.theme": {"size": 10, "text": "[Icon Theme]\nName=MyTheme\n"},
            "/ext/data/MyTheme/metadata.json": {"size": 10, "text": "{\"uuid\": \"my@rexon\", \"KPlugin\": {\"Id\": \"MyTheme\"}}"},
            "/ext/data/MyTheme/lxqt-panel.qss": {"size": 10, "text": ""},
            "/ext/data/MyTheme/conky.conf": {"size": 10, "text": "conky.text = [[x]]"},
            "/home/t/.config/conky/my-theme/conky.conf": {"size": 10, "text": "conky.text = [[x]]"},
        },
    });
    let fs = FsSnap::from_json(&snapshot);
    let tools: HashMap<String, String> = [
        "kwriteconfig6", "plasma-apply-colorscheme", "plasma-apply-cursortheme",
        "plasma-apply-lookandfeel", "plasma-apply-desktoptheme", "kvantummanager",
        "gsettings", "xfconf-query", "qdbus6", "kpackagetool6", "dbus-send",
        "pkexec", "conky", "cinnamon", "gnome-extensions", "mkinitcpio",
    ]
    .iter()
    .map(|t| ((*t).to_string(), format!("/usr/bin/{t}")))
    .collect();
    // Install tools (not enabling ones) — they are allowed.
    const INSTALL_TOOLS: &[&str] = &["kpackagetool6", "kpackagetool5", "fc-cache"];

    // The button is not in every desktop: the frames section (Aurorae) — only in
    // KDE, and on XFCE enabling frames stays with the installation (xfwm4), otherwise
    // there would be nothing to enable them with. The guard applies where the button EXISTS.
    let button_here = |kind: &str, de_id: &str| -> bool {
        rexon_core::categories::categories(std::path::Path::new("/")).iter().any(|c| {
            rexon_core::categories::category_kind(&c.key) == Some(kind)
                && rexon_core::categories::category_available(&c.key, de_id)
        })
    };
    for (xdg, de_id) in [
        ("KDE", "kde"), ("GNOME", "gnome"), ("XFCE", "xfce"), ("X-Cinnamon", "cinnamon"),
        ("MATE", "mate"), ("LXQt", "lxqt"), ("sway", "sway"), ("", ""),
    ] {
        // System themes (login screen, splash) — both with the Rexon helper and
        // via the fallback path without it.
        for helper in [false, true] {
        for kind in button_kinds.iter().filter(|k| de_id.is_empty() || button_here(k, de_id)) {
            let tmp = TmpAlloc::new("/tmp/t");
            let which = |n: &str| tools.get(n).cloned();
            let run_ok = |_: &[String]| false;
            let env = ExtEnv {
                home: "/home/t",
                fs: &fs,
                which: &which,
                run_ok: &run_ok,
                xdg_current_desktop: xdg,
                xdg_session_type: "wayland",
                wayland_display: true,
                stage_helper_available: false,
                system_helper_path: "/usr/lib/rexon/helper",
                system_helper_exists: helper,
                helper_family: "arch",
                tmp: &tmp,
            };
            let m = manifest("my-theme", kind);
            let steps = safe_install_plan(&env, &ExtInfo { dir: "/ext", manifest: &m });
            for step in &steps {
                let bad = match step {
                    // An own file inside the package (the missing `.plymouth`) and the owner mark
                    // of the copy are not a desktop setting.
                    ExtStep::WriteFile { path, .. } => {
                        !path.starts_with("/ext/")
                            && !path.ends_with(&format!("/{}", rexon_core::ext_manager::OWNER_MARK))
                    }
                    // The system part: only a copy of the theme, without selecting it.
                    ExtStep::PkexecScript { script, .. } => {
                        script.contains("zz-rexon-theme") || script.contains("plymouth-set-default-theme")
                    }
                    // The "which login screen works" probe changes nothing.
                    ExtStep::Run { argv, .. } if argv[0] == "systemctl" => {
                        argv.get(1).map(String::as_str) != Some("is-active")
                    }
                    ExtStep::Run { argv, .. } if argv[0] == "pkexec" => {
                        // The helper in install mode: the kind is `sddm`/`plymouth`,
                        // the select mode (`apply-…`) is not allowed for installation.
                        argv.iter().any(|a| a.starts_with("apply-"))
                    }
                    ExtStep::Run { argv, .. } | ExtStep::Spawn { argv } => {
                        let tool = argv[0].rsplit('/').next().unwrap_or("");
                        !INSTALL_TOOLS.contains(&tool)
                    }
                    _ => false,
                };
                assert!(!bad, "{kind} in «{xdg}»: the installation applies the theme — {step:?}");
                assert!(
                    !format!("{step:?}").contains(".icons/default"),
                    "{kind} in «{xdg}»: the installation touches ~/.icons/default — {step:?}"
                );
            }
            // And the installation itself is not empty: the theme files are put down (system ones —
            // through the helper or the fallback copy script).
            // (A login screen without a working display manager on the stand is honestly
            // not installed at all — there is nobody to give the theme to.)
            if *kind != "wallpaper-pack" && *kind != "theme-login-screen" {
                assert!(
                    steps.iter().any(|s| matches!(
                        s,
                        ExtStep::CopyTree { .. }
                            | ExtStep::CopyFile { .. }
                            | ExtStep::PkexecScript { .. }
                            | ExtStep::Stage { .. }
                    ) || matches!(s, ExtStep::Run { argv, .. } if argv[0].ends_with("kpackagetool6"))),
                    "{kind} in «{xdg}»: the install plan copies nothing: {steps:?}"
                );
            }
        }
        }
    }

    // Window frames (`theme-window-decoration`) on XFCE go into ~/.themes and
    // are enabled by the «Window Borders (XFCE)» section button, not by the installation.
    let tmp = TmpAlloc::new("/tmp/t");
    let which = |n: &str| tools.get(n).cloned();
    let run_ok = |_: &[String]| false;
    let env = ExtEnv {
        home: "/home/t",
        fs: &fs,
        which: &which,
        run_ok: &run_ok,
        xdg_current_desktop: "XFCE",
        xdg_session_type: "x11",
        wayland_display: false,
        stage_helper_available: false,
        system_helper_path: "/usr/lib/rexon/helper",
        system_helper_exists: false,
        helper_family: "arch",
        tmp: &tmp,
    };
    let m = manifest("my-theme", "theme-window-decoration");
    let steps = safe_install_plan(&env, &ExtInfo { dir: "/ext", manifest: &m });
    assert!(
        !steps.iter().any(|s| format!("{s:?}").contains("xfconf-query")),
        "window borders on XFCE: the installation enables the theme — {steps:?}"
    );
}

/// A desktop section's button writes exactly what the installation used to write:
/// the enable steps are the same as the installation's before the decision of 04.10.2026, and
/// disabling is the same as what removal does before deleting.
#[test]
fn section_button_writes_what_install_used_to() {
    use rexon_core::ext_manager::{de_disable_steps, de_enable_steps};
    let fs = FsSnap::from_json(&json!({}));
    let tmp = TmpAlloc::new("/tmp/t");
    let which = |n: &str| Some(format!("/usr/bin/{n}"));
    let run_ok = |_: &[String]| false;
    let env = ExtEnv {
        home: "/home/t",
        fs: &fs,
        which: &which,
        run_ok: &run_ok,
        xdg_current_desktop: "XFCE",
        xdg_session_type: "x11",
        wayland_display: false,
        stage_helper_available: false,
        system_helper_path: "",
        system_helper_exists: false,
        helper_family: "arch",
        tmp: &tmp,
    };
    let argv = |kind: &str, on: bool| -> Vec<Vec<String>> {
        let steps = if on { de_enable_steps(&env, kind, "Name") } else { de_disable_steps(&env, kind, "Name") };
        steps
            .into_iter()
            .map(|s| match s {
                ExtStep::Run { argv, .. } | ExtStep::Spawn { argv } => argv,
                other => panic!("{kind}: the step is not a command: {other:?}"),
            })
            .collect()
    };
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(argv("theme-xfwm4", true), vec![s(&["xfconf-query", "-c", "xfwm4", "-p", "/general/theme", "-n", "-t", "string", "-s", "Name"])]);
    assert_eq!(argv("theme-marco", true), vec![s(&["gsettings", "set", "org.mate.Marco.general", "theme", "Name"])]);
    assert_eq!(argv("theme-cinnamon", true), vec![s(&["gsettings", "set", "org.cinnamon.theme", "name", "Name"])]);
    assert_eq!(
        argv("theme-gnome-shell", true),
        vec![s(&["gsettings", "set", "org.gnome.shell.extensions.user-theme", "name", "Name"])]
    );
    assert_eq!(
        argv("kde-window-switcher", true)[0],
        s(&["kwriteconfig6", "--file", "kwinrc", "--group", "TabBox", "--key", "LayoutName", "Name"])
    );
    // KWin script: the plugin key + reconfigure; disabling — false + unload.
    let on = argv("kde-kwin-script", true);
    assert_eq!(on[0], s(&["kwriteconfig6", "--file", "kwinrc", "--group", "Plugins", "--key", "NameEnabled", "true"]));
    assert!(on[1].ends_with(&s(&["org.kde.KWin", "/KWin", "reconfigure"])));
    let off = argv("kde-kwin-script", false);
    assert_eq!(off[0], s(&["kwriteconfig6", "--file", "kwinrc", "--group", "Plugins", "--key", "NameEnabled", "false"]));
    assert!(off.iter().any(|a| a.contains(&"unloadScript".to_string())));
    // Cinnamon applet: add to the list / remove from it.
    assert!(argv("cinnamon-applet", true)[0][2].contains("next-applet-id"));
    assert!(argv("cinnamon-applet", false)[0][2].contains("grep -vF ':Name:'"));
    // GNOME extension: the stock utility, the fallback path is the list.
    assert!(argv("gnome-shell-extension", true)[0][2].contains("gnome-extensions enable 'Name'"));
    assert!(argv("gnome-shell-extension", false)[0][2].contains("gnome-extensions disable 'Name'"));
}

/// Removing an `.rxn` with a splash or login screen theme removes its directory in
/// `/usr/share` through the system helper (mode remove-<kind>) — previously
/// the system part silently stayed in place. No theme on the system or no
/// helper — no step (the password window is not shown in vain).
#[test]
fn rxn_uninstall_removes_system_theme_through_helper() {
    let tmp = TmpAlloc::new("/tmp/t");
    let which = |n: &str| Some(format!("/usr/bin/{n}"));
    let run_ok = |_: &[String]| false;
    for (ext_type, kind) in [("theme-plymouth", "plymouth"), ("theme-login-screen", "sddm")] {
        let themes = format!("/usr/share/{kind}/themes");
        let with_theme = FsSnap::from_json(&json!({"dirs": {
            "/ext": ["data"], "/ext/data": ["neon"], "/ext/data/neon": [],
            (themes.clone()): ["neon"], (format!("{themes}/neon")): [],
        }}));
        let without = FsSnap::from_json(&json!({"dirs": {
            "/ext": ["data"], "/ext/data": ["neon"], "/ext/data/neon": [], (themes.clone()): [],
        }}));
        let m = manifest("neon-pack", ext_type);
        let plan = |fs: &FsSnap, helper: bool| {
            let env = ExtEnv {
                home: "/home/t",
                fs,
                which: &which,
                run_ok: &run_ok,
                xdg_current_desktop: "KDE",
                xdg_session_type: "wayland",
                wayland_display: true,
                stage_helper_available: false,
                system_helper_path: "/nonexistent/rexon-system-helper",
                system_helper_exists: helper,
                helper_family: "arch",
                tmp: &tmp,
            };
            safe_uninstall_plan(&env, &ExtInfo { dir: "/ext", manifest: &m })
        };
        let steps = plan(&with_theme, true);
        let argv: Vec<&Vec<String>> = steps
            .iter()
            .filter_map(|s| match s {
                ExtStep::Run { argv, .. } => Some(argv),
                _ => None,
            })
            .collect();
        assert_eq!(argv.len(), 1, "{ext_type}: {steps:?}");
        assert_eq!(argv[0][0], "pkexec");
        let tail: Vec<&str> = argv[0][argv[0].len() - 4..].iter().map(String::as_str).collect();
        assert_eq!(tail, [format!("remove-{kind}").as_str(), "neon", "-", "arch"], "{ext_type}");
        assert!(plan(&without, true).is_empty(), "{ext_type}: the theme is not in the system");
        assert!(plan(&with_theme, false).is_empty(), "{ext_type}: there is no helper");
    }
}

/// The Conky widget window per desktop (`conky_overlay::window_preset`):
/// 13 desktops × X11/Wayland. On X11 — everywhere an ordinary window with "below
/// all windows" hints; on Wayland with layers — the background layer, on KWin with conky
/// 1.25 — an ordinary window with the `desktop` namespace, on niri — the bottom
/// layer; GNOME, Cinnamon and Pantheon on Wayland know no layers — no advice,
/// there is a warning.
#[test]
fn conky_window_preset_matrix() {
    use rexon_core::conky_overlay::{preset_note, window_preset, DESKTOP_HINTS};
    let v125 = Some((1, 25));
    let des = [
        "kde", "gnome", "cinnamon", "xfce", "mate", "budgie", "pantheon", "deepin", "lxqt", "cosmic", "hyprland",
        "sway", "niri",
    ];
    for de in des {
        let x = window_preset(de, false, v125).unwrap_or_else(|| panic!("{de}: X11"));
        assert_eq!((x.own_window_type, x.hints, x.namespace), ("normal", Some(DESKTOP_HINTS), None), "{de}: X11");
        assert!(preset_note(de, false).is_none(), "{de}: X11 without a caveat");
        let w = window_preset(de, true, v125);
        let wl = w.as_ref().map(|p| (p.own_window_type, p.hints, p.namespace));
        let want = match de {
            "gnome" | "cinnamon" | "pantheon" => None,
            "kde" => Some(("normal", Some(DESKTOP_HINTS), Some("desktop"))),
            "niri" => Some(("normal", Some(DESKTOP_HINTS), None)),
            _ => Some(("desktop", None, None)),
        };
        assert_eq!(wl, want, "{de}: Wayland");
        assert_eq!(preset_note(de, true).is_some(), want.is_none(), "{de}: Wayland caveat");
        // Before 1.25 the hints do not raise an ordinary window into a layer — background everywhere.
        if want.is_some() {
            assert_eq!(window_preset(de, true, Some((1, 19))).unwrap().own_window_type, "desktop", "{de}: 1.19");
            assert_eq!(window_preset(de, true, None).unwrap().own_window_type, "desktop", "{de}: version unknown");
        }
    }
    // The section caveat: with conky 1.25 — «regular window», without a version — the former one.
    rexon_core::i18n::set_language(Some("en"));
    let new = rexon_core::de_apply::conky_wayland_note_for("gnome", true, v125).unwrap();
    assert!(new.contains("regular window"), "{new}");
    let old = rexon_core::de_apply::conky_wayland_note_for("gnome", true, None).unwrap();
    assert_eq!(Some(old), rexon_core::de_apply::conky_wayland_note("gnome", true));
    assert!(rexon_core::de_apply::conky_wayland_note_for("kde", true, v125).is_none());
}

// ============================================================================
// The owner rule: plasmoids by KPlugin.Id and single files
// ============================================================================

/// Put a file `path` with the text `text` into the snapshot (the directory goes into the listing).
fn with_file(mut snap: Value, path: &str, text: &str) -> Value {
    let (dir, name) = path.rsplit_once('/').unwrap();
    let mut list = snap["dirs"][dir].as_array().cloned().unwrap_or_default();
    if !list.iter().any(|n| n == name) {
        list.push(json!(name));
    }
    snap["dirs"][dir] = Value::Array(list);
    snap["files"][path] = json!({"size": text.len(), "text": text});
    snap
}

/// Whether the plan touched the path `dest`: a copy there, a write inside, a removal, a KDE tool.
fn touches(steps: &[ExtStep], dest: &str) -> bool {
    steps.iter().any(|s| match s {
        ExtStep::CopyTree { dest: d, .. } | ExtStep::CopyFile { dest: d, .. } => d == dest,
        ExtStep::WriteFile { path, .. } => path.starts_with(&format!("{dest}/")),
        ExtStep::RemoveTree { path, .. } | ExtStep::RemoveFile { path } => path == dest,
        ExtStep::Run { argv, .. } => argv[0].contains("kpackagetool"),
        _ => false,
    })
}

/// The tests look for the skip reason by the English word "skipped": the process
/// language is set explicitly, otherwise with a Russian machine locale the text arrives
/// translated and the test turns red (all tests of the file expect the source language).
fn english() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
}

/// The plan named the reason for skipping `dest`: as a note or as a total refusal.
fn says_skipped(steps: &[ExtStep], dest: &str) -> bool {
    steps.iter().any(|s| matches!(s, ExtStep::Notice { text } | ExtStep::Refuse { text }
        if text.contains("skipped") && text.contains(dest)))
}

const PLASMOIDS: &str = "/home/t/.local/share/plasma/plasmoids";

/// A plasmoid as a directory: in place of `plasmoids/<KPlugin.Id>` there is a foreign one (without a mark —
/// by hand, by the store or by an earlier Rexon; with another package's mark). No
/// copy, no removal, no `kpackagetool`; the reason is named. The folder is named as the
/// id — the package has nothing else to install: total refusal. Removal does not take away
/// a foreign one.
#[test]
fn plasmoid_dir_never_replaces_foreign_with_same_id() {
    english();
    for (inner, text) in [("main.qml", "human"), (".rexon-owner", "other-pack\n")] {
        let snap = snap_with_meta("org.human.clock", "metadata.json", r#"{"KPlugin": {"Id": "org.human.clock"}}"#, &[]);
        let dest = format!("{PLASMOIDS}/org.human.clock");
        let snap = with_file(snap, &format!("{dest}/{inner}"), text);
        let case = Case::new(snap, &["kpackagetool6"], true, false);
        let m = manifest("clock-pack", "theme-plasmoid");
        let install = case.plan(&m, false);
        assert!(!touches(&install, &dest), "{inner}: the installation touched a foreign plasmoid: {install:?}");
        assert!(
            matches!(install.as_slice(), [ExtStep::Refuse { text }] if text.contains("skipped") && text.contains(&dest)),
            "{inner}: everything was skipped — a refusal with a reason is needed: {install:?}"
        );
        assert_eq!(install.iter().filter(|s| matches!(s, ExtStep::Refuse { .. })).count(), 1);
        let uninstall = case.plan(&m, true);
        assert!(!touches(&uninstall, &dest), "{inner}: the removal removed a foreign plasmoid: {uninstall:?}");
    }
}

/// Our own former plasmoid (this package's mark) — removed and put down
/// again, with the mark; removal takes it away (`kpackagetool -r` and the directory).
#[test]
fn plasmoid_dir_replaces_own_previous_copy() {
    let snap = snap_with_meta("W", "metadata.json", r#"{"KPlugin": {"Id": "org.own.clock"}}"#, &[]);
    let dest = format!("{PLASMOIDS}/org.own.clock");
    let snap = with_file(snap, &format!("{dest}/.rexon-owner"), "clock-pack\n");
    let case = Case::new(snap, &["kpackagetool6"], true, false);
    let m = manifest("clock-pack", "theme-plasmoid");
    let install = case.plan(&m, false);
    assert!(install.iter().any(|s| matches!(s, ExtStep::RemoveTree { path, .. } if *path == dest)), "{install:?}");
    assert!(copies(&install).iter().any(|(_, d)| *d == dest), "{install:?}");
    assert!(install.iter().any(|s| matches!(s, ExtStep::WriteFile { path, content }
        if *path == format!("{dest}/.rexon-owner") && content == "clock-pack\n")), "{install:?}");
    assert!(!says_skipped(&install, &dest), "{install:?}");
    let uninstall = case.plan(&m, true);
    assert!(argvs(&uninstall).iter().any(|a| a == &["/usr/bin/kpackagetool6", "-r", "org.own.clock"]), "{uninstall:?}");
    assert!(uninstall.iter().any(|s| matches!(s, ExtStep::RemoveTree { path, .. } if *path == dest)), "{uninstall:?}");
}

/// The folder in `data/` is not named as the id: its common copy goes down, while the
/// plasmoid itself by id (a foreign one) is skipped — a partial installation, with a note.
#[test]
fn plasmoid_partial_skip_is_a_notice_not_a_refusal() {
    let snap = snap_with_meta("W", "metadata.json", r#"{"KPlugin": {"Id": "org.human.clock"}}"#, &[]);
    let dest = format!("{PLASMOIDS}/org.human.clock");
    let snap = with_file(snap, &format!("{dest}/main.qml"), "human");
    let case = Case::new(snap, &[], true, false);
    let install = case.plan(&manifest("clock-pack", "theme-plasmoid"), false);
    assert!(!touches(&install, &dest), "{install:?}");
    assert!(!install.iter().any(|s| matches!(s, ExtStep::Refuse { .. })), "{install:?}");
    assert!(install.iter().any(|s| matches!(s, ExtStep::Notice { text } if text.contains(&dest))), "{install:?}");
}

/// A package snapshot with a `.plasmoid` archive that the plan probe has already unpacked
/// (`ext_manager::PLASMOID_PROBE_DIR`) — the id inside the archive: `org.zip.clock`.
fn plasmoid_archive_snapshot() -> Value {
    let probe = "/tmp/t/rexon_plasmoid_probe/clock.plasmoid";
    json!({
        "dirs": {
            "/ext": ["data"],
            "/ext/data": ["clock.plasmoid"],
            probe: ["contents", "metadata.json"],
            format!("{probe}/contents"): [],
        },
        "files": {
            "/ext/data/clock.plasmoid": {"size": 10, "text": null},
            format!("{probe}/metadata.json"): {"size": 1, "text": r#"{"KPlugin": {"Id": "org.zip.clock"}}"#},
        },
    })
}

/// A `.plasmoid` archive: the id is known only after unpacking — the snapshot probe gives it,
/// and a foreign plasmoid with the same id is not handed to `kpackagetool -u` (which
/// would have replaced it already while building the plan). Refusal with a reason; removal does not
/// touch a foreign one.
#[test]
fn plasmoid_archive_never_updates_foreign_with_same_id() {
    let dest = format!("{PLASMOIDS}/org.zip.clock");
    for tools in [&["kpackagetool6"][..], &[][..]] {
        let snap = with_file(plasmoid_archive_snapshot(), &format!("{dest}/main.qml"), "human");
        let case = Case::new(snap, tools, true, false);
        let m = manifest("zip-pack", "theme-plasmoid");
        let install = case.plan(&m, false);
        assert!(!touches(&install, &dest), "{tools:?}: {install:?}");
        assert!(!install.iter().any(|s| matches!(s, ExtStep::ExtractZip { .. })), "{tools:?}: {install:?}");
        assert!(
            matches!(install.as_slice(), [ExtStep::Refuse { text }] if text.contains("org.zip.clock") && text.contains(&dest)),
            "{tools:?}: {install:?}"
        );
        let uninstall = case.plan(&m, true);
        assert!(!touches(&uninstall, &dest), "{tools:?}: removal: {uninstall:?}");
    }
}

/// A `.plasmoid` archive in a clean place: the KDE tool installs it, and the directory
/// by the id from the probe gets the mark; removal takes the plasmoid away by the REAL
/// id (previously — by the package id, missing the directory).
#[test]
fn plasmoid_archive_installed_by_tool_is_marked_and_removed_by_real_id() {
    let dest = format!("{PLASMOIDS}/org.zip.clock");
    let case = Case::new(plasmoid_archive_snapshot(), &["kpackagetool6"], true, false);
    let m = manifest("zip-pack", "theme-plasmoid");
    let install = case.plan(&m, false);
    assert!(argvs(&install).iter().any(|a| a[1] == "-i"), "{install:?}");
    assert!(install.iter().any(|s| matches!(s, ExtStep::WriteFile { path, content }
        if *path == format!("{dest}/.rexon-owner") && content == "zip-pack\n")), "{install:?}");
    let marked = with_file(plasmoid_archive_snapshot(), &format!("{dest}/.rexon-owner"), "zip-pack\n");
    let uninstall = Case::new(marked, &["kpackagetool6"], true, false).plan(&m, true);
    assert!(argvs(&uninstall).iter().any(|a| a == &["/usr/bin/kpackagetool6", "-r", "org.zip.clock"]), "{uninstall:?}");
    assert!(uninstall.iter().any(|s| matches!(s, ExtStep::RemoveTree { path, .. } if *path == dest)), "{uninstall:?}");
}

const SCHEMES: &str = "/home/t/.local/share/color-schemes";

/// A color scheme package `names` (`(file name, text)`).
fn colors_snapshot(names: &[(&str, &str)]) -> Value {
    let mut snap = json!({"dirs": {"/ext": ["data"], "/ext/data": []}, "files": {}});
    for (n, t) in names {
        snap = with_file(snap, &format!("/ext/data/{n}"), t);
    }
    snap
}

/// A single file (`.colors`, a Nemo action): in its place there is a foreign file with different
/// contents — it is not overwritten and not removed by uninstall; the reason
/// is named; if it is the package's only file — total refusal.
#[test]
fn single_file_never_overwrites_foreign() {
    english();
    for (t, base, name) in [
        ("theme-kde-colors", SCHEMES, "Nord.colors"),
        ("nemo-action", "/home/t/.local/share/nemo/actions", "open.nemo_action"),
    ] {
        let dest = format!("{base}/{name}");
        let snap = with_file(colors_snapshot(&[(name, "package\n")]), &dest, "person\n");
        let case = Case::new(snap, &[], true, false);
        let m = manifest("pack", t);
        let install = case.plan(&m, false);
        assert!(!touches(&install, &dest), "{t}: the installation overwrote a foreign file: {install:?}");
        assert!(
            matches!(install.as_slice(), [ExtStep::Refuse { text }] if text.contains("skipped") && text.contains(&dest)),
            "{t}: {install:?}"
        );
        let uninstall = case.plan(&m, true);
        assert!(!touches(&uninstall, &dest), "{t}: the removal removed a foreign file: {uninstall:?}");
    }
}

/// The same file byte for byte is ours (replaced, removed); a file from this package's
/// common log (`owned_files`) is ours whatever its contents; a link
/// in its place is foreign.
#[test]
fn single_file_same_bytes_or_journal_is_own() {
    let dest = format!("{SCHEMES}/Nord.colors");
    let m = manifest("pack", "theme-kde-colors");
    let same = Case::new(with_file(colors_snapshot(&[("Nord.colors", "same\n")]), &dest, "same\n"), &[], true, false);
    assert_eq!(copies(&same.plan(&m, false)), vec![("/ext/data/Nord.colors".to_string(), dest.clone())]);
    assert!(same.plan(&m, true).iter().any(|s| matches!(s, ExtStep::RemoveFile { path } if *path == dest)));

    let mut journal = with_file(colors_snapshot(&[("Nord.colors", "new\n")]), &dest, "own previous\n");
    journal["owned_files"] = json!([dest]);
    let own = Case::new(journal, &[], true, false);
    let install = own.plan(&m, false);
    assert_eq!(copies(&install), vec![("/ext/data/Nord.colors".to_string(), dest.clone())], "{install:?}");
    assert!(!says_skipped(&install, &dest), "{install:?}");
    assert!(own.plan(&m, true).iter().any(|s| matches!(s, ExtStep::RemoveFile { path } if *path == dest)));

    let mut link = with_file(colors_snapshot(&[("Nord.colors", "same\n")]), &dest, "same\n");
    link["links"] = json!([dest]);
    let linked = Case::new(link, &[], true, false);
    assert!(!touches(&linked.plan(&m, false), &dest), "the file was written through the link");
}

/// Two schemes, one taken by a foreign file: the second is installed, the first gets a note
/// (a partial skip is not a refusal).
#[test]
fn single_file_partial_skip_installs_the_rest() {
    let taken = format!("{SCHEMES}/Nord.colors");
    let free = format!("{SCHEMES}/Gold.colors");
    let snap = with_file(colors_snapshot(&[("Nord.colors", "package\n"), ("Gold.colors", "package\n")]), &taken, "person\n");
    let install = Case::new(snap, &[], true, false).plan(&manifest("pack", "theme-kde-colors"), false);
    assert_eq!(copies(&install), vec![("/ext/data/Gold.colors".to_string(), free)], "{install:?}");
    assert!(install.iter().any(|s| matches!(s, ExtStep::Notice { text } if text.contains(&taken))), "{install:?}");
    assert!(!install.iter().any(|s| matches!(s, ExtStep::Refuse { .. })), "{install:?}");
}

/// A bundle: a font in the common fonts directory and a theme of a bundle part under
/// its own name — by the same rules (the foreign stays intact, the reason is named).
#[test]
fn bundle_parts_never_overwrite_foreign() {
    english();
    let font = "/home/t/.local/share/fonts/Sans.ttf";
    let gtk = "/home/t/.themes/BDark";
    let mut snap = json!({
        "dirs": {"/ext": ["data"], "/ext/data": ["fonts", "gtk"], "/ext/data/fonts": [], "/ext/data/gtk": ["BDark"], "/ext/data/gtk/BDark": []},
        "files": {},
    });
    snap = with_file(snap, "/ext/data/fonts/Sans.ttf", "package");
    snap = with_file(snap, font, "person");
    snap = with_file(snap, &format!("{gtk}/gtk.css"), "person");
    let mut m = manifest("bun", "theme-bundle");
    m.insert("bundle".into(), json!({"font-pack": "data/fonts", "theme-gtk": "data/gtk"}));
    let case = Case::new(snap, &[], true, false);
    let install = case.plan(&m, false);
    for dest in [font, gtk] {
        assert!(!touches(&install, dest), "{dest}: {install:?}");
        assert!(says_skipped(&install, dest), "{dest}: {install:?}");
    }
    let uninstall = case.plan(&m, true);
    for dest in [font, gtk] {
        assert!(!touches(&uninstall, dest), "removal of {dest}: {uninstall:?}");
    }
}
