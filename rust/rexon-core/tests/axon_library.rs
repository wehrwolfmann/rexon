//! Библиотека Razer Axon под Wine (`rexon_core::axon_library`) на
//! подставных файлах: поиск префиксов, разбор библиотеки (оба имени поля
//! типа, BOM, битый JSON), пути Windows → Linux, папка с обоями, указанная
//! вручную, подготовка к показу через подменную и внешнюю программу.
//!
//! Итог разбора всего подставного дома сверяется с эталоном
//! `golden/axon_library.json` (пути дома заменены на `$HOME`);
//! `REXON_UPDATE_GOLDEN=1` — переснять.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};


use rexon_core::axon_library as ax;
use serde_json::{json, Value};

/// Пустой каталог для одного теста.
fn bench(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("axon_library").join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn put(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

const BOM: &str = "\u{feff}";

fn axon_root(prefix: &Path, user: &str) -> PathBuf {
    prefix.join("drive_c/users").join(user).join("AppData/Local/Razer/RazerAxon")
}

/// Конфиг скачанных обоев: новый вид (`WallPaperType`, в строку) либо
/// старый (`Type`, с отступами).
fn resource_config(kind: &str, source: &str, old: bool) -> String {
    let source = source.replace('\\', "\\\\");
    if old {
        format!("{{\n\t\"Type\":\"{kind}\",\n\t\"Version\":\"1.0.0.0\",\n\t\"Source\":\"{source}\"\n}}")
    } else {
        format!("{BOM}{{\"WallPaperType\":\"{kind}\",\"Version\":\"1.0.0.0\",\"Source\":\"{source}\",\"ChromaPreset\":[]}}")
    }
}

fn download(dir: &Path, kind: &str, file: &str, old: bool) {
    put(&dir.join(ax::RESOURCE_CONFIG), &resource_config(kind, &format!("Resource\\{file}"), old));
    put(&dir.join("Resource").join(file), "контейнер");
}

/// Запись библиотеки.
fn entry(id: &str, title: &str, type_key: &str, kind: &str, extra: Value) -> Value {
    let mut v = json!({
        "Id": id,
        "Title": title,
        type_key: kind,
        "Audible": "0",
        "Author": {"author_name": format!("Автор {id}"), "author_icon": ""},
        "Resolution": null,
        "WallPaperStatus": "Uninstalled",
        "PlayResoucePath": null,
        "ThumbnailSource": format!("https://cdn.example/thumb/{id}.jpg"),
        "PreviewSource": format!("https://cdn.example/preview/{id}.webp"),
        "EffectType": "Dynamic",
    });
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

/// Подставной дом: `~/.wine` с Axon (вход выполнен + гость), префикс
/// Proton с битой библиотекой, бутылка Bottles без Axon.
fn home_with_axon(home: &Path) {
    let wine = home.join(".wine");
    let root = axon_root(&wine, "wehrwolfmann");
    let store = root.join("WallpaperSource");
    let account = root.join("RZR_0280435");
    let lib = json!([
        entry("12034", "Skyline", "Type", "VIDEO", json!({
            "Audible": "1",
            "Resolution": "3840x2160",
            "WallPaperStatus": "Installed",
            "PlayResoucePath": "C:\\users\\wehrwolfmann\\AppData\\Local\\Razer\\RazerAxon\\WallpaperSource\\1203421603840\\Resource\\3840x2160.mp4",
        })),
        // Путь показа пуст, разрешение есть — каталог по `{Id}{H}{W}`.
        entry("10038", "The Beauty of Space", "Type", "VIDEO", json!({
            "Resolution": "3840x2160",
            "WallPaperStatus": "Playing",
        })),
        entry("13300", "Skyline GTR", "Type", "VIDEO", json!({"Audible": "1"})),
        // Новый ключ типа в библиотеке.
        entry("13479", "Neo Rey by Night", "WallPaperType", "IMAGE", json!({
            "Resolution": "3840x2400",
            "WallPaperStatus": "Installed",
        })),
        entry("14001", "Web demo", "WallPaperType", "WEB", json!({})),
        // Записи без номера пропускаются.
        {"Title": "без номера"},
    ]);
    put(&account.join(ax::LIBRARY_FILE), &format!("{BOM}{lib}"));
    put(
        &account.join(ax::PLAYLIST_FILE),
        &format!("{BOM}{}", json!({"PlayLists": [{"MonitorId": "\\\\.\\DISPLAY1", "WallpaperIds": ["13305", "10038", "12034"], "CurrentPlayIndex": 1, "IsMute": true}]})),
    );
    put(
        &account.join(ax::USER_SETTING_FILE),
        &format!("{BOM}{}", json!({"SourceSavedPath": "C:\\users\\wehrwolfmann\\AppData\\Local\\Razer\\RazerAxon\\WallpaperSource"})),
    );
    download(&store.join("1203421603840"), "VIDEO", "3840x2160.mp4", false);
    download(&store.join("1003821603840"), "VIDEO", "RazerSpace_3840x2160.mp4", false);
    // Раскладка стороннего загрузчика (`{Id}`), старый ключ `Type`.
    download(&store.join("10038"), "VIDEO", "RazerSpace_3840x2160.mp4", true);
    download(&store.join("1347924003840"), "IMAGE", "3840x2400.jpg", false);
    // Служебные каталоги без описания — не обои.
    fs::create_dir_all(store.join("ResourceTmp/12034")).unwrap();
    fs::create_dir_all(store.join("Extracted")).unwrap();
    // Гость (до входа): старая библиотека с теми же обоями под другим именем.
    let guest = root.join("guest-2b48d659");
    put(&guest.join(ax::LIBRARY_FILE), &json!([entry("12034", "Skyline (гость)", "Type", "VIDEO", json!({}))]).to_string());
    let old = fs::File::options().write(true).open(guest.join(ax::LIBRARY_FILE)).unwrap();
    old.set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000)).unwrap();
    // Гость без библиотеки — не учётная запись.
    put(&root.join("guest-0000/UserSetting"), "{}");

    // Proton: битая библиотека — в журнал, остальное читается.
    let pfx = home.join(".local/share/Steam/steamapps/compatdata/1493710/pfx");
    put(&axon_root(&pfx, "steamuser").join("RZR_x/WallPaperData"), "[{\"Id\": \"1\",");
    // Bottles без Axon.
    fs::create_dir_all(home.join(".local/share/bottles/bottles/Games/drive_c/users/u")).unwrap();
}

fn norm(home: &Path, p: &Option<PathBuf>) -> Value {
    match p {
        Some(p) => Value::from(p.to_string_lossy().replace(&*home.to_string_lossy(), "$HOME")),
        None => Value::Null,
    }
}

fn summary(home: &Path, lib: &ax::AxonLibrary) -> Value {
    let h = home.to_string_lossy().into_owned();
    json!({
        "found": lib.found,
        "sourceDirs": lib.source_dirs.iter().map(|d| d.to_string_lossy().replace(&h, "$HOME")).collect::<Vec<_>>(),
        "problems": lib.problems.iter().map(|p| {
            // Текст ошибки разбора — от serde, сверяем только файл.
            p.split(": ").next().unwrap_or("").replace(&h, "$HOME")
        }).collect::<Vec<_>>(),
        "wallpapers": lib.wallpapers.iter().map(|w| json!({
            "id": w.id,
            "title": w.title,
            "author": w.author,
            "kind": format!("{:?}", w.kind),
            "effect": w.effect,
            "audible": w.audible,
            "resolution": w.resolution.map(|(x, y)| format!("{x}x{y}")),
            "cover": w.cover_url,
            "resourceDir": norm(home, &w.resource_dir),
            "container": norm(home, &w.container),
            "downloaded": w.downloaded,
            "playing": w.playing,
            "prefix": norm(home, &Some(w.prefix.clone())),
            "tile": ax::tile_line(w),
            "title_shown": ax::display_title(w),
        })).collect::<Vec<_>>(),
    })
}

fn check_golden(name: &str, doc: &Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name);
    let text = serde_json::to_string_pretty(doc).unwrap() + "\n";
    if std::env::var("REXON_UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        fs::write(&path, &text).unwrap();
    } else {
        let want = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (снять: REXON_UPDATE_GOLDEN=1)", path.display()));
        assert!(want == text, "эталон {} разошёлся:\n{text}", path.display());
    }
}

