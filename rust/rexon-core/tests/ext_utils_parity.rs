//! Сверка порта `rexon/extensions/utils.py`.
//!
//! ⚠️ Упаковка ZIP сверяется СТРУКТУРНО, не побайтово: `.zip` содержит
//! таймстемпы и зависит от реализации deflate — байт-в-байт с Python не
//! совпадёт. Эталон `golden/pack_structure.json` — карта {имя-в-архиве:
//! содержимое}, снятая с архива, собранного самим Python `_pack_rxn` из того
//! же дерева файлов, что воссоздаёт этот тест. `golden/sample.rxn` — реальный
//! Python-архив (его читает Rust-ридер `parse_v4_manifest`).
//! `golden/zipslip.rxn` — вредоносный архив с записью `../evil.txt`.
//!
//! Кривые входы (`golden/ext_archive_cases.json`): для КАЖДОГО настоящего
//! архива из `golden/` записано, что живой Python сделал с ним в
//! `_py_safe_zip_extract` (набор путей либо тип+текст исключения) и в
//! `_py_read_manifest_text`. Архивы собраны настоящим `zipfile`:
//! `manifest_cp1251.rxn` (манифест не в UTF-8), `dotdot_middle.rxn` (`..` в
//! середине имени), `absolute_member.rxn` (запись `/abs.txt`),
//! `truncated.rxn` (обрезанный `sample.rxn`), `no_manifest.rxn`.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use rexon_core::ext_utils::{
    pack_rxn, rmtree_allowed, safe_rmtree, safe_zip_extract, write_manifest, ArchiveError,
};
use rexon_core::manifest_v4::parse_v4_manifest;
use serde_json::Value;

mod i18n_pin;

/// То же детерминированное дерево, что в скрипте снятия эталона (snap.py).
const TREE: &[(&str, &str)] = &[
    ("manifest.json", "{\"format_version\": 4, \"type\": \"wallpaper-video-adaptive\", \"title\": \"Demo\"}"),
    ("data/video/1080p.mp4", "VIDEO-1080-BYTES"),
    ("data/audio/ru.mp3", "AUDIO-RU-BYTES"),
    ("icon.png", "PNG-BYTES"),
];

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn unique_tmp(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rexon_ext_{tag}_{}", std::process::id()))
}

fn write_tree(root: &Path) {
    for (rel, content) in TREE {
        let full = root.join(rel);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, content).unwrap();
    }
}

/// Прочитать архив как карту {имя: содержимое}.
fn archive_structure(path: &Path) -> BTreeMap<String, String> {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut out = BTreeMap::new();
    for i in 0..zip.len() {
        let mut e = zip.by_index(i).unwrap();
        let name = e.name().to_string();
        let mut s = String::new();
        e.read_to_string(&mut s).unwrap();
        out.insert(name, s);
    }
    out
}

/// `pack_rxn` даёт ту же СТРУКТУРУ (имена + содержимое), что Python `_pack_rxn`.
#[test]
fn pack_structure_matches_python() {
    let golden: Value = serde_json::from_str(include_str!("golden/pack_structure.json")).unwrap();
    let want: BTreeMap<String, String> = golden
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();

    let src = unique_tmp("packsrc");
    let rxn = unique_tmp("pack").with_extension("rxn");
    let _ = std::fs::remove_dir_all(&src);
    write_tree(&src);
    pack_rxn(&src, &rxn).unwrap();

    let got = archive_structure(&rxn);
    let _ = std::fs::remove_dir_all(&src);
    let _ = std::fs::remove_file(&rxn);
    assert_eq!(got, want);
}

/// `parse_v4_manifest` читает `manifest.json` из реального Python-архива.
#[test]
fn parse_reads_python_sample_rxn() {
    let sample = golden_dir().join("sample.rxn");
    let meta = parse_v4_manifest(&sample).unwrap();
    assert_eq!(meta["format_version"], serde_json::json!(4));
    assert_eq!(meta["type"], serde_json::json!("wallpaper-video-adaptive"));
    assert_eq!(meta["title"], serde_json::json!("Demo"));
}

/// Несуществующий архив → NotFound (как FileNotFoundError).
#[test]
fn parse_missing_is_not_found() {
    use rexon_core::manifest_v4::ParseError;
    let missing = unique_tmp("nope").with_extension("rxn");
    assert!(matches!(parse_v4_manifest(&missing), Err(ParseError::NotFound(_))));
}

/// `safe_zip_extract` отвергает ZipSlip (`../evil.txt`) ДО записи файлов.
#[test]
fn extract_rejects_zipslip() {
    let dest = unique_tmp("zsdest");
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).unwrap();
    let evil = golden_dir().join("zipslip.rxn");
    let res = safe_zip_extract(&evil, &dest);
    assert!(matches!(res, Err(ArchiveError::ZipSlip(_))), "ожидался ZipSlip, получено {res:?}");
    // Ни один файл не записан за пределами dest.
    assert!(!dest.parent().unwrap().join("evil.txt").exists());
    let _ = std::fs::remove_dir_all(&dest);
}

