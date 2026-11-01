//! `package-polkit` — печать привилегированных помощников Rexon и их
//! polkit-политик для ПАКЕТА дистрибутива.
//!
//! Зачем
//! -----
//! Помощники (`rexon-de-helper`, `rexon-pkg-helper`, `rexon-stage-helper`,
//! `rexon-system-helper`) и их политики дают вариант «пароль реже»
//! (`auth_admin_keep`: пять минут на процесс Rexon, а подтверждённое
//! действие `org.rexon.pkg` покрывает и три остальных): без них каждая
//! системная операция спрашивает пароль сырым `pkexec`, а темы экрана
//! входа и загрузки из «Оформления» не ставятся вовсе. Ручная установка
//! кладёт их сама (`installer_exec`), а у пакета такого шага нет — и
//! человек с пакетом жил на пароле за операцию.
//!
//! Тексты помощников и политик живут в ядре
//! ([`rexon_core::system_plan`], помощник тем — [`rexon_core::installer`]). Второй копии в PKGBUILD быть не должно:
//! расходятся они молча, а видно это только по пароля́м на живой машине.
//! Поэтому пакет печатает их этой программой при сборке.
//!
//! Путь у пакета свой ([`rexon_core::system_plan::HELPER_DIR_PACKAGE`] —
//! `/usr/lib`): нормы Arch запрещают пакету писать в `/usr/local`, а файлы
//! вне базы pacman не снял бы `pacman -R`. Политика обязана annotate'ить
//! ровно тот путь, по которому помощник и лежит, — иначе pkexec наше
//! действие не выберет.
//!
//! Помощник сеанса прав `rexon-priv-helper` (действие `org.rexon.session`,
//! «один пароль на запуск») — не скрипт, а бинарь того же крейта: его тело
//! собирает cargo, а эта программа печатает только его путь в пакете и
//! политику. PKGBUILD перебирает такие помощники по `--list-binaries` и
//! берёт файл из каталога сборки по имени из `--path`.
//!
//! ```text
//! package-polkit --helper de|pkg|stage|system    # тело скрипта (режим 755)
//! package-polkit --policy de|pkg|stage|system|session   # XML политики (режим 644)
//! package-polkit --path   de|pkg|stage|system|session   # путь помощника в пакете
//! package-polkit --policy-path de|pkg|stage|system|session   # путь файла политики
//! package-polkit --list                          # помощники-скрипты пакета
//! package-polkit --list-binaries                 # помощники-бинари пакета
//! ```
//! Коды возврата: 0 — напечатано; 2 — неверные аргументы.

use rexon_core::installer as inst;
use rexon_core::system_plan as plan;

const USAGE: &str = "использование: package-polkit \
(--helper | --policy | --path | --policy-path) (de | pkg | stage | system | session) \
| --list | --list-binaries";

/// Помощники, которые ставит пакет. PKGBUILD перебирает их по `--list`, а
/// не своим списком: пятый помощник, заведённый в ядре, иначе молча не
/// попал бы в пакет — ровно так в пакет не попал `rexon-system-helper`.
const HELPERS: [&str; 4] = ["de", "pkg", "stage", "system"];

/// Помощники-бинари: тело собирает cargo (`rexon-priv-helper`), пакет кладёт
/// файл из каталога сборки. Перебор — по `--list-binaries`, по той же
/// причине, что и у [`HELPERS`].
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
                // Тело — бинарь сборки, а не текст: печатать нечего.
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

    fn pkgbuild() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packaging/aur/PKGBUILD");
        std::fs::read_to_string(path).expect("PKGBUILD читается")
    }

    /// Пакет обязан ставить помощников и печатать их этой программой.
    ///
    /// Без помощников человек с пакетом получает пароль на КАЖДУЮ системную
    /// операцию, и заметно это только на живой машине — тест краснеет
    /// раньше. Вторая половина: тексты берутся у ядра, а не переписаны в
    /// PKGBUILD руками.
    #[test]
    fn пакет_ставит_помощников_polkit() {
        let src = pkgbuild();
        assert!(
            src.contains("package-polkit"),
            "PKGBUILD не печатает помощников генератором ядра"
        );
        // Список помощников — у генератора (`--list`), а не в PKGBUILD.
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
        // Passwordless-правило пакет не ставит: это отдельный выбор человека.
        assert!(
            !src.contains("49-rexon-nopasswd"),
            "пакет ставит passwordless-правило polkit"
        );
    }

    /// ⛔ Помощник тем экрана входа и загрузки едет в пакете (28.09.2026:
    /// не ехал, и темы SDDM/Plymouth у покупателя пакета не ставились), и
    /// его политика указывает на путь пакета.
    #[test]
    fn пакет_везёт_помощника_тем_экрана_входа() {
        use rexon_core::installer as inst;
        assert!(super::HELPERS.contains(&"system"));
        let path = plan::helper_path_in(plan::HELPER_DIR_PACKAGE, inst::SYSTEM_HELPER_NAME);
        assert_eq!(path, "/usr/lib/rexon-system-helper");
        let xml = inst::system_policy_xml_for(&path);
        assert!(xml.contains(&format!(
            "<annotate key=\"org.freedesktop.policykit.exec.path\">{path}</annotate>"
        )));
        assert!(xml.contains(inst::SYSTEM_INSTALL_ACTION));
        // Форма ручной установки не сдвинулась.
        assert_eq!(inst::system_policy_xml(), inst::system_policy_xml_for(inst::SYSTEM_HELPER_PATH));
        assert_eq!(
            inst::SYSTEM_HELPER_PATH,
            plan::helper_path_in(plan::HELPER_DIR_SYSTEM, inst::SYSTEM_HELPER_NAME)
        );
    }

    /// ⛔ «Один пароль на запуск» (Б11): помощник сеанса едет в пакете
    /// бинарём в `/usr/lib`, его политика указывает ровно на этот путь, и
    /// PKGBUILD ставит его с правами 755 (клиент окна откажется от файла с
    /// записью для группы или всех).
    #[test]
    fn пакет_везёт_помощника_сеанса_прав() {
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

    /// Каталог пакета и каталог ручной установки — разные, и политика
    /// печатается под тот путь, по которому помощник лежит.
    #[test]
    fn политика_пакета_указывает_на_путь_пакета() {
        let path = plan::helper_path_in(plan::HELPER_DIR_PACKAGE, plan::DE_HELPER_NAME);
        assert_eq!(path, "/usr/lib/rexon-de-helper");
        assert!(plan::de_policy_xml_for(&path).contains(&format!(
            "<annotate key=\"org.freedesktop.policykit.exec.path\">{path}</annotate>"
        )));
        // А зарегистрированная без пути форма остаётся за ручной установкой.
        assert!(plan::de_policy_xml().contains(plan::DE_HELPER_PATH));
        assert_ne!(plan::DE_HELPER_PATH, path);
    }
}