fn english() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
}

#[test]
fn golden_library_of_fake_home() {
    english();
    let home = bench("golden");
    home_with_axon(&home);
    // Ручная папка без библиотеки рядом: номер — из имени каталога.
    let manual = home.join("Copies/AxonCopy/WallpaperSource");
    download(&manual.join("1349424003840"), "IMAGE", "3840x2400.jpg", false);
    download(&manual.join("777"), "VIDEO", "clip.mp4", true);
    // Те же обои, что уже в префиксе, — без повтора.
    download(&manual.join("1203421603840"), "VIDEO", "3840x2160.mp4", false);
    let mut env = ax::AxonEnv::with_home(&home);
    env.source_dirs = vec![manual.clone(), home.join("нет-такой")];
    let lib = ax::load_library(&env);
    check_golden("axon_library.json", &summary(&home, &lib));
}

#[test]
fn prefixes_are_found_without_walking_the_disk() {
    let home = bench("prefixes");
    home_with_axon(&home);
    put(&home.join("Games/umu/umu-default/drive_c/.keep"), "");
    let other = bench("prefixes-env");
    fs::create_dir_all(other.join("drive_c")).unwrap();
    // Префикс Lutris из описания игры.
    let lutris = bench("prefixes-lutris");
    fs::create_dir_all(lutris.join("drive_c")).unwrap();
    put(
        &home.join(".config/lutris/games/axon-1.yml"),
        &format!("game:\n  exe: x.exe\n  prefix: '{}'\n", lutris.display()),
    );
    // Глубже одного уровня не ищем.
    fs::create_dir_all(home.join("Games/a/b/drive_c")).unwrap();
    let mut env = ax::AxonEnv::with_home(&home);
    env.wineprefix = Some(other.clone());
    let found = ax::candidate_prefixes(&env);
    assert_eq!(found[0], other, "$WINEPREFIX — первым");
    assert_eq!(found[1], home.join(".wine"));
    for want in [
        home.join(".local/share/bottles/bottles/Games"),
        home.join("Games/umu/umu-default"),
        home.join(".local/share/Steam/steamapps/compatdata/1493710/pfx"),
        lutris.clone(),
    ] {
        assert!(found.contains(&want), "{want:?} не найден: {found:?}");
    }
    assert!(!found.contains(&home.join("Games/a/b")));
    // Каталоги Axon — только с библиотекой.
    let roots = ax::find_roots(&env);
    assert_eq!(roots.len(), 2, "{roots:?}");
}

