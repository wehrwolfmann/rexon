//! Safety net before merging the installers: the LIVE `.rxn` install path
//! (`ext_exec::install_from_file` → `exec_plan_noted` → `uninstall`) on a
//! temporary profile.
//!
//! Plans per type and desktop are guarded by `rxn_install_plans_parity.rs`;
//! here — what the real executor does to the disk: theme files in place, the
//! "enabled" record, a repeat install refused, a bad package refused without
//! traces, rollback on a failure mid-install (the package copy, the plan's
//! files, what was replaced — the profile byte-for-byte as before, for both
//! `.rxn` and the store) and a clean profile after removal.
//!
//! ⚠️ The owner's profile is not touched. The body runs in a CHILD process with
//! temporary `HOME`/`XDG_*`, an EMPTY `PATH` (not a single program — neither
//! gsettings nor kpackagetool will start) and a nonexistent session bus.
//! Without its own environment variable the child body does nothing.

use std::io::Write;
use std::path::{Path, PathBuf};

const CHILD_ENV: &str = "REXON_LIVE_ROUNDTRIP_CHILD";

/// The run's temporary directory is in the parent's `$HOME/.cache`, not in `/tmp`
/// (that is RAM), and carries the process id: parallel runs do not interfere.
fn sandbox(tag: &str) -> PathBuf {
    // Run number: two tests may call the same child body at once.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join(".cache").join(format!("rexon-live-roundtrip-{}-{n}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run the child body `child` on a temporary profile.
fn run_child(child: &str) {
    run_child_env(child, &[]);
}

fn run_child_env(child: &str, extra: &[(&str, &str)]) {
    let sb = sandbox(child);
    let home = sb.join("home");
    let bin = sb.join("bin");
    for d in [&home, &bin, &sb.join("run"), &sb.join("tmp")] {
        std::fs::create_dir_all(d).unwrap();
    }
    let out = std::process::Command::new(std::env::current_exe().expect("путь бинаря"))
        .args(["--exact", child, "--ignored", "--test-threads=1", "--nocapture"])
        .env_clear()
        .env(CHILD_ENV, "1")
        .env("HOME", &home)
        .env("PATH", &bin)
        .env("TMPDIR", sb.join("tmp"))
        .env("XDG_RUNTIME_DIR", sb.join("run"))
        .env("XDG_CURRENT_DESKTOP", "XFCE")
        .env("XDG_SESSION_TYPE", "x11")
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/rexon-test-bus")
        .env("LANG", "C.UTF-8")
        .env("LANGUAGE", "en")
        .envs(extra.iter().copied())
        .output()
        .expect("дочерний прогон");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let ok = out.status.success() && stdout.contains("1 passed");
    if ok {
        let _ = std::fs::remove_dir_all(&sb);
    }
    assert!(
        ok,
        "дочерний прогон {child} не прошёл (песочница оставлена: {}):\n{stdout}\n{}",
        sb.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// D4. Removing a GTK theme by the purchase log (`LiveInstaller::uninstall` →
/// `uninstall_from_log`) removes `~/.local/share/themes/<theme>` and the link
/// `~/.themes/<theme>` that installation creates
/// (`installed_scan::sync_theme_links`) — with the same shared cleanup
/// (`theme_removal::tidy_after_removal`) as the tile «Remove».
/// Previously the link dangled until the next scan (found by the safety net
/// 25.10.2026, fixed). Checked in `live_appearance_roundtrip_on_temp_profile`.
#[test]
fn defect_gtk_uninstall_drops_theme_link() {
    run_child("live_appearance_child");
}

/// `.rxn` packages: `ext_exec`.
#[test]
fn live_install_roundtrip_on_temp_profile() {
    run_child("live_roundtrip_child");
}

/// «Appearance»: `LiveInstaller`.
#[test]
fn live_appearance_roundtrip_on_temp_profile() {
    run_child("live_appearance_child");
}

// ============================================================================
// Child body
// ============================================================================

/// Build an `.rxn`: `manifest.json` + files `(path, contents)`.
fn make_rxn(path: &Path, manifest: &serde_json::Value, files: &[(&str, &str)]) {
    let f = std::fs::File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    let opt = zip::write::SimpleFileOptions::default();
    z.start_file("manifest.json", opt).unwrap();
    z.write_all(manifest.to_string().as_bytes()).unwrap();
    for (name, text) in files {
        z.start_file(*name, opt).unwrap();
        z.write_all(text.as_bytes()).unwrap();
    }
    z.finish().unwrap();
}

/// All FILES under `root` (relative to it; empty base directories like
/// `~/.local/share/icons` created by installation do not count as a trace —
/// the theme directory itself is checked separately), except Rexon's own housekeeping (`.config/rexon` — the extensions directory, the state map,
/// the bridge) — that is checked separately.
fn tree(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            if rel.starts_with(".config/rexon") || rel.starts_with(".cache") || rel.starts_with(".local/state") {
                continue;
            }
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

/// The whole profile byte-for-byte — together with Rexon's housekeeping (extensions
/// directory, state map, shared log): path, kind, contents or link target.
/// Without run logs (`.cache`, `.local/state`).
fn bytes(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
            if rel.starts_with(".cache") || rel.starts_with(".local/state") {
                continue;
            }
            let md = std::fs::symlink_metadata(&p).unwrap();
            let what = if md.file_type().is_symlink() {
                format!("link → {}", std::fs::read_link(&p).unwrap().display())
            } else if md.is_dir() {
                stack.push(p.clone());
                "dir".to_string()
            } else {
                format!("file {}", String::from_utf8_lossy(&std::fs::read(&p).unwrap()))
            };
            out.push((rel, what));
        }
    }
    out.sort();
    out
}

/// Make a directory read-only (`ro = true`) or writable again.
fn read_only(dir: &Path, ro: bool) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(if ro { 0o555 } else { 0o755 })).unwrap();
}

/// Writing into a read-only directory fails (not root): a failure mid-copy is
/// reproducible.
fn read_only_bites(dir: &Path) -> bool {
    let probe = dir.join(".rexon-probe");
    let wrote = std::fs::write(&probe, "").is_ok();
    let _ = std::fs::remove_file(&probe);
    !wrote
}

fn ext_dir(home: &Path) -> PathBuf {
    home.join(".config/rexon/extensions")
}

