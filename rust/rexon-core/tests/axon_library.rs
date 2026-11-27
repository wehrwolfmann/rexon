//! The Razer Axon library under Wine (`rexon_core::axon_library`) on
//! fake files: finding prefixes, parsing the library (both names of the type
//! field, BOM, broken JSON), Windows → Linux paths, a wallpaper folder given
//! by hand, preparing for display through a fake and an external program.
//!
//! The parse result of the whole fake home is checked against the golden file
//! `golden/axon_library.json` (home paths replaced with `$HOME`);
//! `REXON_UPDATE_GOLDEN=1` — recapture.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};


use rexon_core::axon_library as ax;
use serde_json::{json, Value};

/// An empty directory for one test.
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

/// Config of a downloaded wallpaper: the new form (`WallPaperType`, on one line) or
/// the old one (`Type`, indented).
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

/// A library entry.
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

/// Fake home: `~/.wine` with Axon (logged in + guest), a Proton
/// prefix with a broken library, a Bottles bottle without Axon.
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
            "IsFavorite": 1,
            "Category": "18,17",
            "AllTags": " Город,Автор 12034, Ночь",
            "CollectionInfos": [{"collection_id": "154", "title": "Серия первая"}],
            "Upvoted": 89,
            "Downloads": 9218,
            "PlayEffects": {"Brightness": "0.8", "PlayRate": 1.5, "Hue": null},
        })),
        // The display path is empty, the resolution is there — the directory by `{Id}{H}{W}`.
        entry("10038", "The Beauty of Space", "Type", "VIDEO", json!({
            "Resolution": "3840x2160",
            "WallPaperStatus": "Playing",
        })),
        // Generated wallpapers: the creation number and `PaperType`; properties as a JSON string.
        entry("13300", "Skyline GTR", "Type", "VIDEO", json!({
            "Audible": "1",
            "DetailId": 5,
            "PaperType": "Generate",
            "Method": "txt2img",
            "IsMute": true,
            "PlayEffects": "{\"WallpaperFillingMode\":\"Fill\"}",
        })),
        // The new type key in the library.
        entry("13479", "Neo Rey by Night", "WallPaperType", "IMAGE", json!({
            "Resolution": "3840x2400",
            "WallPaperStatus": "Installed",
            "Silver": "120",
            "IsRedeemed": 1,
            "EffectType": "Static",
        })),
        entry("14001", "Web demo", "WallPaperType", "WEB", json!({})),
        // Entries without a number are skipped.
        {"Title": "без номера"},
    ]);
    put(&account.join(ax::LIBRARY_FILE), &format!("{BOM}{lib}"));
    put(
        &account.join(ax::PLAYLIST_FILE),
        &format!("{BOM}{}", json!({
            "SwitchIntervalSeconds": 3600,
            "IsCustomInterval": false,
            "PlayOrder": "Ordered",
            "DefaultVolume": 50,
            "PlayLists": [{"MonitorId": "\\\\.\\DISPLAY1", "WallpaperIds": ["13305", "10038", "12034"], "CurrentPlayIndex": 1, "IsMute": true, "MaxVolume": 80, "IsSelect": true, "IsStopped": false}],
            "IsPauseWhenFullScreen": true,
            "IsPauseAllWhenD3dFullScreen": false,
            "PlayMode": "None",
        })),
    );
    put(
        &account.join(ax::USER_SETTING_FILE),
        &format!("{BOM}{}", json!({"SourceSavedPath": "C:\\users\\wehrwolfmann\\AppData\\Local\\Razer\\RazerAxon\\WallpaperSource"})),
    );
    download(&store.join("1203421603840"), "VIDEO", "3840x2160.mp4", false);
    download(&store.join("1003821603840"), "VIDEO", "RazerSpace_3840x2160.mp4", false);
    // Layout of a third-party downloader (`{Id}`), the old `Type` key.
    download(&store.join("10038"), "VIDEO", "RazerSpace_3840x2160.mp4", true);
    download(&store.join("1347924003840"), "IMAGE", "3840x2400.jpg", false);
    // Service directories without a description are not wallpapers.
    fs::create_dir_all(store.join("ResourceTmp/12034")).unwrap();
    fs::create_dir_all(store.join("Extracted")).unwrap();
    // Guest (before login): the old library with the same wallpapers under a different name.
    let guest = root.join("guest-2b48d659");
    put(&guest.join(ax::LIBRARY_FILE), &json!([entry("12034", "Skyline (гость)", "Type", "VIDEO", json!({}))]).to_string());
    let old = fs::File::options().write(true).open(guest.join(ax::LIBRARY_FILE)).unwrap();
    old.set_modified(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000)).unwrap();
    // A guest without a library is not an account.
    put(&root.join("guest-0000/UserSetting"), "{}");

    // Proton: a broken library goes to the log, the rest is read.
    let pfx = home.join(".local/share/Steam/steamapps/compatdata/1493710/pfx");
    put(&axon_root(&pfx, "steamuser").join("RZR_x/WallPaperData"), "[{\"Id\": \"1\",");
    // Bottles without Axon.
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
            // The parse error text comes from serde; we check only the file.
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
            "tags": ax::tag_list(&w.tags),
            "details": details(&w.details),
        })).collect::<Vec<_>>(),
        "playSetting": lib.play_setting.as_ref().map(play_setting),
    })
}

