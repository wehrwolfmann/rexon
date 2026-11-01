//! Golden-сверка порта `appearance_installer.rs` с оригиналом
//! `rexon/appearance/installer.py` (+ `ocs_store._detect_actual_kde_type`).
//!
//! Эталоны сняты с ЖИВОГО Python read-only (`gen_golden.py`): синтетические
//! раскладки распакованного архива материализованы в песочницу, по ним
//! гонялись чистые функции определения типа/имён/маппингов. Реальную
//! установку/apply/скачивание НЕ исполняли — план/mock (правило задачи).
//! Пути нормализованы в стабильный префикс `/sb`; снимок хранит порядок имён
//! так, как их вернул `os.scandir` эталону → Rust итерирует так же.
//!
//! `golden/appearance_installer_cases.json` — все кейсы (detect, разбор дерева,
//! file_ext, looks_like_image с реальными magic-байтами, имя одиночной
//! картинки, решение переклассификации, маппинги).

use std::path::Path;

// Сообщения установщика переводятся; сверки с прежним текстом идут на
// русском каталоге `po/ru.po` («по-русски как было»), английский исходник —
// `english_messages`.
mod i18n_pin;

use rexon_core::appearance_installer as ai;
use rexon_core::ext_manager::ExtStep;
use rexon_core::categories::category_by_id;
use rexon_core::installed_scan::FsSnapshot;
use rexon_core::models::ThemeItem;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/appearance_installer_cases.json")).unwrap()
}

fn snap(v: &Value) -> FsSnapshot {
    FsSnapshot::from_json(&v["snapshot"])
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(String::from)
}

fn hex_decode(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn detect_actual_kde_type_parity() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for case in g["detect"].as_array().unwrap() {
        let fs = snap(case);
        let cr = case["content_root"].as_str().unwrap();
        let got = ai::detect_actual_kde_type(&fs, cr);
        let want = opt_str(&case["expected"]);
        assert_eq!(got, want, "detect: {}", case["name"]);
    }
}

#[test]
fn find_content_root_parity() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for case in g["find_content_root"].as_array().unwrap() {
        let fs = snap(case);
        let ed = case["extract_dir"].as_str().unwrap();
        let got = ai::find_content_root(&fs, ed);
        assert_eq!(got, case["expected"].as_str().unwrap(), "find: {}", case["name"]);
    }
}

#[test]
fn theme_id_from_parity() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for case in g["theme_id"].as_array().unwrap() {
        let fs = snap(case);
        let cr = case["content_root"].as_str().unwrap();
        let got = ai::theme_id_from(&fs, cr);
        assert_eq!(got, case["expected"].as_str().unwrap(), "theme_id: {}", case["name"]);
    }
}

#[test]
fn plymouth_theme_name_parity() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for case in g["plymouth"].as_array().unwrap() {
        let fs = snap(case);
        let cr = case["content_root"].as_str().unwrap();
        let got = ai::plymouth_theme_name(&fs, cr);
        assert_eq!(got, case["expected"].as_str().unwrap(), "plymouth: {}", case["name"]);
    }
}

#[test]
fn file_ext_parity() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for case in g["file_ext"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let got = ai::file_ext(name);
        assert_eq!(got, case["expected"].as_str().unwrap(), "file_ext: {name:?}");
    }
}

#[test]
fn looks_like_image_parity() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for case in g["looks_like_image"].as_array().unwrap() {
        let is_archive = case["is_archive"].as_bool().unwrap();
        let head = hex_decode(case["head_hex"].as_str().unwrap());
        let hints: Vec<&str> = case["hints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h.as_str().unwrap())
            .collect();
        let got = ai::looks_like_image(is_archive, &head, &hints);
        assert_eq!(got, case["expected"].as_bool().unwrap(), "lli: {}", case["name"]);
    }
}

#[test]
fn single_file_fname_image_parity() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let wp = category_by_id(Path::new("/sb/home"), 299).unwrap(); // Обои HD
    for case in g["single_image_fname"].as_array().unwrap() {
        let item = ThemeItem {
            id: case["id"].as_str().unwrap().to_string(),
            download_name: case["download_name"].as_str().unwrap().to_string(),
            download_url: case["download_url"].as_str().unwrap().to_string(),
            ..Default::default()
        };
        let got = ai::single_file_fname(&item, &wp);
        assert_eq!(
            got,
            case["expected"].as_str().unwrap(),
            "fname: name={:?} url={:?}",
            case["download_name"],
            case["download_url"]
        );
    }
}

