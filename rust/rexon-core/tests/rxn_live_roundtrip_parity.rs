//! Страховка перед слиянием установщиков: ЖИВОЙ путь установки `.rxn`
//! (`ext_exec::install_from_file` → `exec_plan_noted` → `uninstall`) на
//! временном профиле.
//!
//! Планы по типам и столам сторожит `rxn_install_plans_parity.rs`; здесь —
//! что настоящий исполнитель делает с диском: файлы темы на месте, запись
//! «включено», отказ повторной установки, отказ на неверном пакете без
//! следов, откат при сбое посреди установки (копия пакета, файлы плана,
//! заменённое — профиль побайтно прежний, у `.rxn` и магазина) и чистый
//! профиль после удаления.
//!
//! ⚠️ Профиль владельца не трогается. Тело идёт в ДОЧЕРНЕМ процессе с
//! временными `HOME`/`XDG_*`, ПУСТЫМ `PATH` (ни одной программы — ни
//! gsettings, ни kpackagetool не запустятся) и несуществующей шиной
//! сеанса. Без своей переменной окружения дочернее тело ничего не делает.

use std::io::Write;
use std::path::{Path, PathBuf};

const CHILD_ENV: &str = "REXON_LIVE_ROUNDTRIP_CHILD";

/// Временный каталог прогона — в `$HOME/.cache` родителя, а не в `/tmp`
/// (там ОЗУ), и с номером процесса: параллельные прогоны не мешают друг другу.
fn sandbox(tag: &str) -> PathBuf {
    // Номер прогона: одно дочернее тело могут звать два теста сразу.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let base = std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    let dir = base.join(".cache").join(format!("rexon-live-roundtrip-{}-{n}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Прогнать дочернее тело `child` на временном профиле.
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

/// Д4. Удаление темы GTK по журналу покупки (`LiveInstaller::uninstall` →
/// `uninstall_from_log`) снимает `~/.local/share/themes/<тема>` и ссылку
/// `~/.themes/<тема>`, которую ставит установка
/// (`installed_scan::sync_theme_links`), — той же общей уборкой
/// (`theme_removal::tidy_after_removal`), что и плиточное «Удалить».
/// Прежде ссылка висела в пустоту до следующего скана (найдено страховкой
/// 25.10.2026, починено). Проверка — в `live_appearance_roundtrip_on_temp_profile`.
#[test]
fn defect_gtk_uninstall_drops_theme_link() {
    run_child("live_appearance_child");
}

/// Пакеты `.rxn`: `ext_exec`.
#[test]
fn live_install_roundtrip_on_temp_profile() {
    run_child("live_roundtrip_child");
}

/// «Оформление»: `LiveInstaller`.
#[test]
fn live_appearance_roundtrip_on_temp_profile() {
    run_child("live_appearance_child");
}

// ============================================================================
// Дочернее тело
// ============================================================================

/// Собрать `.rxn`: `manifest.json` + файлы `(путь, содержимое)`.
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

/// Все ФАЙЛЫ под `root` (относительно него; пустые базовые каталоги вроде
/// `~/.local/share/icons`, заведённые установкой, следом не считаются —
/// каталог самой темы проверяется отдельно), кроме служебного хозяйства Rexon (`.config/rexon` — каталог расширений, карта состояний,
/// мост) — оно проверяется отдельно.
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

/// Весь профиль побайтно — вместе с хозяйством Rexon (каталог расширений,
/// карта состояний, общий журнал): путь, вид, содержимое или цель ссылки.
/// Без журналов запуска (`.cache`, `.local/state`).
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

/// Закрыть каталог на запись (`ro = true`) или открыть снова.
fn read_only(dir: &Path, ro: bool) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(if ro { 0o555 } else { 0o755 })).unwrap();
}

/// Запись на закрытый каталог не проходит (не root): сбой посреди копии
/// воспроизводим.
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

