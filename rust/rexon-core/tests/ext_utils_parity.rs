//! Check of the `rexon/extensions/utils.py` port.
//!
//! ⚠️ ZIP packing is checked STRUCTURALLY, not byte-wise: a `.zip` contains
//! timestamps and depends on the deflate implementation — it will not match Python
//! byte-for-byte. The golden `golden/pack_structure.json` is a map {name-in-archive:
//! content}, taken from an archive built by Python's own `_pack_rxn` from the
//! same file tree this test recreates. `golden/sample.rxn` is a real
//! Python archive (read by the Rust reader `parse_v4_manifest`).
//! `golden/zipslip.rxn` is a malicious archive with an `../evil.txt` entry.
//!
//! Malformed inputs (`golden/ext_archive_cases.json`): for EVERY real
//! archive in `golden/` it records what live Python did with it in
//! `_py_safe_zip_extract` (a set of paths or exception type+text) and in
//! `_py_read_manifest_text`. The archives were built by the real `zipfile`:
//! `manifest_cp1251.rxn` (manifest not in UTF-8), `dotdot_middle.rxn` (`..` in
//! the middle of a name), `absolute_member.rxn` (entry `/abs.txt`),
//! `truncated.rxn` (a truncated `sample.rxn`), `no_manifest.rxn`.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use rexon_core::ext_utils::{
    pack_rxn, rmtree_allowed, safe_rmtree, safe_zip_extract, write_manifest, ArchiveError,
};
use rexon_core::manifest_v4::parse_v4_manifest;
use serde_json::Value;

mod i18n_pin;

/// The same deterministic tree as in the golden capture script (snap.py).
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

/// Read an archive as a map {name: content}.
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

/// `pack_rxn` gives the same STRUCTURE (names + content) as Python's `_pack_rxn`.
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

/// `parse_v4_manifest` reads `manifest.json` from a real Python archive.
#[test]
fn parse_reads_python_sample_rxn() {
    let sample = golden_dir().join("sample.rxn");
    let meta = parse_v4_manifest(&sample).unwrap();
    assert_eq!(meta["format_version"], serde_json::json!(4));
    assert_eq!(meta["type"], serde_json::json!("wallpaper-video-adaptive"));
    assert_eq!(meta["title"], serde_json::json!("Demo"));
}

/// A missing archive → NotFound (like FileNotFoundError).
#[test]
fn parse_missing_is_not_found() {
    use rexon_core::manifest_v4::ParseError;
    let missing = unique_tmp("nope").with_extension("rxn");
    assert!(matches!(parse_v4_manifest(&missing), Err(ParseError::NotFound(_))));
}

/// `safe_zip_extract` rejects ZipSlip (`../evil.txt`) BEFORE writing files.
#[test]
fn extract_rejects_zipslip() {
    let dest = unique_tmp("zsdest");
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).unwrap();
    let evil = golden_dir().join("zipslip.rxn");
    let res = safe_zip_extract(&evil, &dest);
    assert!(matches!(res, Err(ArchiveError::ZipSlip(_))), "ожидался ZipSlip, получено {res:?}");
    // Not a single file was written outside dest.
    assert!(!dest.parent().unwrap().join("evil.txt").exists());
    let _ = std::fs::remove_dir_all(&dest);
}

/// Malformed inputs on REAL archives: extraction must give exactly the same
/// set of paths as the original's `zipfile.extractall`, or refuse where
/// it refuses.
///
/// This very test closes the mismatch found during the audit: a `..` in
/// the MIDDLE of a name (`a/../b.txt`) Python drops as a component and puts
/// the file at `a/b.txt`, while the port used to collapse the path and write to `b.txt` — for an `.rxn`
/// that is a different root with `manifest.json`.
#[test]
fn extract_matches_python_on_real_archives() {
    let _lang = i18n_pin::ru(); // the Python golden is Russian, it lives in po/ru.po
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
            // Python refused — the port must refuse too, with the same class of reason
            // AND the same text: it reaches the buyer's window verbatim.
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

/// Relative paths of all FILES under `dir` (for comparison with the Python golden).
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

/// Round trip: pack → extract returns the same tree (names + content).
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

/// `write_manifest` writes `manifest.json` with `indent=2` (like Python).
#[test]
fn write_manifest_indent2() {
    let dir = unique_tmp("wm");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = serde_json::json!({"a": 1, "b": "текст"});
    write_manifest(&manifest, &dir).unwrap();
    let got = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
    // json.dump(indent=2, ensure_ascii=False): 2 spaces, Cyrillic without \u.
    assert_eq!(got, "{\n  \"a\": 1,\n  \"b\": \"текст\"\n}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `safe_rmtree`: missing → false; a directory in $TMPDIR → removed, true.
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

/// `safe_rmtree` on a symlink removes the LINK ITSELF, not the target.
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
    assert!(!link.exists()); // link removed
    assert!(target.join("keep.txt").exists()); // target intact
    let _ = std::fs::remove_dir_all(&target);
}

/// `safe_rmtree` refuses to remove a path outside $HOME and outside $TMPDIR → false.
#[test]
fn rmtree_refuses_outside_home_and_tmp() {
    // /usr exists and lies outside $HOME/$TMPDIR → refusal (and without delete permissions).
    assert!(!safe_rmtree(Path::new("/usr")));
}

/// `$HOME` itself (and `$TMPDIR` itself) must NOT be removed — the reference compares with
/// `home + os.sep`, i.e. it requires a strict descendant.
///
/// The pure [`rmtree_allowed`] is checked, not `safe_rmtree`: a test on the live
/// `$HOME` would itself wipe the home directory on a regression.
#[test]
fn rmtree_refuses_home_and_tmp_roots() {
    let home = Path::new("/home/wehrwolfmann");
    let tmp = Path::new("/tmp");

    // Roots — refusal.
    assert!(!rmtree_allowed(home, home, tmp), "$HOME удалять нельзя");
    assert!(!rmtree_allowed(tmp, home, tmp), "$TMPDIR удалять нельзя");

    // Descendants — allowed.
    assert!(rmtree_allowed(&home.join(".themes/X"), home, tmp));
    assert!(rmtree_allowed(&tmp.join("rexon_ext_1"), home, tmp));

    // A neighbour with a common string prefix but a different directory — refusal.
    assert!(!rmtree_allowed(Path::new("/home/wehrwolfmann2"), home, tmp));
    assert!(!rmtree_allowed(Path::new("/usr"), home, tmp));
}