#[test]
fn reclassify_decision_parity() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let home = Path::new(g["maps"]["home"].as_str().unwrap());
    for case in g["reclassify"].as_array().unwrap() {
        let fs = snap(case);
        let cid = case["category_id"].as_i64().unwrap();
        let cr = case["content_root"].as_str().unwrap();
        let cat = category_by_id(home, cid).expect("категория есть");
        let out = ai::plan_install(home, &fs, &cat, cr);

        // dest_base — путь первого шага MakeDirs.
        let dest_base = match &out.steps[0] {
            ExtStep::MakeDirs { path } => path.clone(),
            other => panic!("ожидался MakeDirs первым, было {other:?}"),
        };
        assert_eq!(
            dest_base,
            case["expected_dest_base"].as_str().unwrap(),
            "dest_base: {}",
            case["name"]
        );
        assert_eq!(
            out.reclassified,
            opt_str(&case["expected_reclassified"]),
            "reclassified: {}",
            case["name"]
        );
        // apply_kind виден в записи журнала (когда установка успешна).
        if let Some(rec) = &out.record {
            assert_eq!(
                rec.apply_kind,
                case["expected_apply_kind"].as_str().unwrap(),
                "apply_kind: {}",
                case["name"]
            );
        }
    }
}

#[test]
fn maps_parity() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let m = &g["maps"];

    for (k, v) in m["kind_to_rexon_type"].as_object().unwrap() {
        assert_eq!(ai::kind_to_rexon_type(k), v.as_str().unwrap(), "k2rt: {k}");
    }
    // дефолт .get(kind, "plugin")
    assert_eq!(ai::kind_to_rexon_type("something-unknown"), "plugin");

    for (k, v) in m["actual_to_kind"].as_object().unwrap() {
        assert_eq!(ai::actual_to_kind(k), v.as_str().unwrap(), "a2k: {k}");
    }

    let home = Path::new(m["home"].as_str().unwrap());
    for (k, v) in m["actual_type_dir"].as_object().unwrap() {
        assert_eq!(
            ai::actual_type_dir(home, k),
            Some(v.as_str().unwrap().to_string()),
            "atd: {k}"
        );
    }
    // тип вне таблицы → None
    assert_eq!(ai::actual_type_dir(home, "bundle"), None);

    let mut exts: Vec<&str> = ai::IMAGE_EXTS.to_vec();
    exts.sort_unstable();
    let want_exts: Vec<String> = m["image_exts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect();
    assert_eq!(exts, want_exts);

    let magic_hex: Vec<String> = ai::IMAGE_MAGIC
        .iter()
        .map(|s| s.iter().map(|b| format!("{b:02x}")).collect())
        .collect();
    let want_magic: Vec<String> = m["image_magic_hex"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect();
    assert_eq!(magic_hex, want_magic);
}

// ── детерминированные проверки композиции плана (mock, без исполнения) ──────

#[test]
fn plan_install_icons_gtk_cache() {
    let _lang = i18n_pin::ru();
    // Иконочная тема (реклассификации нет — категория icons и реально icons):
    // makedirs → (rmtree?) → copytree → gtk-update-icon-cache.
    let g = fx();
    let case = g["reclassify"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "icons_is_icons")
        .unwrap();
    let fs = snap(case);
    let home = Path::new("/sb/home");
    let cat = category_by_id(home, 132).unwrap();
    let out = ai::plan_install(home, &fs, &cat, case["content_root"].as_str().unwrap());
    assert!(out.ok);
    // последний шаг — gtk-update-icon-cache -f -t <dest>
    let last = out.steps.last().unwrap();
    match last {
        ExtStep::Run { argv, timeout_secs } => {
            assert_eq!(argv[0], "gtk-update-icon-cache");
            assert_eq!(argv[1], "-f");
            assert_eq!(argv[2], "-t");
            assert_eq!(*timeout_secs, Some(60));
        }
        other => panic!("ожидался Run gtk-update-icon-cache, было {other:?}"),
    }
    assert_eq!(out.names.len(), 1);
    let rec = out.record.unwrap();
    assert_eq!(rec.apply_kind, "icons");
    assert_eq!(rec.rexon_type, "theme-icons");
}

#[test]
fn plan_single_file_message_and_steps() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/sb/home");
    let cat = category_by_id(home, 299).unwrap(); // Обои HD
    let item = ThemeItem {
        id: "42".into(),
        download_name: "Sunset NO LOGO.png".into(),
        ..Default::default()
    };
    let out = ai::plan_single_file(&item, &cat, "/dl/a");
    assert!(out.ok);
    assert_eq!(out.message, "Установлено: Sunset NO LOGO.png");
    assert_eq!(out.names, vec!["Sunset NO LOGO.png".to_string()]);
    assert!(matches!(out.steps[0], ExtStep::MakeDirs { .. }));
    assert!(matches!(out.steps[1], ExtStep::CopyFile { .. }));
    let rec = out.record.unwrap();
    // Вид берётся из категории, а не жёстким "none" (починен баг оригинала):
    // одиночная картинка для фона блокировки писалась тем же видом, что
    // обычная обоя.
    assert_eq!(rec.apply_kind, "wallpapers");
    assert_eq!(rec.rexon_type, "wallpaper-pack");
    let lock = category_by_id(home, 904).unwrap();
    let rec = ai::plan_single_file(&item, &lock, "/dl/a").record.unwrap();
    assert_eq!(rec.apply_kind, "lockwallpaper");
}

