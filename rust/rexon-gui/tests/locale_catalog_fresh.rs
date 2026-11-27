//! Guard against stale translation catalogues next to the binaries.
//!
//! The window looks for `.mo` in `<binary directory>/locale` before the
//! system one (`i18n::locale_dirs`), and it is built by this crate's
//! build.rs. On 15.11.2026, with cargo 1.100, build.rs put fresh `.mo`
//! files into `target/<profile>/build/locale` (OUT_DIR moved one level
//! deeper), while the directories from the previous build stayed next to
//! the binaries — old strings went into the package and the window with
//! them, and tooltips with new msgids were shown in English.
//!
//! The test compares the catalogue next to `rexon-main-window` with the
//! current `po/*.po` (`scripts/i18n-update.sh --verify`): it fails if
//! build.rs again puts the `.mo` files in the wrong place or does not
//! rebuild them after a po edit.

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