/// Кривые входы на НАСТОЯЩИХ архивах: распаковка обязана дать ровно тот же
/// набор путей, что `zipfile.extractall` у оригинала, либо отказать там, где
/// отказывает он.
///
/// Именно этот тест закрывает расхождение, найденное при аудите: `..` в
/// СЕРЕДИНЕ имени (`a/../b.txt`) Python отбрасывает как компонент и кладёт
/// файл в `a/b.txt`, а порт раньше схлопывал путь и писал в `b.txt` — у `.rxn`
/// это другой корень с `manifest.json`.
#[test]
fn extract_matches_python_on_real_archives() {
    let _lang = i18n_pin::ru(); // эталон Python — русский, он в po/ru.po
    let golden: Value =
        serde_json::from_str(include_str!("golden/ext_archive_cases.json")).unwrap();
    let mut checked = 0;
    for (name, case) in golden.as_object().unwrap() {
        let want = &case["extract"];
        let dest = unique_tmp(&format!("arc_{}", name.replace('.', "_")));
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest).unwrap();
        let res = safe_zip_extract(&golden_dir().join(name), &dest);

        if let Some(kind) = want["error"].as_str() {
            // Python отказал — обязан отказать и порт, тем же классом причины
            // И тем же текстом: он доезжает до окна покупателя дословно.
            let want_msg = want["msg"].as_str().unwrap();
            match (kind, &res) {
                ("ValueError", Err(ArchiveError::ZipSlip(m)))
                | ("BadZipFile", Err(ArchiveError::BadZip(m))) => {
                    assert_eq!(m, want_msg, "{name}: текст отказа");
                }
                _ => panic!("{name}: Python дал {kind}, порт — {res:?}"),
            }
        } else {
            res.unwrap_or_else(|e| panic!("{name}: порт отказал, Python нет: {e}"));
            let mut got: Vec<String> = walk_files(&dest, &dest);
            got.sort();
            let expect: Vec<String> = want["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect();
            assert_eq!(got, expect, "{name}: распаковалось не туда, куда у Python");
        }
        let _ = std::fs::remove_dir_all(&dest);
        checked += 1;
    }
    assert_eq!(checked, 7, "все архивы golden/ должны быть в эталоне");
}

/// Относительные пути всех ФАЙЛОВ под `dir` (для сверки с Python-эталоном).
fn walk_files(root: &Path, dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk_files(root, &p));
        } else {
            out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
        }
    }
    out
}

/// Роундтрип: pack → extract возвращает то же дерево (имена + содержимое).
#[test]
fn pack_then_extract_roundtrip() {
    let src = unique_tmp("rtsrc");
    let rxn = unique_tmp("rt").with_extension("rxn");
    let dest = unique_tmp("rtdest");
    for p in [&src, &dest] {
        let _ = std::fs::remove_dir_all(p);
    }
    write_tree(&src);
    pack_rxn(&src, &rxn).unwrap();
    std::fs::create_dir_all(&dest).unwrap();
    safe_zip_extract(&rxn, &dest).unwrap();

    for (rel, content) in TREE {
        let got = std::fs::read_to_string(dest.join(rel)).unwrap();
        assert_eq!(&got, content, "файл {rel} после роундтрипа отличается");
    }
    for p in [&src, &dest] {
        let _ = std::fs::remove_dir_all(p);
    }
    let _ = std::fs::remove_file(&rxn);
}

/// `write_manifest` пишет `manifest.json` с `indent=2` (как Python).
#[test]
fn write_manifest_indent2() {
    let dir = unique_tmp("wm");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = serde_json::json!({"a": 1, "b": "текст"});
    write_manifest(&manifest, &dir).unwrap();
    let got = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
    // json.dump(indent=2, ensure_ascii=False): 2 пробела, кириллица без \u.
    assert_eq!(got, "{\n  \"a\": 1,\n  \"b\": \"текст\"\n}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `safe_rmtree`: несуществующий → false; каталог в $TMPDIR → удаляется, true.
#[test]
fn rmtree_missing_and_tmp() {
    let missing = unique_tmp("rmnope");
    assert!(!safe_rmtree(&missing));

    let dir = unique_tmp("rmdir");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("sub/f.txt"), "x").unwrap();
    assert!(safe_rmtree(&dir));
    assert!(!dir.exists());
}

/// `safe_rmtree` симлинка удаляет САМУ ссылку, а не цель.
#[test]
fn rmtree_symlink_removes_link_only() {
    let target = unique_tmp("rmtarget");
    let link = unique_tmp("rmlink");
    let _ = std::fs::remove_dir_all(&target);
    let _ = std::fs::remove_file(&link);
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("keep.txt"), "y").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    assert!(safe_rmtree(&link));
    assert!(!link.exists()); // ссылка удалена
    assert!(target.join("keep.txt").exists()); // цель цела
    let _ = std::fs::remove_dir_all(&target);
}

/// `safe_rmtree` отказывается удалять путь вне $HOME и вне $TMPDIR → false.
#[test]
fn rmtree_refuses_outside_home_and_tmp() {
    // /usr существует, лежит вне $HOME/$TMPDIR → отказ (и без прав на удаление).
    assert!(!safe_rmtree(Path::new("/usr")));
}

/// Сам `$HOME` (и сам `$TMPDIR`) удалять НЕЛЬЗЯ — эталон сравнивает с
/// `home + os.sep`, то есть требует строгого потомка.
///
/// Проверяется чистая [`rmtree_allowed`], а не `safe_rmtree`: тест на живом
/// `$HOME` при регрессии сам бы и снёс домашний каталог.
#[test]
fn rmtree_refuses_home_and_tmp_roots() {
    let home = Path::new("/home/wehrwolfmann");
    let tmp = Path::new("/tmp");

    // Корни — отказ.
    assert!(!rmtree_allowed(home, home, tmp), "$HOME удалять нельзя");
    assert!(!rmtree_allowed(tmp, home, tmp), "$TMPDIR удалять нельзя");

    // Потомки — разрешены.
    assert!(rmtree_allowed(&home.join(".themes/X"), home, tmp));
    assert!(rmtree_allowed(&tmp.join("rexon_ext_1"), home, tmp));

    // Сосед с общим префиксом строки, но другой каталог — отказ.
    assert!(!rmtree_allowed(Path::new("/home/wehrwolfmann2"), home, tmp));
    assert!(!rmtree_allowed(Path::new("/usr"), home, tmp));
}