#[test]
fn root_preflight_gates() {
    let _lang = i18n_pin::en();
    let home = Path::new("/sb/home");
    let sddm = category_by_id(home, 101).unwrap();

    // plasmalogin блокирует SDDM.
    let env = ai::RootEnv {
        is_plasmalogin: true,
        has_pkexec: true,
        system_helper_path: "/usr/local/lib/rexon/system-helper",
        system_helper_exists: true,
        is_steamos_or_immutable: false,
        is_steamos: false,
        family_is_debian: false,
    };
    let out = ai::root_preflight(&env, &sddm).unwrap();
    assert!(!out.ok);
    assert!(out.message.contains("Plasma Login Manager"));

    // нет pkexec.
    let env2 = ai::RootEnv { is_plasmalogin: false, has_pkexec: false, ..env };
    let out2 = ai::root_preflight(&env2, &sddm).unwrap();
    // Один текст на продукт — тот же, что у восстановления форков и тем.
    assert_eq!(out2.message, rexon_core::system_plan::pkexec_missing_message());
    assert!(out2.message.contains("polkit"), "{}", out2.message);

    // всё ок → None.
    let env_ok = ai::RootEnv {
        is_plasmalogin: false,
        has_pkexec: true,
        system_helper_path: "/h",
        system_helper_exists: true,
        is_steamos_or_immutable: false,
        is_steamos: false,
        family_is_debian: false,
    };
    assert!(ai::root_preflight(&env_ok, &sddm).is_none());

    // SteamOS (Steam Deck и Steam Machine): отказ ПЕРВЫМ и своей причиной —
    // даже когда системной части Rexon нет (версия для Steam), совет
    // «переустановите Rexon в систему» там невыполним.
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    use rexon_core::distro_compat::SteamOsBlock;
    let deck = ai::RootEnv {
        is_plasmalogin: true,
        has_pkexec: false,
        system_helper_exists: false,
        is_steamos_or_immutable: true,
        is_steamos: true,
        ..env_ok
    };
    let ply = category_by_id(home, 108).unwrap();
    assert_eq!(ai::root_preflight(&deck, &sddm).unwrap().message, SteamOsBlock::LoginScreen.reason());
    assert_eq!(ai::root_preflight(&deck, &ply).unwrap().message, SteamOsBlock::BootSplash.reason());
}

#[test]
fn root_plan_sddm_and_plymouth() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/sb/home");
    let env = ai::RootEnv {
        is_plasmalogin: false,
        has_pkexec: true,
        system_helper_path: "/usr/local/lib/rexon/system-helper",
        system_helper_exists: true,
        is_steamos_or_immutable: false,
        is_steamos: false,
        family_is_debian: false,
    };

    // SDDM: theme_id из папки, pkexec argv.
    let sddm = category_by_id(home, 101).unwrap();
    let fs = FsSnapshot::from_json(&serde_json::json!({
        "dirs": {"/x/MyTheme": ["metadata.desktop", "Main.qml"]},
        "files": {"/x/MyTheme/metadata.desktop": "x", "/x/MyTheme/Main.qml": ""}
    }));
    let plan = ai::plan_root_install(&env, &fs, &sddm, "/x/MyTheme").unwrap();
    assert_eq!(
        plan.argv,
        vec![
            "pkexec".to_string(),
            "/usr/local/lib/rexon/system-helper".into(),
            "sddm".into(),
            "MyTheme".into(),
            "/x/MyTheme".into(),
            "arch".into(),
        ]
    );
    assert!(plan.success_message.contains("экран SDDM"));

    // Plymouth без <name>.plymouth → ошибка.
    let ply = category_by_id(home, 108).unwrap();
    let fs2 = FsSnapshot::from_json(&serde_json::json!({
        "dirs": {"/x/P": ["logo.png"]}, "files": {"/x/P/logo.png": ""}
    }));
    let err = ai::plan_root_install(&env, &fs2, &ply, "/x/P").unwrap_err();
    assert!(err.message.contains("<name>.plymouth"));

    // Plymouth с <name>.plymouth → theme_id из имени файла, debian-argv.
    let env_deb = ai::RootEnv { family_is_debian: true, ..env };
    let fs3 = FsSnapshot::from_json(&serde_json::json!({
        "dirs": {"/x/P": ["cool.plymouth"]}, "files": {"/x/P/cool.plymouth": ""}
    }));
    let plan3 = ai::plan_root_install(&env_deb, &fs3, &ply, "/x/P").unwrap();
    assert_eq!(plan3.theme_id, "cool");
    assert_eq!(plan3.argv[5], "debian");
    assert!(plan3.success_message.contains("ПЕРЕЗАГРУЗКИ"));
}

#[test]
fn root_error_messages() {
    let _lang = i18n_pin::ru();
    assert_eq!(ai::root_cancelled_message(), "Установка отменена: пароль администратора не подтверждён.");
    assert_eq!(
        ai::root_error_message(1, "line1\nline2\nline3\nline4\nline5"),
        "Ошибка установки (код 1): line3 / line4 / line5"
    );
    assert_eq!(
        ai::root_error_message(2, "   "),
        "Ошибка установки (код 2): см. журнал"
    );
}