#[test]
fn empty_home_has_no_library() {
    let home = bench("empty");
    let lib = ax::load_library(&ax::AxonEnv::with_home(&home));
    assert!(!lib.found && lib.wallpapers.is_empty() && lib.source_dirs.is_empty());
}

#[test]
fn library_parses_both_type_fields_and_bom() {
    let text = format!(
        "{BOM}{}",
        json!([
            {"Id": "1", "Type": "VIDEO", "Audible": "1", "Author": {"author_name": "A"}},
            {"Id": 2, "WallPaperType": "IMAGE", "Author": "B", "Resolution": "1920x1080"},
            {"Id": "3", "Type": "", "WallPaperType": "WEB"},
        ])
    );
    let e = ax::parse_library(&text).unwrap();
    assert_eq!(e.len(), 3);
    assert_eq!((e[0].kind.as_str(), e[0].audible, e[0].author.as_str()), ("VIDEO", true, "A"));
    assert_eq!((e[1].id.as_str(), e[1].kind.as_str(), e[1].author.as_str()), ("2", "IMAGE", "B"));
    assert_eq!(e[1].resolution, Some((1920, 1080)));
    assert_eq!(e[2].kind, "WEB", "пустой `Type` не перебивает `WallPaperType`");
}

#[test]
fn broken_json_is_an_error_not_a_panic() {
    assert!(ax::parse_library("[{\"Id\": ").is_err());
    assert!(ax::parse_library("{\"Id\": \"1\"}").is_err(), "не массив");
    assert!(ax::parse_library("").is_err());
    assert!(ax::parse_playlists("{}").is_err());
    assert!(ax::parse_resource_config("<xml/>").is_err());
    let c = ax::parse_resource_config(&resource_config("VIDEO", "Resource\\a.mp4", true)).unwrap();
    assert_eq!((c.kind, c.source.as_str()), (ax::AxonKind::Video, "Resource\\a.mp4"));
}

