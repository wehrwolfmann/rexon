//! Golden-сверка порта `installer.rs` с оригиналом `rexon/installer.py`.
//!
//! Эталон снят генератором из ЖИВОГО Python (READ-ONLY: pkexec/copytree/
//! subprocess никогда не исполнялись — перехвачены заглушками; tempfile.mkstemp
//! перенаправлен в scratch, tmp-пути затем нормализованы к `/tmp/rexon_gen/`).
//! Снято: тексты helper/policy/desktop/launcher; пути по режимам;
//! detect_default_mode; зависимости per family; install-скрипт (обе dep-ветки)
//! с pkexec argv; деинсталляция (скрипт с pkg-remove argv); таблицы owner и
//! remove; parse владельца; get_installed_files.
//!
//! Порт обязан совпасть байт-в-байт. ЕДИНСТВЕННОЕ отступление —
//! `install_cleanup_paths` (в golden не фиксируется): там ПОЧИНЕН баг
//! оригинала (утечка tmp_stage_helper/tmp_stage_policy), проверяется
//! отдельным unit-тестом.

use rexon_core::installer as inst;
use rexon_core::installer::{
    HelperTmp, IconSet, InstallMode, InstallPaths, InstallScriptParams,
};
use rexon_core::system::PkgFamily;
use serde_json::Value;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/installer_cases.json")).unwrap()
}

fn s(v: &Value) -> String {
    v.as_str().unwrap().to_string()
}

fn opt_s(v: &Value) -> Option<String> {
    if v.is_null() {
        None
    } else {
        Some(v.as_str().unwrap().to_string())
    }
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(s).collect()
}

// ============================================================================
// Константы + тексты
// ============================================================================

#[test]
fn constants_match() {
    let g = fx();
    let c = &g["constants"];
    assert_eq!(inst::SYSTEM_INSTALL_ACTION, s(&c["system_install_action"]));
    assert_eq!(inst::SYSTEM_HELPER_PATH, s(&c["system_helper_path"]));
    assert_eq!(inst::SYSTEM_POLICY_PATH, s(&c["system_policy_path"]));
    assert_eq!(inst::INSTALL_DIR, s(&c["install_dir"]));
    assert_eq!(inst::LIB_DIR, s(&c["lib_dir"]));
    assert_eq!(inst::DESKTOP_DIR, s(&c["desktop_dir"]));
}

#[test]
fn texts_match() {
    let g = fx();
    let t = &g["texts"];
    // ⛔ ОСОЗНАННОЕ РАСХОЖДЕНИЕ С ЖИВЫМ PYTHON (28.09.2026): эталон
    // `system_helper_script` переснят после закрытия кода от root через тему
    // Plymouth (`ModuleName=../…` + `.so` в теме) и подмены каталога между
    // проверкой и копией. Байт-паритет с прежним Python означал бы «верни
    // дыру»; пин стоит на исправленном тексте и ловит любую его правку.
    // Переснято 30.09.2026: сообщения помощника переведены (решение
    // Wehrwolfmann) — ключ из вшитой таблицы (`system_l10n`) и `--lang` из
    // белого списка po/LINGUAS; проверки темы Plymouth и пути не менялись.
    // Переснято 09.10.2026: режим «Удалить» (`remove-<вид>`) — только каталог
    // /usr/share/<вид>/themes/<имя>, не ссылка, не выбранная тема, не из пакета.
    assert_eq!(i18n_pin::ru_only(&inst::system_helper_script()), s(&t["system_helper_script"]));
    assert_eq!(i18n_pin::ru_only(&inst::system_policy_xml()), s(&t["system_policy_xml"]));
    // Эталон — Python-вход (он же откат): живые `desktop_content()` зависят
    // от того, есть ли на машине Rust-окно, и в сверке байт-в-байт не
    // участвуют. Rust-вход накрыт тестами в `app_parity.rs`.
    let py = inst::LaunchEntry::Python;
    assert_eq!(i18n_pin::ru_only(&inst::desktop_content_for(&py)), s(&t["desktop_content"]));
    assert_eq!(
        i18n_pin::ru_only(&inst::recover_desktop_content_for(&py)),
        s(&t["recover_desktop_content"])
    );
}

/// ⭐ 16.09.2026 эталон переснят ОСОЗНАННО: оба лаунчера были на Python и
/// звали `from rexon.app import main` — классическое окно, которого поставка
/// больше не везёт. Теперь это `sh`-лаунчер на бинарь окна, как у пакета
/// дистрибутива, но БЕЗ `REXON_PACKAGED`: ручная установка — не пакет.
#[test]
fn launchers_match() {
    let g = fx();
    let l = &g["launchers"];
    assert_eq!(
        inst::system_launcher(&s(&l["system"]["lib_dir"])),
        s(&l["system"]["text"])
    );
    assert_eq!(
        inst::user_launcher(&s(&l["user"]["lib_dir"])),
        s(&l["user"]["text"])
    );
}

// ============================================================================
// detect_default_mode + пути по режимам
// ============================================================================

#[test]
fn detect_default_mode_matches() {
    let g = fx();
    for case in g["detect_default_mode"].as_array().unwrap() {
        let env = case["env"].as_object().unwrap().clone();
        let distro_id = case["distro_id"].as_str().unwrap_or("");
        let mode = inst::detect_default_mode(
            |k| env.get(k).map(|v| v.as_str().unwrap().to_string()),
            distro_id,
        );
        assert_eq!(mode.as_str(), s(&case["mode"]),
                   "env={env:?} distro_id={distro_id}");
    }
}