/// Общий журнал установки песочницы и ключ записи пакета `id`.
fn journal_entry(home: &Path, id: &str) -> (rexon_core::install_journal::Journal, String) {
    use rexon_core::install_journal as journal;
    let j = journal::load_at(&home.join(".config/rexon")).expect("общий журнал читается");
    let key = journal::key_of(journal::Source::Rxn, id);
    assert!(j.items.contains_key(&key), "{id}: нет в общем журнале: {:#?}", j.items.keys().collect::<Vec<_>>());
    (j, key)
}

/// Снять запись `key` по общему журналу — файловым слоем, с подменным
/// запуском программ (ни одна не запускается), и записать журнал.
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

/// Пакет темы: `data/<Theme>/` с файлами, похожими на тему любого вида.
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
        // Не дочерний прогон — живой профиль. Ничего не делаем.
        eprintln!("live_roundtrip_child: только из live_install_roundtrip_on_temp_profile");
        return;
    }
    use rexon_core::ext_exec;
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    assert!(home.to_string_lossy().contains("rexon-live-roundtrip-"), "HOME не песочница: {home:?}");
    let work = home.parent().unwrap().join("pkgs");
    std::fs::create_dir_all(&work).unwrap();

    // ── 1. Установка → повтор → удаление по ходовым типам ──────────────
    // Рамки окон на XFCE кладут ещё и копию в ~/.themes, Conky и комплект —
    // копии подпапок под общими именами: удаление снимает их по метке
    // владельца (дефекты Д1–Д3, починены 25.10.2026).
    let types: &[(&str, &str)] = &[
        ("theme-window-decoration", ".themes/GoldenTheme/xfwm4/themerc"),
        ("theme-conky", ".config/conky/GoldenTheme/index.theme"),
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

        // Повтор того же пакета — отказ, на диске ничего не меняется.
        let again = ext_exec::install_from_file(&rxn.to_string_lossy());
        let err = again.err().unwrap_or_else(|| panic!("{t}: повторная установка прошла молча"));
        assert!(err.contains("already installed"), "{t}: не тот отказ повтора: {err}");
        assert_eq!(tree(&home), after_install, "{t}: отказ повтора тронул профиль");

        // Удаление — профиль как до установки, записи нет.
        ext_exec::uninstall(&id).unwrap_or_else(|e| panic!("{t}: удаление отказало: {e}"));
        assert_eq!(tree(&home), pristine, "{t}: после удаления в профиле остались следы");
        let theme_root = &landed[..landed.find("GoldenTheme").unwrap() + "GoldenTheme".len()];
        assert!(!home.join(theme_root).exists(), "{t}: каталог темы {theme_root} остался");
        assert!(!ext_dir(&home).join(&id).exists(), "{t}: копия пакета осталась");
        assert!(state(&home).get(&id).is_none(), "{t}: отметка состояния осталась");
    }

    // ── 1б. Те же пакеты — снятие по ОБЩЕМУ журналу установки ─────────
    // Установка записывает положенные пути (шаг 3 слияния установщиков);
    // снятие по журналу — только файловый слой, без программ, — обязано
    // вернуть профиль к исходному так же, как удаление по плану.
    for (t, _) in types {
        let id = format!("golden-{t}");
        let rxn = work.join(format!("{id}.rxn"));
        ext_exec::install_from_file(&rxn.to_string_lossy()).unwrap_or_else(|e| panic!("{t}: установка: {e}"));
        let after_install = tree(&home);
        let (mut j, key) = journal_entry(&home, &id);
        let e = &j.items[&key];
        assert!(e.placed_known && !e.placed.is_empty(), "{t}: журнал без положенных путей: {e:#?}");
        assert_eq!(e.enabled, Some(true), "{t}: журнал: «включено»");
        // Журнал сверен с картой и каталогом: файл есть, профиль тот же.
        assert_eq!(tree(&home), after_install, "{t}: запись журнала тронула профиль");
        remove_by_journal(&home, &mut j, &key);
        assert_eq!(tree(&home), pristine, "{t}: снятие по журналу оставило следы или сняло лишнее");
        assert!(!ext_dir(&home).join(&id).exists(), "{t}: копия пакета осталась");
    }

    // ── 1в. Удаление `ext_exec::uninstall` по общему журналу (шаг 3г) ──
    use rexon_core::install_journal as journal;
    let rexon_dir = home.join(".config/rexon");
    let pkg = |id: &str, t: &str| {
        let rxn = work.join(format!("{id}.rxn"));
        let files = theme_files("GoldenTheme");
        let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        make_rxn(&rxn, &serde_json::json!({"id": id, "name": id, "type": t, "version": "1.0"}), &refs);
        ext_exec::install_from_file(&rxn.to_string_lossy()).unwrap_or_else(|e| panic!("{id}: установка: {e}"));
    };

    // Пакет, поставленный ДО журнала (путей в записи нет): прежним планом.
    pkg("golden-pre-journal", "theme-icons");
    std::fs::remove_file(journal::journal_path(&rexon_dir)).expect("общий журнал был");
    let (j, key) = journal_entry(&home, "golden-pre-journal");
    assert!(!j.items[&key].placed_known && j.items[&key].placed.is_empty(), "запись до журнала: {:#?}", j.items[&key]);
    ext_exec::uninstall("golden-pre-journal").expect("пакет до журнала: удаление");
    assert_eq!(tree(&home), pristine, "пакет до журнала: после удаления остались следы");
    assert!(!ext_dir(&home).join("golden-pre-journal").exists(), "пакет до журнала: копия пакета осталась");

    // На месте каталога темы — уже ЧУЖОЕ (человек заменил его ссылкой на
    // свою тему): журнал его не снимает, прочее своё снято.
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

    // Тема GTK пакета в `~/.local/share/themes` со ссылкой `~/.themes`.
    // Ни один тип `.rxn` сегодня туда не кладёт (тема GTK ложится в
    // `~/.themes`), поэтому раскладка — подменой: тема перенесена туда,
    // запись журнала поправлена, ссылку ставит настоящий `sync_theme_links`.
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

    // Рамки поверх ТЕМЫ ЧЕЛОВЕКА с тем же именем в ~/.themes: установка её
    // не перезаписывает, удаление не снимает (Д1).
    let human = home.join(".themes/FamilyTheme");
    std::fs::create_dir_all(human.join("gtk-3.0")).unwrap();
    std::fs::write(human.join("gtk-3.0/gtk.css"), "/* своя */").unwrap();
    let with_human = tree(&home);
    // То же — снятием по общему журналу: тема человека не записана и цела.
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

    // ОСНОВНАЯ копия темы поверх темы человека с тем же именем (дефект
    // 25.10.2026: план снимал её и клал свою). Тема человека (она же —
    // копия прежнего Rexon без метки: их не отличить) остаётся побайтно.
    // Другой темы в пакете нет — ставить нечего: отказ целиком с причиной,
    // профиль (с каталогом расширений и журналом) побайтно прежний. Оба
    // пути плана: общий (значки) и матрица окружений (xfwm4).
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

    // Своя прежняя копия (метка ЭТОГО пакета) — заменяется: пакет забыт
    // (копии в каталоге расширений нет), тема осталась; установка новой
    // версии кладёт новое содержимое и метку, заметок нет.
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

    // Цветовая схема — файл, а не каталог.
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

    // ── Правило владельца у плазмоидов (по KPlugin.Id) и одиночных файлов ─
    // Дефект 25.10.2026: план снимал каталог плазмоида с тем же id (и
    // обновлял его `kpackagetool -u`), даже чужой, и молча перезаписывал
    // одноимённую цветовую схему. Чужое остаётся побайтно, причина названа;
    // пропущено всё — отказ целиком, профиль как до установки; своё
    // заменяется. Подмена kpackagetool6 только пишет argv в журнал и
    // отказывает (ручная ветка плана) — настоящий инструмент не запускается.
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
    // Каталогом (папка названа как id) и архивом `.plasmoid` (id — внутри).
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

    // Свой плазмоид: архив ставится (инструмент отказал — ручная ветка),
    // каталог по id из архива — с меткой; пакет забыт, новая версия
    // заменяет свою прежнюю копию; удаление снимает по настоящему id.
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

    // Цветовые схемы. Схема человека с тем же именем файла.
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
    // Две схемы, одна занята: вторая ставится, про первую — заметка; удаление
    // схему человека не снимает.
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
    // Тот же файл байт в байт — свой: установка без заметок, путь в журнале,
    // удаление снимает.
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

    // Ссылки в архиве (синоним и висячая) переносятся ссылками.
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

    // Шрифт — копия ФАЙЛА в файл под своим именем внутри каталога набора
    // (прежде шаг звал «файл в каталог»).
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

    // ── 2. Неверные пакеты — отказ без следов ─────────────────────────
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
    // Не архив и архив без манифеста.
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

    // ── 3. Откат: сбой посреди установки снимает копию пакета ─────────
    // Каталог тем значков занят ФАЙЛОМ — копия темы не ляжет.
    let icons = home.join(".local/share/icons");
    let _ = std::fs::remove_dir_all(&icons); // пустой остаток прежних установок
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

    // ── 3б. Откат посреди плана: замена и чужой каталог (шаг 4) ───────
    // Рамка окон на XFCE кладёт тему в `~/.local/share/aurorae/themes`, затем
    // копию в `~/.themes`. Там уже стоит прежняя копия этого же пакета (план
    // её заменит), а `~/.themes` с чужим файлом закрыт на запись:
    // первая копия ложится, на второй — сбой. Откат обязан снять положенное
    // (и копию пакета), вернуть заменённую тему и не тронуть чужое; профиль
    // (с картой состояний и общим журналом) — побайтно как до установки.
    let aurorae = home.join(".local/share/aurorae/themes/RollbackTheme");
    std::fs::create_dir_all(aurorae.join("xfwm4")).unwrap();
    std::fs::write(aurorae.join("xfwm4/themerc"), "прежняя рамка").unwrap();
    // Прежняя копия ЭТОГО пакета (с его меткой) — её план заменяет; чужую
    // с тем же именем он не тронул бы вовсе.
    std::fs::write(aurorae.join(rexon_core::ext_manager::OWNER_MARK), "golden-rollback-mid\n").unwrap();
    let themes = home.join(".themes");
    let _ = std::fs::remove_dir_all(&themes); // пустой остаток прежних установок
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

    // Удаление несуществующего — отказ с именем.
    let err = ext_exec::uninstall("golden-absent").err().expect("удалилось несуществующее");
    assert!(err.contains("golden-absent"), "не тот отказ удаления: {err}");
}