#[test]
fn apply_action_and_messages() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/sb/home");
    let global = category_by_id(home, 722).unwrap();
    assert_eq!(ai::apply_action_for(Some(&global), "x"), ai::ApplyAction::KdeGlobal);
    assert_eq!(
        ai::apply_message(&ai::ApplyAction::KdeGlobal, true),
        "Global Theme применена"
    );
    assert_eq!(
        ai::apply_message(&ai::ApplyAction::Cursor, false),
        "Курсор не применён (возможно небезопасная тема)"
    );
    assert_eq!(
        ai::apply_message(&ai::ApplyAction::Noop, true),
        "Установлено (применение не требуется для этого типа)"
    );

    // Обои HD с одиночной картинкой → фон рабочего стола.
    let wp = category_by_id(home, 299).unwrap();
    assert_eq!(
        ai::apply_action_for(Some(&wp), "pic.png"),
        ai::ApplyAction::WallpaperBackground
    );
    // Обои каталогом (пачка) — своё действие: картинку из неё выбираем сами.
    assert_eq!(
        ai::apply_action_for(Some(&wp), "Пейзажи"),
        ai::ApplyAction::WallpaperPack
    );
    // Звуковая тема — полноценный вид, а не «применение не требуется».
    let sounds = category_by_id(home, 316).unwrap();
    assert_eq!(
        ai::apply_action_for(Some(&sounds), "zorinos2"),
        ai::ApplyAction::SoundTheme
    );
    assert_eq!(
        ai::apply_message(&ai::ApplyAction::SoundTheme, true),
        "Звуковая тема применена"
    );
    assert_eq!(
        ai::apply_message(&ai::ApplyAction::SoundTheme, false),
        "Не удалось применить звуковую тему"
    );
    // Шрифты и виджеты применять по-прежнему нечем.
    let fonts = category_by_id(home, 103).unwrap();
    assert_eq!(ai::apply_action_for(Some(&fonts), "x.png"), ai::ApplyAction::Noop);
    let plasmoids = category_by_id(home, 706).unwrap();
    assert_eq!(
        ai::apply_action_for(Some(&plasmoids), "Widget"),
        ai::ApplyAction::Noop
    );
    // Плагин обоев каталогом (kpackage) — применять нечего, а вот
    // одиночная картинка из того же раздела ставится фоном.
    let wpp = category_by_id(home, 715).unwrap();
    assert_eq!(ai::apply_action_for(Some(&wpp), "some-plugin"), ai::ApplyAction::Noop);
    assert_eq!(
        ai::apply_action_for(Some(&wpp), "pic.png"),
        ai::ApplyAction::WallpaperBackground
    );
}

/// Виды, у которых есть «применена сейчас», и виды, принимающие одиночную
/// картинку, — списки не должны разъезжаться с картой активных тем.
#[test]
fn kind_flags() {
    let _lang = i18n_pin::ru();
    for k in ["sounds", "wallpapers", "icons", "global", "kvantum"] {
        assert!(ai::kind_has_active_state(k), "{k} должен иметь активное состояние");
    }
    for k in ["fonts", "plasmoids", "wallpaper_plugins", "none", "lockwallpaper"] {
        assert!(!ai::kind_has_active_state(k), "{k} активным быть не может");
    }
    for k in ["wallpapers", "wallpaper_plugins", "lockwallpaper"] {
        assert!(ai::kind_single_file_rule(k).is_some(), "{k} принимает одиночный файл");
    }
    for k in ["colors", "fonts", "kvantum"] {
        assert!(
            ai::kind_single_file_rule(k).is_some(),
            "{k} самодостаточен одним файлом — Магазин его так и раздаёт"
        );
    }
    for k in ["sounds", "icons", "global", "cursor", "plasma"] {
        assert!(
            ai::kind_single_file_rule(k).is_none(),
            "{k} физически является деревом, одним файлом не бывает"
        );
    }
}

/// Голый файл вместо архива: развилка срабатывает по расширению, которого
/// ждёт КАТЕГОРИЯ, и молчит на всём остальном.
#[test]
fn single_file_accepted_by_category_extension() {
    let _lang = i18n_pin::ru();
    // Товар 2369054 «Breeze High Contrast»: Магазин отдаёт сам .colors.
    assert!(ai::single_file_accepted(
        "colors",
        false,
        b"[ColorEffects:D",
        &["BreezeHighContrast.colors", ""]
    ));
    // Тот же файл в категории иконок — не наш случай.
    assert!(!ai::single_file_accepted(
        "icons",
        false,
        b"[ColorEffects:D",
        &["BreezeHighContrast.colors", ""]
    ));
    // Настоящий архив идёт распаковщиком, как бы он ни назывался.
    assert!(!ai::single_file_accepted(
        "colors",
        true,
        b"PK\x03\x04",
        &["scheme.colors", ""]
    ));
    // Картинка без имени опознаётся подписью содержимого (прежнее поведение).
    assert!(ai::single_file_accepted(
        "wallpapers",
        false,
        b"\x89PNG\r\n\x1a\n",
        &["", ""]
    ));
    // А цветовая схема без имени — нет: подписи у неё не бывает.
    assert!(!ai::single_file_accepted("colors", false, b"[General]", &["", ""]));
}