#[test]
fn playlist_current_wallpaper() {
    let p = ax::parse_playlists(&format!(
        "{BOM}{}",
        json!({"PlayLists": [{"WallpaperIds": ["a", "b"], "CurrentPlayIndex": 1, "IsMute": true}, {"WallpaperIds": [], "CurrentPlayIndex": 3}]})
    ))
    .unwrap();
    assert_eq!(p[0].current(), Some("b"));
    assert!(p[0].mute);
    assert_eq!(p[1].current(), None);
}

#[test]
fn windows_paths_map_into_the_prefix() {
    let pfx = bench("paths");
    fs::create_dir_all(pfx.join("drive_c/users/Wehrwolfmann/AppData")).unwrap();
    fs::create_dir_all(pfx.join("dosdevices")).unwrap();
    std::os::unix::fs::symlink("/", pfx.join("dosdevices/z:")).unwrap();
    assert_eq!(
        ax::windows_to_linux(&pfx, "C:\\users\\wehrwolfmann\\appdata\\x.mp4"),
        Some(pfx.join("drive_c/users/Wehrwolfmann/AppData/x.mp4")),
        "регистр сегментов — как на диске"
    );
    assert_eq!(ax::windows_to_linux(&pfx, "Z:\\home\\u\\a.mp4"), Some(PathBuf::from("/home/u/a.mp4")));
    assert_eq!(ax::windows_to_linux(&pfx, "D:\\x"), None, "нет такого диска");
    assert_eq!(ax::windows_to_linux(&pfx, "relative\\x"), None);
    assert_eq!(ax::windows_to_linux(&pfx, ""), None);
}

#[test]
fn chosen_folder_is_checked() {
    english();
    let d = bench("chosen");
    assert_eq!(ax::check_source_dir(&d.join("нет")), Err(ax::folder_missing_text()));
    fs::create_dir_all(d.join("Empty/1")).unwrap();
    assert_eq!(ax::check_source_dir(&d.join("Empty")), Err(ax::folder_not_axon_text()));
    download(&d.join("WallpaperSource/1203421603840"), "VIDEO", "3840x2160.mp4", false);
    assert_eq!(ax::check_source_dir(&d.join("WallpaperSource")), Ok(d.join("WallpaperSource")));
    // Выбрали каталог одних обоев — берём папку над ним.
    assert_eq!(
        ax::check_source_dir(&d.join("WallpaperSource/1203421603840")),
        Ok(d.join("WallpaperSource"))
    );
    assert!(ax::read_source_dir(&d.join("Empty")).is_err());
}