/// Собрать zip-архив темы «как из магазина»: `(путь, содержимое)`.
fn make_zip(path: &Path, files: &[(String, String)]) {
    make_zip_with_links(path, files, &[]);
}

/// То же со ссылками внутри архива: `(путь ссылки, цель)`.
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

/// Тема значков со ссылкой-синонимом и висячей ссылкой: обе обязаны лечь
/// ССЫЛКАМИ с прежней целью (общее копирование, шаг 2 слияния
/// установщиков), установка из-за висячей не падает.
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

    // (раздел, файлы архива, куда ляжет тема)
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

        // Повтор той же покупки — удаётся и кладёт то же самое.
        let (ok, msg, _) = inst.install(&item, &zip.to_string_lossy());
        assert!(ok, "{cat_id}: повторная установка отказала: {msg}");
        assert_eq!(tree(&home), after, "{cat_id}: повтор положил другое");

        // Общий журнал установки сверен с журналом магазина: та же покупка,
        // те же пути, у каждого — вид с диска.
        use rexon_core::install_journal as journal;
        let rexon = home.join(".config/rexon");
        let key = journal::key_of(journal::Source::Store, &id);
        let j = journal::parse(std::fs::read_to_string(journal::journal_path(&rexon)).ok().as_deref())
            .expect("общий журнал записан установкой");
        let e = j.items.get(&key).unwrap_or_else(|| panic!("{cat_id}: покупки нет в общем журнале"));
        let old: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(rexon.join("ocs_installed.json")).unwrap()).unwrap();
        // Пути — те же, а у темы GTK вслед за ней ещё её ссылка `~/.themes`
        // (её ставит `sync_theme_links`; удаление по журналу снимет и её).
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
        // Читатели берут журнал магазина через общий — видят то же, что в
        // самом журнале магазина.
        let view = journal::store_log_view(&rexon.join("ocs_installed.json").to_string_lossy());
        assert_eq!(serde_json::Value::Object(view), old, "{cat_id}: читатель видит не то, что в журнале магазина");

        // Удаление по журналу — профиль как был.
        let (ok, msg) = inst.uninstall(&id);
        assert!(ok, "{cat_id}: удаление отказало: {msg}");
        let j = journal::parse(std::fs::read_to_string(journal::journal_path(&rexon)).ok().as_deref()).unwrap();
        assert!(!j.items.contains_key(&key), "{cat_id}: снятая покупка осталась в общем журнале");
        let now = tree(&home);
        let link = home.join(".themes/GoldGtk");
        assert!(std::fs::symlink_metadata(&link).is_err(), "{cat_id}: осталась ссылка ~/.themes/GoldGtk (Д4)");
        assert_eq!(now, pristine, "{cat_id}: после удаления остались следы");
    }

    // Цветовая схема одним голым файлом.
    let colors = work.join("Gold.colors");
    std::fs::write(&colors, "[General]\nName=Gold\n[Colors:Window]\nBackgroundNormal=1,2,3\n").unwrap();
    let item = ThemeItem { id: "900100".into(), category_id: 112, download_name: "Gold.colors".into(), ..Default::default() };
    let (ok, msg, _) = inst.install(&item, &colors.to_string_lossy());
    assert!(ok, "цвета: {msg}");
    assert!(home.join(".local/share/color-schemes/Gold.colors").is_file(), "цвета: файла нет");
    let (ok, msg) = inst.uninstall("900100");
    assert!(ok, "цвета: удаление: {msg}");
    assert_eq!(tree(&home), pristine, "цвета: следы после удаления");

    // Ссылки в архиве магазина (синоним и висячая) — тем же поведением, что
    // у `.rxn`.
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

    // Одиночный файл поверх чужого (правило владельца для файлов, дефект
    // 25.10.2026: схема перезаписывалась молча). Ссылка наружу на месте —
    // тоже чужое: ни копии сквозь неё, ни замены самой ссылки; схема
    // человека с другим содержимым цела — обе с причиной отказа.
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
    // Архив со схемами: занятая чужой — пропуск с причиной, свободная ложится.
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
    // Тот же файл байт в байт — свой: ставится и снимается удалением.
    std::fs::copy(&colors, &landed).unwrap();
    let item = ThemeItem { id: "900103".into(), category_id: 112, download_name: "Gold.colors".into(), ..Default::default() };
    let (ok, msg, _) = inst.install(&item, &colors.to_string_lossy());
    assert!(ok, "цвета, тот же файл: {msg}");
    let (ok, msg) = inst.uninstall("900103");
    assert!(ok, "цвета, тот же файл: удаление: {msg}");
    assert_eq!(tree(&home), pristine, "цвета: следы после удаления");

    // Откат посреди установки (шаг 4). Рамка `GoldA` куплена (v1); новая
    // версия той же покупки (v2) несёт `GoldA` и `GoldB`. `GoldB` ложится
    // подпапкой внутрь темы GTK с тем же именем, поставленной руками и
    // закрытой на запись, — на ней сбой. Откат: прежняя копия `GoldA` (v1)
    // возвращена, чужая тема цела, журналы — как до установки; профиль
    // побайтно прежний.
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

    // Запасная ветка «обои / шрифты и прочее» (правило владельца, дефект
    // 25.10.2026: каталог с тем же именем снимался без проверки). Каталог
    // человека с тем же именем побайтно цел, причина названа; каталог другой
    // покупки — тоже; своя прежняя покупка заменяется, удаление снимает
    // только своё.
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

        // Своя покупка (v1), повтор её же новой версией (v2) — заменяется.
        let (ok, msg, _) = inst.install(&item, &v1.to_string_lossy());
        assert!(ok, "{cat_id}: v1: {msg}");
        let (ok, msg, _) = inst.install(&item, &v2.to_string_lossy());
        assert!(ok, "{cat_id}: своя прежняя покупка не заменена: {msg}");
        assert_eq!(std::fs::read_to_string(landed.join(file)).unwrap(), "v2", "{cat_id}: прежняя версия осталась");

        // Другая покупка с тем же именем — чужое: отказ, наша цела.
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

    // Отказы без следов: исходники вместо виджета, битый архив.
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
    // Удаление того, чего нет в журнале, — отказ.
    let (ok, _) = inst.uninstall("900999");
    assert!(!ok, "удалилось то, чего нет в журнале");

    // Шаг 5а: «Применить» — план нажатия исполняет общий исполнитель.
    // Значки в XFCE: команда (`xfconf-query` — подделка в PATH песочницы,
    // отвечает успехом) и записи `settings.ini` GTK 3/4 и `kdeglobals`.
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
    // Значки человека «Mine» стоят — иначе санитайзер GTK после нажатия
    // сам заменит несуществующее имя, и сверка поймала бы его, а не план.
    let mine = home.join(".local/share/icons/Mine");
    std::fs::create_dir_all(mine.join("scalable")).unwrap();
    std::fs::write(mine.join("index.theme"), "[Icon Theme]\nName=Mine\nDirectories=scalable\n\n[scalable]\nSize=48\nType=Scalable\n").unwrap();
    std::fs::create_dir_all(cfg.join("gtk-3.0")).unwrap();
    std::fs::write(cfg.join("gtk-3.0/settings.ini"), "[Settings]\ngtk-icon-theme-name=Mine\ngtk-font-name=Sans 10\n").unwrap();
    // Конфиги без каталога Rexon (там журнал, история тем, мост).
    let configs = || -> Vec<(String, String)> {
        bytes(&cfg).into_iter().filter(|(p, _)| p != "rexon" && !p.starts_with("rexon/")).collect()
    };
    let before = configs();

    // Сбой посреди нажатия: каталог GTK 4 закрыт на запись — запись GTK 3
    // уже прошла, на GTK 4 сбой. Конфиги побайтно прежние, записи нет.
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

    // Удачное нажатие → запись «applied» со снимком → «вернуть как было».
    let (ok, msg) = inst.apply(&item, "PressIcons");
    assert!(ok, "«Применить» значков: {msg}");
    let ini = std::fs::read_to_string(cfg.join("gtk-3.0/settings.ini")).unwrap();
    assert!(ini.contains("gtk-icon-theme-name=PressIcons") && ini.contains("gtk-font-name=Sans 10"), "settings.ini: {ini}");
    assert!(cfg.join("gtk-4.0/settings.ini").is_file(), "GTK 4 не записан");
    let j = rexon_core::install_journal::view_at(&rexon);
    let rec = j.applied.get("icons").unwrap_or_else(|| panic!("«Применить» не записано в общий журнал: {j:#?}"));
    assert_eq!(rec.theme, "PressIcons");
    assert_eq!(rec.item.as_deref(), Some("store:900600"), "запись не связана с покупкой");
    // Значение xfconf (подделка: свойство есть, пустое) снято до записи.
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

    // Шаг 5а, курсор и цвета — тоже планом нажатия. Профиль целиком (кроме
    // хозяйства Rexon: журнал, снимки, история тем): курсор пишет и вне
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

    // Сбой посреди нажатия курсора: `environment.d` закрыт на запись —
    // `settings.ini` GTK 3/4 уже записаны, на переменных сбой.
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
        // Размер — тот же путь.
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

    // Удачное нажатие → снимок «что было» → «вернуть как было» побайтно.
    let (ok, msg) = inst.apply(&cursor_item, "PressCursor");
    assert!(ok, "«Применить» курсора: {msg}");
    let ini = std::fs::read_to_string(cfg.join("gtk-3.0/settings.ini")).unwrap();
    assert!(ini.contains("gtk-cursor-theme-name=PressCursor"), "курсор: settings.ini: {ini}");
    assert!(home.join(".icons/default/index.theme").is_file(), "курсор: ~/.icons/default не записан");
    let rec = journal_applied("cursor").unwrap_or_else(|| panic!("курсор: «Применить» не записано в общий журнал"));
    assert_eq!(rec.theme, "PressCursor");
    assert!(rec.files.iter().any(|f| f.path.ends_with("environment.d/rexon-cursor.conf")), "курсор: файл переменных не в снимке: {rec:#?}");
    // Размер дописывает в тот же раздел: раннее «что было» не затёрто.
    let (ok, msg) = inst.apply_cursor_size(48);
    assert!(ok, "размер курсора: {msg}");
    assert!(std::fs::read_to_string(cfg.join("gtk-3.0/settings.ini")).unwrap().contains("gtk-cursor-theme-size=48"), "размер не записан");
    assert_eq!(journal_applied("cursor").unwrap().files[0].before, rec.files[0].before, "размер затёр раннее «что было»");
    let left = rexon_core::install_journal::revert_applied_at(&rexon, "cursor").unwrap();
    assert_eq!(left, Some(Vec::new()), "курсор: вернулось не всё");
    assert_eq!(profile(), before, "курсор: «вернуть как было» — профиль не как до нажатия");
    assert!(journal_applied("cursor").is_none(), "курсор: запись осталась после возврата");
    std::fs::remove_dir_all(&cursor_dir).unwrap();

    // Цвета вне Plasma (XFCE) — слои Rexon: `kdeglobals`, палитра qt6ct,
    // переменная сеанса и памятка «что было» (`~/.local/state`).
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

    // Сбой посреди: каталог палитр qt6ct закрыт — памятка и `kdeglobals`
    // уже записаны, на палитре сбой. Всё прежнее, памятки нет.
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

    // Удачное нажатие → снимок → «вернуть как было» по записи `applied`.
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

    // Прежний возврат по памятке слоя работает и после нового пути
    // (записи сосуществуют): файлы памятки прежние. Чего памятка не знает
    // (заведённые каталоги, блок переменных, отзеркаленный в
    // `labwc/environment`), добирает оставшаяся запись `applied` — после
    // неё профиль побайтно прежний.
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

    // Шаг 5а, звук — планом нажатия: записи командами (`xfconf-query`
    // окружения и `kwriteconfig6` для программ KDE — подделки в PATH
    // песочницы, только встроенные команды sh: в PATH песочницы нет даже
    // `cat`; хранилище xfconf — вне профиля), затем прочитанное
    // обратно имя. `kdeglobals` пишет команда мимо отката — его «что было»
    // снимает шаг `Guard`, и «вернуть как было» возвращает его побайтно.
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
    // Темы нет — отказ без единой записи.
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
    // Программы проверки нет — записано, но не применено; журнал не пишется.
    std::fs::write(&store, "").unwrap();
    fake("xfconf-query", "#!/bin/sh\nexit 0\n".to_string());
    let (ok, msg) = inst.apply(&sound_item, "PressSound");
    assert!(!ok && msg.contains("sound theme"), "звук: непрочитанное имя сочтено удачей: {msg}");
    assert!(journal_applied("sounds").is_none(), "звук: неудача записана в журнал");

    // Шаг 5а, обои экрана блокировки — планом нажатия: ключи
    // `kscreenlockerrc` пишет `kwriteconfig6` (подделка); файла до нажатия
    // нет — снимок помнит, что его завело нажатие, и возврат его снимает.
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
    // Команда отказала — не применено, журнал не пишется, профиль прежний.
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

    // Шаг 5а, раздел окружения (тема LXQt) — планом нажатия: ключ в
    // `lxqt.conf` пишет сценарий bash (настоящие bash, grep, sed — ссылками
    // в PATH песочницы, пишут только в её профиль), итог — сверка «окружение
    // отдаёт после записи». «Что было» `lxqt.conf` снимает шаг `Guard`.
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
    // Окружение не приняло (каталог закрыт — сценарий не записал): не
    // применено, профиль прежний, журнал пуст.
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