fn details(d: &ax::AxonDetails) -> Value {
    json!({
        "favorite": d.is_favorite,
        "silver": d.silver,
        "redeemed": d.is_redeemed,
        "reward": d.download_reward,
        "paperType": d.paper_type,
        "method": d.method,
        "detailId": d.detail_id,
        "ai": d.is_ai(),
        "categories": d.categories,
        "collections": d.collections.iter().map(|(id, t)| json!([id, t])).collect::<Vec<_>>(),
        "upvoted": d.upvoted,
        "downloads": d.downloads,
        "mute": d.is_mute,
        "playEffects": d.play_effects,
    })
}

fn play_setting(s: &ax::PlayListSetting) -> Value {
    json!({
        "interval": s.switch_interval_seconds,
        "custom": s.is_custom_interval,
        "order": format!("{:?}", s.play_order),
        "mode": format!("{:?}", s.play_mode),
        "pauseFullscreen": s.pause_when_fullscreen,
        "pauseAllGame": s.pause_all_when_game_fullscreen,
        "defaultVolume": s.default_volume,
        "playlists": s.playlists.iter().map(|p| json!({
            "monitor": p.monitor,
            "ids": p.wallpaper_ids,
            "current": p.current_index,
            "mute": p.mute,
            "volume": p.volume,
            "stopped": p.stopped,
            "selected": p.selected,
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
    // A manual folder without a library next to it: the number comes from the directory name.
    let manual = home.join("Copies/AxonCopy/WallpaperSource");
    download(&manual.join("1349424003840"), "IMAGE", "3840x2400.jpg", false);
    download(&manual.join("777"), "VIDEO", "clip.mp4", true);
    // The same wallpapers that are already in the prefix — without a repeat.
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
    // A Lutris prefix from the game description.
    let lutris = bench("prefixes-lutris");
    fs::create_dir_all(lutris.join("drive_c")).unwrap();
    put(
        &home.join(".config/lutris/games/axon-1.yml"),
        &format!("game:\n  exe: x.exe\n  prefix: '{}'\n", lutris.display()),
    );
    // We do not search deeper than one level.
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
    // Axon directories — only with a library.
    let roots = ax::find_roots(&env);
    assert_eq!(roots.len(), 2, "{roots:?}");
}

#[test]
fn source_shows_only_with_axon_or_a_chosen_folder() {
    use rexon_core::axon_window::show_source;
    let empty = bench("tab-empty");
    // There is a Wine prefix, but no Axon in it — no source.
    fs::create_dir_all(empty.join(".wine/drive_c/users/u")).unwrap();
    assert!(!show_source(&ax::AxonEnv::with_home(&empty)));
    // An Axon directory without a library (never logged in) — none either.
    put(&axon_root(&empty.join(".wine"), "u").join("guest-0/UserSetting"), "{}");
    assert!(!show_source(&ax::AxonEnv::with_home(&empty)));
    // A folder given by hand keeps the source even if it is already gone.
    let mut env = ax::AxonEnv::with_home(&empty);
    env.source_dirs = vec![empty.join("пропавшая")];
    assert!(show_source(&env));
    // Axon with a library is found by itself.
    let home = bench("tab-axon");
    home_with_axon(&home);
    assert!(show_source(&ax::AxonEnv::with_home(&home)));
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
fn playlist_setting_reads_timer_order_mode_and_pauses() {
    let s = ax::parse_playlist_setting(&format!(
        "{BOM}{}",
        json!({
            "SwitchIntervalSeconds": 5400, "IsCustomInterval": true, "PlayOrder": "Random",
            "PlayMode": "Span", "IsPauseWhenFullScreen": false, "IsPauseAllWhenD3dFullScreen": true,
            "DefaultVolume": 250,
            "PlayLists": [{"WallpaperIds": ["a"], "MaxVolume": "30", "IsStopped": true}],
        })
    ))
    .unwrap();
    assert_eq!((s.switch_interval_seconds, s.is_custom_interval), (5400, true));
    assert_eq!((s.play_order, s.play_mode), (ax::PlayOrder::Random, ax::PlayMode::Span));
    assert!(!s.pause_when_fullscreen && s.pause_all_when_game_fullscreen);
    assert_eq!(s.default_volume, 100, "громкость не выше 100");
    assert_eq!((s.playlists[0].volume, s.playlists[0].stopped), (30, true));
    // A bare file: «Никогда», in order, different wallpapers on the screens.
    let bare = ax::parse_playlist_setting("{\"PlayLists\": [{\"SwitchIntervalSeconds\": 1}]}").unwrap();
    assert_eq!(bare.switch_interval_seconds, ax::SWITCH_NEVER);
    assert_eq!((bare.play_order, bare.play_mode), (ax::PlayOrder::Ordered, ax::PlayMode::Separate));
    assert_eq!((bare.default_volume, bare.playlists[0].volume), (50, 100));
    let never = ax::parse_playlist_setting("{\"SwitchIntervalSeconds\": -1, \"PlayLists\": []}").unwrap();
    assert_eq!(never.switch_interval_seconds, ax::SWITCH_NEVER);
    assert!(ax::parse_playlist_setting("{\"SwitchIntervalSeconds\": 60}").is_err(), "без PlayLists — не тот файл");
}

#[test]
fn details_tolerate_strings_numbers_and_nulls() {
    let e = ax::parse_library(&json!([
        {"Id": "1", "IsFavorite": "1", "Silver": 12.0, "IsRedeemed": true, "Upvoted": "7",
         "Downloads": null, "Category": " 3 ,, 4", "CollectionInfos": [{"collection_id": 9}, {}],
         "PlayEffects": "не JSON", "DownloadReward": 2},
        {"Id": "2", "PlayEffects": null, "CollectionInfos": null, "Category": null},
    ]).to_string())
    .unwrap();
    let d = &e[0].details;
    assert!(d.is_favorite && d.is_redeemed && !d.is_ai());
    assert_eq!((d.silver, d.upvoted, d.downloads, d.download_reward), (12, 7, 0, 2));
    assert_eq!(d.categories, ["3", "4"]);
    assert_eq!(d.collections, [("9".to_string(), String::new())], "пустая серия отброшена");
    assert!(d.play_effects.is_empty());
    assert_eq!(e[1].details, ax::AxonDetails::default());
    assert_eq!(ax::tag_list(" a, ,b ,"), ["a", "b"]);
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
    // The directory of a single wallpaper was chosen — take the folder above it.
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

// ── Preparing for display ───────────────────────────────────────────────

/// Fake preparation: puts the file `name` into the cache directory and counts the calls.
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
    // The container changed (Axon downloaded it again) — prepare again.
    let container = sky.container.clone().unwrap();
    fs::write(&container, "новый контейнер, другой размер").unwrap();
    ax::prepare(sky, Some(&fake), &cache).unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 2);
    // A picture — "static", no sound.
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
    // A file outside the given directory — not trusted.
    let f = Fake { outside: true, ..Fake::ok("x.mp4") };
    assert_eq!(ax::prepare(sky, Some(&f), &cache), Err(ax::prepare_failed_text("Skyline")));
    // A file of an unknown kind.
    assert_eq!(
        ax::prepare(sky, Some(&Fake::ok("x.bin")), &cache),
        Err(ax::kind_unsupported_text("Skyline"))
    );
    // The refusal texts say nothing about the internals of Axon files.
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
    // A fake file without encryption (the old layout) — copied as is.
    let sky = lib.get("12034").unwrap();
    let ex = ax::default_extractor();
    let got = ax::prepare(sky, Some(ex.as_ref()), &cache).unwrap();
    assert_eq!(got.path, cache.join("12034/3840x2160.mp4"));
    assert_eq!(fs::read_to_string(&got.path).unwrap(), "контейнер");
    // An unknown scheme is declared — "the kind is not supported".
    let dir = sky.resource_dir.clone().unwrap();
    put(
        &dir.join(ax::RESOURCE_CONFIG),
        r#"{"WallPaperType":"VIDEO","Source":"Resource/3840x2160.mp4","SourceEncryptedTypes":"AES"}"#,
    );
    fs::write(sky.container.as_ref().unwrap(), "другой").unwrap();
    assert_eq!(ax::prepare(sky, Some(ex.as_ref()), &cache), Err(ax::kind_unsupported_text("Skyline")));
    // ZIP is declared, but what lies there is not ZIP — "the files are incomplete".
    put(
        &dir.join(ax::RESOURCE_CONFIG),
        r#"{"WallPaperType":"VIDEO","Source":"Resource/3840x2160.mp4","SourceEncryptedTypes":"ZIP"}"#,
    );
    assert_eq!(ax::prepare(sky, Some(ex.as_ref()), &cache), Err(ax::damaged_text("Skyline")));
}