/// Цветовая схема одним файлом: имя темы — основа (её ждёт
/// `plasma-apply-colorscheme`), файл ложится прямо в каталог схем.
#[test]
fn plan_single_file_color_scheme() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/sb/home");
    let colors = category_by_id(home, 112).unwrap();
    let item = ThemeItem {
        id: "2369054".into(),
        download_name: "BreezeHighContrast.colors".into(),
        ..Default::default()
    };
    let out = ai::plan_single_file(&item, &colors, "/dl/a");
    assert!(out.ok);
    assert_eq!(out.names, vec!["BreezeHighContrast".to_string()]);
    let dest = "/sb/home/.local/share/color-schemes/BreezeHighContrast.colors";
    assert_eq!(
        out.steps,
        vec![
            ExtStep::MakeDirs {
                path: "/sb/home/.local/share/color-schemes".into()
            },
            ExtStep::CopyFile {
                src: "/dl/a".into(),
                dest: dest.into()
            },
        ]
    );
    let rec = out.record.unwrap();
    assert_eq!(rec.apply_kind, "colors");
    // Путь в журнале — сам файл: удаление снимает ровно его.
    assert_eq!(rec.paths, vec![dest.to_string()]);
}

/// Kvantum ищет тему как `<имя>/<имя>.kvconfig` — голый файл кладётся в
/// собственную папку, и она же уходит в журнал.
#[test]
fn plan_single_file_kvantum_gets_own_dir() {
    let _lang = i18n_pin::ru();
    let home = Path::new("/sb/home");
    let kv = category_by_id(home, 123).unwrap();
    let item = ThemeItem {
        id: "5".into(),
        download_name: "Sweet.kvconfig".into(),
        ..Default::default()
    };
    let out = ai::plan_single_file(&item, &kv, "/dl/a");
    assert!(out.ok);
    assert_eq!(out.names, vec!["Sweet".to_string()]);
    let dir = "/sb/home/.config/Kvantum/Sweet";
    assert_eq!(
        out.steps,
        vec![
            ExtStep::MakeDirs { path: dir.into() },
            ExtStep::CopyFile {
                src: "/dl/a".into(),
                dest: format!("{dir}/Sweet.kvconfig"),
            },
        ]
    );
    assert_eq!(out.record.unwrap().paths, vec![dir.to_string()]);
}

/// Выбор картинки из пачки обоев: правило детерминированное и объяснимое.
#[test]
fn pack_image_choice_is_deterministic() {
    let _lang = i18n_pin::ru();
    let pack = "/sb/home/.local/share/wallpapers/Пейзажи";
    let snap = serde_json::json!({
        "dirs": {
            pack: ["contents", "screenshot.png", ".rexon-preview.png"],
            format!("{pack}/contents"): ["images"],
            format!("{pack}/contents/images"): [
                "1280x1024.png", "1920x1080.png", "3840x2160.png",
                "2560x1080.png", "logo.svg"
            ],
        },
        "files": {
            format!("{pack}/screenshot.png"): "",
            format!("{pack}/.rexon-preview.png"): "",
            format!("{pack}/contents/images/1280x1024.png"): "",
            format!("{pack}/contents/images/1920x1080.png"): "",
            format!("{pack}/contents/images/3840x2160.png"): "",
            format!("{pack}/contents/images/2560x1080.png"): "",
            format!("{pack}/contents/images/logo.svg"): "",
        },
    });
    let fs = FsSnapshot::from_json(&snap);
    let files = ai::pack_images(&fs, pack);
    // Штатная подпапка есть → снаружи ничего не берём (ни скриншот, ни наше
    // превью).
    assert_eq!(files.len(), 5, "{files:?}");
    assert!(files.iter().all(|f| f.contains("/contents/images/")));

    // 16:9 экран → из двух 16:9 берём тот, что ближе по размеру.
    assert_eq!(
        ai::pick_pack_image(&files, 1920, 1080).as_deref(),
        Some(format!("{pack}/contents/images/1920x1080.png").as_str())
    );
    assert_eq!(
        ai::pick_pack_image(&files, 3840, 2160).as_deref(),
        Some(format!("{pack}/contents/images/3840x2160.png").as_str())
    );
    // Ультраширокий экран → 21:9.
    assert_eq!(
        ai::pick_pack_image(&files, 2560, 1080).as_deref(),
        Some(format!("{pack}/contents/images/2560x1080.png").as_str())
    );
    // Экран неизвестен → первое имя по естественной сортировке; файл без
    // разрешения в имени уступает файлам с разрешением.
    assert_eq!(
        ai::pick_pack_image(&files, 0, 0).as_deref(),
        Some(format!("{pack}/contents/images/1280x1024.png").as_str())
    );

    // Пачка без штатной подпапки: собираем всё дерево, кроме своего превью.
    let flat = "/sb/home/.local/share/wallpapers/Плоская";
    let snap = serde_json::json!({
        "dirs": {
            flat: ["10.jpg", "9.jpg", "readme.txt", ".rexon-preview.png"],
        },
        "files": {
            format!("{flat}/10.jpg"): "",
            format!("{flat}/9.jpg"): "",
            format!("{flat}/readme.txt"): "",
            format!("{flat}/.rexon-preview.png"): "",
        },
    });
    let fs = FsSnapshot::from_json(&snap);
    let files = ai::pack_images(&fs, flat);
    assert_eq!(files.len(), 2, "{files:?}");
    // Естественная сортировка: 9 раньше 10.
    assert_eq!(
        ai::pick_pack_image(&files, 1920, 1080).as_deref(),
        Some(format!("{flat}/9.jpg").as_str())
    );

    // Пачка без картинок вовсе — применять нечего, и это НЕ ошибка.
    let empty = "/sb/home/.local/share/sounds/zorin";
    let snap = serde_json::json!({
        "dirs": { empty: ["index.theme"] },
        "files": { format!("{empty}/index.theme"): "" },
    });
    let fs = FsSnapshot::from_json(&snap);
    assert!(ai::pack_images(&fs, empty).is_empty());
    assert_eq!(
        ai::pack_no_images_message(),
        "Установлено. В этой пачке нет изображений — применять нечего."
    );
}

