//! `package-polkit` prints Rexon's privileged helpers and their polkit
//! policies for a distribution PACKAGE.
//!
//! Why
//! ---
//! The helpers (`rexon-de-helper`, `rexon-pkg-helper`, `rexon-stage-helper`,
//! `rexon-system-helper`) and their policies provide the "ask for the password
//! less often" option (`auth_admin_keep`: five minutes per Rexon process, and a
//! confirmed `org.rexon.pkg` action also covers the other three): without them
//! every system operation asks for the password through bare `pkexec`, and
//! login screen and boot themes from "Appearance" cannot be installed at all.
//! A manual installation puts them in place itself (`installer_exec`), but a
//! package has no such step, so a person with the package lived with a
//! password prompt for every operation.
//!
//! The texts of the helpers and policies live in the core
//! ([`rexon_core::system_plan`], the theme helper in [`rexon_core::installer`]). There must be no second copy in the PKGBUILD:
//! the copies drift apart silently, and that only shows up as password prompts
//! on a live machine. That is why the package prints them with this program at
//! build time.
//!
//! The package has its own path ([`rexon_core::system_plan::HELPER_DIR_PACKAGE`],
//! `/usr/lib`): Arch packaging guidelines forbid a package from writing to
//! `/usr/local`, and files outside the pacman database would not be removed by
//! `pacman -R`. The policy must annotate exactly the path where the helper
//! lives, otherwise pkexec will not pick our action.
//!
//! The privilege session helper `rexon-priv-helper` (action `org.rexon.session`,
//! "one password per launch") is not a script but a binary of the same crate:
//! its body is built by cargo, and this program prints only its path in the
//! package and its policy. The PKGBUILD iterates over such helpers via
//! `--list-binaries` and takes the file from the build directory by the name
//! given by `--path`.
//!
//! ```text
//! package-polkit --helper de|pkg|stage|system    # script body (mode 755)
//! package-polkit --policy de|pkg|stage|system|session   # policy XML (mode 644)
//! package-polkit --path   de|pkg|stage|system|session   # helper path in the package
//! package-polkit --policy-path de|pkg|stage|system|session   # policy file path
//! package-polkit --list                          # the package's script helpers
//! package-polkit --list-binaries                 # the package's binary helpers
//! ```
//! Exit codes: 0 — printed; 2 — invalid arguments.

use rexon_core::installer as inst;
use rexon_core::system_plan as plan;

const USAGE: &str = "использование: package-polkit \
(--helper | --policy | --path | --policy-path) (de | pkg | stage | system | session) \
| --list | --list-binaries";

/// Helpers installed by the package. The PKGBUILD iterates over them via
/// `--list` rather than its own list: otherwise a fifth helper added in the
/// core would silently miss the package — exactly how `rexon-system-helper`
/// once missed it.
const HELPERS: [&str; 4] = ["de", "pkg", "stage", "system"];

/// Binary helpers: cargo builds the body (`rexon-priv-helper`), and the
/// package installs the file from the build directory. They are iterated via
/// `--list-binaries`, for the same reason as [`HELPERS`].
const BINARY_HELPERS: [&str; 1] = ["session"];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.as_slice(), [a] if a == "--list") {
        println!("{}", HELPERS.join(" "));
        return;
    }
    if matches!(args.as_slice(), [a] if a == "--list-binaries") {
        println!("{}", BINARY_HELPERS.join(" "));
        return;
    }
    let [what, which] = match args.as_slice() {
        [a, b] => [a.as_str(), b.as_str()],
        _ => fail("нужны ровно два аргумента"),
    };
    let (name, policy_path) = match which {
        "de" => (plan::DE_HELPER_NAME, plan::DE_POLICY_PATH),
        "pkg" => (plan::PKG_HELPER_NAME, plan::PKG_POLICY_PATH),
        "stage" => (plan::STAGE_HELPER_NAME, plan::STAGE_POLICY_PATH),
        "system" => (inst::SYSTEM_HELPER_NAME, inst::SYSTEM_POLICY_PATH),
        "session" => (plan::SESSION_HELPER_NAME, plan::SESSION_POLICY_PATH),
        other => fail(&format!("неизвестный помощник: {other}")),
    };
    let path = plan::helper_path_in(plan::HELPER_DIR_PACKAGE, name);
    match what {
        "--helper" => print!(
            "{}",
            match which {
                "de" => plan::de_helper_script(),
                "pkg" => plan::pkg_helper_script(),
                "stage" => plan::stage_helper_script(),
                "system" => inst::system_helper_script(),
                // The body is a build binary, not text: there is nothing to print.
                _ => fail(&format!("{which}: помощник-бинарь, его собирает cargo")),
            }
        ),
        "--policy" => print!(
            "{}",
            match which {
                "de" => plan::de_policy_xml_for(&path),
                "pkg" => plan::pkg_policy_xml_for(&path),
                "stage" => plan::stage_policy_xml_for(&path),
                "system" => inst::system_policy_xml_for(&path),
                _ => plan::session_policy_xml_for(&path),
            }
        ),
        "--path" => println!("{path}"),
        "--policy-path" => println!("{policy_path}"),
        other => fail(&format!("неизвестный аргумент: {other}")),
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("package-polkit: {msg}\n{USAGE}");
    std::process::exit(2);
}

#[cfg(test)]
mod tests {
    use rexon_core::system_plan as plan;

