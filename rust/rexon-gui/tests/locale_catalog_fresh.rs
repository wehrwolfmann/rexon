//! Сторож устаревших каталогов перевода рядом с бинарями.
//!
//! Окно ищет `.mo` в `<каталог бинаря>/locale` раньше системного
//! (`i18n::locale_dirs`), а собирает его build.rs этого крейта. 15.11.2026
//! на cargo 1.100 build.rs клал свежие `.mo` в `target/<профиль>/build/locale`
//! (OUT_DIR стал на уровень глубже), а рядом с бинарями оставались каталоги
//! прошлой сборки — с ними в пакет и в окно уехали старые строки, и
//! подсказки с новыми msgid показывались по-английски.
//!
//! Тест сверяет каталог рядом с `rexon-main-window` с нынешними `po/*.po`
//! (`scripts/i18n-update.sh --verify`): упадёт, если build.rs снова положит
//! `.mo` не туда или не пересоберёт их после правки po.

use std::path::Path;
use std::process::Command;

#[test]
fn locale_next_to_binaries_matches_po() {
    let exe = Path::new(env!("CARGO_BIN_EXE_rexon-main-window"));
    let locale = exe.parent().expect("у бинаря есть каталог").join("locale");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new("bash")
        .arg(root.join("scripts/i18n-update.sh"))
        .arg("--verify")
        .arg(&locale)
        .output()
        .expect("bash запускается");
    assert!(
        out.status.success(),
        "каталог перевода {} не совпадает с po/:\n{}",
        locale.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}