#[test]
fn apply_config_plans() {
    let _lang = i18n_pin::ru();
    // widget style
    let ws = ai::widget_style_plan("Fusion");
    assert_eq!(
        ws.primary.argv,
        vec![
            "kwriteconfig6", "--file", "kdeglobals", "--group", "KDE",
            "--key", "widgetStyle", "Fusion"
        ]
    );
    assert_eq!(ws.notify_if_ok.argv[0], "dbus-send");

    // splash / lockscreen
    let sp = ai::lnf_component_plan("splash", "MySplash");
    assert_eq!(sp.argv[2], "ksplashrc");
    // Заставка: «Нет» — как модуль KDE (Theme=None, Engine=none), любая
    // другая тема возвращает Engine=KSplashQML.
    assert_eq!(*ai::splash_engine_plan("MySplash").argv.last().unwrap(), "KSplashQML");
    assert_eq!(*ai::splash_engine_plan("None").argv.last().unwrap(), "none");
    assert_eq!(ai::splash_engine_plan("None").argv[6], "Engine");
    assert_eq!(*ai::lnf_component_plan("splash", "None").argv.last().unwrap(), "None");
    let facts = ai::PressFacts::default();
    match ai::press_plan_with(&ai::ApplyAction::Splash, "None", &facts) {
        ai::PressPlan::Steps { steps, say, .. } => {
            assert_eq!(steps.len(), 2);
            assert!(matches!(say, ai::PressSay::Done(_)), "у «Нет» свой итог");
        }
        other => panic!("{other:?}"),
    }
    // Текущая плитка: Engine не KSplashQML — «Нет», что бы ни стояло в Theme.
    let t = || Some("Breeze".to_string());
    assert_eq!(ai::splash_active_value(t(), Some("none")).as_deref(), Some("None"));
    assert_eq!(ai::splash_active_value(t(), Some("None\n")).as_deref(), Some("None"));
    assert_eq!(ai::splash_active_value(t(), Some("KSplashQML\n")).as_deref(), Some("Breeze"));
    assert_eq!(ai::splash_active_value(t(), Some("")).as_deref(), Some("Breeze"));
    assert_eq!(ai::splash_active_value(t(), None).as_deref(), Some("Breeze"));
    assert_eq!(ai::splash_active_value(None, None), None);
    // «Продемонстрировать» — `ksplashqml <тема> --test`, у «Нет» кнопки нет.
    assert_eq!(
        ai::splash_demo_argv("org.kde.breeze.desktop").unwrap(),
        ["ksplashqml", "org.kde.breeze.desktop", "--test"]
    );
    for bad in ["", "None", "--window", "../x", "  "] {
        assert!(ai::splash_demo_argv(bad).is_none(), "{bad:?}");
    }
    assert!(ai::splash_demo_failed("").lines().count() == 1);
    assert!(ai::splash_demo_failed("qml error").ends_with("\nqml error"));
    let ls = ai::lnf_component_plan("lockscreen", "MyLock");
    assert_eq!(ls.argv[2], "kscreenlockerrc");
    assert_eq!(ls.argv[4], "Greeter");

    // aurorae
    let au = ai::aurorae_plan("Breezy");
    assert_eq!(*au.primary.argv.last().unwrap(), "__aurorae__svg__Breezy");
    assert_eq!(au.reconfigure_if_ok.len(), 2);
    assert_eq!(au.reconfigure_if_ok[0].argv[0], "qdbus");
    assert_eq!(au.reconfigure_if_ok[1].argv[0], "qdbus6");
}

