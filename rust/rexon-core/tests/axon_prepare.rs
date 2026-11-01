//! Подготовка обоев Razer Axon заранее (`rexon_core::axon_prepare`) на
//! подставном доме: расшифровка на месте, бережная правка описания (BOM,
//! CRLF), резервная копия, пропуск занятых, повторный проход без работы,
//! дописывание после обрыва, «Применить» прямо из `Resource/`, возврат
//! исходных описаний. Ключ — свой тестовый, не Razer.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rexon_core::axon_extract::derive_password_with_key;
use rexon_core::axon_library as ax;
use rexon_core::axon_prepare as ap;
use serde_json::json;
use zip::unstable::write::FileOptionsExt;
use zip::write::SimpleFileOptions;

const KEY: &[u8] = b"test-key-not-razer";
const BOM: &str = "\u{feff}";

fn bench(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join("axon_prepare").join(name);
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn payload(seed: u32) -> Vec<u8> {
    (0..200_000u32).flat_map(|i| (i.wrapping_mul(2654435761) ^ seed).to_le_bytes()).collect()
}

/// Скачанные обои: описание `config` как есть и контейнер ZIP с `data`.
fn encrypted(dir: &Path, file: &str, config: &str, data: &[u8]) {
    fs::create_dir_all(dir.join("Resource")).unwrap();
    fs::write(dir.join(ax::RESOURCE_CONFIG), config).unwrap();
    let pw = derive_password_with_key(KEY, config.as_bytes());
    let mut z = zip::ZipWriter::new(File::create(dir.join("Resource").join(file)).unwrap());
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .with_deprecated_encryption(pw.as_bytes());
    z.start_file(file, opts).unwrap();
    z.write_all(data).unwrap();
    z.finish().unwrap();
}

struct Home {
    home: PathBuf,
    store: PathBuf,
}

impl Home {
    fn env(&self) -> ax::AxonEnv {
        ax::AxonEnv::with_home(&self.home)
    }
    fn lib(&self) -> ax::AxonLibrary {
        ax::load_library(&self.env())
    }
    fn opts(&self, keep: bool) -> ap::Options {
        ap::Options { originals_root: self.env().originals_root(), keep_media: keep, pause: Duration::ZERO }
    }
    fn dir(&self, name: &str) -> PathBuf {
        self.store.join(name)
    }
}

/// Компактное описание нового вида (как у Axon 2.11).
fn compact(file: &str) -> String {
    format!(
        "{{\"WallPaperType\":\"VIDEO\",\"Version\":\"1.0.0.0\",\"Source\":\"Resource\\\\{file}\",\
         \"Integrity\":[\"Resource\\\\{file}\"],\"SourceEncryptedTypes\":\"ZIP\",\"SourceEncryptedLevel\":\"LOW\"}}"
    )
}

/// Описание с BOM, CRLF и табами, поля шифрования в середине.
fn pretty(file: &str) -> String {
    format!(
        "{BOM}{{\r\n\t\"Type\": \"VIDEO\",\r\n\t\"SourceEncryptedTypes\": \"ZIP\",\r\n\
         \t\"Source\": \"Resource\\\\{file}\",\r\n\t\"SourceEncryptedLevel\": \"LOW\",\r\n\
         \t\"Integrity\": [\"Resource\\\\{file}\"]\r\n}}\r\n"
    )
}

/// Подставной дом: 12034 (компактное), 13241 (BOM+CRLF), 10038 (играет),
/// 11111 (старые без шифрования), 13300 (не скачаны).
fn home(name: &str) -> Home {
    let home = bench(name);
    let root = home.join(".wine/drive_c/users/u/AppData/Local/Razer/RazerAxon");
    let store = root.join("WallpaperSource");
    let account = root.join("RZR_1");
    let e = |id: &str, title: &str, status: &str, res: &str| {
        json!({"Id": id, "Title": title, "Type": "VIDEO", "Audible": "1", "Resolution": res,
               "WallPaperStatus": status, "Author": {"author_name": "A"}})
    };
    let lib = json!([
        e("12034", "Skyline", "Installed", "3840x2160"),
        e("13241", "The Arcade", "Installed", "3840x2160"),
        e("10038", "Space", "Playing", "3840x2160"),
        e("11111", "Old", "Installed", "3840x2160"),
        e("13300", "GTR", "Uninstalled", "3840x2160"),
    ]);
    fs::create_dir_all(&account).unwrap();
    fs::write(account.join(ax::LIBRARY_FILE), format!("{BOM}{lib}")).unwrap();
    encrypted(&store.join("1203421603840"), "3840x2160.mp4", &compact("3840x2160.mp4"), &payload(1));
    encrypted(&store.join("1324121603840"), "3840x2160.mp4", &pretty("3840x2160.mp4"), &payload(2));
    encrypted(&store.join("1003821603840"), "space.mp4", &compact("space.mp4"), &payload(3));
    let old = store.join("1111121603840");
    fs::create_dir_all(old.join("Resource")).unwrap();
    fs::write(old.join(ax::RESOURCE_CONFIG), "{\"Type\":\"VIDEO\",\"Source\":\"Resource\\\\old.mp4\"}").unwrap();
    fs::write(old.join("Resource/old.mp4"), b"old plain video").unwrap();
    Home { home, store }
}

fn never(_: &ax::AxonWallpaper) -> bool {
    false
}

fn no_stop() -> bool {
    false
}

fn listing(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    v.sort();
    v
}

#[test]
fn pass_prepares_in_place_and_keeps_text_byte_exact() {
    let h = home("pass");
    let lib = h.lib();
    assert_eq!(ap::counts(&lib, &h.opts(false).originals_root), ap::Counts { prepared: 0, remaining: 3 });

    let busy = |w: &ax::AxonWallpaper| ap::is_busy(w, &Default::default());
    let r = ap::run_pass_with_key(KEY, &lib, &h.opts(false), &busy, &no_stop);
    assert_eq!(r, ap::PassReport { prepared: 2, busy: 1, failed: 0 });

    // Ролик — на месте контейнера, под тем же именем, временных файлов нет.
    let sky = h.dir("1203421603840");
    assert_eq!(fs::read(sky.join("Resource/3840x2160.mp4")).unwrap(), payload(1));
    assert_eq!(listing(&sky.join("Resource")), ["3840x2160.mp4"]);
    assert_eq!(listing(&sky), ["Resource", "ResourceConfig.txt"]);
    // Описание: ровно без двух полей.
    assert_eq!(
        fs::read_to_string(sky.join(ax::RESOURCE_CONFIG)).unwrap(),
        "{\"WallPaperType\":\"VIDEO\",\"Version\":\"1.0.0.0\",\"Source\":\"Resource\\\\3840x2160.mp4\",\
         \"Integrity\":[\"Resource\\\\3840x2160.mp4\"]}"
    );
    // BOM, CRLF и табы целы.
    let arcade = h.dir("1324121603840");
    assert_eq!(fs::read(arcade.join("Resource/3840x2160.mp4")).unwrap(), payload(2));
    assert_eq!(
        fs::read_to_string(arcade.join(ax::RESOURCE_CONFIG)).unwrap(),
        format!(
            "{BOM}{{\r\n\t\"Type\": \"VIDEO\",\r\n\t\"Source\": \"Resource\\\\3840x2160.mp4\",\r\n\
             \t\"Integrity\": [\"Resource\\\\3840x2160.mp4\"]\r\n}}\r\n"
        )
    );
    // Резервная копия: исходное описание байт в байт, ролика нет.
    let backup = h.opts(false).originals_root.join("13241");
    assert_eq!(fs::read_to_string(backup.join(ax::RESOURCE_CONFIG)).unwrap(), pretty("3840x2160.mp4"));
    assert_eq!(listing(&backup), [ax::RESOURCE_CONFIG, ap::ORIGIN_FILE]);
    // Играющие не тронуты, старые — тоже.
    let space = h.dir("1003821603840");
    assert_eq!(fs::read_to_string(space.join(ax::RESOURCE_CONFIG)).unwrap(), compact("space.mp4"));
    assert_eq!(fs::read(h.dir("1111121603840/Resource/old.mp4")).unwrap(), b"old plain video");
    assert!(!h.opts(false).originals_root.join("11111").exists());

    let lib = h.lib();
    assert_eq!(ap::counts(&lib, &h.opts(false).originals_root), ap::Counts { prepared: 2, remaining: 1 });
    // Повторный проход ничего не делает (и ничего не пишет).
    let before = fs::metadata(sky.join(ax::RESOURCE_CONFIG)).unwrap().modified().unwrap();
    let r = ap::run_pass_with_key(KEY, &lib, &h.opts(false), &busy, &no_stop);
    assert_eq!(r, ap::PassReport { prepared: 0, busy: 1, failed: 0 });
    assert_eq!(fs::metadata(sky.join(ax::RESOURCE_CONFIG)).unwrap().modified().unwrap(), before);
    // Перестали играть — готовятся.
    let r = ap::run_pass_with_key(KEY, &lib, &h.opts(false), &never, &no_stop);
    assert_eq!(r.prepared, 1);
    assert_eq!(fs::read(space.join("Resource/space.mp4")).unwrap(), payload(3));
    assert_eq!(ap::counts(&h.lib(), &h.opts(false).originals_root), ap::Counts { prepared: 3, remaining: 0 });
}

#[test]
fn file_open_by_a_process_is_busy() {
    let h = home("open");
    let lib = h.lib();
    let sky = lib.get("12034").unwrap();
    let holder = File::open(sky.container.as_ref().unwrap()).unwrap();
    let open = ap::open_files_under(&[h.store.canonicalize().unwrap()]);
    assert!(ap::is_busy(sky, &open), "{open:?}");
    assert!(!ap::is_busy(lib.get("13241").unwrap(), &open));
    drop(holder);
    assert!(!ap::is_busy(sky, &ap::open_files_under(&[h.store.canonicalize().unwrap()])));
}

#[test]
fn stop_and_errors_do_not_block_others() {
    let h = home("errors");
    // Испорченный контейнер у 12034 — 13241 всё равно готовится.
    fs::write(h.dir("1203421603840/Resource/3840x2160.mp4"), b"PK\x03\x04 broken").unwrap();
    let r = ap::run_pass_with_key(KEY, &h.lib(), &h.opts(false), &never, &no_stop);
    assert_eq!((r.prepared, r.failed), (2, 1));
    // Неудача ничего не испортила: описание прежнее, временных файлов нет.
    let sky = h.dir("1203421603840");
    assert_eq!(fs::read_to_string(sky.join(ax::RESOURCE_CONFIG)).unwrap(), compact("3840x2160.mp4"));
    assert_eq!(listing(&sky.join("Resource")), ["3840x2160.mp4"]);
    // Остановка — до первых обоев.
    let h = home("stop");
    let r = ap::run_pass_with_key(KEY, &h.lib(), &h.opts(false), &never, &|| true);
    assert_eq!(r, ap::PassReport::default());
}

#[test]
fn wrong_key_fails_without_damage() {
    let h = home("wrongkey");
    let r = ap::run_pass_with_key(b"other", &h.lib(), &h.opts(false), &never, &no_stop);
    assert_eq!((r.prepared, r.failed), (0, 3));
    let sky = h.dir("1203421603840");
    assert_eq!(fs::read_to_string(sky.join(ax::RESOURCE_CONFIG)).unwrap(), compact("3840x2160.mp4"));
    assert!(rexon_core::axon_extract::is_zip_file(&sky.join("Resource/3840x2160.mp4")).unwrap());
    assert_eq!(listing(&sky.join("Resource")), ["3840x2160.mp4"]);
}

#[test]
fn interrupted_after_media_is_finished_next_time() {
    let h = home("resume");
    let sky = h.dir("1203421603840");
    let backup = h.opts(false).originals_root.join("12034");
    // Как будто оборвались между расшифровкой и правкой описания.
    fs::create_dir_all(&backup).unwrap();
    fs::copy(sky.join(ax::RESOURCE_CONFIG), backup.join(ax::RESOURCE_CONFIG)).unwrap();
    fs::write(sky.join("Resource/3840x2160.mp4"), payload(1)).unwrap();
    fs::write(sky.join("Resource/.3840x2160.mp4.tmp-999999"), b"junk").unwrap();
    let out = ap::prepare_dir_with_key(KEY, &sky, &backup, false).unwrap();
    assert_eq!(out, ap::Outcome::Prepared);
    assert!(!fs::read_to_string(sky.join(ax::RESOURCE_CONFIG)).unwrap().contains("SourceEncrypted"));
    // Без копии такой случай — ошибка, а не правка вслепую.
    let arcade = h.dir("1324121603840");
    fs::write(arcade.join("Resource/3840x2160.mp4"), payload(2)).unwrap();
    let err = ap::prepare_dir_with_key(KEY, &arcade, &h.opts(false).originals_root.join("13241"), false);
    assert!(err.is_err());
    assert_eq!(fs::read_to_string(arcade.join(ax::RESOURCE_CONFIG)).unwrap(), pretty("3840x2160.mp4"));
}

#[test]
fn apply_takes_prepared_file_from_resource() {
    let h = home("apply");
    let env = h.env();
    ap::run_pass_with_key(KEY, &h.lib(), &h.opts(false), &never, &no_stop);
    let lib = h.lib();
    let sky = lib.get("12034").unwrap();
    let got = ap::prepare_for_apply(sky, None, &env).unwrap();
    assert_eq!(&got.path, sky.container.as_ref().unwrap());
    assert_eq!((got.format, got.audio), ("video", true));
    assert!(!env.cache_root().exists(), "кэш не нужен");
    // Старые без шифрования — прежним путём (через подготовку и кэш).
    let old = lib.get("11111").unwrap();
    assert_eq!(ap::prepare_for_apply(old, None, &env), Err(ax::unavailable_text()));
}

#[test]
fn restore_puts_original_descriptions_back() {
    let h = home("restore");
    let root = h.opts(true).originals_root;
    ap::run_pass_with_key(KEY, &h.lib(), &h.opts(true), &never, &no_stop);
    // Оригинал хранился (keep): копия ролика лежит рядом с описанием.
    let sky = h.dir("1203421603840");
    assert!(root.join("12034/3840x2160.mp4").is_file());
    let r = ap::restore_originals(&root);
    assert_eq!(r, ap::RestoreReport { restored: 3, with_media: 3, failed: 0 });
    // Всё как было: описание байт в байт, контейнер снова ZIP с тем же
    // паролем — повторная расшифровка даёт тот же ролик.
    assert_eq!(fs::read_to_string(sky.join(ax::RESOURCE_CONFIG)).unwrap(), compact("3840x2160.mp4"));
    let out = h.home.join("check");
    let p = rexon_core::axon_extract::extract_with_key(KEY, &sky, &out).unwrap();
    assert_eq!(fs::read(p).unwrap(), payload(1));
    // Второй возврат — нечего.
    assert_eq!(ap::restore_originals(&root), ap::RestoreReport::default());

    // Без хранения оригинала: описание вернулось, ролик расшифрован.
    let h = home("restore-nomedia");
    let root = h.opts(false).originals_root;
    ap::run_pass_with_key(KEY, &h.lib(), &h.opts(false), &never, &no_stop);
    let r = ap::restore_originals(&root);
    assert_eq!(r, ap::RestoreReport { restored: 3, with_media: 0, failed: 0 });
    let sky = h.dir("1203421603840");
    assert_eq!(fs::read_to_string(sky.join(ax::RESOURCE_CONFIG)).unwrap(), compact("3840x2160.mp4"));
    assert_eq!(fs::read(sky.join("Resource/3840x2160.mp4")).unwrap(), payload(1));
    // Следующий проход (если включат снова) узнаёт полусделанное по копии.
    let r = ap::run_pass_with_key(KEY, &h.lib(), &h.opts(false), &never, &no_stop);
    assert_eq!(r.prepared, 3);
}

/// Стенд на КОПИИ библиотеки Axon (не на живой!): настоящий фоновый поток с
/// ключом Razer в доме `REXON_AXON_STAND_HOME`. Сначала играющие обои
/// пропускаются; затем в копии библиотеки их статус меняется на `Installed`,
/// и второй запуск готовит всё. Итог «Применить» — путь в `Resource/`; сумму
/// файла сверяет вызывающий. Запуск:
/// `REXON_AXON_STAND_HOME=… cargo test --test axon_prepare -- --ignored stand --nocapture`.
#[test]
#[ignore]
fn stand_background_on_copied_library() {
    let Some(home) = std::env::var_os("REXON_AXON_STAND_HOME").map(PathBuf::from) else { return };
    assert!(home.to_string_lossy().contains("stand"), "стенд — только на копии библиотеки");
    std::env::set_var("HOME", &home);
    std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    for k in ["WINEPREFIX", "XDG_CACHE_HOME", "XDG_DATA_HOME"] {
        std::env::remove_var(k);
    }
    let wait = |done: &dyn Fn() -> bool| {
        for _ in 0..600 {
            if done() {
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        panic!("стенд: не дождались");
    };
    let counts = || {
        let (env, _) = ap::live_env();
        ap::counts(&ax::load_library(&env), &env.originals_root())
    };
    println!("до: {:?}", counts());
    ap::spawn_background(Duration::ZERO);
    wait(&|| counts().remaining <= 2 && counts().prepared >= 1);
    std::thread::sleep(Duration::from_secs(2));
    println!("первый проход (играющие пропущены): {:?}, идёт: {}", counts(), ap::is_running());
    ap::request_stop();
    wait(&|| !ap::is_running());

    // Обои перестали играть — в копии библиотеки.
    let (env, _) = ap::live_env();
    for acc in fs::read_dir(ax::find_roots(&env)[0].root.clone()).unwrap().flatten() {
        let lib = acc.path().join(ax::LIBRARY_FILE);
        if let Ok(text) = fs::read_to_string(&lib) {
            fs::write(&lib, text.replace("\"Playing\"", "\"Installed\"")).unwrap();
            let pl = acc.path().join(ax::PLAYLIST_FILE);
            let _ = fs::remove_file(pl);
        }
    }
    ap::spawn_background(Duration::ZERO);
    wait(&|| !ap::is_running());
    println!("второй проход: {:?}", counts());
    let (env, _) = ap::live_env();
    let lib = ax::load_library(&env);
    for w in &lib.wallpapers {
        if let Ok(p) = ap::prepare_for_apply(w, None, &env) {
            println!("ПРИМЕНИТЬ {} «{}»: {} ({})", w.id, w.title, p.path.display(), p.format);
        }
    }
    // Повторный запуск ничего не делает.
    ap::spawn_background(Duration::ZERO);
    wait(&|| !ap::is_running());
    println!("третий проход: {:?}", counts());
}