/// Лаунчер пакета дистрибутива выставляет `REXON_PACKAGED` → portable, и на
/// обычном дистрибутиве, и на неизменяемом (иначе там вышел бы user).
/// Прежде это делал sed в PKGBUILD; он сломался, когда выбор режима
/// переехал в ядро.
#[test]
fn packaged_launcher_means_portable() {
    let packaged = |k: &str| (k == inst::PACKAGED_ENV).then(|| "1".to_string());
    for distro_id in ["arch", "cachyos", "steamos", ""] {
        assert_eq!(inst::detect_default_mode(packaged, distro_id).as_str(), "portable", "{distro_id}");
    }
    assert_eq!(inst::detect_default_mode(|_| None, "arch").as_str(), "system");
}

fn assert_paths(out: &InstallPaths, g: &Value) {
    assert_eq!(out.install_dir, opt_s(&g["install_dir"]));
    assert_eq!(out.lib_dir, opt_s(&g["lib_dir"]));
    assert_eq!(out.desktop_dir, opt_s(&g["desktop_dir"]));
    assert_eq!(out.icon_root, opt_s(&g["icon_root"]));
    assert_eq!(out.pixmap_dir, opt_s(&g["pixmap_dir"]));
    assert_eq!(out.gnome_ext_dir, opt_s(&g["gnome_ext_dir"]));
    assert_eq!(out.cinnamon_ext_dir, opt_s(&g["cinnamon_ext_dir"]));
}

#[test]
fn install_paths_match() {
    let g = fx();
    let p = &g["paths"];
    assert_paths(
        &inst::install_paths(InstallMode::System, "/home/w", None),
        &p["system"]["out"],
    );
    assert_paths(
        &inst::install_paths(InstallMode::User, "/home/w", None),
        &p["user_default_xdg"]["out"],
    );
    assert_paths(
        &inst::install_paths(InstallMode::User, "/home/w", Some("/data/x")),
        &p["user_custom_xdg"]["out"],
    );
    assert_paths(
        &inst::install_paths(InstallMode::Portable, "/home/w", None),
        &p["portable"]["out"],
    );
}

// ============================================================================
// Зависимости времени выполнения per family
// ============================================================================

/// ⭐ 16.09.2026 эталон переснят ОСОЗНАННО: до этого он держал список PySide6
/// и python-cairo — зависимости классического окна на Python, которого
/// поставка больше не везёт. Теперь список — библиотеки, которыми живёт
/// Rust-поставка, тот же набор, что у `depends` пакета дистрибутива.
///
/// ⭐⭐ Переснят ещё раз тем же днём: список разрезан на два разряда, и в
/// эталоне у каждого семейства стало по четыре поля вместо двух —
/// `packages`/`cmd` (обязательные, строго под `set -e`) и
/// `optional_packages`/`optional_cmd` (желательные, отдельной строкой с
/// `|| true`). Ни одно имя пакета при разрезе не менялось и не пропадало:
/// это проверяет `dependency_ranks_split_the_same_set` ниже.
#[test]
fn dependencies_match() {
    let g = fx();
    for (fam_str, case) in g["dependencies"].as_object().unwrap() {
        let fam = PkgFamily::from_value(fam_str)
            .unwrap_or_else(|| panic!("unknown family value {fam_str}"));
        assert_eq!(
            inst::dep_packages(fam),
            str_vec(&case["packages"]),
            "packages for {fam_str}"
        );
        assert_eq!(
            inst::dep_install_cmd(fam),
            s(&case["cmd"]),
            "cmd for {fam_str}"
        );
        assert_eq!(
            inst::optional_dep_packages(fam),
            str_vec(&case["optional_packages"]),
            "optional packages for {fam_str}"
        );
        assert_eq!(
            inst::optional_dep_install_cmd(fam),
            s(&case["optional_cmd"]),
            "optional cmd for {fam_str}"
        );
    }
}

/// ⭐⭐ Сторож разреза 16.09.2026. Строка обязательных зависимостей идёт в
/// корневом скрипте под `set -e`, а имена в одиннадцати семействах из
/// двенадцати выписаны по правилам именования и вживую не проверены —
/// собирался только Arch (`packaging/aur/PKGBUILD`). Одно неверное имя
/// обрывало всю установку. Разряда стало два, и проверяется это У ВСЕХ
/// семейств, а не у одного Arch:
///
///   * в обязательных — только то, без чего окно не откроется: Qt6,
///     layer-shell, загрузчик Vulkan, libmpv;
///   * база (bash, D-Bus, Wayland, GL, OpenSSL, polkit, тема значков) —
///     в желательных;
///   * разряды не пересекаются и вместе дают прежний список целиком.
#[test]
fn dependency_ranks_split_the_same_set() {
    // Прежний (до разреза) список каждого семейства — выписан заново руками,
    // чтобы разрез нельзя было «починить» правкой одного кода.
    let whole: [(PkgFamily, usize); 12] = [
        (PkgFamily::Arch, 16),
        (PkgFamily::Debian, 17),
        (PkgFamily::Fedora, 16),
        (PkgFamily::Suse, 16),
        (PkgFamily::Gentoo, 16),
        (PkgFamily::Alpine, 16),
        (PkgFamily::Void, 16),
        (PkgFamily::Nix, 17),
        (PkgFamily::Solus, 15),
        (PkgFamily::Clear, 17),
        (PkgFamily::Alt, 17),
        (PkgFamily::Unknown, 17),
    ];
    // Слова, по которым узнаётся «база». Имена у семейств разные, общий у них
    // корень — по нему и ищем.
    const BASE: [&str; 7] =
        ["wayland", "glvnd", "dbus", "openssl", "polkit", "bash", "hicolor"];
    for (family, total) in whole {
        let must = inst::dep_packages(family);
        let want = inst::optional_dep_packages(family);
        assert_eq!(
            must.len() + want.len(),
            total,
            "{}: при разрезе потерялось или прибавилось имя пакета",
            family.as_str()
        );
        for pkg in &must {
            assert!(
                !want.contains(pkg),
                "{}: «{pkg}» в обоих разрядах сразу",
                family.as_str()
            );
            let low = pkg.to_lowercase();
            // `qt6-wayland` у Debian — не Wayland, а часть Qt6, которая
            // рисует окно на Wayland: по корню «wayland» она попала бы под
            // проверку зря. Части Qt6 узнаются по началу имени.
            let is_qt = low.starts_with("qt6-") || low.starts_with("qml6-")
                || low.starts_with("libqt6") || low.starts_with("dev-qt/");
            // Debian: `pkexec` — это polkit, и он в желательных; корня
            // «polkit» в имени нет, поэтому назван отдельно.
            assert!(
                is_qt || (low != "pkexec" && !BASE.iter().any(|b| low.contains(b))),
                "{}: «{pkg}» — база, ей место в желательных",
                family.as_str()
            );
        }
        // Qt6 и layer-shell в желательные не проваливаются.
        for pkg in &want {
            let low = pkg.to_lowercase();
            assert!(
                !low.contains("qt6") && !low.contains("qml6") && !low.contains("layershell")
                    && !low.contains("layer-shell") && !low.contains("vulkan")
                    && !low.contains("mpv"),
                "{}: «{pkg}» — без него окно не откроется, ему место в обязательных",
                family.as_str()
            );
        }
        // Обязательных без Qt6 не бывает ни у одного семейства.
        assert!(
            must.iter().any(|p| {
                let l = p.to_lowercase();
                l.contains("qt6") || l.contains("qml6") || l.contains("qtbase")
            }),
            "{}: в обязательных нет ни одной части Qt6",
            family.as_str()
        );
    }
}