#[test]
fn lock_wallpaper_uri_encoding() {
    let _lang = i18n_pin::ru();
    // percent-кодирование не-ASCII (кириллица) как Path.as_uri().
    let plan = ai::lock_wallpaper_plan(
        "kde",
        "/sb/home/.local/share/wallpapers",
        "Закат.png",
        true,
    )
    .unwrap();
    assert_eq!(
        plan.uri,
        "file:///sb/home/.local/share/wallpapers/%D0%97%D0%B0%D0%BA%D0%B0%D1%82.png"
    );
    assert_eq!(*plan.image.argv.last().unwrap(), plan.uri);
    assert_eq!(plan.image.argv[12], "Image");
    assert_eq!(plan.extra[0].argv[12], "PreviewImage");
    assert_eq!(plan.extra[1].argv.last().unwrap(), "org.kde.image");

    // нет файла → мягкий отказ.
    let err = ai::lock_wallpaper_plan("kde", "/d", "x.png", false).unwrap_err();
    assert!(err.contains("Файл обои не найден"));
}

/// Deepin: обои блокировки — только в сеансе Treeland, через Центр
/// управления (`appearance_installer.rs`); вне Treeland — отказ «Rexon
/// поддерживает Deepin только на Treeland», без kscreenlockerrc. Раздел
/// «Экран блокировки» в Deepin есть, но только с режимом обоев.
#[test]
fn lock_wallpaper_deepin_outside_treeland_is_refused() {
    let _lang = i18n_pin::ru();
    let err = ai::lock_wallpaper_plan("deepin", "/h/.local/share/wallpapers", "a.png", true).unwrap_err();
    assert_eq!(err, rexon_core::dde::not_treeland_message());
    assert!(err.starts_with("Rexon поддерживает Deepin только на Treeland"), "{err}");
    // Остальные окружения — прежний путь KDE.
    let kde = ai::lock_wallpaper_plan("", "/d", "a.png", true).unwrap();
    assert_eq!(kde.image.argv[0], "kwriteconfig6");

    use rexon_core::appearance_page as ap;
    use rexon_core::categories as c;
    assert!(c::category_available("lockscreen", "deepin"));
    assert!(c::category_available("lockscreen", "kde"));
    assert!(!c::category_available("lockscreen", "gnome"));
    // Полных тем блокировки в Deepin нет — только чип обоев.
    let keys = |de: &str| ap::lock_chips_for(de).iter().map(|ch| ch.key).collect::<Vec<_>>();
    assert_eq!(keys("deepin"), vec![ap::LOCK_MODE_WALLPAPER]);
    assert_eq!(keys("kde"), vec![ap::LOCK_MODE_THEMES, ap::LOCK_MODE_WALLPAPER]);
    let home = std::path::Path::new("/h");
    let mut view = ap::CatalogView::new(c::get_category(home, "lockscreen").unwrap(), "deepin");
    view.lock_mode = ap::LOCK_MODE_THEMES.to_string();
    assert_eq!(view.lock_mode_effective(), ap::LOCK_MODE_WALLPAPER);
    assert_eq!(view.query_category_id(), ap::LOCK_STORE_WALLPAPER.to_string());
    assert!(!view.de_blocked());
}

#[test]
fn path_as_uri_ascii() {
    let _lang = i18n_pin::ru();
    assert_eq!(ai::path_as_uri("/home/u/a b.png"), "file:///home/u/a%20b.png");
    assert_eq!(ai::path_as_uri("/a/b~c-d_e.png"), "file:///a/b~c-d_e.png");
}

#[test]
fn safe_extract_entry_traversal() {
    let _lang = i18n_pin::ru();
    assert!(ai::safe_extract_entry("/tmp/ex", "theme/file.txt"));
    assert!(ai::safe_extract_entry("/tmp/ex", "file.txt"));
    assert!(!ai::safe_extract_entry("/tmp/ex", "../escape.txt"));
    assert!(!ai::safe_extract_entry("/tmp/ex", "/etc/passwd"));
    assert!(!ai::safe_extract_entry("/tmp/ex", "a/../../b"));
}

#[test]
fn global_stale_and_uninstall_plan() {
    let _lang = i18n_pin::ru();
    // Global Theme без metadata.json (Plasma 5) → устаревшая.
    let fs = FsSnapshot::from_json(&serde_json::json!({
        "dirs": {"/sb/home/.local/share/plasma/look-and-feel/old": ["metadata.desktop"]},
        "files": {"/sb/home/.local/share/plasma/look-and-feel/old/metadata.desktop": "x"}
    }));
    assert!(ai::global_theme_missing_metadata_json(&fs, "/sb/home", "old"));

    let fs2 = FsSnapshot::from_json(&serde_json::json!({
        "dirs": {"/sb/home/.local/share/plasma/look-and-feel/new": ["metadata.json"]},
        "files": {"/sb/home/.local/share/plasma/look-and-feel/new/metadata.json": "{}"}
    }));
    assert!(!ai::global_theme_missing_metadata_json(&fs2, "/sb/home", "new"));

    // uninstall: каталог → RemoveTree, файл → RemoveFile, нет → пропуск.
    let paths = vec![
        "/d/theme".to_string(),
        "/d/scheme.colors".to_string(),
        "/d/gone".to_string(),
    ];
    let is_dir = |p: &str| p == "/d/theme";
    let is_file = |p: &str| p == "/d/scheme.colors";
    let steps = ai::plan_uninstall(&paths, &is_dir, &is_file);
    assert_eq!(
        steps,
        vec![
            ExtStep::RemoveTree { path: "/d/theme".into(), safe: false, ignore_errors: true },
            ExtStep::RemoveFile { path: "/d/scheme.colors".into() },
        ]
    );
    assert_eq!(ai::uninstall_message(2, "Моя тема"), "Удалено (путей: 2): Моя тема");
}