fn state(home: &Path) -> serde_json::Value {
    std::fs::read_to_string(home.join(".config/rexon/extensions_state.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// The sandbox's shared install log and the record key of package `id`.
fn journal_entry(home: &Path, id: &str) -> (rexon_core::install_journal::Journal, String) {
    use rexon_core::install_journal as journal;
    let j = journal::load_at(&home.join(".config/rexon")).expect("общий журнал читается");
    let key = journal::key_of(journal::Source::Rxn, id);
    assert!(j.items.contains_key(&key), "{id}: нет в общем журнале: {:#?}", j.items.keys().collect::<Vec<_>>());
    (j, key)
}

/// Remove record `key` by the shared log — with the file layer, with a stub
/// program runner (none is started), and write the log.
fn remove_by_journal(home: &Path, j: &mut rexon_core::install_journal::Journal, key: &str) {
    use rexon_core::install_journal as journal;
    let run = |_: &[String], _: Option<u64>| rexon_core::utils::RunReport::default();
    let have = |_: &str| false;
    let spawn = |argv: &[String]| panic!("снятие по журналу запустило программу: {argv:?}");
    let ctx = rexon_core::install_plan::Ctx {
        install: false,
        steamos: false,
        privileged: None,
        run: &run,
        have: &have,
        spawn: &spawn,
        wayland: false,
        sync_env: &rexon_core::install_plan::no_sync_env,
    };
    journal::remove(j, key, &journal::probe_live, &ctx).unwrap_or_else(|e| panic!("{key}: снятие по журналу: {e:?}"));
    journal::save_at(&home.join(".config/rexon"), j).expect("журнал записан");
}

/// Theme package: `data/<Theme>/` with files resembling a theme of any kind.
fn theme_files(theme: &str) -> Vec<(String, String)> {
    vec![
        (format!("data/{theme}/index.theme"), format!("[Icon Theme]\nName={theme}\nDirectories=\n")),
        (format!("data/{theme}/cursors/left_ptr"), String::new()),
        (format!("data/{theme}/gtk-3.0/gtk.css"), String::new()),
        (format!("data/{theme}/metadata.json"), format!("{{\"KPlugin\": {{\"Id\": \"{theme}\"}}}}")),
        (format!("data/{theme}/{theme}.kvconfig"), String::new()),
        (format!("data/{theme}/contents/defaults"), String::new()),
        (format!("data/{theme}/xfwm4/themerc"), String::new()),
    ]
}

#[test]
#[ignore = "запускается дочерним процессом live_install_roundtrip_on_temp_profile"]
fn live_roundtrip_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        // Not a child run — a live profile. Do nothing.
        eprintln!("live_roundtrip_child: только из live_install_roundtrip_on_temp_profile");
        return;
    }
    use rexon_core::ext_exec;
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    assert!(home.to_string_lossy().contains("rexon-live-roundtrip-"), "HOME не песочница: {home:?}");
    let work = home.parent().unwrap().join("pkgs");
    std::fs::create_dir_all(&work).unwrap();

    // ── 1. Install → repeat → remove for the common types ───────────────
    // Window borders on XFCE also put a copy into ~/.themes, a bundle puts copies
    // of subfolders under shared names: removal takes them off by the owner mark
    // (defects D1–D3, fixed 25.10.2026). A plain Conky widget is one
    // `<id>` directory (a second copy of the subfolder was defect 14.11.2026).
    let types: &[(&str, &str)] = &[
        ("theme-window-decoration", ".themes/GoldenTheme/xfwm4/themerc"),
        ("theme-conky", ".config/conky/golden-theme-conky/GoldenTheme/index.theme"),
        ("theme-bundle", ".local/share/rexon/bundles/GoldenTheme/index.theme"),
        ("theme-icons", ".local/share/icons/GoldenTheme/index.theme"),
        ("theme-cursors", ".local/share/icons/GoldenTheme"),
        ("theme-gtk", ".themes/GoldenTheme/gtk-3.0/gtk.css"),
        ("theme-kvantum", ".config/Kvantum/GoldenTheme"),
        ("theme-kde-plasma", ".local/share/plasma/desktoptheme/GoldenTheme"),
        ("theme-kde-global", ".local/share/plasma/look-and-feel/GoldenTheme"),
        ("theme-sounds", ".local/share/sounds/GoldenTheme"),
        ("theme-xfwm4", ".themes/GoldenTheme"),
    ];
    let pristine = tree(&home);
    for (t, landed) in types {
        let id = format!("golden-{t}");
        let rxn = work.join(format!("{id}.rxn"));
        let manifest = serde_json::json!({"id": id, "name": format!("Golden {t}"), "type": t, "version": "1.0"});
        let files = theme_files("GoldenTheme");
        let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        make_rxn(&rxn, &manifest, &refs);

        let got = ext_exec::install_from_file(&rxn.to_string_lossy());
        let installed = got.unwrap_or_else(|e| panic!("{t}: установка отказала: {e}"));
        assert_eq!(installed.name, format!("Golden {t}"), "{t}: имя");
        assert!(home.join(landed).exists(), "{t}: тема не легла в {landed}; дерево: {:#?}", tree(&home));
        assert!(ext_dir(&home).join(&id).join("manifest.json").is_file(), "{t}: копии пакета нет");
        assert_eq!(state(&home)[&id], serde_json::json!(true), "{t}: нет отметки «включено»");
        let after_install = tree(&home);

        // Repeating the same package is refused, nothing changes on disk.
        let again = ext_exec::install_from_file(&rxn.to_string_lossy());
        let err = again.err().unwrap_or_else(|| panic!("{t}: повторная установка прошла молча"));
        assert!(err.contains("already installed"), "{t}: не тот отказ повтора: {err}");
        assert_eq!(tree(&home), after_install, "{t}: отказ повтора тронул профиль");

        // Removal: the profile as before installation, no record.
        ext_exec::uninstall(&id).unwrap_or_else(|e| panic!("{t}: удаление отказало: {e}"));
        assert_eq!(tree(&home), pristine, "{t}: после удаления в профиле остались следы");
        let theme_root = &landed[..landed.find("GoldenTheme").unwrap() + "GoldenTheme".len()];
        assert!(!home.join(theme_root).exists(), "{t}: каталог темы {theme_root} остался");
        assert!(!ext_dir(&home).join(&id).exists(), "{t}: копия пакета осталась");
        assert!(state(&home).get(&id).is_none(), "{t}: отметка состояния осталась");
    }

    // ── 1b. The same packages — removal by the SHARED install log ──────
    // Installation records the paths it put (step 3 of the installer merge);
    // removal by the log — only the file layer, no programs — must bring the
    // profile back to the original just like removal by the plan.
    for (t, _) in types {
        let id = format!("golden-{t}");
        let rxn = work.join(format!("{id}.rxn"));
        ext_exec::install_from_file(&rxn.to_string_lossy()).unwrap_or_else(|e| panic!("{t}: установка: {e}"));
        let after_install = tree(&home);
        let (mut j, key) = journal_entry(&home, &id);
        let e = &j.items[&key];
        assert!(e.placed_known && !e.placed.is_empty(), "{t}: журнал без положенных путей: {e:#?}");
        assert_eq!(e.enabled, Some(true), "{t}: журнал: «включено»");
        // Log checked against the map and the directory: the file exists, the profile is the same.
        assert_eq!(tree(&home), after_install, "{t}: запись журнала тронула профиль");
        remove_by_journal(&home, &mut j, &key);
        assert_eq!(tree(&home), pristine, "{t}: снятие по журналу оставило следы или сняло лишнее");
        assert!(!ext_dir(&home).join(&id).exists(), "{t}: копия пакета осталась");
    }

    // ── 1c. Removal with `ext_exec::uninstall` by the shared log (step 3d) ──
    use rexon_core::install_journal as journal;
    let rexon_dir = home.join(".config/rexon");
    let pkg = |id: &str, t: &str| {
        let rxn = work.join(format!("{id}.rxn"));
        let files = theme_files("GoldenTheme");
        let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        make_rxn(&rxn, &serde_json::json!({"id": id, "name": id, "type": t, "version": "1.0"}), &refs);
        ext_exec::install_from_file(&rxn.to_string_lossy()).unwrap_or_else(|e| panic!("{id}: установка: {e}"));
    };

    // A package installed BEFORE the log (no paths in the record): by the old plan.
    pkg("golden-pre-journal", "theme-icons");
    std::fs::remove_file(journal::journal_path(&rexon_dir)).expect("общий журнал был");
    let (j, key) = journal_entry(&home, "golden-pre-journal");
    assert!(!j.items[&key].placed_known && j.items[&key].placed.is_empty(), "запись до журнала: {:#?}", j.items[&key]);
    ext_exec::uninstall("golden-pre-journal").expect("пакет до журнала: удаление");
    assert_eq!(tree(&home), pristine, "пакет до журнала: после удаления остались следы");
    assert!(!ext_dir(&home).join("golden-pre-journal").exists(), "пакет до журнала: копия пакета осталась");

    // In place of the theme directory there is now SOMEONE ELSE'S (the person replaced it with a link to
    // their own theme): the log does not remove it, the rest of ours is removed.
    pkg("golden-foreign", "theme-icons");
    let landed = home.join(".local/share/icons/GoldenTheme");
    std::fs::remove_dir_all(&landed).unwrap();
    let mine = home.join("Темы/GoldenTheme");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::write(mine.join("index.theme"), "[Icon Theme]\nName=Моя\n").unwrap();
    std::os::unix::fs::symlink(&mine, &landed).unwrap();
    let with_foreign = tree(&home);
    ext_exec::uninstall("golden-foreign").expect("чужое на месте: удаление");
    assert!(std::fs::symlink_metadata(&landed).is_ok_and(|m| m.file_type().is_symlink()), "снята чужая ссылка на месте темы");
    assert_eq!(tree(&home), with_foreign, "чужое на месте: тронуто чужое или остались следы");
    assert!(!ext_dir(&home).join("golden-foreign").exists(), "чужое на месте: копия пакета осталась");
    std::fs::remove_file(&landed).unwrap();
    std::fs::remove_dir_all(home.join("Темы")).unwrap();
    assert_eq!(tree(&home), pristine, "чужое на месте: после уборки остались следы");

    // The package's GTK theme in `~/.local/share/themes` with a `~/.themes` link.
    // No `.rxn` type puts anything there today (a GTK theme goes into
    // `~/.themes`), so the layout is faked: the theme is moved there,
    // the log record is adjusted, the link is set by the real `sync_theme_links`.
    pkg("golden-gtk-link", "theme-gtk");
    let in_themes = home.join(".themes/GoldenTheme");
    let xdg_theme = home.join(".local/share/themes/GoldenTheme");
    std::fs::create_dir_all(xdg_theme.parent().unwrap()).unwrap();
    std::fs::rename(&in_themes, &xdg_theme).unwrap();
    let (mut j, key) = journal_entry(&home, "golden-gtk-link");
    let was = in_themes.to_string_lossy().into_owned();
    for p in &mut j.items.get_mut(&key).unwrap().placed {
        if p.path == was {
            p.path = xdg_theme.to_string_lossy().into_owned();
        }
    }
    journal::save_at(&rexon_dir, &j).unwrap();
    let lines = rexon_core::installed_scan::sync_theme_links(&home.to_string_lossy());
    assert_eq!(lines.len(), 1, "ссылка ~/.themes не поставлена: {lines:?}");
    let (j, key) = journal_entry(&home, "golden-gtk-link");
    let link = j.items[&key].placed.iter().find(|p| p.path == was).unwrap_or_else(|| panic!("ссылка не в журнале: {:#?}", j.items[&key]));
    assert_eq!(link.what, Some(journal::What::Link), "ссылка ~/.themes записана не ссылкой");
    ext_exec::uninstall("golden-gtk-link").expect("тема GTK со ссылкой: удаление");
    assert!(std::fs::symlink_metadata(&in_themes).is_err(), "осталась ссылка ~/.themes/GoldenTheme");
    assert!(!xdg_theme.exists(), "тема осталась в ~/.local/share/themes");
    assert_eq!(tree(&home), pristine, "тема GTK со ссылкой: после удаления остались следы");

    // Borders over THE PERSON'S THEME with the same name in ~/.themes: installation does not
    // overwrite it, removal does not take it off (D1).
    let human = home.join(".themes/FamilyTheme");
    std::fs::create_dir_all(human.join("gtk-3.0")).unwrap();
    std::fs::write(human.join("gtk-3.0/gtk.css"), "/* своя */").unwrap();
    let with_human = tree(&home);
    // Same with removal by the shared log: the person's theme is not recorded and stays intact.
    let rxn = work.join("golden-deco-human-j.rxn");
    let files = theme_files("FamilyTheme");
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    make_rxn(
        &rxn,
        &serde_json::json!({"id": "golden-deco-human-j", "name": "Deco", "type": "theme-window-decoration", "version": "1.0"}),
        &refs,
    );
    ext_exec::install_from_file(&rxn.to_string_lossy()).expect("рамки поверх своей темы (журнал): установка");
    let (mut j, key) = journal_entry(&home, "golden-deco-human-j");
    assert!(
        !j.items[&key].placed.iter().any(|p| Path::new(&p.path) == human),
        "тема человека попала в журнал пакета"
    );
    remove_by_journal(&home, &mut j, &key);
    assert_eq!(tree(&home), with_human, "снятие по журналу тронуло тему человека или оставило следы");
    let rxn = work.join("golden-deco-human.rxn");
    let files = theme_files("FamilyTheme");
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    make_rxn(
        &rxn,
        &serde_json::json!({"id": "golden-deco-human", "name": "Deco", "type": "theme-window-decoration", "version": "1.0"}),
        &refs,
    );
    ext_exec::install_from_file(&rxn.to_string_lossy()).expect("рамки поверх своей темы: установка");
    assert!(home.join(".local/share/aurorae/themes/FamilyTheme").is_dir(), "рамки не легли в Aurorae");
    assert!(!human.join("xfwm4").exists(), "установка перезаписала тему человека");
    assert!(!human.join(rexon_core::ext_manager::OWNER_MARK).exists(), "на теме человека метка пакета");
    ext_exec::uninstall("golden-deco-human").expect("рамки поверх своей темы: удаление");
    assert_eq!(tree(&home), with_human, "удаление тронуло тему человека или оставило следы");
    std::fs::remove_dir_all(&human).unwrap();
    assert_eq!(tree(&home), pristine, "после уборки темы человека остались следы");

    // The MAIN copy of a theme over the person's theme with the same name (defect
    // 25.10.2026: the plan removed it and put its own). The person's theme (which is also
    // a copy from an earlier Rexon without a mark: they cannot be told apart) stays byte-for-byte.
    // There is no other theme in the package — nothing to install: refused entirely with a reason,
    // the profile (with the extensions directory and the log) byte-for-byte as before. Both
    // plan paths: the shared one (icons) and the desktop matrix (xfwm4).
    for (t, base) in [("theme-icons", ".local/share/icons"), ("theme-xfwm4", ".themes")] {
        let human = home.join(base).join("HumanTheme");
        std::fs::create_dir_all(human.join("sub")).unwrap();
        std::fs::write(human.join("index.theme"), "[Icon Theme]\nName=Своя\n").unwrap();
        std::fs::write(human.join("sub/file"), "своё").unwrap();
        let human_bytes = bytes(&human);
        let with_human = tree(&home);
        let id = format!("golden-main-human-{t}");
        let rxn = work.join(format!("{id}.rxn"));
        let files = theme_files("HumanTheme");
        let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        make_rxn(&rxn, &serde_json::json!({"id": id, "name": id, "type": t, "version": "1.0"}), &refs);
        let before = bytes(&home);
        let err = ext_exec::install_from_file(&rxn.to_string_lossy())
            .err()
            .unwrap_or_else(|| panic!("{t}: установка поверх темы человека прошла, хотя ставить нечего"));
        assert_eq!(bytes(&human), human_bytes, "{t}: установка тронула тему человека");
        assert!(!human.join(rexon_core::ext_manager::OWNER_MARK).exists(), "{t}: на теме человека метка пакета");
        let human_s = human.to_string_lossy().into_owned();
        assert!(
            err.contains("was not installed") && err.contains("“HumanTheme” skipped") && err.contains(&human_s),
            "{t}: отказ без причины: {err}"
        );
        assert_eq!(bytes(&home), before, "{t}: отказ тронул профиль");
        assert_eq!(tree(&home), with_human, "{t}: отказ оставил следы");
        std::fs::remove_dir_all(&human).unwrap();
    }
    assert_eq!(tree(&home), pristine, "после уборки тем человека остались следы");

    // Our own earlier copy (the mark of THIS package) is replaced: the package is forgotten
    // (no copy in the extensions directory), the theme remains; installing the new
    // version puts the new contents and the mark, no notes.
    for (t, base) in [("theme-icons", ".local/share/icons"), ("theme-xfwm4", ".themes")] {
        let id = format!("golden-main-update-{t}");
        let theme = home.join(base).join("UpdTheme");
        let mk = |ver: &str| {
            let rxn = work.join(format!("{id}-{ver}.rxn"));
            let mut files = theme_files("UpdTheme");
            files.push(("data/UpdTheme/version".to_string(), ver.to_string()));
            let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            make_rxn(&rxn, &serde_json::json!({"id": id, "name": id, "type": t, "version": ver}), &refs);
            rxn
        };
        ext_exec::install_from_file(&mk("1.0").to_string_lossy()).unwrap_or_else(|e| panic!("{t}: первая установка: {e}"));
        assert_eq!(std::fs::read_to_string(theme.join(rexon_core::ext_manager::OWNER_MARK)).unwrap().trim(), id, "{t}: нет метки на своей копии");
        std::fs::write(theme.join("stale"), "от прежней версии").unwrap();
        std::fs::remove_dir_all(ext_dir(&home).join(&id)).unwrap();
        let done = ext_exec::install_from_file(&mk("2.0").to_string_lossy()).unwrap_or_else(|e| panic!("{t}: повтор своей: {e}"));
        assert!(done.notes.is_empty(), "{t}: повтор своей установки с заметками: {:?}", done.notes);
        assert_eq!(std::fs::read_to_string(theme.join("version")).unwrap(), "2.0", "{t}: своя копия не обновлена");
        assert!(!theme.join("stale").exists(), "{t}: прежняя копия не снята перед новой");
        assert_eq!(std::fs::read_to_string(theme.join(rexon_core::ext_manager::OWNER_MARK)).unwrap().trim(), id, "{t}: метка после обновления");
        ext_exec::uninstall(&id).unwrap_or_else(|e| panic!("{t}: удаление: {e}"));
        assert!(!theme.exists(), "{t}: обновлённая тема не снята удалением");
        assert_eq!(tree(&home), pristine, "{t}: после удаления обновлённой остались следы");
    }

    // A color scheme is a file, not a directory.
    let rxn = work.join("golden-colors.rxn");
    make_rxn(
        &rxn,
        &serde_json::json!({"id": "golden-colors", "name": "Golden Colors", "type": "theme-kde-colors", "version": "1.0"}),
        &[("data/Golden.colors", "[General]\nName=Golden\n")],
    );
    ext_exec::install_from_file(&rxn.to_string_lossy()).expect("цвета: установка");
    assert!(home.join(".local/share/color-schemes/Golden.colors").is_file(), "цвета: файла схемы нет");
    ext_exec::uninstall("golden-colors").expect("цвета: удаление");
    assert_eq!(tree(&home), pristine, "цвета: после удаления остались следы");

    // ── Owner rule for plasmoids (by KPlugin.Id) and single files ─────────
    // Defect 25.10.2026: the plan removed a plasmoid directory with the same id (and
    // updated it with `kpackagetool -u`), even someone else's, and silently overwrote
    // a color scheme of the same name. Someone else's stays byte-for-byte, the reason is named;
    // everything skipped means refused entirely, the profile as before installation; ours
    // is replaced. The kpackagetool6 stub only writes argv into a log and
    // fails (the manual branch of the plan) — the real tool is not started.
    let bin = PathBuf::from(std::env::var("PATH").unwrap());
    let kpt_log = home.parent().unwrap().join("kpackagetool.log");
    let fake_kpt = bin.join("kpackagetool6");
    std::fs::write(&fake_kpt, format!("#!/bin/sh\necho \"$@\" >> '{}'\nexit 1\n", kpt_log.display())).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_kpt, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let plasmoid_meta = r#"{"KPlugin": {"Id": "org.human.clock"}}"#;
    let plasmoid_archive = |qml: &str| -> Vec<u8> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opt = zip::write::SimpleFileOptions::default();
        z.start_file("metadata.json", opt).unwrap();
        z.write_all(plasmoid_meta.as_bytes()).unwrap();
        z.start_file("contents/ui/main.qml", opt).unwrap();
        z.write_all(qml.as_bytes()).unwrap();
        z.finish().unwrap().into_inner()
    };
    let make_plasmoid_rxn = |path: &Path, id: &str, archive: &[u8]| {
        let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        let opt = zip::write::SimpleFileOptions::default();
        z.start_file("manifest.json", opt).unwrap();
        z.write_all(serde_json::json!({"id": id, "name": id, "type": "theme-plasmoid", "version": "1.0"}).to_string().as_bytes()).unwrap();
        z.start_file("data/clock.plasmoid", opt).unwrap();
        z.write_all(archive).unwrap();
        z.finish().unwrap();
    };
    let plasmoid = home.join(".local/share/plasma/plasmoids/org.human.clock");
    std::fs::create_dir_all(plasmoid.join("contents/ui")).unwrap();
    std::fs::write(plasmoid.join("metadata.json"), plasmoid_meta).unwrap();
    std::fs::write(plasmoid.join("contents/ui/main.qml"), "часы человека").unwrap();
    let plasmoid_s = plasmoid.to_string_lossy().into_owned();
    let before = bytes(&home);
    // As a directory (folder named as the id) and as a `.plasmoid` archive (id inside).
    let as_dir = work.join("golden-plasmoid-dir.rxn");
    make_rxn(
        &as_dir,
        &serde_json::json!({"id": "golden-plasmoid-dir", "name": "golden-plasmoid-dir", "type": "theme-plasmoid", "version": "1.0"}),
        &[("data/org.human.clock/metadata.json", plasmoid_meta), ("data/org.human.clock/contents/ui/main.qml", "пакет")],
    );
    let as_archive = work.join("golden-plasmoid-zip.rxn");
    make_plasmoid_rxn(&as_archive, "golden-plasmoid-zip", &plasmoid_archive("пакет"));
    for rxn in [&as_dir, &as_archive] {
        let err = ext_exec::install_from_file(&rxn.to_string_lossy())
            .err()
            .unwrap_or_else(|| panic!("{}: установка поверх чужого плазмоида прошла", rxn.display()));
        assert!(err.contains("org.human.clock") && err.contains("skipped") && err.contains(&plasmoid_s), "{}: причина: {err}", rxn.display());
        assert_eq!(bytes(&home), before, "{}: отказ тронул профиль (плазмоид человека, пакет, журнал)", rxn.display());
    }
    assert!(!kpt_log.exists(), "kpackagetool запускался поверх чужого плазмоида: {:?}", std::fs::read_to_string(&kpt_log));
    std::fs::remove_dir_all(&plasmoid).unwrap();

    // Our own plasmoid: the archive installs (the tool failed — manual branch),
    // the directory by the id from the archive carries the mark; the package is forgotten, a new version
    // replaces our earlier copy; removal takes it off by the real id.
    let own_id = "golden-plasmoid-own";
    let v1 = work.join("golden-plasmoid-own-1.rxn");
    make_plasmoid_rxn(&v1, own_id, &plasmoid_archive("v1"));
    let done = ext_exec::install_from_file(&v1.to_string_lossy()).unwrap_or_else(|e| panic!("свой плазмоид: {e}"));
    assert!(done.notes.is_empty(), "свой плазмоид: заметки {:?}", done.notes);
    assert_eq!(std::fs::read_to_string(plasmoid.join("contents/ui/main.qml")).unwrap(), "v1");
    assert_eq!(std::fs::read_to_string(plasmoid.join(rexon_core::ext_manager::OWNER_MARK)).unwrap().trim(), own_id);
    assert!(std::fs::read_to_string(&kpt_log).unwrap().contains("-i"), "инструмент KDE не пробовался на чистом месте");
    std::fs::remove_dir_all(ext_dir(&home).join(own_id)).unwrap();
    let v2 = work.join("golden-plasmoid-own-2.rxn");
    make_plasmoid_rxn(&v2, own_id, &plasmoid_archive("v2"));
    let done = ext_exec::install_from_file(&v2.to_string_lossy()).unwrap_or_else(|e| panic!("своя прежняя копия: {e}"));
    assert!(done.notes.is_empty(), "своя прежняя копия: заметки {:?}", done.notes);
    assert_eq!(std::fs::read_to_string(plasmoid.join("contents/ui/main.qml")).unwrap(), "v2", "своя копия не заменена");
    ext_exec::uninstall(own_id).unwrap_or_else(|e| panic!("свой плазмоид: удаление: {e}"));
    assert!(!plasmoid.exists(), "свой плазмоид не снят удалением");
    assert!(std::fs::read_to_string(&kpt_log).unwrap().contains("-r org.human.clock"), "удаление сняло не тот id");
    assert_eq!(tree(&home), pristine, "плазмоиды: следы после удаления");
    std::fs::remove_file(&fake_kpt).unwrap();

    // Color schemes. The person's scheme with the same file name.
    let schemes = home.join(".local/share/color-schemes");
    std::fs::create_dir_all(&schemes).unwrap();
    let human_colors = schemes.join("Golden.colors");
    std::fs::write(&human_colors, "[General]\nName=Своя\n").unwrap();
    let human_colors_s = human_colors.to_string_lossy().into_owned();
    let with_human = tree(&home);
    let before = bytes(&home);
    let golden = "[General]\nName=Golden\n";
    let rxn = work.join("golden-colors-human.rxn");
    make_rxn(
        &rxn,
        &serde_json::json!({"id": "golden-colors-human", "name": "Golden Colors", "type": "theme-kde-colors", "version": "1.0"}),
        &[("data/Golden.colors", golden)],
    );
    let err = ext_exec::install_from_file(&rxn.to_string_lossy()).err().expect("цвета: поверх схемы человека прошло");
    assert!(err.contains("“Golden.colors” skipped") && err.contains(&human_colors_s), "цвета: причина: {err}");
    assert_eq!(bytes(&home), before, "цвета: отказ тронул профиль");
    // Two schemes, one taken: the second installs, the first gets a note; removal
    // does not take off the person's scheme.
    let rxn = work.join("golden-colors-two.rxn");
    make_rxn(
        &rxn,
        &serde_json::json!({"id": "golden-colors-two", "name": "Two", "type": "theme-kde-colors", "version": "1.0"}),
        &[("data/Golden.colors", golden), ("data/Silver.colors", "[General]\nName=Silver\n")],
    );
    let done = ext_exec::install_from_file(&rxn.to_string_lossy()).unwrap_or_else(|e| panic!("цвета: частичный пропуск: {e}"));
    assert!(done.notes.iter().any(|n| n.contains(&human_colors_s)), "цвета: заметки {:?}", done.notes);
    assert!(schemes.join("Silver.colors").is_file(), "цвета: свободная схема не легла");
    assert_eq!(std::fs::read_to_string(&human_colors).unwrap(), "[General]\nName=Своя\n", "цвета: схема человека перезаписана");
    ext_exec::uninstall("golden-colors-two").unwrap_or_else(|e| panic!("цвета: удаление: {e}"));
    assert_eq!(tree(&home), with_human, "цвета: удаление сняло лишнее или оставило своё");
    assert_eq!(std::fs::read_to_string(&human_colors).unwrap(), "[General]\nName=Своя\n", "цвета: удаление тронуло схему человека");
    // The same file byte-for-byte is ours: installed without notes, the path is in the log,
    // removal takes it off.
    std::fs::write(&human_colors, golden).unwrap();
    let rxn = work.join("golden-colors-same.rxn");
    make_rxn(
        &rxn,
        &serde_json::json!({"id": "golden-colors-same", "name": "Same", "type": "theme-kde-colors", "version": "1.0"}),
        &[("data/Golden.colors", golden)],
    );
    let done = ext_exec::install_from_file(&rxn.to_string_lossy()).unwrap_or_else(|e| panic!("цвета: тот же файл: {e}"));
    assert!(done.notes.is_empty(), "цвета: тот же файл с заметками {:?}", done.notes);
    let (j, key) = journal_entry(&home, "golden-colors-same");
    assert!(j.items[&key].placed.iter().any(|p| Path::new(&p.path) == human_colors), "цвета: тот же файл не записан своим");
    ext_exec::uninstall("golden-colors-same").unwrap_or_else(|e| panic!("цвета: тот же файл: удаление: {e}"));
    assert_eq!(tree(&home), pristine, "цвета: тот же файл не снят удалением");

    // Links in the archive (an alias and a dangling one) are carried over as links.
    let rxn = work.join("golden-links.rxn");
    let (mut files, links) = link_theme_files("data/");
    files.insert(0, (
        "manifest.json".to_string(),
        serde_json::json!({"id": "golden-links", "name": "Links", "type": "theme-icons", "version": "1.0"}).to_string(),
    ));
    let link_refs: Vec<(&str, &str)> = links.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    make_zip_with_links(&rxn, &files, &link_refs);
    ext_exec::install_from_file(&rxn.to_string_lossy()).expect("ссылки: установка");
    assert_links_landed(&home.join(".local/share/icons/LinkTheme"), ".rxn");
    ext_exec::uninstall("golden-links").expect("ссылки: удаление");
    assert_eq!(tree(&home), pristine, "ссылки: после удаления остались следы");

    // A font is a copy of a FILE into a file under its own name inside the set directory
    // (previously the step called "file into directory").
    let rxn = work.join("golden-font.rxn");
    make_rxn(
        &rxn,
        &serde_json::json!({"id": "golden-font", "name": "Font", "type": "font-pack", "version": "1.0"}),
        &[("data/Gold.ttf", "ttf")],
    );
    ext_exec::install_from_file(&rxn.to_string_lossy()).expect("шрифт: установка");
    let font = home.join(".local/share/fonts/rexon-golden-font/Gold.ttf");
    assert_eq!(std::fs::read_to_string(&font).ok().as_deref(), Some("ttf"), "шрифт не лёг файлом: {:#?}", tree(&home));
    ext_exec::uninstall("golden-font").expect("шрифт: удаление");
    assert_eq!(tree(&home), pristine, "шрифт: после удаления остались следы");

    // ── 2. Bad packages — refused without traces ──────────────────────
    let cases: &[(&str, serde_json::Value, &str)] = &[
        ("unknown-type", serde_json::json!({"id": "golden-bad1", "name": "x", "type": "theme-nonsense"}), "Unknown extension type"),
        ("no-id", serde_json::json!({"name": "x", "type": "theme-icons"}), "id"),
        ("traversal", serde_json::json!({"id": "../escape", "name": "x", "type": "theme-icons"}), ""),
        ("scripted-no-install", serde_json::json!({"id": "golden-bad3", "name": "x", "type": "plugin"}), "install.sh"),
        ("too-new", serde_json::json!({"id": "golden-bad4", "name": "x", "type": "theme-icons", "rexon_version": ">=999.0.0"}), ""),
    ];
    for (what, manifest, must) in cases {
        let rxn = work.join(format!("{what}.rxn"));
        let files = theme_files("BadTheme");
        let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        make_rxn(&rxn, manifest, &refs);
        let before_ext: Vec<String> = std::fs::read_dir(ext_dir(&home))
            .map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        let err = ext_exec::install_from_file(&rxn.to_string_lossy())
            .err()
            .unwrap_or_else(|| panic!("{what}: неверный пакет установился"));
        assert!(err.contains(must), "{what}: не тот отказ: {err}");
        assert_eq!(tree(&home), pristine, "{what}: отказ оставил следы в профиле");
        let after_ext: Vec<String> = std::fs::read_dir(ext_dir(&home))
            .map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        assert_eq!(after_ext, before_ext, "{what}: в каталоге расширений что-то появилось");
        assert!(!home.join(".config/escape").exists(), "{what}: пакет вышел за каталог расширений");
    }
    // Not an archive, and an archive without a manifest.
    let junk = work.join("junk.rxn");
    std::fs::write(&junk, b"not a zip").unwrap();
    let err = ext_exec::install_from_file(&junk.to_string_lossy()).err().expect("мусор установился");
    assert!(err.contains("damaged"), "мусор: не тот отказ: {err}");
    let nomani = work.join("nomani.rxn");
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&nomani).unwrap());
        z.start_file("data/x.txt", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(b"x").unwrap();
        z.finish().unwrap();
    }
    let err = ext_exec::install_from_file(&nomani.to_string_lossy()).err().expect("без манифеста установился");
    assert!(err.contains("manifest.json"), "без манифеста: не тот отказ: {err}");
    assert_eq!(tree(&home), pristine, "мусорные пакеты оставили следы");

    // ── 3. Rollback: a failure mid-install removes the package copy ───
    // The icon themes directory is taken by a FILE — the theme copy will not land.
    let icons = home.join(".local/share/icons");
    let _ = std::fs::remove_dir_all(&icons); // empty leftover of earlier installs
    std::fs::create_dir_all(icons.parent().unwrap()).unwrap();
    std::fs::write(&icons, b"blocker").unwrap();
    let rxn = work.join("golden-rollback.rxn");
    let files = theme_files("RollbackTheme");
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    make_rxn(
        &rxn,
        &serde_json::json!({"id": "golden-rollback", "name": "Rollback", "type": "theme-icons", "version": "1.0"}),
        &refs,
    );
    let res = ext_exec::install_from_file(&rxn.to_string_lossy());
    assert!(res.is_err(), "сбой копии темы не стал отказом установки");
    assert!(!ext_dir(&home).join("golden-rollback").exists(), "откат не снял копию пакета");
    assert!(state(&home).get("golden-rollback").is_none(), "откат оставил отметку «включено»");
    std::fs::remove_file(&icons).unwrap();

    // ── 3b. Rollback mid-plan: a replacement and someone else's directory (step 4) ──
    // Window borders on XFCE put the theme into `~/.local/share/aurorae/themes`, then
    // a copy into `~/.themes`. An earlier copy of this same package is already there (the plan
    // will replace it), and `~/.themes` with someone else's file is read-only:
    // the first copy lands, the second fails. Rollback must remove what was put
    // (and the package copy), restore the replaced theme and not touch someone else's; the profile
    // (with the state map and the shared log) byte-for-byte as before installation.
    let aurorae = home.join(".local/share/aurorae/themes/RollbackTheme");
    std::fs::create_dir_all(aurorae.join("xfwm4")).unwrap();
    std::fs::write(aurorae.join("xfwm4/themerc"), "прежняя рамка").unwrap();
    // An earlier copy of THIS package (with its mark) — the plan replaces it; someone else's
    // with the same name it would not touch at all.
    std::fs::write(aurorae.join(rexon_core::ext_manager::OWNER_MARK), "golden-rollback-mid\n").unwrap();
    let themes = home.join(".themes");
    let _ = std::fs::remove_dir_all(&themes); // empty leftover of earlier installs
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(themes.join("keep.txt"), "чужой файл").unwrap();
    read_only(&themes, true);
    if read_only_bites(&themes) {
        let before = bytes(&home);
        let rxn = work.join("golden-rollback-mid.rxn");
        make_rxn(
            &rxn,
            &serde_json::json!({"id": "golden-rollback-mid", "name": "Rollback mid", "type": "theme-window-decoration", "version": "1.0"}),
            &refs,
        );
        let res = ext_exec::install_from_file(&rxn.to_string_lossy());
        let after = bytes(&home);
        read_only(&themes, false);
        let err = res.err().expect("сбой посреди плана не стал отказом установки");
        assert!(err.contains(".themes"), "не тот сбой: {err}");
        assert_eq!(after, before, "откат посреди плана: профиль не как до установки");
        let j = rexon_core::install_journal::load_at(&home.join(".config/rexon")).expect("общий журнал читается");
        let key = rexon_core::install_journal::key_of(rexon_core::install_journal::Source::Rxn, "golden-rollback-mid");
        assert!(!j.items.contains_key(&key), "сбойная установка записана в общий журнал");
    } else {
        read_only(&themes, false);
        eprintln!("НЕ ПРОВЕРЕНО: запись в закрытый каталог проходит (root?) — сбой посреди плана не воспроизвести");
    }
    std::fs::remove_dir_all(home.join(".local/share/aurorae")).unwrap();
    std::fs::remove_dir_all(&themes).unwrap();

    // Removing something nonexistent is refused with the name.
    let err = ext_exec::uninstall("golden-absent").err().expect("удалилось несуществующее");
    assert!(err.contains("golden-absent"), "не тот отказ удаления: {err}");
}

