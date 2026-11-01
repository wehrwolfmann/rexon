//! Сторож того, что эталоны ядра и слоя интерфейса живы и читаются.
//!
//! ЗАЧЕМ. Сверки берут ожидания из `tests/golden/*.json` через
//! `include_str!` + `serde_json::from_str(…).unwrap()`. Битый эталон красит
//! только ту сверку, что его читает, и выглядит как её собственный дефект;
//! эталон, который не читает уже ни одна сверка, не красит ничего. Здесь —
//! одна проверка на все эталоны сразу: каждый `.json` разбирается и не пуст.
//!
//! ЧТО БЫЛО РАНЬШЕ. До 02.10.2026 этот файл стерёг ссылки генераторов
//! эталонов на Python (`tests/golden/*_cases.py`, объявления `# blob:` и
//! запрет `git show <коммит>:путь`): эталоны переснимались с прежнего
//! Python-продукта. Решение владельца 02.10.2026 — Python в Rexon только
//! архив, не эталон и не мост, — снимает этот путь: переснимать эталоны с
//! Python больше не будут, ожидания в JSON — собственные эталоны Rust. Сами
//! генераторы лежат в архиве как есть, но ядро их не читает и от их наличия
//! не зависит.

use std::fs;
use std::path::{Path, PathBuf};

/// Каталоги эталонов: ядро и слой интерфейса.
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

/// Нижний порог числа JSON-эталонов ядра (02.10.2026 их 89). Страховка
/// самого обхода: перестань он находить файлы — «битых нет» стало бы
/// правдой на пустом множестве. Эталонов убыло по делу — поправьте число.
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