/// Куда ложится скачанный из Магазина скриншот (`_save_ocs_preview`, «Вариант
/// 1»): РЕАЛЬНЫЕ установленные каталоги этой машины (иконки/GTK/plasma-темы/
/// шрифты, user+system) — в них `<dir>/.rexon-preview.png`; цвет-схемы и
/// несуществующие пути — в сайдкар-кеш с безопасным именем; пустое имя и
/// пустой путь — `None` (копировать некуда).
///
/// Эталон снят с ЖИВОГО `installer.py` read-only (файлы НЕ копировались).
#[test]
fn ocs_preview_dest_matches_python() {
    let _lang = i18n_pin::ru();
    let g: Value =
        serde_json::from_str(include_str!("golden/appearance_extra_cases.json")).unwrap();
    let home = g["cache_paths"]["home"].as_str().unwrap();
    let cases = g["ocs_preview_dests"].as_array().unwrap();
    assert!(cases.len() >= 12, "мало кейсов размещения превью");
    let mut in_dir = 0;
    let mut sidecar = 0;
    for c in cases {
        let got = ai::ocs_preview_dest(
            home,
            c["apply_kind"].as_str().unwrap(),
            c["theme_dir"].as_str().unwrap(),
            c["theme_dir_is_dir"].as_bool().unwrap(),
            c["item_name"].as_str().unwrap(),
        );
        assert_eq!(got.as_deref(), c["dest"].as_str(), "ocs_preview_dest для {c:?}");
        match c["dest"].as_str() {
            Some(d) if d.ends_with("/.rexon-preview.png") => in_dir += 1,
            Some(_) => sidecar += 1,
            None => {}
        }
        // ПРАВИЛО, а не состав машины. Прежде здесь стояло
        // `assert!(Path::new(theme_dir).is_dir())` — требование, чтобы девять
        // записанных каталогов (`~/.local/share/icons/Bibata-Modern-Ice`,
        // `/usr/share/themes/Atlanta`, …) существовали и СЕГОДНЯ. Такая
        // проверка меряет не порт, а набор установленных тем: удалил тему —
        // сверка красная, а сломай `ocs_preview_dest` — зелёная, если темы на
        // месте. Тот же класс, что вычищен из сверок моста (`c7d5687`).
        // Само `ocs_preview_dest` файловой системы не касается вовсе:
        // `theme_dir_is_dir` — уже готовое наблюдение. Значит и сверять надо
        // ответ на это наблюдение: переверни его — и назначение обязано
        // перейти из каталога темы в сайдкар-кеш.
        let flipped = ai::ocs_preview_dest(
            home,
            c["apply_kind"].as_str().unwrap(),
            c["theme_dir"].as_str().unwrap(),
            !c["theme_dir_is_dir"].as_bool().unwrap(),
            c["item_name"].as_str().unwrap(),
        );
        if c["theme_dir_is_dir"].as_bool().unwrap() && c["apply_kind"] != "colors" {
            assert_ne!(flipped, got, "каталога нет — путь обязан смениться");
            assert!(
                flipped.is_none()
                    || flipped.as_deref().unwrap().starts_with(&format!(
                        "{home}/.cache/rexon/local_previews/"
                    )),
                "без каталога превью ложится в сайдкар-кеш, получили {flipped:?}"
            );
        }
    }
    assert!(in_dir >= 6, "мало кейсов с каталогом темы");
    assert!(sidecar >= 4, "мало сайдкар-кейсов");
}

/// Сообщения установщика на исходном языке.
#[test]
fn english_messages() {
    let _lang = i18n_pin::en();
    assert_eq!(
        ai::root_cancelled_message(),
        "Installation canceled: the administrator password was not confirmed."
    );
    assert_eq!(ai::root_error_message(2, "   "), "Installation error (code 2): see the log");
    assert_eq!(ai::apply_message(&ai::ApplyAction::KdeGlobal, true), "Global Theme applied");
    assert_eq!(
        ai::apply_message(&ai::ApplyAction::SoundTheme, false),
        "Couldn't apply the sound theme"
    );
    assert_eq!(ai::gtk_sanitizer_message(3), "🛡️ GTK sanitizer: fixed 3 links");
    let home = Path::new("/sb/home");
    let cat = category_by_id(home, 299).unwrap();
    let item = ThemeItem { id: "42".into(), download_name: "a.png".into(), ..Default::default() };
    assert_eq!(ai::plan_single_file(&item, &cat, "/dl/a").message, "Installed: a.png");
}