    /// The package's install step (`scripts/package-build`, called by the PKGBUILD).
    fn pkgbuild() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/package-build");
        std::fs::read_to_string(path).expect("scripts/package-build читается")
    }

    /// The package must install the helpers and print them with this program.
    ///
    /// Without the helpers a person with the package gets a password prompt for
    /// EVERY system operation, and that only shows on a live machine — the test
    /// turns red earlier. The second half: the texts come from the core rather
    /// than being rewritten by hand in the PKGBUILD.
    #[test]
    fn package_installs_polkit_helpers() {
        let src = pkgbuild();
        assert!(
            src.contains("package-polkit"),
            "PKGBUILD не печатает помощников генератором ядра"
        );
        // The list of helpers belongs to the generator (`--list`), not to the PKGBUILD.
        assert!(
            src.contains("package-polkit\" --list)"),
            "PKGBUILD перебирает помощников своим списком, а не `package-polkit --list`"
        );
        assert!(
            src.contains("package-polkit\" --list-binaries)"),
            "PKGBUILD не ставит помощников-бинарей (`package-polkit --list-binaries`)"
        );
        for name in [
            plan::DE_HELPER_NAME,
            plan::PKG_HELPER_NAME,
            plan::STAGE_HELPER_NAME,
            rexon_core::installer::SYSTEM_HELPER_NAME,
            plan::SESSION_HELPER_NAME,
        ] {
            for line in src.lines() {
                assert!(
                    !line.contains(name) || line.trim_start().starts_with('#'),
                    "имя помощника {name} прибито в PKGBUILD второй копией: {line}"
                );
            }
        }
        // The package does not install the passwordless rule: that is a separate choice for the person.
        assert!(
            !src.contains("49-rexon-nopasswd"),
            "пакет ставит passwordless-правило polkit"
        );
    }

    /// Never drop this: the login screen and boot theme helper ships in the
    /// package (28.09.2026: it did not, and SDDM/Plymouth themes could not be
    /// installed by people using the package), and its policy points at the
    /// package path.
    #[test]
    fn package_ships_login_screen_theme_helper() {
        use rexon_core::installer as inst;
        assert!(super::HELPERS.contains(&"system"));
        let path = plan::helper_path_in(plan::HELPER_DIR_PACKAGE, inst::SYSTEM_HELPER_NAME);
        assert_eq!(path, "/usr/lib/rexon-system-helper");
        let xml = inst::system_policy_xml_for(&path);
        assert!(xml.contains(&format!(
            "<annotate key=\"org.freedesktop.policykit.exec.path\">{path}</annotate>"
        )));
        assert!(xml.contains(inst::SYSTEM_INSTALL_ACTION));
        // The manual installation form has not moved.
        assert_eq!(inst::system_policy_xml(), inst::system_policy_xml_for(inst::SYSTEM_HELPER_PATH));
        assert_eq!(
            inst::SYSTEM_HELPER_PATH,
            plan::helper_path_in(plan::HELPER_DIR_SYSTEM, inst::SYSTEM_HELPER_NAME)
        );
    }

    /// Never drop this: "one password per launch" (B11): the session helper ships
    /// in the package as a binary in `/usr/lib`, its policy points at exactly that
    /// path, and the PKGBUILD installs it with mode 755 (the window's client
    /// refuses a file writable by the group or by everyone).
    #[test]
    fn package_ships_privilege_session_helper() {
        assert!(super::BINARY_HELPERS.contains(&"session"));
        assert!(!super::HELPERS.contains(&"session"), "тело сеанса не печатается: это бинарь");
        let path = plan::helper_path_in(plan::HELPER_DIR_PACKAGE, plan::SESSION_HELPER_NAME);
        assert_eq!(path, "/usr/lib/rexon-priv-helper");
        assert!(rexon_core::privilege::client::helper_candidates()
            .contains(&std::path::PathBuf::from(&path)));
        let xml = plan::session_policy_xml_for(&path);
        assert!(xml.contains(&format!(
            "<annotate key=\"org.freedesktop.policykit.exec.path\">{path}</annotate>"
        )));
        assert!(xml.contains(plan::SESSION_ACTION));
        assert_eq!(plan::session_policy_xml(), plan::session_policy_xml_for(plan::SESSION_HELPER_PATH));
        let src = pkgbuild();
        let block = &src[src.find("package-polkit\" --list-binaries)").expect("цикл бинарей")..];
        let block = &block[..block.find("done").expect("конец цикла")];
        assert!(block.contains("install -Dm755 \"$rel/${path##*/}\" \"$pkgdir$path\""), "{block}");
        assert!(block.contains("--policy-path \"$who\""), "{block}");
        assert!(block.contains("--policy \"$who\""), "{block}");
    }

    /// The package directory and the manual installation directory differ, and the
    /// policy is printed for the path where the helper actually lives.
    #[test]
    fn package_policy_points_to_package_path() {
        let path = plan::helper_path_in(plan::HELPER_DIR_PACKAGE, plan::DE_HELPER_NAME);
        assert_eq!(path, "/usr/lib/rexon-de-helper");
        assert!(plan::de_policy_xml_for(&path).contains(&format!(
            "<annotate key=\"org.freedesktop.policykit.exec.path\">{path}</annotate>"
        )));
        // The form registered without a path stays with the manual installation.
        assert!(plan::de_policy_xml().contains(plan::DE_HELPER_PATH));
        assert_ne!(plan::DE_HELPER_PATH, path);
    }
}