#[test]
fn chosen_folder_takes_titles_from_library_next_to_it() {
    let home = bench("chosen-lib");
    home_with_axon(&home);
    let store = axon_root(&home.join(".wine"), "wehrwolfmann").join("WallpaperSource");
    let list = ax::read_source_dir(&store).unwrap();
    let ids: Vec<&str> = list.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["10038", "12034", "13479"], "по одному на номер");
    let sky = list.iter().find(|w| w.id == "12034").unwrap();
    assert_eq!((sky.title.as_str(), sky.audible), ("Skyline", true), "из свежей библиотеки, не гостевой");
    assert_eq!(sky.prefix, home.join(".wine"));
    let space = list.iter().find(|w| w.id == "10038").unwrap();
    assert!(space.resource_dir.as_ref().unwrap().ends_with("1003821603840"), "родная раскладка Axon");
    assert!(space.playing);
}

#[test]
fn id_from_dir_name_without_library() {
    assert_eq!(ax::id_from_dir_name("1203421603840", Some((3840, 2160))), "12034");
    assert_eq!(ax::id_from_dir_name("1349424003840", None), "13494");
    assert_eq!(ax::id_from_dir_name("2010801920", None), "20");
    assert_eq!(ax::id_from_dir_name("777", None), "777");
    assert_eq!(ax::id_from_dir_name("21603840", None), "21603840", "номер не бывает пустым");
}

#[test]
fn settings_list_without_repeats() {
    let mut m = serde_json::Map::new();
    m.insert(ax::SETTINGS_KEY.into(), json!(["/a", "", "/a", 5, "/b"]));
    let saved = ax::saved_source_dirs(&m);
    assert_eq!(saved, [PathBuf::from("/a"), PathBuf::from("/b")]);
    assert_eq!(ax::with_source_dir(&saved, Path::new("/a")), json!(["/a", "/b"]));
    assert_eq!(ax::with_source_dir(&saved, Path::new("/c")), json!(["/a", "/b", "/c"]));
    assert!(ax::saved_source_dirs(&serde_json::Map::new()).is_empty());
}

// ── Подготовка к показу ─────────────────────────────────────────────────

/// Подменная подготовка: кладёт файл `name` в каталог кэша и считает вызовы.
struct Fake {
    name: &'static str,
    calls: AtomicUsize,
    result: Option<ax::AxonExtractError>,
    outside: bool,
}

impl Fake {
    fn ok(name: &'static str) -> Fake {
        Fake { name, calls: AtomicUsize::new(0), result: None, outside: false }
    }
}

impl ax::AxonExtractor for Fake {
    fn extract(&self, resource_dir: &Path, out_dir: &Path) -> Result<PathBuf, ax::AxonExtractError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(resource_dir.join(ax::RESOURCE_CONFIG).is_file());
        if let Some(e) = &self.result {
            return Err(e.clone());
        }
        let dir = if self.outside { out_dir.parent().unwrap().to_path_buf() } else { out_dir.to_path_buf() };
        let p = dir.join(self.name);
        fs::write(&p, "готово").unwrap();
        Ok(p)
    }
}

fn library(name: &str) -> (PathBuf, ax::AxonLibrary) {
    let home = bench(name);
    home_with_axon(&home);
    let lib = ax::load_library(&ax::AxonEnv::with_home(&home));
    (home, lib)
}

#[test]
fn prepare_uses_extractor_once_then_cache() {
    english();
    let (home, lib) = library("prepare");
    let cache = home.join(".cache/rexon/axon");
    let sky = lib.get("12034").unwrap();
    let fake = Fake::ok("3840x2160.mp4");
    let got = ax::prepare(sky, Some(&fake), &cache).unwrap();
    assert_eq!(got.path, cache.join("12034/3840x2160.mp4"));
    assert_eq!((got.format, got.audio, got.name.as_str()), ("video", true, "Skyline"));
    let again = ax::prepare(sky, Some(&fake), &cache).unwrap();
    assert_eq!(again, got);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1, "второй раз — из кэша");
    // Контейнер сменился (Axon скачал заново) — готовим снова.
    let container = sky.container.clone().unwrap();
    fs::write(&container, "новый контейнер, другой размер").unwrap();
    ax::prepare(sky, Some(&fake), &cache).unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    // Картинка — «static», звука нет.
    let pic = ax::prepare(lib.get("13479").unwrap(), Some(&Fake::ok("3840x2400.jpg")), &cache).unwrap();
    assert_eq!((pic.format, pic.audio), ("static", false));
}

