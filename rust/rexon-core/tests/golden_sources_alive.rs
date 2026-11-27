//! Guard that the goldens of the core and the interface layer are alive and readable.
//!
//! WHY. The checks take their expectations from `tests/golden/*.json` via
//! `include_str!` + `serde_json::from_str(…).unwrap()`. A broken golden turns red
//! only the check that reads it, and looks like that check's own defect;
//! a golden that no check reads any more turns nothing red. Here is
//! one check for all goldens at once: every `.json` parses and is not empty.
//!
//! WHAT IT USED TO BE. Until 02.10.2026 this file guarded the references of the
//! Python golden generators (`tests/golden/*_cases.py`, the `# blob:` declarations and
//! the ban on `git show <commit>:path`): the goldens were recaptured from the former
//! Python product. The owner's decision of 02.10.2026 — Python in Rexon is only
//! an archive, not a golden and not a bridge — removes that path: goldens will no longer be
//! recaptured from Python, the expectations in JSON are Rust's own goldens. The
//! generators themselves lie in the archive as they are, but the core does not read them and does not
//! depend on their presence.

use std::fs;
use std::path::{Path, PathBuf};

/// Golden directories: the core and the interface layer.
fn golden_dirs() -> Vec<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    vec![
        manifest.join("tests/golden"),
        manifest.join("../rexon-gui/tests/golden"),
    ]
}

fn json_goldens(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("нет каталога эталонов {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
        .collect();
    out.sort();
    out
}

/// Lower bound on the number of the core's JSON goldens (89 on 02.10.2026). A safeguard
/// of the walk itself: should it stop finding files, "nothing is broken" would become
/// true on an empty set. If goldens were removed for a reason, adjust the number.
const KNOWN_CORE_GOLDENS: usize = 85;

#[test]
fn every_json_golden_parses_and_is_not_empty() {
    let mut broken: Vec<String> = Vec::new();
    let mut core_count = 0usize;

    for (i, dir) in golden_dirs().iter().enumerate() {
        for file in json_goldens(dir) {
            if i == 0 {
                core_count += 1;
            }
            let name = file.display().to_string();
            let text = match fs::read_to_string(&file) {
                Ok(t) => t,
                Err(e) => {
                    broken.push(format!("{name}: не читается — {e}"));
                    continue;
                }
            };
            match serde_json::from_str::<serde_json::Value>(&text) {
                Err(e) => broken.push(format!("{name}: не JSON — {e}")),
                Ok(v) => {
                    let empty = match &v {
                        serde_json::Value::Null => true,
                        serde_json::Value::Object(m) => m.is_empty(),
                        serde_json::Value::Array(a) => a.is_empty(),
                        _ => false,
                    };
                    if empty {
                        broken.push(format!("{name}: пустой эталон"));
                    }
                }
            }
        }
    }

    assert!(
        broken.is_empty(),
        "эталоны битые:\n  {}",
        broken.join("\n  ")
    );
    assert!(
        core_count >= KNOWN_CORE_GOLDENS,
        "JSON-эталонов ядра найдено {core_count}, а известно не меньше \
         {KNOWN_CORE_GOLDENS}: либо их убрали, либо сломался обход каталога"
    );
}