/// Переключатель стиля Qt приезжает вместе с самим Rexon — тем паролем,
/// который человек и так вводит, устанавливая программу. Вопроса «доставить
/// библиотеку?» нет намеренно: ответ «нет» даёт худший результат.
#[test]
fn qt_style_helper_installs_with_rexon() {
    // Команда — та же, что у обязательных зависимостей, только один пакет.
    assert_eq!(
        inst::qt_style_install_cmd(PkgFamily::Arch),
        "pacman -S --needed --noconfirm qt6ct"
    );
    assert_eq!(
        inst::qt_style_install_cmd(PkgFamily::Debian),
        "apt-get install -y --no-install-recommends qt6ct"
    );
    assert_eq!(inst::qt_style_install_cmd(PkgFamily::Fedora), "dnf install -y qt6ct");
    assert_eq!(
        inst::qt_style_install_cmd(PkgFamily::Suse),
        "zypper install -y --no-recommends qt6ct"
    );
    // Gentoo хранит имя с категорией.
    assert_eq!(
        inst::qt_style_install_cmd(PkgFamily::Gentoo),
        "emerge --ask=n --quiet gui-apps/qt6ct"
    );
    assert_eq!(inst::qt_style_install_cmd(PkgFamily::Void), "xbps-install -y qt6ct");
    assert_eq!(
        inst::qt_style_install_cmd(PkgFamily::Alpine),
        "apk add --no-progress qt6ct"
    );
    assert_eq!(inst::qt_style_install_cmd(PkgFamily::Solus), "eopkg it -y qt6ct");
    // ALT (APT-RPM) знает qt6ct и знает, чем его поставить: пустой ответ был
    // дырой в снятой отсюда таблице, а не отсутствием пакета.
    assert_eq!(
        inst::qt_style_install_cmd(PkgFamily::Alt),
        "apt-get install -y qt6ct"
    );

    // Ставить нечем — пусто, и это НЕ отказ: там молча работает
    // QT_STYLE_OVERRIDE. В Clear Linux бандла с qt6ct нет вовсе, в NixOS
    // пакеты ставятся правкой конфигурации, у UNKNOWN не опознан менеджер.
    for fam in [PkgFamily::Nix, PkgFamily::Clear, PkgFamily::Unknown] {
        assert!(
            inst::qt_style_install_cmd(fam).is_empty(),
            "{}: доустанавливать нечего",
            fam.as_str()
        );
    }

    // Обязательные зависимости не изменились: это ОТДЕЛЬНАЯ строка скрипта,
    // потому что её падение не должно обрывать установку целиком.
    assert!(!inst::dep_install_cmd(PkgFamily::Arch).contains("qt6ct"));
}

/// Все двенадцать значений перечисления — чтобы новое семейство не проехало
/// мимо сторожей ниже.
const ALL_FAMILIES: [PkgFamily; 12] = [
    PkgFamily::Arch,
    PkgFamily::Debian,
    PkgFamily::Fedora,
    PkgFamily::Suse,
    PkgFamily::Gentoo,
    PkgFamily::Alpine,
    PkgFamily::Void,
    PkgFamily::Nix,
    PkgFamily::Solus,
    PkgFamily::Clear,
    PkgFamily::Alt,
    PkgFamily::Unknown,
];

/// Золотые случаи строки установки для КОРНЕВОГО СКРИПТА: по одному на каждое
/// семейство. Ожидаемое записано руками, а не снято с кода.
///
/// Здесь и виден дефект, ради которого заход: у Gentoo стоял голый `emerge`
/// без `--ask=n`. Portage спрашивает «Would you like to merge these
/// packages? [Yes/No]», ответа в корневом скрипте нет, скрипт идёт под
/// `pkexec` без терминала — установка Rexon вставала молча, внутри `set -e`.
#[test]
fn pkg_install_cmd_per_family() {
    let pkgs = vec!["foo".to_string(), "bar".to_string()];
    let cases: [(PkgFamily, &str); 12] = [
        (PkgFamily::Arch, "pacman -S --needed --noconfirm foo bar"),
        (PkgFamily::Debian, "apt-get install -y --no-install-recommends foo bar"),
        (PkgFamily::Fedora, "dnf install -y foo bar"),
        (PkgFamily::Suse, "zypper install -y --no-recommends foo bar"),
        (PkgFamily::Gentoo, "emerge --ask=n --quiet foo bar"),
        (PkgFamily::Alpine, "apk add --no-progress foo bar"),
        (PkgFamily::Void, "xbps-install -y foo bar"),
        // NixOS — единственное осознанное расхождение с движком, см. ниже.
        (PkgFamily::Nix, ""),
        (PkgFamily::Solus, "eopkg it -y foo bar"),
        (PkgFamily::Clear, "swupd bundle-add foo bar"),
        (PkgFamily::Alt, "apt-get install -y foo bar"),
        // Менеджер не опознан — выдумывать команду нечем.
        (PkgFamily::Unknown, ""),
    ];
    for (family, want) in cases {
        assert_eq!(
            inst::pkg_install_cmd(family, &pkgs),
            want,
            "строка установки для {}",
            family.as_str()
        );
    }
    // Список случаев обязан покрывать всё перечисление целиком.
    for family in ALL_FAMILIES {
        assert!(
            cases.iter().any(|(f, _)| *f == family),
            "{}: семейство не покрыто золотыми случаями",
            family.as_str()
        );
    }
}