#[test]
fn prepare_refusals_have_human_reasons() {
    english();
    let (home, lib) = library("refusals");
    let cache = home.join(".cache/rexon/axon");
    let fake = Fake::ok("x.mp4");
    let gtr = lib.get("13300").unwrap();
    assert_eq!(ax::prepare(gtr, Some(&fake), &cache), Err(ax::not_downloaded_text("Skyline GTR")));
    let web = lib.get("14001").unwrap();
    assert_eq!(ax::prepare(web, Some(&fake), &cache), Err(ax::web_unsupported_text("Web demo")));
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0, "до подготовки не дошло");
    let sky = lib.get("12034").unwrap();
    assert_eq!(ax::prepare(sky, None, &cache), Err(ax::unavailable_text()));
    for (err, want) in [
        (ax::AxonExtractError::Unavailable, ax::unavailable_text()),
        (ax::AxonExtractError::Unsupported("x".into()), ax::kind_unsupported_text("Skyline")),
        (ax::AxonExtractError::Damaged("x".into()), ax::damaged_text("Skyline")),
        (ax::AxonExtractError::Failed("x".into()), ax::prepare_failed_text("Skyline")),
    ] {
        let f = Fake { result: Some(err), ..Fake::ok("x.mp4") };
        assert_eq!(ax::prepare(sky, Some(&f), &cache), Err(want));
    }
    // Файл вне выданного каталога — не верим.
    let f = Fake { outside: true, ..Fake::ok("x.mp4") };
    assert_eq!(ax::prepare(sky, Some(&f), &cache), Err(ax::prepare_failed_text("Skyline")));
    // Файл неизвестного вида.
    assert_eq!(
        ax::prepare(sky, Some(&Fake::ok("x.bin")), &cache),
        Err(ax::kind_unsupported_text("Skyline"))
    );
    // Тексты отказов не говорят о внутреннем устройстве файлов Axon.
    for t in [ax::unavailable_text(), ax::prepare_failed_text("S"), ax::damaged_text("S")] {
        let low = t.to_lowercase();
        for word in ["module", "decrypt", "encrypt", "password", "key"] {
            assert!(!low.contains(word), "{t}");
        }
    }
}

#[test]
fn builtin_extractor_through_prepare() {
    english();
    let (home, lib) = library("builtin");
    let cache = home.join(".cache/rexon/axon");
    // Подставной файл без шифрования (старая раскладка) — копируется как есть.
    let sky = lib.get("12034").unwrap();
    let ex = ax::default_extractor();
    let got = ax::prepare(sky, Some(ex.as_ref()), &cache).unwrap();
    assert_eq!(got.path, cache.join("12034/3840x2160.mp4"));
    assert_eq!(fs::read_to_string(&got.path).unwrap(), "контейнер");
    // Объявлена неизвестная схема — «вид не поддерживается».
    let dir = sky.resource_dir.clone().unwrap();
    put(
        &dir.join(ax::RESOURCE_CONFIG),
        r#"{"WallPaperType":"VIDEO","Source":"Resource/3840x2160.mp4","SourceEncryptedTypes":"AES"}"#,
    );
    fs::write(sky.container.as_ref().unwrap(), "другой").unwrap();
    assert_eq!(ax::prepare(sky, Some(ex.as_ref()), &cache), Err(ax::kind_unsupported_text("Skyline")));
    // Объявлен ZIP, а лежит не ZIP — «файлы неполные».
    put(
        &dir.join(ax::RESOURCE_CONFIG),
        r#"{"WallPaperType":"VIDEO","Source":"Resource/3840x2160.mp4","SourceEncryptedTypes":"ZIP"}"#,
    );
    assert_eq!(ax::prepare(sky, Some(ex.as_ref()), &cache), Err(ax::damaged_text("Skyline")));
}