/// Build a theme zip archive "as from the store": `(path, contents)`.
fn make_zip(path: &Path, files: &[(String, String)]) {
    make_zip_with_links(path, files, &[]);
}

/// Same with links inside the archive: `(link path, target)`.
fn make_zip_with_links(path: &Path, files: &[(String, String)], links: &[(&str, &str)]) {
    let mut z = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let opt = zip::write::SimpleFileOptions::default();
    for (name, text) in files {
        z.start_file(name.as_str(), opt).unwrap();
        z.write_all(text.as_bytes()).unwrap();
    }
    for (name, target) in links {
        z.add_symlink(*name, *target, opt).unwrap();
    }
    z.finish().unwrap();
}

/// An icon theme with an alias link and a dangling link: both must land
/// AS LINKS with the original target (shared copying, step 2 of the installer
/// merge), installation does not fail because of the dangling one.
fn assert_links_landed(theme_dir: &Path, what: &str) {
    let alias = theme_dir.join("scalable/alias.svg");
    let gone = theme_dir.join("scalable/gone.svg");
    for (link, target) in [(&alias, "a.svg"), (&gone, "missing.svg")] {
        let md = std::fs::symlink_metadata(link).unwrap_or_else(|_| panic!("{what}: ссылки {link:?} нет"));
        assert!(md.file_type().is_symlink(), "{what}: {link:?} легла не ссылкой");
        assert_eq!(std::fs::read_link(link).unwrap(), Path::new(target), "{what}: цель ссылки {link:?}");
    }
    assert_eq!(std::fs::read_to_string(&alias).unwrap(), "<svg/>", "{what}: синоним не ведёт к файлу");
}