/// «Поставить ничего» командой не выражается: `pacman -S --needed
/// --noconfirm` без единого имени вернул бы от имени root ошибку
/// использования. То же правило, что у движка (`install_argv`).
#[test]
fn pkg_install_cmd_refuses_empty_package_list() {
    for family in ALL_FAMILIES {
        assert_eq!(
            inst::pkg_install_cmd(family, &[]),
            "",
            "{}: пустой список — команды нет",
            family.as_str()
        );
    }
}

/// Имя пакета попадает в скрипт, который выполняется от root, поэтому каждое
/// слово экранируется как `shlex.quote`. Заголовок команды из безопасных
/// знаков состоит целиком и кавычек не получает — вид команды не меняется.
///
/// ⛔ Правка 25.09.2026: имя с метасимволом, путём или файлом пакета теперь
/// не «экранируется», а ОТВЕРГАЕТСЯ движком ([`is_safe_pkg_arg`]) — строки
/// команды не будет вовсе. Кавычки остаются вторым слоем (слово скрипта,
/// идущего от root, обязано быть одним словом), но первым стоит отказ:
/// `pacman -S 'foo; rm -rf /'` ставил бы пакет с таким именем, а такого
/// имени не бывает — бывает подлог.
#[test]
fn pkg_install_cmd_quotes_odd_package_names() {
    // Обычные имена (в том числе с категорией Gentoo и дефисами Debian)
    // проходят как есть.
    assert_eq!(
        inst::pkg_install_cmd(PkgFamily::Gentoo, &["dev-qt/qtdeclarative".to_string()]),
        "emerge --ask=n --quiet dev-qt/qtdeclarative"
    );
    assert_eq!(
        inst::pkg_install_cmd(
            PkgFamily::Debian,
            &["qml6-module-org-kde-layershell".to_string()]
        ),
        "apt-get install -y --no-install-recommends qml6-module-org-kde-layershell"
    );
    // А необычные — никак: команды нет, и честное имя рядом её не спасает.
    for bad in [
        "foo; rm -rf /", "$(id -u)", "it's", "/tmp/evil.deb", "../etc/passwd",
        "-Rns", "evil.rpm",
    ] {
        for family in ALL_FAMILIES {
            assert_eq!(
                inst::pkg_install_cmd(family, &[bad.to_string()]),
                "",
                "{}: {bad:?} попало в корневой скрипт",
                family.as_str()
            );
            assert_eq!(
                inst::pkg_install_cmd(
                    family,
                    &[bad.to_string(), "qt6ct".to_string()]
                ),
                "",
                "{}: {bad:?} отмылось честным именем рядом",
                family.as_str()
            );
        }
    }
}

/// Сторож: своей таблицы команд у корневого скрипта БОЛЬШЕ НЕТ. Для каждого
/// семейства строка обязана слово в слово совпасть с ответом движка
/// [`rexon_core::system_plan::install_cmd`], проведённым через `shlex.quote`.
///
/// Два исключения, оба намеренные и оба про контекст, а не про таблицу:
/// * `UNKNOWN` — движок отвечает ошибкой, строка пуста;
/// * `NIX` — движок знает `nix-env -iA`, но эта команда наполняет профиль
///   ЗАПУСТИВШЕГО. Корневой скрипт целиком идёт под одним `pkexec`, сбросить
///   права внутри нечем, и пакеты легли бы в профиль root: команда отработала
///   бы успешно, а покупатель не увидел бы ни ошибки, ни библиотек. Пустая
///   строка честнее молчаливой пустышки.
#[test]
fn pkg_install_cmd_has_no_table_of_its_own() {
    let pkgs = vec!["foo".to_string(), "bar".to_string()];
    for family in ALL_FAMILIES {
        let ours = inst::pkg_install_cmd(family, &pkgs);
        match family {
            PkgFamily::Unknown | PkgFamily::Nix => {
                assert_eq!(ours, "", "{}: строки быть не должно", family.as_str());
                // Движок при этом семейство знает (кроме UNKNOWN) — пустота
                // здесь именно решение, а не дыра в таблице.
                if family == PkgFamily::Nix {
                    assert_eq!(
                        rexon_core::system_plan::install_cmd(family, &pkgs).unwrap(),
                        vec!["nix-env", "-iA", "nixpkgs.foo", "nixpkgs.bar"],
                    );
                }
            }
            _ => {
                let engine = rexon_core::system_plan::install_cmd(family, &pkgs)
                    .unwrap_or_else(|e| panic!("{}: движок отказал: {e}", family.as_str()));
                let want = engine
                    .iter()
                    .map(|w| rexon_core::ext_manager::shlex_quote(w))
                    .collect::<Vec<String>>()
                    .join(" ");
                assert_eq!(
                    ours,
                    want,
                    "{}: строка корневого скрипта разошлась с движком",
                    family.as_str()
                );
            }
        }
    }
}