fn link_theme_files(prefix: &str) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let files = vec![
        (format!("{prefix}LinkTheme/index.theme"), "[Icon Theme]\nName=LinkTheme\nDirectories=\n".to_string()),
        (format!("{prefix}LinkTheme/scalable/a.svg"), "<svg/>".to_string()),
    ];
    let links = vec![
        (format!("{prefix}LinkTheme/scalable/alias.svg"), "a.svg".to_string()),
        (format!("{prefix}LinkTheme/scalable/gone.svg"), "missing.svg".to_string()),
    ];
    (files, links)
}

#[test]
#[ignore = "запускается дочерним процессом live_appearance_roundtrip_on_temp_profile"]
fn live_appearance_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        eprintln!("live_appearance_child: только из live_appearance_roundtrip_on_temp_profile");
        return;
    }
    use rexon_core::appearance_installer::LiveInstaller;
    use rexon_core::models::ThemeItem;
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    assert!(home.to_string_lossy().contains("rexon-live-roundtrip-"), "HOME не песочница: {home:?}");
    let work = home.parent().unwrap().join("pkgs");
    std::fs::create_dir_all(&work).unwrap();
    let mut inst = LiveInstaller::new("xfce", Box::new(|_| {}));
    let pristine = tree(&home);

    // (section, archive files, where the theme lands)
    let icon = |t: &str| vec![
        (format!("{t}/index.theme"), format!("[Icon Theme]\nName={t}\nDirectories=\n")),
        (format!("{t}/scalable/a.svg"), "<svg/>".to_string()),
    ];
    let cases: Vec<(i64, Vec<(String, String)>, &str)> = vec![
        (132, icon("GoldIcons"), ".local/share/icons/GoldIcons/index.theme"),
        (107, vec![
            ("GoldCursor/index.theme".into(), "[Icon Theme]\nName=GoldCursor\n".into()),
            ("GoldCursor/cursors/left_ptr".into(), "x".into()),
        ], ".local/share/icons/GoldCursor/cursors/left_ptr"),
        (135, vec![
            ("GoldGtk/index.theme".into(), "[Desktop Entry]\nName=GoldGtk\n".into()),
            ("GoldGtk/gtk-3.0/gtk.css".into(), "".into()),
        ], ".themes/GoldGtk/gtk-3.0/gtk.css"),
        (123, vec![
            ("GoldKv/GoldKv.kvconfig".into(), "[%General]\n".into()),
            ("GoldKv/GoldKv.svg".into(), "<svg/>".into()),
        ], ".config/Kvantum/GoldKv/GoldKv.kvconfig"),
        (104, vec![
            ("GoldPlasma/metadata.json".into(), "{\"KPlugin\": {\"Id\": \"GoldPlasma\"}}".into()),
            ("GoldPlasma/widgets/panel-background.svg".into(), "<svg/>".into()),
        ], ".local/share/plasma/desktoptheme/GoldPlasma/metadata.json"),
        (722, vec![
            ("GoldLnf/metadata.json".into(), "{\"KPlugin\": {\"Id\": \"GoldLnf\"}}".into()),
            ("GoldLnf/contents/defaults".into(), "[kdeglobals]\n".into()),
        ], ".local/share/plasma/look-and-feel/GoldLnf/contents/defaults"),
    ];
    for (n, (cat_id, files, landed)) in cases.into_iter().enumerate() {
        let id = format!("90000{n}");
        let zip = work.join(format!("{id}.zip"));
        make_zip(&zip, &files);
        let item = ThemeItem {
            id: id.clone(),
            name: format!("Gold {cat_id}"),
            category_id: cat_id,
            download_name: format!("{id}.zip"),
            ..Default::default()
        };
        let (ok, msg, names) = inst.install(&item, &zip.to_string_lossy());
        assert!(ok, "{cat_id}: установка отказала: {msg}");
        assert!(!names.is_empty(), "{cat_id}: имён тем нет");
        assert!(home.join(landed).exists(), "{cat_id}: тема не легла в {landed}: {:#?}", tree(&home));
        let after = tree(&home);

        // Repeating the same purchase succeeds and puts the same thing.
        let (ok, msg, _) = inst.install(&item, &zip.to_string_lossy());
        assert!(ok, "{cat_id}: повторная установка отказала: {msg}");
        assert_eq!(tree(&home), after, "{cat_id}: повтор положил другое");

        // The shared install log checked against the store log: the same purchase,
        // the same paths, each with its kind from disk.
        use rexon_core::install_journal as journal;
        let rexon = home.join(".config/rexon");
        let key = journal::key_of(journal::Source::Store, &id);
        let j = journal::parse(std::fs::read_to_string(journal::journal_path(&rexon)).ok().as_deref())
            .expect("общий журнал записан установкой");
        let e = j.items.get(&key).unwrap_or_else(|| panic!("{cat_id}: покупки нет в общем журнале"));
        let old: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(rexon.join("ocs_installed.json")).unwrap()).unwrap();
        // The paths are the same, and a GTK theme is followed by its `~/.themes` link
        // (set by `sync_theme_links`; removal by the log takes it off too).
        let xdg_themes = home.join(".local/share/themes");
        let mut want: Vec<String> = Vec::new();
        for p in old[&id]["paths"].as_array().unwrap().iter().filter_map(|p| p.as_str()) {
            want.push(p.to_string());
            let theme = Path::new(p.trim_end_matches('/'));
            if theme.parent() == Some(xdg_themes.as_path()) {
                let link = home.join(".themes").join(theme.file_name().unwrap());
                if std::fs::symlink_metadata(&link).is_ok_and(|m| m.file_type().is_symlink()) {
                    want.push(link.to_string_lossy().into_owned());
                }
            }
        }
        assert_eq!(e.placed.iter().map(|p| p.path.clone()).collect::<Vec<_>>(), want, "{cat_id}: пути журналов разошлись");
        assert!(e.placed.iter().all(|p| p.what.is_some()), "{cat_id}: путь без вида: {e:#?}");
        if cat_id == 135 {
            let link = home.join(".themes/GoldGtk").to_string_lossy().into_owned();
            let rec = e.placed.iter().find(|p| p.path == link).unwrap_or_else(|| panic!("ссылка ~/.themes/GoldGtk не в журнале: {e:#?}"));
            assert_eq!(rec.what, Some(journal::What::Link), "ссылка ~/.themes записана видом «ссылка»");
        }
        // Readers get the store log through the shared one — they see the same as in
        // the store log itself.
        let view = journal::store_log_view(&rexon.join("ocs_installed.json").to_string_lossy());
        assert_eq!(serde_json::Value::Object(view), old, "{cat_id}: читатель видит не то, что в журнале магазина");

        // Removal by the log: the profile as it was.
        let (ok, msg) = inst.uninstall(&id);
        assert!(ok, "{cat_id}: удаление отказало: {msg}");
        let j = journal::parse(std::fs::read_to_string(journal::journal_path(&rexon)).ok().as_deref()).unwrap();
        assert!(!j.items.contains_key(&key), "{cat_id}: снятая покупка осталась в общем журнале");
        let now = tree(&home);
        let link = home.join(".themes/GoldGtk");
        assert!(std::fs::symlink_metadata(&link).is_err(), "{cat_id}: осталась ссылка ~/.themes/GoldGtk (Д4)");
        assert_eq!(now, pristine, "{cat_id}: после удаления остались следы");
    }

    // A color scheme as a single bare file.
    let colors = work.join("Gold.colors");
    std::fs::write(&colors, "[General]\nName=Gold\n[Colors:Window]\nBackgroundNormal=1,2,3\n").unwrap();
    let item = ThemeItem { id: "900100".into(), category_id: 112, download_name: "Gold.colors".into(), ..Default::default() };
    let (ok, msg, _) = inst.install(&item, &colors.to_string_lossy());
    assert!(ok, "цвета: {msg}");
    assert!(home.join(".local/share/color-schemes/Gold.colors").is_file(), "цвета: файла нет");
    let (ok, msg) = inst.uninstall("900100");
    assert!(ok, "цвета: удаление: {msg}");
    assert_eq!(tree(&home), pristine, "цвета: следы после удаления");

    // Links in a store archive (an alias and a dangling one) — the same behavior as
    // for `.rxn`.
    let zip = work.join("links.zip");
    let (files, links) = link_theme_files("");
    let link_refs: Vec<(&str, &str)> = links.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    make_zip_with_links(&zip, &files, &link_refs);
    let item = ThemeItem { id: "900300".into(), category_id: 132, download_name: "links.zip".into(), ..Default::default() };
    let (ok, msg, _) = inst.install(&item, &zip.to_string_lossy());
    assert!(ok, "ссылки: {msg}");
    assert_links_landed(&home.join(".local/share/icons/LinkTheme"), "магазин");
    let (ok, msg) = inst.uninstall("900300");
    assert!(ok, "ссылки: удаление: {msg}");
    assert_eq!(tree(&home), pristine, "ссылки: следы после удаления");

    // A single file over someone else's (owner rule for files, defect
    // 25.10.2026: the scheme was silently overwritten). A link pointing outside in its place is
    // also someone else's: no copy through it, no replacement of the link itself; the person's
    // scheme with different contents stays intact — both with a refusal reason.
    let outside = work.join("outside.colors");
    std::fs::write(&outside, "чужой").unwrap();
    let schemes = home.join(".local/share/color-schemes");
    std::fs::create_dir_all(&schemes).unwrap();
    let landed = schemes.join("Gold.colors");
    std::os::unix::fs::symlink(&outside, &landed).unwrap();
    let item = ThemeItem { id: "900101".into(), category_id: 112, download_name: "Gold.colors".into(), ..Default::default() };
    let before = bytes(&home);
    let (ok, msg, _) = inst.install(&item, &colors.to_string_lossy());
    assert!(!ok && msg.contains("“Gold") && msg.contains("skipped"), "цвета поверх ссылки: {msg}");
    assert_eq!(bytes(&home), before, "цвета поверх ссылки: отказ тронул профиль");
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "чужой", "копия прошла сквозь ссылку");
    std::fs::remove_file(&landed).unwrap();
    std::fs::write(&landed, "[General]\nName=Своя\n").unwrap();
    let before = bytes(&home);
    let (ok, msg, _) = inst.install(&item, &colors.to_string_lossy());
    assert!(!ok && msg.contains("skipped"), "цвета поверх схемы человека: {msg}");
    assert_eq!(bytes(&home), before, "цвета поверх схемы человека: отказ тронул профиль");
    // An archive with schemes: one taken by someone else is skipped with a reason, a free one lands.
    let pack = work.join("schemes.zip");
    make_zip(&pack, &[
        ("Pack/Gold.colors".into(), "[General]\nName=Gold v2\n".into()),
        ("Pack/Plain.colors".into(), "[General]\nName=Plain\n".into()),
    ]);
    let item = ThemeItem { id: "900102".into(), category_id: 112, download_name: "schemes.zip".into(), ..Default::default() };
    let (ok, msg, _) = inst.install(&item, &pack.to_string_lossy());
    assert!(ok && msg.contains("skipped"), "архив схем: {msg}");
    assert_eq!(std::fs::read_to_string(&landed).unwrap(), "[General]\nName=Своя\n", "архив схем перезаписал схему человека");
    assert!(schemes.join("Plain.colors").is_file(), "архив схем: свободная не легла");
    let (ok, msg) = inst.uninstall("900102");
    assert!(ok, "архив схем: удаление: {msg}");
    assert_eq!(std::fs::read_to_string(&landed).unwrap(), "[General]\nName=Своя\n", "удаление архива схем сняло схему человека");
    // The same file byte-for-byte is ours: it installs and is taken off by removal.
    std::fs::copy(&colors, &landed).unwrap();
    let item = ThemeItem { id: "900103".into(), category_id: 112, download_name: "Gold.colors".into(), ..Default::default() };
    let (ok, msg, _) = inst.install(&item, &colors.to_string_lossy());
    assert!(ok, "цвета, тот же файл: {msg}");
    let (ok, msg) = inst.uninstall("900103");
    assert!(ok, "цвета, тот же файл: удаление: {msg}");
    assert_eq!(tree(&home), pristine, "цвета: следы после удаления");

    // Rollback mid-install (step 4). Borders `GoldA` are purchased (v1); a new
    // version of the same purchase (v2) carries `GoldA` and `GoldB`. `GoldB` lands
    // as a subfolder inside a GTK theme of the same name, installed by hand and
    // made read-only — it fails there. Rollback: the earlier copy of `GoldA` (v1)
    // is restored, someone else's theme is intact, the logs as before installation; the profile
    // byte-for-byte as before.
    let item = ThemeItem { id: "900400".into(), name: "Gold frames".into(), category_id: 906, download_name: "frames.zip".into(), ..Default::default() };
    let v1 = work.join("frames-v1.zip");
    make_zip(&v1, &[("GoldA/xfwm4/themerc".into(), "v1".into())]);
    let (ok, msg, _) = inst.install(&item, &v1.to_string_lossy());
    assert!(ok, "рамка v1: {msg}");
    assert_eq!(std::fs::read_to_string(home.join(".themes/GoldA/xfwm4/themerc")).unwrap(), "v1", "рамка v1 не легла");
    let gtk = home.join(".themes/GoldB");
    std::fs::create_dir_all(gtk.join("gtk-3.0")).unwrap();
    std::fs::write(gtk.join("gtk-3.0/gtk.css"), "чужая тема").unwrap();
    read_only(&gtk, true);
    if read_only_bites(&gtk) {
        let before = bytes(&home);
        let v2 = work.join("frames-v2.zip");
        make_zip(&v2, &[
            ("GoldA/xfwm4/themerc".into(), "v2".into()),
            ("GoldB/xfwm4/themerc".into(), "v2".into()),
        ]);
        let (ok, msg, _) = inst.install(&item, &v2.to_string_lossy());
        let after = bytes(&home);
        read_only(&gtk, false);
        assert!(!ok, "сбой посреди установки не стал отказом: {msg}");
        assert_eq!(after, before, "откат магазина: профиль не как до установки");
    } else {
        read_only(&gtk, false);
        eprintln!("НЕ ПРОВЕРЕНО: запись в закрытый каталог проходит (root?) — сбой посреди копии не воспроизвести");
    }
    std::fs::remove_dir_all(&gtk).unwrap();
    let (ok, msg) = inst.uninstall("900400");
    assert!(ok, "рамка: удаление: {msg}");
    assert_eq!(tree(&home), pristine, "рамка: следы после удаления");

    // The fallback branch "wallpapers / fonts and the rest" (owner rule, defect
    // 25.10.2026: a directory with the same name was removed without a check). The person's
    // directory with the same name stays byte-for-byte, the reason is named; another
    // purchase's directory too; our own earlier purchase is replaced, removal takes off
    // only ours.
    for (cat_id, dir, file) in [
        (299, ".local/share/wallpapers", "contents/images/1920x1080.jpg"),
        (103, ".local/share/fonts", "GoldFont.ttf"),
    ] {
        let name = format!("Gold{cat_id}");
        let landed = home.join(dir).join(&name);
        std::fs::create_dir_all(&landed).unwrap();
        std::fs::write(landed.join("mine.txt"), "моё").unwrap();
        let zip_v = |v: &str| {
            let zip = work.join(format!("{name}-{v}.zip"));
            make_zip(&zip, &[(format!("{name}/{file}"), v.to_string())]);
            zip
        };
        let (v1, v2) = (zip_v("v1"), zip_v("v2"));
        let item = ThemeItem { id: format!("9005{cat_id}"), name: name.clone(), category_id: cat_id, download_name: format!("{name}.zip"), ..Default::default() };
        let before = bytes(&home);
        let (ok, msg, _) = inst.install(&item, &v1.to_string_lossy());
        assert!(!ok, "{cat_id}: каталог человека перезаписан: {msg}");
        assert!(msg.contains(&format!("“{name}”")) && msg.contains("skipped"), "{cat_id}: причина не названа: {msg}");
        assert_eq!(bytes(&home), before, "{cat_id}: отказ тронул профиль");
        std::fs::remove_dir_all(&landed).unwrap();

        // Our own purchase (v1), repeated with its new version (v2), is replaced.
        let (ok, msg, _) = inst.install(&item, &v1.to_string_lossy());
        assert!(ok, "{cat_id}: v1: {msg}");
        let (ok, msg, _) = inst.install(&item, &v2.to_string_lossy());
        assert!(ok, "{cat_id}: своя прежняя покупка не заменена: {msg}");
        assert_eq!(std::fs::read_to_string(landed.join(file)).unwrap(), "v2", "{cat_id}: прежняя версия осталась");

        // Another purchase with the same name is someone else's: refused, ours is intact.
        let other = ThemeItem { id: format!("9006{cat_id}"), ..item.clone() };
        let before = bytes(&home);
        let (ok, msg, _) = inst.install(&other, &v1.to_string_lossy());
        assert!(!ok && msg.contains("skipped"), "{cat_id}: другая покупка заменила нашу: {msg}");
        assert_eq!(bytes(&home), before, "{cat_id}: отказ другой покупки тронул профиль");
        let (ok, _) = inst.uninstall(&other.id);
        assert!(!ok, "{cat_id}: отказавшая покупка попала в журнал");

        let (ok, msg) = inst.uninstall(&item.id);
        assert!(ok, "{cat_id}: удаление: {msg}");
        assert_eq!(tree(&home), pristine, "{cat_id}: следы после удаления");
    }

    // Refusals without traces: sources instead of a widget, a broken archive.
    let src = work.join("src.zip");
    make_zip(&src, &[("widget-src/CMakeLists.txt".into(), "project(x)".into()), ("widget-src/main.cpp".into(), "".into())]);
    let item = ThemeItem { id: "900200".into(), category_id: 705, download_name: "src.zip".into(), ..Default::default() };
    let (ok, msg, _) = inst.install(&item, &src.to_string_lossy());
    assert!(!ok, "исходники вместо виджета установились: {msg}");
    assert!(msg.contains("ready-to-use package"), "исходники: не тот отказ: {msg}");
    let junk = work.join("junk.zip");
    std::fs::write(&junk, b"PK\x03\x04 broken").unwrap();
    let item = ThemeItem { id: "900201".into(), category_id: 132, download_name: "junk.zip".into(), ..Default::default() };
    let (ok, msg, _) = inst.install(&item, &junk.to_string_lossy());
    assert!(!ok, "битый архив установился: {msg}");
    assert_eq!(tree(&home), pristine, "отказы оставили следы");
    // Removing what is not in the log is refused.
    let (ok, _) = inst.uninstall("900999");
    assert!(!ok, "удалилось то, чего нет в журнале");

    // Step 5a: «Apply» — the click plan is run by the shared executor.
    // Icons on XFCE: a command (`xfconf-query` — a fake in the sandbox PATH,
    // answers with success) and writes to GTK 3/4 `settings.ini` and `kdeglobals`.
    let bin = home.parent().unwrap().join("bin");
    std::fs::write(bin.join("xfconf-query"), "#!/bin/sh\nexit 0\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin.join("xfconf-query"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let zip = work.join("press-icons.zip");
    make_zip(&zip, &[
        ("PressIcons/index.theme".into(), "[Icon Theme]\nName=PressIcons\nDirectories=scalable\n\n[scalable]\nSize=48\nType=Scalable\n".into()),
        ("PressIcons/scalable/a.svg".into(), "<svg/>".into()),
    ]);
    let item = ThemeItem { id: "900600".into(), name: "Press icons".into(), category_id: 132, download_name: "press-icons.zip".into(), ..Default::default() };
    let (ok, msg, _) = inst.install(&item, &zip.to_string_lossy());
    assert!(ok, "значки для «Применить»: {msg}");
    let cfg = home.join(".config");
    let rexon = cfg.join("rexon");
    // The person's icons "Mine" are installed — otherwise the GTK sanitizer would replace a nonexistent
    // name by itself after the click, and the check would catch it rather than the plan.
    let mine = home.join(".local/share/icons/Mine");
    std::fs::create_dir_all(mine.join("scalable")).unwrap();
    std::fs::write(mine.join("index.theme"), "[Icon Theme]\nName=Mine\nDirectories=scalable\n\n[scalable]\nSize=48\nType=Scalable\n").unwrap();
    std::fs::create_dir_all(cfg.join("gtk-3.0")).unwrap();
    std::fs::write(cfg.join("gtk-3.0/settings.ini"), "[Settings]\ngtk-icon-theme-name=Mine\ngtk-font-name=Sans 10\n").unwrap();
    // Configs without the Rexon directory (the log, theme history, the bridge live there).
    let configs = || -> Vec<(String, String)> {
        bytes(&cfg).into_iter().filter(|(p, _)| p != "rexon" && !p.starts_with("rexon/")).collect()
    };
    let before = configs();

    // A failure mid-click: the GTK 4 directory is read-only — the GTK 3 write
    // has already gone through, GTK 4 fails. Configs byte-for-byte as before, no record.
    let gtk4 = cfg.join("gtk-4.0");
    std::fs::create_dir_all(&gtk4).unwrap();
    let before_fail = configs();
    read_only(&gtk4, true);
    if read_only_bites(&gtk4) {
        let (ok, msg) = inst.apply(&item, "PressIcons");
        let after = configs();
        read_only(&gtk4, false);
        assert!(!ok, "сбой посреди «Применить» не стал отказом: {msg}");
        assert!(msg.contains("Not applied") && msg.contains("gtk-4.0/settings.ini"), "причина сбоя не названа: {msg}");
        assert_eq!(after, before_fail, "сбой посреди «Применить»: конфиги не как до нажатия");
        let j = rexon_core::install_journal::view_at(&rexon);
        assert!(!j.applied.contains_key("icons"), "неудачное нажатие записано в журнал");
    } else {
        read_only(&gtk4, false);
        eprintln!("НЕ ПРОВЕРЕНО: запись в закрытый каталог проходит (root?) — сбой посреди «Применить» не воспроизвести");
    }
    std::fs::remove_dir(&gtk4).unwrap();

    // A successful click → an "applied" record with a snapshot → «Restore as it was».
    let (ok, msg) = inst.apply(&item, "PressIcons");
    assert!(ok, "«Применить» значков: {msg}");
    let ini = std::fs::read_to_string(cfg.join("gtk-3.0/settings.ini")).unwrap();
    assert!(ini.contains("gtk-icon-theme-name=PressIcons") && ini.contains("gtk-font-name=Sans 10"), "settings.ini: {ini}");
    assert!(cfg.join("gtk-4.0/settings.ini").is_file(), "GTK 4 не записан");
    let j = rexon_core::install_journal::view_at(&rexon);
    let rec = j.applied.get("icons").unwrap_or_else(|| panic!("«Применить» не записано в общий журнал: {j:#?}"));
    assert_eq!(rec.theme, "PressIcons");
    assert_eq!(rec.item.as_deref(), Some("store:900600"), "запись не связана с покупкой");
    // The xfconf value (fake: the property exists, empty) is snapshotted before writing.
    let vals = j.applied_values.get("icons").unwrap_or_else(|| panic!("значения стола не сняты: {j:#?}"));
    assert!(
        vals.iter().any(|v| v.key == "xfconf xsettings /Net/IconThemeName" && v.restore.ends_with(&["-s".to_string(), String::new()])),
        "прежнее значение значков xfconf не снято: {vals:?}"
    );
    let first = &rec.files[0];
    assert_eq!(first.path, cfg.join("gtk-3.0/settings.ini").to_string_lossy());
    assert_eq!(
        std::fs::read_to_string(first.before.as_ref().expect("снимок прежнего settings.ini")).unwrap(),
        "[Settings]\ngtk-icon-theme-name=Mine\ngtk-font-name=Sans 10\n"
    );
    let left = rexon_core::install_journal::revert_applied_at(&rexon, "icons").unwrap();
    assert_eq!(left, Some(Vec::new()), "вернулось не всё");
    assert_eq!(configs(), before, "«вернуть как было»: конфиги не как до нажатия");
    assert!(!rexon_core::install_journal::view_at(&rexon).applied.contains_key("icons"), "запись осталась после возврата");
    let (ok, msg) = inst.uninstall("900600");
    assert!(ok, "значки для «Применить»: удаление: {msg}");

    // Step 5a, cursor and colors — also via the click plan. The whole profile (except
    // Rexon's housekeeping: the log, snapshots, theme history): the cursor also writes outside
    // `.config` (`~/.icons/default`).
    let profile = || -> Vec<(String, String)> {
        bytes(&home).into_iter().filter(|(p, _)| !p.starts_with(".config/rexon")).collect()
    };
    let journal_applied = |section: &str| rexon_core::install_journal::view_at(&rexon).applied.get(section).cloned();
    let cursor_dir = home.join(".local/share/icons/PressCursor");
    std::fs::create_dir_all(cursor_dir.join("cursors")).unwrap();
    std::fs::write(cursor_dir.join("cursors/left_ptr"), "x").unwrap();
    std::fs::write(cursor_dir.join("index.theme"), "[Icon Theme]\nName=PressCursor\n").unwrap();
    let cursor_item = ThemeItem { id: "900700".into(), name: "Press cursor".into(), category_id: 107, ..Default::default() };
    let before = profile();

    // A failure mid cursor click: `environment.d` is read-only —
    // GTK 3/4 `settings.ini` already written, the variables fail.
    let envd = cfg.join("environment.d");
    std::fs::create_dir_all(&envd).unwrap();
    let before_fail = profile();
    read_only(&envd, true);
    if read_only_bites(&envd) {
        let (ok, msg) = inst.apply(&cursor_item, "PressCursor");
        let after = profile();
        read_only(&envd, false);
        assert!(!ok, "курсор: сбой посреди нажатия не стал отказом: {msg}");
        assert!(msg.contains("Not applied") && msg.contains("environment.d"), "курсор: причина сбоя не названа: {msg}");
        assert_eq!(after, before_fail, "курсор: сбой посреди нажатия — профиль не как до нажатия");
        assert!(journal_applied("cursor").is_none(), "курсор: неудачное нажатие записано в журнал");
        // Size goes the same path.
        read_only(&envd, true);
        let (ok, msg) = inst.apply_cursor_size(48);
        let after = profile();
        read_only(&envd, false);
        assert!(!ok, "размер курсора: сбой не стал отказом: {msg}");
        assert_eq!(after, before_fail, "размер курсора: сбой — профиль не как до нажатия");
    } else {
        read_only(&envd, false);
        eprintln!("НЕ ПРОВЕРЕНО: запись в закрытый каталог проходит (root?) — сбой посреди курсора не воспроизвести");
    }
    std::fs::remove_dir(&envd).unwrap();

    // A successful click → a "what was there" snapshot → «Restore as it was» byte-for-byte.
    let (ok, msg) = inst.apply(&cursor_item, "PressCursor");
    assert!(ok, "«Применить» курсора: {msg}");
    let ini = std::fs::read_to_string(cfg.join("gtk-3.0/settings.ini")).unwrap();
    assert!(ini.contains("gtk-cursor-theme-name=PressCursor"), "курсор: settings.ini: {ini}");
    assert!(home.join(".icons/default/index.theme").is_file(), "курсор: ~/.icons/default не записан");
    let rec = journal_applied("cursor").unwrap_or_else(|| panic!("курсор: «Применить» не записано в общий журнал"));
    assert_eq!(rec.theme, "PressCursor");
    assert!(rec.files.iter().any(|f| f.path.ends_with("environment.d/rexon-cursor.conf")), "курсор: файл переменных не в снимке: {rec:#?}");
    // Size appends to the same section: the earlier "what was there" is not overwritten.
    let (ok, msg) = inst.apply_cursor_size(48);
    assert!(ok, "размер курсора: {msg}");
    assert!(std::fs::read_to_string(cfg.join("gtk-3.0/settings.ini")).unwrap().contains("gtk-cursor-theme-size=48"), "размер не записан");
    assert_eq!(journal_applied("cursor").unwrap().files[0].before, rec.files[0].before, "размер затёр раннее «что было»");
    let left = rexon_core::install_journal::revert_applied_at(&rexon, "cursor").unwrap();
    assert_eq!(left, Some(Vec::new()), "курсор: вернулось не всё");
    assert_eq!(profile(), before, "курсор: «вернуть как было» — профиль не как до нажатия");
    assert!(journal_applied("cursor").is_none(), "курсор: запись осталась после возврата");
    std::fs::remove_dir_all(&cursor_dir).unwrap();

    // Colors outside Plasma (XFCE) — Rexon layers: `kdeglobals`, the qt6ct palette,
    // a session variable and a "what was there" memo (`~/.local/state`).
    let schemes = home.join(".local/share/color-schemes");
    std::fs::create_dir_all(&schemes).unwrap();
    std::fs::write(schemes.join("Press.colors"), "[General]\nName=Press\n[Colors:Window]\nBackgroundNormal=10,20,30\n").unwrap();
    std::fs::write(cfg.join("kdeglobals"), "[General]\nfont=Sans,10\n").unwrap();
    std::fs::create_dir_all(cfg.join("qt6ct")).unwrap();
    std::fs::write(cfg.join("qt6ct/qt6ct.conf"), "[Appearance]\nstyle=Fusion\n").unwrap();
    let colors_item = ThemeItem { id: "900800".into(), name: "Press".into(), category_id: 112, ..Default::default() };
    let memo = home.join(".local/state/rexon/color-scheme-layer.json");
    let before = profile();
    assert!(!memo.exists());

    // A failure midway: the qt6ct palettes directory is read-only — the memo and `kdeglobals`
    // already written, the palette fails. Everything as before, no memo.
    let palettes = cfg.join("qt6ct/colors");
    std::fs::create_dir_all(&palettes).unwrap();
    let before_fail = profile();
    read_only(&palettes, true);
    if read_only_bites(&palettes) {
        let (ok, msg) = inst.apply(&colors_item, "Press");
        let after = profile();
        read_only(&palettes, false);
        assert!(!ok, "цвета: сбой посреди нажатия не стал отказом: {msg}");
        assert!(msg.contains("Not applied") && msg.contains("qt6ct/colors"), "цвета: причина сбоя не названа: {msg}");
        assert_eq!(after, before_fail, "цвета: сбой посреди нажатия — профиль не как до нажатия");
        assert!(!memo.exists(), "цвета: памятка осталась после сбоя");
        assert!(journal_applied("colors").is_none(), "цвета: неудачное нажатие записано в журнал");
    } else {
        read_only(&palettes, false);
        eprintln!("НЕ ПРОВЕРЕНО: запись в закрытый каталог проходит (root?) — сбой посреди цветов не воспроизвести");
    }
    std::fs::remove_dir(&palettes).unwrap();

    // A successful click → a snapshot → «Restore as it was» by the `applied` record.
    let (ok, msg) = inst.apply(&colors_item, "Press");
    assert!(ok, "«Применить» цветов: {msg}");
    assert!(msg.contains("KDE and Qt apps"), "цвета: не тот текст удачи: {msg}");
    let kg = std::fs::read_to_string(cfg.join("kdeglobals")).unwrap();
    assert!(kg.contains("ColorScheme=Press") && kg.contains("font=Sans,10"), "kdeglobals: {kg}");
    assert!(cfg.join("qt6ct/colors/Press.conf").is_file(), "палитра qt6ct не записана");
    assert!(memo.is_file(), "памятка слоя не записана");
    let rec = journal_applied("colors").unwrap_or_else(|| panic!("цвета: «Применить» не записано в общий журнал"));
    assert_eq!(rec.theme, "Press");
    let memo_rec = rec.files.iter().position(|f| f.path == memo.to_string_lossy()).unwrap_or_else(|| panic!("памятки нет в снимке: {rec:#?}"));
    let kg_rec = rec.files.iter().position(|f| f.path == cfg.join("kdeglobals").to_string_lossy()).expect("kdeglobals в снимке");
    assert!(memo_rec < kg_rec && rec.files[memo_rec].before.is_none(), "памятка — до записей слоя, заведена нажатием: {rec:#?}");
    let left = rexon_core::install_journal::revert_applied_at(&rexon, "colors").unwrap();
    assert_eq!(left, Some(Vec::new()), "цвета: вернулось не всё");
    assert_eq!(profile(), before, "цвета: «вернуть как было» — профиль не как до нажатия");
    assert!(!memo.exists(), "цвета: памятка не снята возвратом");

    // The old revert by the layer memo still works after the new path
    // (the records coexist): the memo files are as before. What the memo does not know
    // (created directories, the variables block mirrored into
    // `labwc/environment`) is picked up by the remaining `applied` record — after
    // it the profile is byte-for-byte as before.
    let (ok, msg) = inst.apply(&colors_item, "Press");
    assert!(ok, "цвета, второй раз: {msg}");
    use rexon_core::shell_palette as sp;
    sp::revert_scheme_layer(&sp::PaletteHome::live()).expect("возврат по памятке слоя");
    let file = |rel: &str| before.iter().find(|(p, _)| p == rel).cloned();
    let now = profile();
    for rel in [".config/kdeglobals", ".config/qt6ct/qt6ct.conf"] {
        assert_eq!(now.iter().find(|(p, _)| p == rel).cloned(), file(rel), "цвета: возврат по памятке не вернул {rel}");
    }
    for rel in [".config/qt6ct/colors/Press.conf", ".config/environment.d/rexon-qt-style.conf"] {
        assert!(!now.iter().any(|(p, _)| p == rel), "цвета: возврат по памятке оставил {rel}");
    }
    assert!(!memo.exists(), "цвета: памятка осталась");
    let left = rexon_core::install_journal::revert_applied_at(&rexon, "colors").unwrap();
    assert_eq!(left, Some(Vec::new()), "цвета: запись applied после возврата по памятке");
    assert_eq!(profile(), before, "цвета: запись applied после возврата по памятке испортила профиль");
    assert!(!memo.exists(), "цвета: памятка вернулась");

    // Step 5a, sound — via the click plan: writes with commands (the desktop's `xfconf-query`
    // and `kwriteconfig6` for KDE programs — fakes in the sandbox
    // PATH, only sh builtins: the sandbox PATH does not even have
    // `cat`; the xfconf store is outside the profile), then the name read
    // back. `kdeglobals` is written by a command bypassing rollback — its "what was there" is
    // snapshotted by the `Guard` step, and «Restore as it was» restores it byte-for-byte.
    let fake = |name: &str, body: String| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(bin.join(name), body).unwrap();
        std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    let store = home.parent().unwrap().join("xfconf-store");
    fake("xfconf-query", format!(
        "#!/bin/sh\nset_=0; val=\"\"\nwhile [ $# -gt 0 ]; do case \"$1\" in -s) set_=1; val=\"$2\"; shift;; esac; shift; done\n\
         if [ $set_ = 1 ]; then printf %s \"$val\" > '{0}'; else IFS= read -r v < '{0}'; printf %s \"$v\"; fi\n",
        store.display()
    ));
    fake("kwriteconfig6", "#!/bin/sh\nfile=; group=; key=; val=\n\
        while [ $# -gt 0 ]; do case \"$1\" in --file) file=$2; shift;; --group) group=$2; shift;; --key) key=$2; shift;; --notify) ;; *) val=$1;; esac; shift; done\n\
        printf '[%s]\\n%s=%s\\n' \"$group\" \"$key\" \"$val\" >> \"${XDG_CONFIG_HOME:-$HOME/.config}/$file\"\n".to_string());
    let sound_dir = home.join(".local/share/sounds/PressSound");
    std::fs::create_dir_all(&sound_dir).unwrap();
    std::fs::write(sound_dir.join("index.theme"), "[Sound Theme]\nName=PressSound\n").unwrap();
    let sound_item = ThemeItem { id: "900900".into(), name: "Press sound".into(), category_id: 316, ..Default::default() };
    let before = profile();
    // No theme — refused without a single write.
    let (ok, msg) = inst.apply(&sound_item, "NoSuchSound");
    assert!(!ok, "звук: несуществующая тема применилась: {msg}");
    assert_eq!(profile(), before, "звук: отказ что-то записал");
    assert!(journal_applied("sounds").is_none(), "звук: отказ записан в журнал");
    let (ok, msg) = inst.apply(&sound_item, "PressSound");
    assert!(ok, "«Применить» звука: {msg}");
    assert_eq!(std::fs::read_to_string(&store).unwrap(), "PressSound", "звук: xfconf не записан");
    let kg = std::fs::read_to_string(cfg.join("kdeglobals")).unwrap();
    assert!(kg.contains("[Sounds]\nTheme=PressSound"), "звук: kdeglobals: {kg}");
    let rec = journal_applied("sounds").unwrap_or_else(|| panic!("звук: «Применить» не записано в общий журнал"));
    assert_eq!(rec.theme, "PressSound");
    assert!(
        rec.files.iter().any(|f| f.path == cfg.join("kdeglobals").to_string_lossy() && f.before.is_some()),
        "звук: «что было» kdeglobals не в снимке: {rec:#?}"
    );
    let left = rexon_core::install_journal::revert_applied_at(&rexon, "sounds").unwrap();
    assert_eq!(left, Some(Vec::new()), "звук: вернулось не всё");
    assert_eq!(profile(), before, "звук: «вернуть как было» — профиль не как до нажатия");
    // No checking program — written but not applied; the log is not written.
    std::fs::write(&store, "").unwrap();
    fake("xfconf-query", "#!/bin/sh\nexit 0\n".to_string());
    let (ok, msg) = inst.apply(&sound_item, "PressSound");
    assert!(!ok && msg.contains("sound theme"), "звук: непрочитанное имя сочтено удачей: {msg}");
    assert!(journal_applied("sounds").is_none(), "звук: неудача записана в журнал");

    // Step 5a, lock screen wallpaper — via the click plan: the
    // `kscreenlockerrc` keys are written by `kwriteconfig6` (a fake); the file does not exist before the click
    // — the snapshot remembers that the click created it, and revert removes it.
    let walls = home.join(".local/share/wallpapers");
    std::fs::create_dir_all(&walls).unwrap();
    std::fs::write(walls.join("press-lock.png"), "png").unwrap();
    let lock_item = ThemeItem { id: "901000".into(), name: "Press lock".into(), category_id: 904, ..Default::default() };
    let locker = cfg.join("kscreenlockerrc");
    let before = profile();
    assert!(!locker.exists());
    let (ok, msg) = inst.apply(&lock_item, "no-such.png");
    assert!(!ok && msg.contains("not found"), "обои блокировки: нет картинки — не отказ: {msg}");
    assert_eq!(profile(), before, "обои блокировки: отказ что-то записал");
    // The command failed — not applied, the log is not written, the profile as before.
    let kwrite_ok = std::fs::read_to_string(bin.join("kwriteconfig6")).unwrap();
    fake("kwriteconfig6", "#!/bin/sh\nexit 1\n".to_string());
    let (ok, msg) = inst.apply(&lock_item, "press-lock.png");
    assert!(!ok && msg.contains("Couldn't write the lock screen wallpaper"), "обои блокировки: отказ команды: {msg}");
    assert_eq!(profile(), before, "обои блокировки: отказ команды что-то оставил");
    assert!(journal_applied("lockwallpaper").is_none(), "обои блокировки: неудача записана в журнал");
    fake("kwriteconfig6", kwrite_ok);
    let (ok, msg) = inst.apply(&lock_item, "press-lock.png");
    assert!(ok && msg.contains("Lock screen wallpaper applied"), "«Применить» обоев блокировки: {msg}");
    let text = std::fs::read_to_string(&locker).unwrap();
    assert!(text.contains("Image=file://") && text.contains("WallpaperPlugin=org.kde.image"), "kscreenlockerrc: {text}");
    let rec = journal_applied("lockwallpaper").unwrap_or_else(|| panic!("обои блокировки: «Применить» не записано в общий журнал"));
    assert!(
        rec.files.iter().any(|f| f.path == locker.to_string_lossy() && f.before.is_none()),
        "обои блокировки: заведённый kscreenlockerrc не в снимке: {rec:#?}"
    );
    let left = rexon_core::install_journal::revert_applied_at(&rexon, "lockwallpaper").unwrap();
    assert_eq!(left, Some(Vec::new()), "обои блокировки: вернулось не всё");
    assert_eq!(profile(), before, "обои блокировки: «вернуть как было» — профиль не как до нажатия");

    // Step 5a, the desktop section (LXQt theme) — via the click plan: the key in
    // `lxqt.conf` is written by a bash script (real bash, grep, sed — as links
    // in the sandbox PATH, they write only into its profile), the result is the check "the desktop
    // returns after writing". The "what was there" of `lxqt.conf` is snapshotted by the `Guard` step.
    for tool in ["bash", "mkdir", "dirname", "grep", "sed"] {
        let real = ["/usr/bin", "/bin"].iter().map(|d| Path::new(d).join(tool)).find(|p| p.exists());
        std::os::unix::fs::symlink(real.unwrap_or_else(|| panic!("нет {tool}")), bin.join(tool)).unwrap();
    }
    let lxqt_theme = home.join(".local/share/lxqt/themes/PressLx");
    std::fs::create_dir_all(&lxqt_theme).unwrap();
    std::fs::write(lxqt_theme.join("lxqt-panel.qss"), "").unwrap();
    let lxqt_dir = cfg.join("lxqt");
    std::fs::create_dir_all(&lxqt_dir).unwrap();
    let lxqt_conf = lxqt_dir.join("lxqt.conf");
    std::fs::write(&lxqt_conf, "[General]\ntheme=Old\nicon_theme=Mine\n").unwrap();
    let lx_item = ThemeItem { id: "901100".into(), name: "Press LXQt".into(), category_id: 910, ..Default::default() };
    let before = profile();
    // The desktop did not accept it (directory read-only — the script did not write): not
    // applied, the profile as before, the log empty.
    read_only(&lxqt_dir, true);
    if read_only_bites(&lxqt_dir) {
        let (ok, msg) = inst.apply(&lx_item, "PressLx");
        read_only(&lxqt_dir, false);
        assert!(!ok && msg.contains("Couldn't apply"), "тема LXQt: незаписанное сочтено удачей: {msg}");
        assert_eq!(profile(), before, "тема LXQt: отказ что-то оставил");
        assert!(journal_applied("lxqttheme").is_none(), "тема LXQt: неудача записана в журнал");
    } else {
        read_only(&lxqt_dir, false);
        eprintln!("НЕ ПРОВЕРЕНО: запись в закрытый каталог проходит (root?) — отказ темы LXQt не воспроизвести");
    }
    let (ok, msg) = inst.apply(&lx_item, "PressLx");
    assert!(ok, "«Применить» темы LXQt: {msg}");
    let conf = std::fs::read_to_string(&lxqt_conf).unwrap();
    assert!(conf.contains("theme=PressLx") && conf.contains("icon_theme=Mine"), "lxqt.conf: {conf}");
    let rec = journal_applied("lxqttheme").unwrap_or_else(|| panic!("тема LXQt: «Применить» не записано в общий журнал"));
    assert!(
        rec.files.iter().any(|f| f.path == lxqt_conf.to_string_lossy() && f.before.is_some()),
        "тема LXQt: «что было» lxqt.conf не в снимке: {rec:#?}"
    );
    let left = rexon_core::install_journal::revert_applied_at(&rexon, "lxqttheme").unwrap();
    assert_eq!(left, Some(Vec::new()), "тема LXQt: вернулось не всё");
    assert_eq!(profile(), before, "тема LXQt: «вернуть как было» — профиль не как до нажатия");
}