// ============================================================================
// Install-скрипт (system) — обе dep-ветки
// ============================================================================

fn params_from(g: &Value) -> InstallScriptParams {
    let h = &g["helpers"];
    let helpers = HelperTmp {
        tmp_helper: s(&h["tmp_helper"]),
        tmp_policy: s(&h["tmp_policy"]),
        tmp_de_helper: s(&h["tmp_de_helper"]),
        tmp_de_policy: s(&h["tmp_de_policy"]),
        tmp_pkg_helper: s(&h["tmp_pkg_helper"]),
        tmp_pkg_policy: s(&h["tmp_pkg_policy"]),
        tmp_stage_helper: s(&h["tmp_stage_helper"]),
        tmp_stage_policy: s(&h["tmp_stage_policy"]),
        tmp_de_rules: s(&h["tmp_de_rules"]),
    };
    let ic = &g["icons"];
    let mut sizes: Vec<(u32, String)> = Vec::new();
    for size in [16u32, 24, 32, 48, 64, 128, 256, 512] {
        if let Some(v) = ic["sizes"].get(size.to_string()) {
            sizes.push((size, s(v)));
        }
    }
    let icons = IconSet {
        sizes,
        svg: opt_s(&ic["svg"]),
        master: opt_s(&ic["master"]),
    };
    InstallScriptParams {
        lib_dir: s(&g["lib_dir"]),
        install_dir: s(&g["install_dir"]),
        desktop_dir: s(&g["desktop_dir"]),
        tmp_pkg: s(&g["tmp_pkg"]),
        tmp_launcher: s(&g["tmp_launcher"]),
        tmp_desktop: s(&g["tmp_desktop"]),
        tmp_recover: s(&g["tmp_recover"]),
        icons,
        helpers,
        dep_cmd: s(&g["dep_cmd"]),
        // Эталоны сняты без согласия: правило «без пароля» не ставится.
        passwordless_rule: g["passwordless_rule"].as_bool().unwrap_or(false),
    }
}

fn check_install_case(case: &Value) {
    let p = params_from(&case["params"]);
    assert_eq!(inst::system_install_script(&p), s(&case["script"]));
    // pkexec argv + timeout (последний аргумент — tmp_sh install-скрипта).
    let argv = str_vec(&case["pkexec"]["argv"]);
    let tmp_sh = argv.last().unwrap().clone();
    let cmd = inst::system_install_cmd(&tmp_sh);
    assert_eq!(cmd.argv, argv);
    assert_eq!(cmd.timeout_secs, case["pkexec"]["timeout"].as_u64());
}

#[test]
fn install_system_script_matches() {
    let g = fx();
    check_install_case(&g["install_system"]["deps_installed"]);
    check_install_case(&g["install_system"]["deps_missing"]);
}

/// Починка бага оригинала (правило #16): cleanup включает stage-файлы.
#[test]
fn install_cleanup_includes_stage_files() {
    let g = fx();
    let p = params_from(&g["install_system"]["deps_missing"]["params"]);
    let paths = inst::install_cleanup_paths(&p, "/tmp/rexon_install_X.sh");
    assert!(
        paths.contains(&p.helpers.tmp_stage_helper),
        "tmp_stage_helper должен очищаться (баг оригинала)"
    );
    assert!(
        paths.contains(&p.helpers.tmp_stage_policy),
        "tmp_stage_policy должен очищаться (баг оригинала)"
    );
    // Остальные пути тоже присутствуют.
    assert!(paths.contains(&p.tmp_pkg));
    assert!(paths.contains(&"/tmp/rexon_install_X.sh".to_string()));
    for (_, ip) in &p.icons.sizes {
        assert!(paths.contains(ip));
    }
}

// ============================================================================
// Деинсталляция
// ============================================================================

fn paths_from(g: &Value) -> InstallPaths {
    InstallPaths {
        install_dir: opt_s(&g["install_dir"]),
        lib_dir: opt_s(&g["lib_dir"]),
        desktop_dir: opt_s(&g["desktop_dir"]),
        icon_root: opt_s(&g["icon_root"]),
        pixmap_dir: opt_s(&g["pixmap_dir"]),
        gnome_ext_dir: opt_s(&g["gnome_ext_dir"]),
        cinnamon_ext_dir: opt_s(&g["cinnamon_ext_dir"]),
    }
}

#[test]
fn uninstall_system_script_matches() {
    let g = fx();
    let u = &g["uninstall_system"];
    let sys = paths_from(&u["sys_paths"]);
    assert_eq!(inst::uninstall_system_script(&sys), s(&u["script"]));
    let argv = str_vec(&u["pkexec"]["argv"]);
    let tmp_sh = argv.last().unwrap().clone();
    let cmd = inst::uninstall_system_cmd(&tmp_sh);
    assert_eq!(cmd.argv, argv);
    assert_eq!(cmd.timeout_secs, u["pkexec"]["timeout"].as_u64());
}

#[test]
fn distro_tables_match() {
    let g = fx();
    for (fam_str, v) in g["distro_pkg_owner"].as_object().unwrap() {
        let fam = PkgFamily::from_value(fam_str).unwrap();
        assert_eq!(inst::distro_pkg_owner(fam), Some(str_vec(v)), "{fam_str}");
    }
    for (fam_str, v) in g["distro_pkg_remove"].as_object().unwrap() {
        let fam = PkgFamily::from_value(fam_str).unwrap();
        assert_eq!(inst::distro_pkg_remove(fam), Some(str_vec(v)), "{fam_str}");
    }
    // Семейства вне таблиц → None.
    assert_eq!(inst::distro_pkg_owner(PkgFamily::Gentoo), None);
    assert_eq!(inst::distro_pkg_owner(PkgFamily::Void), None);
    assert_eq!(inst::distro_pkg_remove(PkgFamily::Unknown), None);
}

#[test]
fn candidate_bins_match() {
    let g = fx();
    let want = str_vec(&g["distro_candidate_bins"]);
    assert_eq!(inst::distro_candidate_bins().to_vec(), want);
}

#[test]
fn parse_owner_matches() {
    let g = fx();
    for case in g["parse_owner_cases"].as_array().unwrap() {
        let got = inst::parse_owner_output(&s(&case["tool"]), &s(&case["stdout"]));
        let want = opt_s(&case["expect"]);
        assert_eq!(got, want, "case={case:?}");
    }
}

#[test]
fn pkg_remove_cmd_matches() {
    let g = fx();
    for key in ["arch", "debian"] {
        let case = &g["uninstall_pkg"][key];
        let fam = PkgFamily::from_value(key).unwrap();
        let remove = inst::distro_pkg_remove(fam).unwrap();
        let cmd = inst::pkg_remove_cmd(&remove, &s(&case["pkg_name"]));
        assert_eq!(cmd.argv, str_vec(&case["argv"]), "{key}");
        assert_eq!(cmd.timeout_secs, case["timeout"].as_u64(), "{key}");
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn get_installed_files_matches() {
    // Эталон Python — русский: строка пакета в перечне идёт из po/ru.po.
    let _lang = i18n_pin::ru();
    let g = fx();
    let want = str_vec(&g["get_installed_files"]["case_pkg_and_system"]);
    let sys = paths_from(&g["uninstall_system"]["sys_paths"]);
    let got = inst::get_installed_files(
        Some("rexon"),
        Some(&sys),
        None,
        "/home/w/.config/rexon",
        "/home/w/.local/share/rexon/backups",
        |_| true, // все кандидаты «существуют» (как в golden)
    );
    assert_eq!(got, want);
}

// ══════════════════════════════════════════════════════════════════════════
//  User-режим деинсталляции (секция `uninstall_user_v2`)
// ══════════════════════════════════════════════════════════════════════════
//
// Эталон снят на РЕАЛЬНЫХ user-путях этой машины (`RexonInstaller("user")`):
// какие иконки «есть на диске» задано списком, чтобы тест не зависел от
// состояния ФС и ничего не удалял.

#[test]
fn uninstall_user_paths_match() {
    let fx = fx();
    let g = &fx["uninstall_user_v2"];
    assert_eq!(
        rexon_core::installer::ICON_SIZE_DIRS
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
        str_vec(&g["icon_size_dirs"])
    );
    let u = &g["uninstall_user"];
    let present = str_vec(&u["present"]);
    let paths = paths_from(&u["paths"]);
    assert_eq!(
        rexon_core::installer::uninstall_user_paths(&paths, |x| present
            .iter()
            .any(|q| q == x)),
        str_vec(&u["want"])
    );
}

#[test]
fn uninstall_summary_matches() {
    let fx = fx();
    for c in fx["uninstall_user_v2"]["uninstall_summary"]
        .as_array()
        .unwrap()
    {
        let s = rexon_core::installer::uninstall_summary(
            c["pkg_name"].as_str(),
            c["has_system"].as_bool().unwrap(),
            c["has_user"].as_bool().unwrap(),
        );
        assert_eq!(
            serde_json::json!([s.ok, s.message]),
            c["want"],
            "pkg={} sys={} usr={}",
            c["pkg_name"],
            c["has_system"],
            c["has_user"]
        );
    }
}

// ============================================================================
// Подписи мастера установки, удаления и отказа в установке DE: перевод
// ============================================================================
//
// Тексты переехали с двуязычных пар `Lang::pick` на каталог перевода
// (29.09.2026). Снимок `installer_texts_ru_before.json` снят с ПРЕЖНЕГО
// кода (Lang::Ru) до переезда: по-русски человек обязан видеть то же
// слово в слово. Английский снимок держит выверенный английский и ловит
// любую его правку. Переснять осознанно: `REXON_GOLDEN_WRITE=1`.

mod i18n_pin;

use rexon_core::distro_compat::{InstallReason, Lang};
use rexon_core::installer::{
    InstallFailure, InstallStep, UninstallPart, UserDataKind, WizardOutcome,
};

const TEXTS_HOME: &str = "/home/tester";

fn texts_outcomes(lang: Lang) -> Vec<(String, WizardOutcome)> {
    let mut out = Vec::new();
    for mode in [InstallMode::System, InstallMode::User, InstallMode::Portable] {
        out.push((
            format!("installed_{}", mode.as_str()),
            WizardOutcome::Installed { mode, fell_back: None, command_dir_not_in_path: None },
        ));
    }
    out.push((
        "installed_fell_back".into(),
        WizardOutcome::Installed {
            mode: InstallMode::User,
            fell_back: inst::system_mode_blocked_reason_in("steamos", lang),
            command_dir_not_in_path: Some(format!("{TEXTS_HOME}/.local/bin")),
        },
    ));
    for (name, f) in texts_failures() {
        out.push((format!("failed_{name}"), WizardOutcome::Failed(f)));
    }
    out
}

fn texts_failures() -> Vec<(String, InstallFailure)> {
    let mut out = vec![
        ("password_denied".to_string(), InstallFailure::PasswordDenied),
        ("prompt_missing".into(), InstallFailure::PasswordPromptMissing),
        ("timed_out_300".into(), InstallFailure::TimedOut { secs: 300 }),
        ("timed_out_1".into(), InstallFailure::TimedOut { secs: 1 }),
    ];
    for step in InstallStep::ALL {
        out.push((
            format!("step_{}", serde_json::to_value(step).unwrap().as_str().unwrap()),
            InstallFailure::StepFailed { step, detail: "exit 1".into() },
        ));
    }
    out
}

fn texts_reasons() -> Vec<InstallReason> {
    vec![
        InstallReason::Ok,
        InstallReason::AstraZps,
        InstallReason::AstraNoRepos { de_id: "gnome".into() },
        InstallReason::OsnovaClosed,
        InstallReason::AltUnsupportedDe { de_id: "cosmic".into() },
        InstallReason::CosmicUnavailable,
        InstallReason::RosaNoTaskPackage { de_id: "budgie".into() },
        InstallReason::SteamOsNotYet { de_id: "gnome".into() },
    ]
}

/// Все подписи на одном языке — картой «где показано → текст».
fn all_installer_texts(lang: Lang) -> Value {
    let mut m = serde_json::Map::new();
    let mut put = |k: String, v: Value| {
        assert!(m.insert(k.clone(), v).is_none(), "ключ снимка повторён: {k}");
    };
    let j = |v: &dyn erased::Ser| v.json();
    for distro in ["arch", "steamos"] {
        put(format!("welcome_{distro}"), j(&inst::welcome_texts(distro, TEXTS_HOME, None, lang)));
    }
    put("welcome_xdg".into(), j(&inst::welcome_texts("arch", TEXTS_HOME, Some("/data/xdg"), lang)));
    put("consent".into(), j(&inst::passwordless_consent_texts(lang)));
    for mode in [InstallMode::System, InstallMode::User, InstallMode::Portable] {
        let k = mode.as_str();
        put(format!("progress_{k}"), j(&inst::progress_texts(mode, lang)));
        put(format!("install_done_{k}"), Value::from(inst::install_done_line(mode, lang)));
        put(format!("mode_title_{k}"), Value::from(inst::mode_title(mode, lang)));
        put(format!("mode_description_{k}"), Value::from(inst::mode_description(mode, lang)));
        put(format!("mode_hint_{k}"), Value::from(inst::mode_hint(mode, TEXTS_HOME, None, lang)));
    }
    for (name, outcome) in texts_outcomes(lang) {
        put(format!("result_{name}"), j(&inst::result_texts(&outcome, lang)));
    }
    for (name, f) in texts_failures() {
        put(format!("failure_{name}"), serde_json::json!({
            "title": f.title(lang),
            "problem": f.problem(lang),
            "advice": f.advice(lang),
            "log_line": f.log_line(lang),
            "uninstall": inst::uninstall_failure_line(&f, lang),
        }));
    }
    for step in InstallStep::ALL {
        let k = serde_json::to_value(step).unwrap().as_str().unwrap().to_string();
        put(format!("step_{k}"), serde_json::json!([step.name(lang), step.progress_line(lang)]));
    }
    put("icons_missing".into(), Value::from(inst::icons_missing_notice(lang)));
    put("menu_refresh_later".into(), Value::from(inst::menu_refresh_later_notice(lang)));
    put("dependencies_with_rexon".into(), Value::from(inst::dependencies_with_rexon_notice(lang)));
    put("dependencies_failed".into(), Value::from(inst::dependencies_failed_problem(lang)));
    put("portable_no_install".into(), Value::from(inst::portable_no_install_notice(lang)));
    put("command_path_notice".into(), Value::from(inst::command_path_notice(lang)));
    put("command_path_hint".into(), Value::from(inst::command_path_hint("/home/tester/.local/bin", lang)));
    put("wizard_window_title".into(), Value::from(inst::wizard_window_title(lang)));
    put("dry_run".into(), Value::from(inst::dry_run_notice(lang)));
    put("blocked_steamos".into(), serde_json::json!(inst::system_mode_blocked_reason_in("steamos", lang)));
    put("blocked_arch".into(), serde_json::json!(inst::system_mode_blocked_reason_in("arch", lang)));
    put("without_install".into(), serde_json::json!(inst::without_install_limits(lang)));
    put("settings_install".into(), serde_json::json!([
        inst::settings_install_button(lang),
        inst::settings_install_hint(lang),
        inst::settings_install_started(lang),
        inst::settings_install_no_window(lang),
    ]));
    put("buttons".into(), serde_json::json!([
        inst::continue_button(lang),
        inst::retry_button(lang),
        inst::continue_without_install_button(lang),
    ]));
    for part in UninstallPart::ALL {
        let k = serde_json::to_value(part).unwrap().as_str().unwrap().to_string();
        put(format!("uninstall_part_{k}"), Value::from(part.progress_line(lang)));
    }
    for kind in UserDataKind::ALL {
        let k = serde_json::to_value(kind).unwrap().as_str().unwrap().to_string();
        put(format!("user_data_{k}"), Value::from(kind.title()));
    }
    put("uninstall_done".into(), serde_json::json!([
        inst::uninstall_done_line(true, lang),
        inst::uninstall_done_line(false, lang),
    ]));
    put("uninstall_nothing".into(), Value::from(inst::uninstall_nothing_line(lang)));
    put("uninstall_listed_package".into(), serde_json::json!(inst::get_installed_files(
        Some("rexon-git"), None, None, "/nowhere/config", "/nowhere/backups", |_| false,
    )));
    put("uninstall_kept_link".into(), Value::from(inst::uninstall_kept_link_note("/usr/local/lib/rexon", lang)));
    put("uninstall_removed_link".into(), Value::from(inst::uninstall_removed_link_note("/home/u/.cache/rexon", lang)));
    put("uninstall_package_unsupported".into(), Value::from(inst::uninstall_package_unsupported_line("rexon-git", lang)));
    for r in texts_reasons() {
        put(format!("de_reason_{}", r.code()), serde_json::json!({
            "title": r.title(lang),
            "problem": r.problem(lang),
            "advice": r.advice(lang),
            "explanation": r.explanation(lang),
        }));
    }
    Value::Object(m)
}

/// Serialize без дженерика — чтобы звать одной замыкающей функцией.
mod erased {
    pub trait Ser {
        fn json(&self) -> serde_json::Value;
    }
    impl<T: serde::Serialize> Ser for T {
        fn json(&self) -> serde_json::Value {
            serde_json::to_value(self).expect("подписи сериализуются")
        }
    }
}

fn texts_golden(name: &str, lang: Lang, now: &Value) -> Value {
    let path = format!("{}/tests/golden/{name}", env!("CARGO_MANIFEST_DIR"));
    if std::env::var_os("REXON_GOLDEN_WRITE").is_some() {
        let text = serde_json::to_string_pretty(now).unwrap() + "\n";
        std::fs::write(&path, text).expect("снимок записан");
        eprintln!("снимок {lang:?} переснят: {path}");
    }
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).expect("снимок — JSON")
}

/// Сравнение по ключам — чтобы красное называло, ГДЕ разошлось.
fn assert_same_texts(now: &Value, want: &Value) {
    let (now, want) = (now.as_object().unwrap(), want.as_object().unwrap());
    let mut keys: Vec<&String> = now.keys().chain(want.keys()).collect();
    keys.sort();
    keys.dedup();
    let diff: Vec<String> = keys
        .into_iter()
        .filter(|k| now.get(*k) != want.get(*k))
        .map(|k| format!("{k}:\n  было  {}\n  стало {}", want.get(k).map_or("—".into(), |v| v.to_string()), now.get(k).map_or("—".into(), |v| v.to_string())))
        .collect();
    assert!(diff.is_empty(), "подписи разошлись со снимком:\n{}", diff.join("\n"));
}

/// По-русски — слово в слово как до переезда на каталог.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn russian_installer_texts_as_before() {
    let _lang = i18n_pin::ru();
    let now = all_installer_texts(Lang::Ru);
    let want = texts_golden("installer_texts_ru_before.json", Lang::Ru, &now);
    assert_same_texts(&now, &want);
}

/// Английский — выверенный снимок.
#[test]
fn english_installer_texts() {
    let _lang = i18n_pin::en();
    let now = all_installer_texts(Lang::En);
    let want = texts_golden("installer_texts_en.json", Lang::En, &now);
    assert_same_texts(&now, &want);
}

/// Исполнитель расширений (`ext_exec`): ответы строки состояния по-русски —
/// слово в слово прежние. Своего файла сверки у исполнителя нет, отказы
/// установки проверяются здесь, рядом с остальными подписями установки.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn ext_exec_errors_in_russian_as_before() {
    use rexon_core::ext_exec;
    let _lang = i18n_pin::ru();
    let missing = ext_exec::install_from_file("/nonexistent/rexon-test.rxn").err().unwrap_or_default();
    assert_eq!(missing, "Файл не найден: /nonexistent/rexon-test.rxn");
    let dir = std::env::temp_dir().join(format!("rexon-ext-exec-ru-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("каталог создан");
    let file = dir.join("broken.rxn");
    std::fs::write(&file, b"not a zip").expect("файл записан");
    let broken = ext_exec::install_from_file(&file.to_string_lossy()).err().unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(broken, "Файл повреждён или не является архивом .rxn");
    // Остальные ответы — поштучно по каталогу: прежний русский текст.
    let cat = i18n_pin::ru_catalog();
    for (msgid, before) in [
        ("couldn't create the folder %1: %2", "не создать каталог %1: %2"),
        ("couldn't copy %1 → %2: %3", "не скопировать %1 → %2: %3"),
        ("couldn't remove %1: %2", "не удалить %1: %2"),
        ("couldn't create the link %1: %2", "не создать ссылку %1: %2"),
        ("couldn't write the file %1: %2", "не записать %1: %2"),
        ("couldn't unpack %1: %2", "не распаковать %1: %2"),
        ("couldn't prepare %1: %2", "не разложить %1: %2"),
        ("couldn't save the state: %1", "не записать состояние: %1"),
        ("manifest.json not found in the .rxn archive", "manifest.json не найден в архиве .rxn"),
        ("manifest.json can't be read: %1", "manifest.json не читается: %1"),
        ("couldn't write manifest.json: %1", "не записать manifest.json: %1"),
        ("couldn't create the extensions folder: %1", "не создать каталог расширений: %1"),
        ("couldn't copy the extension: %1", "не скопировать расширение: %1"),
        ("the extension manifest was not accepted: %1", "манифест расширения не принят: %1"),
        ("Extension '%1' not found", "Расширение '%1' не найдено"),
        ("extension “%1” not found", "расширение «%1» не найдено"),
    ] {
        assert_eq!(cat.get(None, msgid), Some(before), "{msgid}");
    }
}

/// Исполнитель установки (`installer_exec`): «подробности для разработчика»
/// под отказом шага по-русски — слово в слово прежние (поштучно по каталогу;
/// `%1`/`%2` стоят там, где прежде подставлялись пути и код возврата).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn installer_exec_details_in_russian_as_before() {
    let cat = i18n_pin::ru_catalog();
    for (msgid, before) in [
        ("exit code %1", "код возврата %1"),
        ("no exit code", "кода возврата нет"),
        (
            "the temporary folder path has spaces or special characters: %1",
            "путь временной папки с пробелами или особыми знаками: %1",
        ),
        ("the installation plan contains an unknown step", "план установки содержит незнакомый шаг"),
        (
            "the administrator script finished successfully, but %1 or %2 was not found",
            "корневой скрипт завершился успешно, но %1 или %2 не найдены",
        ),
        ("the home folder is not known (HOME=%1)", "домашняя папка не определена (HOME=%1)"),
        (
            "%1 is a link to another folder; the installation won't replace it",
            "%1 — ссылка на другую папку; заменять её установка не будет",
        ),
        ("%1 or %2 was not found after the installation", "после установки не найдены %1 или %2"),
        (
            "the Rexon window is missing from the package: %1 was not found next to it",
            "в поставке нет окна Rexon: %1 рядом с ней не найден",
        ),
        ("the removal plan contains an unknown step", "план удаления содержит незнакомый шаг"),
    ] {
        assert_eq!(cat.get(None, msgid), Some(before), "{msgid}");
    }
}
