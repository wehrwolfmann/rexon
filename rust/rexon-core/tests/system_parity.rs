//! Golden-сверка порта `system.rs` с оригиналом `rexon/system.py`.
//!
//! Эталоны сняты с самого Python (генератор импортирует живой `rexon.system`
//! из корня репо и дампит выходы через `json.dump(..., ensure_ascii=False)`):
//!   * `golden/system_detect_cases.json` — `DistroDetector` на батарее
//!     os-release фикстур (CachyOS/SteamOS/Astra/ROSA/ALT/NixOS/Ubuntu/
//!     Tumbleweed/Bazzite/Silverblue + кавычки/комментарии/пустой NAME/
//!     фолбэк /usr/lib/os-release/бинарная детекция); `open()` в оракуле
//!     замокан на текст фикстуры, `shutil.which` — на список бинарников.
//!   * `golden/system_de_db.json` — `ID_MAP`, вся `_DE_DATABASE` (9 DE,
//!     включая LXQt), `_DE_META` и прогон get_packages/is_available/
//!     get_aur_packages по каждому DE × ВСЕ 12 семейств (+ неизвестный DE).
//!   * `golden/system_installer_cases.json` — `_expand_groups` (группы
//!     mate/mate-extra, дедуп общего члена, пустой вывод), `_qi_field`
//!     (версионные констрейнты — "gtk3>=…"→"gtk3" по самому левому
//!     сепаратору, поле None, строка без двоеточия), `_required_by_outside` и
//!     `_filter_removable_transitive` (пример из докстринга, транзитивная
//!     цепочка, ЦИКЛ в зависимостях, всё-safe, не-Arch); `safe_run` в
//!     оракуле замокан на фикстуры вывода pacman. Отдельные кейсы
//!     `russian-locale-*` — вывод pacman на русской локали (грабля LANG=C):
//!     поля не парсятся, поведение Rust совпадает с Python на той же фикстуре.
//!
//! Любое расхождение — ошибка порта, а не теста.

use std::collections::HashSet;

use rexon_core::system::{
    expand_groups, filter_removable_transitive, get_aur_packages, get_check_binary,
    get_config_paths, get_packages, is_available, list_de_ids, parse_group_members,
    polkit_missing_message, qi_field, required_by_outside, DistroDetector, PkgFamily,
    DE_DATABASE, DE_META, ID_MAP,
};
use rexon_core::system_plan::{
    file_owner_queries, foreign_packages_query, gentoo_overlay_for,
    gentoo_overlay_tool_install_cmd, install_argv, install_cmd, package_group_query,
    privileged_pkg_cmd, sync_repo_cmd, FILE_OWNER_BATCH, GENTOO_OVERLAYS,
    GENTOO_OVERLAY_TOOL_PACKAGE, PKG_HELPER_FAMILIES,
};
use serde_json::{json, Map, Value};

mod i18n_pin;

fn detect_fx() -> Value {
    serde_json::from_str(include_str!("golden/system_detect_cases.json")).unwrap()
}

fn db_fx() -> Value {
    serde_json::from_str(include_str!("golden/system_de_db.json")).unwrap()
}

fn installer_fx() -> Value {
    // Каталог помощника — живой ответ этой машины (голден снят там, где
    // пакета не было): `system_plan::retarget_helper_paths`.
    let raw = rexon_core::system_plan::retarget_helper_paths(include_str!(
        "golden/system_installer_cases.json"
    ));
    serde_json::from_str(&raw).unwrap()
}

/// Замыкание «stdout команды по пакету» из JSON-объекта фикстуры
/// (ключ отсутствует = returncode != 0 = None).
fn output_map(fixture: &Value) -> impl Fn(&str) -> Option<String> + '_ {
    let map = fixture.as_object().expect("объект pkg→stdout");
    move |pkg: &str| map.get(pkg).and_then(Value::as_str).map(String::from)
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

// ============================================================================
// DistroDetector
// ============================================================================

#[test]
fn detect_cases_match_python() {
    let fx = detect_fx();
    let cases = fx["cases"].as_array().expect("cases[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");

    for case in cases {
        let label = case["label"].as_str().unwrap();
        let d = DistroDetector::from_os_release_files(
            case["etc_os_release"].as_str(),
            case["usr_lib_os_release"].as_str(),
        );
        let binaries: HashSet<&str> = case["binaries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_str().unwrap())
            .collect();
        let exp = &case["expected"];

        assert_eq!(
            Value::Object(d.info.clone()),
            exp["info"],
            "{label}: разбор os-release"
        );
        assert_eq!(json!(d.name()), exp["name"], "{label}: name");
        assert_eq!(json!(d.pretty_name()), exp["pretty_name"], "{label}: pretty_name");
        assert_eq!(json!(d.distro_id()), exp["distro_id"], "{label}: distro_id");
        assert_eq!(json!(d.id_like()), exp["id_like"], "{label}: id_like");
        assert_eq!(json!(d.version_id()), exp["version_id"], "{label}: version_id");
        let family = d.family(&|b: &str| binaries.contains(b));
        assert_eq!(json!(family.as_str()), exp["family"], "{label}: family");
        assert_eq!(json!(d.is_steamos()), exp["is_steamos"], "{label}: is_steamos");
        assert_eq!(json!(d.is_immutable()), exp["is_immutable"], "{label}: is_immutable");
    }
}

// ============================================================================
// ID_MAP + PackageDB
// ============================================================================

#[test]
fn id_map_matches_python() {
    let fx = db_fx();
    let mut m = Map::new();
    for (id, fam) in ID_MAP {
        m.insert((*id).to_string(), Value::from(fam.as_str()));
    }
    assert_eq!(Value::Object(m), fx["id_map"], "ID_MAP");
    let order: Vec<&str> = ID_MAP.iter().map(|(k, _)| *k).collect();
    assert_eq!(json!(order), fx["id_map_order"], "порядок ID_MAP");
}

#[test]
fn de_database_matches_python() {
    let fx = db_fx();
    assert_eq!(json!(list_de_ids()), fx["de_ids"], "list_de_ids");

    for (de_id, entry) in DE_DATABASE {
        let want = &fx["db"][*de_id];
        assert!(want.is_object(), "{de_id}: лишний DE в Rust-таблице");

        let mut packages = Map::new();
        for (fam, pkgs) in entry.packages {
            packages.insert(fam.as_str().to_string(), json!(pkgs));
        }
        assert_eq!(Value::Object(packages), want["packages"], "{de_id}: packages");
        let fam_order: Vec<&str> = entry.packages.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(json!(fam_order), want["packages_order"], "{de_id}: порядок семейств");
        assert_eq!(json!(entry.aur), want["aur"], "{de_id}: aur");
        assert_eq!(
            json!(entry.check_package),
            want["check_package"],
            "{de_id}: check_package"
        );
        assert_eq!(
            json!(entry.config_paths),
            want["config_paths"],
            "{de_id}: config_paths"
        );
    }
    assert_eq!(
        DE_DATABASE.len(),
        fx["db"].as_object().unwrap().len(),
        "число DE в базе"
    );
}

/// Мета столов для витрины — как у Python, на русском каталоге.
///
/// Тексты для человека в `DE_META` — английские `msgid`; эталон снят с
/// русского оригинала, поэтому сверка идёт через `po/ru.po` тем же путём,
/// каким их показывает интерфейс (`description_text`/`compositor_text`,
/// нагрузка — подписью `CpuLoad`).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn de_meta_matches_python_in_russian() {
    let _lang = i18n_pin::ru();
    let fx = db_fx();
    let mut ours = serde_json::to_value(DE_META).unwrap();
    for (entry, m) in ours.as_array_mut().unwrap().iter_mut().zip(DE_META) {
        entry["description"] = json!(m.description_text());
        entry["compositor"] = json!(m.compositor_text());
    }
    assert_eq!(ours, fx["de_meta"], "_DE_META");
}

/// Английская витрина столов — без единой русской буквы.
#[test]
fn de_meta_english_has_no_russian() {
    let _lang = i18n_pin::en();
    let text = serde_json::to_string(DE_META).unwrap();
    assert!(
        !text.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')),
        "русское в английской мете столов: {text}"
    );
    let kde = DE_META.iter().find(|m| m.id == "kde").unwrap();
    assert_eq!(kde.cpu_usage.label(), "Medium");
    assert_eq!(kde.description_text(), "A powerful, customizable desktop with beautiful effects");
}

#[test]
fn db_queries_match_python() {
    let fx = db_fx();
    for q in fx["queries"].as_array().expect("queries[]") {
        let de = q["de"].as_str().unwrap();
        let fam = PkgFamily::from_value(q["family"].as_str().unwrap()).unwrap();
        assert_eq!(
            json!(get_packages(de, fam)),
            q["get_packages"],
            "{de}/{}: get_packages",
            fam.as_str()
        );
        assert_eq!(
            json!(is_available(de, fam)),
            q["is_available"],
            "{de}/{}: is_available",
            fam.as_str()
        );
        assert_eq!(
            json!(get_aur_packages(de, fam)),
            q["get_aur_packages"],
            "{de}/{}: get_aur_packages",
            fam.as_str()
        );
    }
    for (de, want) in fx["check_binary"].as_object().unwrap() {
        assert_eq!(json!(get_check_binary(de)), *want, "{de}: get_check_binary");
    }
    for (de, want) in fx["config_paths"].as_object().unwrap() {
        assert_eq!(json!(get_config_paths(de)), *want, "{de}: get_config_paths");
    }
}

// ============================================================================
// Чистые алгоритмы установщика
// ============================================================================

#[test]
fn expand_groups_matches_python() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["expand_groups"].as_array().expect("expand_groups[]") {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let packages = str_vec(&case["packages"]);
        let qg = output_map(&case["qg"]);
        assert_eq!(
            json!(expand_groups(family, &packages, &qg)),
            case["expected"],
            "{label}: expand_groups"
        );
    }
}

/// Вопрос движка «что входит в ГРУППУ» — команда по семейству.
///
/// Группы есть не у всех: у Debian понятия нет вовсе, у dnf и zypper они
/// зовутся иначе и печатают другой формат, руками не проверенный. Такие
/// семейства обязаны отвечать `None` — выдуманная команда молча дала бы
/// пустые списки вместо ответа.
#[test]
fn package_group_query_only_where_groups_exist() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["group_query"]["commands"].as_array().expect("commands[]") {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let got = package_group_query(family, case["group"].as_str().unwrap());
        match got {
            None => assert!(
                case["argv"].is_null(),
                "{label}: движок отказался, а голден ждёт команду"
            ),
            Some(q) => {
                assert_eq!(json!(q.cmd.argv), case["argv"], "{label}: argv");
                assert_eq!(json!(q.binary), case["binary"], "{label}: бинарь");
                assert_eq!(json!(q.cmd.lang_c), case["lang_c"], "{label}: LANG=C");
                assert_eq!(json!(q.cmd.timeout_secs), case["timeout"], "{label}: таймаут");
            }
        }
    }
}

/// Разбор ответа: члены группы, «это не группа» и мусор в выводе.
#[test]
fn parse_group_members_matches_golden() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["group_query"]["members"].as_array().expect("members[]") {
        let label = case["label"].as_str().unwrap();
        let got = parse_group_members(
            case["ok"].as_bool().unwrap(),
            case["stdout"].as_str().unwrap(),
        );
        assert_eq!(json!(got), case["expected"], "{label}: parse_group_members");
    }
}

/// Разбор состава группы на весь проект ОДИН: `expand_groups` обязан
/// отвечать ровно тем же, чем `parse_group_members`, иначе копия разбора
/// заведётся снова.
#[test]
fn expand_groups_uses_the_single_parser() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["group_query"]["members"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let stdout = case["stdout"].as_str().unwrap().to_string();
        let ok = case["ok"].as_bool().unwrap();
        let qg = |_pkg: &str| if ok { Some(stdout.clone()) } else { None };
        let asked = vec!["группа".to_string()];
        let want: Vec<String> = match parse_group_members(ok, &stdout) {
            // Дедуп с сохранением порядка — это уже дело `expand_groups`.
            Some(members) => {
                let mut seen = HashSet::new();
                members.into_iter().filter(|m| seen.insert(m.clone())).collect()
            }
            None => asked.clone(),
        };
        assert_eq!(
            expand_groups(PkgFamily::Arch, &asked, &qg),
            want,
            "{label}: expand_groups разошёлся с общим разбором"
        );
    }
}

/// Семейства без групп не платят ни одним запросом: `expand_groups` отдаёт
/// список как есть и колбэка не дёргает вовсе.
#[test]
fn families_without_groups_are_not_asked() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let asked = std::cell::Cell::new(0usize);
    let qg = |_pkg: &str| {
        asked.set(asked.get() + 1);
        Some("группа член\n".to_string())
    };
    let packages = vec!["mate".to_string(), "mate-extra".to_string()];
    for case in fx["group_query"]["commands"].as_array().unwrap() {
        if !case["argv"].is_null() {
            continue;
        }
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        if family == PkgFamily::Arch {
            continue; // негодное имя — семейство тут ни при чём
        }
        assert_eq!(
            expand_groups(family, &packages, &qg),
            packages,
            "{}: список должен пройти насквозь",
            case["label"].as_str().unwrap()
        );
    }
    assert_eq!(asked.get(), 0, "у семейства без групп спрашивать нечего");
}

/// Происхождение установленного пакета («из репозиториев или собран на
/// месте») умеет отвечать не всякое семейство: у Debian такого понятия нет
/// вовсе, а `dpkg --print-foreign-architectures` — ложный друг, там «foreign»
/// про архитектуру. Кто не умеет — обязан ответить `None`: выдуманная команда
/// молча зачислила бы AUR-темы в «Системные».
#[test]
fn foreign_packages_query_only_where_origin_is_known() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let cases = fx["foreign_query"]["commands"].as_array().expect("commands[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");
    for case in cases {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        match foreign_packages_query(family) {
            None => assert!(
                case["argv"].is_null(),
                "{label}: движок отказался, а голден ждёт команду"
            ),
            Some(q) => {
                assert_eq!(json!(q.cmd.argv), case["argv"], "{label}: argv");
                assert_eq!(json!(q.binary), case["binary"], "{label}: бинарь");
                assert_eq!(json!(q.cmd.lang_c), case["lang_c"], "{label}: LANG=C");
                assert_eq!(json!(q.cmd.timeout_secs), case["timeout"], "{label}: таймаут");
            }
        }
    }
}

/// Вопрос о владельце файла: годные пути уходят пачками, негодные не уходят
/// вовсе, а семейство с другим форматом ответа отвечает `None`. Пустой список
/// команд и `None` — РАЗНОЕ: первое «спрашивать не о чем», второе «спросить
/// нечем».
#[test]
fn file_owner_queries_only_where_format_is_known() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let cases = fx["file_owner_query"]["commands"].as_array().expect("commands[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");
    for case in cases {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let paths = str_vec(&case["paths"]);
        match file_owner_queries(family, &paths) {
            None => assert!(
                case["batches"].is_null(),
                "{label}: движок отказался, а голден ждёт команды"
            ),
            Some(qs) => {
                let argvs: Vec<Vec<String>> = qs.iter().map(|q| q.cmd.argv.clone()).collect();
                assert_eq!(json!(argvs), case["batches"], "{label}: argv по пачкам");
                for q in &qs {
                    assert_eq!(json!(q.binary), case["binary"], "{label}: бинарь");
                    assert_eq!(json!(q.cmd.lang_c), case["lang_c"], "{label}: LANG=C");
                    assert_eq!(
                        json!(q.cmd.timeout_secs),
                        case["timeout"],
                        "{label}: таймаут"
                    );
                }
            }
        }
    }
}

/// Резать пути на пачки — дело движка: у исполнителя своего числа больше нет.
/// Одна пачка ровно `FILE_OWNER_BATCH` путей, лишний путь заводит вторую, и
/// вместе они дают исходный список без потерь и без перестановок.
#[test]
fn file_owner_queries_batch_by_the_engine_limit() {
    let paths: Vec<String> = (0..FILE_OWNER_BATCH + 1)
        .map(|i| format!("/usr/share/themes/T{i}"))
        .collect();
    let qs = file_owner_queries(PkgFamily::Arch, &paths).expect("Arch отвечает");
    assert_eq!(qs.len(), 2, "201 путь — две пачки");
    assert_eq!(qs[0].cmd.argv.len(), 2 + FILE_OWNER_BATCH, "первая пачка полная");
    assert_eq!(qs[1].cmd.argv.len(), 2 + 1, "во второй остаток");
    let seen: Vec<String> = qs
        .iter()
        .flat_map(|q| q.cmd.argv[2..].to_vec())
        .collect();
    assert_eq!(seen, paths, "пути не потерялись и не переставились");
    // Ровно граница: лишней пустой пачки быть не должно.
    let exact = file_owner_queries(PkgFamily::Arch, &paths[..FILE_OWNER_BATCH]).unwrap();
    assert_eq!(exact.len(), 1, "ровно предел — одна пачка");
}

/// Девятый вопрос движка — «синхронизировать ОДИН репозиторий». Отдельно
/// подключаемый репозиторий есть только у Gentoo (это и есть overlay): pacman
/// и apt обновляют все базы разом, а у dnf и zypper синтаксис хоть и есть, но
/// репозиториев Rexon там не подключает — имени взять неоткуда. Кто не умеет,
/// обязан ответить `None`: выдуманная привилегированная команда хуже честного
/// «не знаю».
///
/// ⚠️ Gentoo на машине сборки нет: и команда, и отказы закреплены ТОЛЬКО
/// голденами, ничего не выполнялось.
#[test]
fn sync_repo_cmd_only_where_repos_are_separate() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let cases = fx["sync_repo"]["commands"].as_array().expect("commands[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");
    for case in cases {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let got = sync_repo_cmd(family, case["repo"].as_str().unwrap());
        match got {
            None => assert!(
                case["argv"].is_null(),
                "{label}: движок отказался, а голден ждёт команду"
            ),
            Some(argv) => assert_eq!(json!(argv), case["argv"], "{label}: argv"),
        }
    }
}

/// Имена overlay'ев движок берёт из своей же таблицы — значит, каждое обязано
/// проходить проверку имени репозитория и доезжать до команды последним
/// аргументом. Иначе «подключили, но не синхронизировали» случалось бы молча.
#[test]
fn every_known_overlay_can_be_synced() {
    for (de_id, overlay) in GENTOO_OVERLAYS {
        assert_eq!(gentoo_overlay_for(de_id), Some(*overlay), "{de_id}: таблица");
        let argv = sync_repo_cmd(PkgFamily::Gentoo, overlay)
            .unwrap_or_else(|| panic!("{de_id}: overlay '{overlay}' движок не берёт"));
        assert_eq!(argv.last().map(String::as_str), Some(*overlay), "{de_id}: имя");
    }
}

/// Команда установки overlay-инструмента: именованный путь через
/// rexon-pkg-helper и сырой откат. Не-Gentoo отвечает `None` при любом
/// состоянии helper'а.
#[test]
fn gentoo_overlay_tool_install_cmd_matches_golden() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let cases = fx["gentoo_overlay_tool"]["commands"].as_array().expect("commands[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");
    assert_eq!(
        json!(GENTOO_OVERLAY_TOOL_PACKAGE),
        fx["gentoo_overlay_tool"]["package"],
        "имя пакета инструмента"
    );
    for case in cases {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let avail = case["helper_available"].as_bool().unwrap();
        match gentoo_overlay_tool_install_cmd(family, avail) {
            None => assert!(
                case["argv"].is_null(),
                "{label}: движок отказался, а голден ждёт команду"
            ),
            Some(argv) => assert_eq!(json!(argv), case["argv"], "{label}: argv"),
        }
    }
}

/// Второй копии команды установки быть не должно: сырой откат обязан
/// совпасть с `pkexec` + [`install_cmd`] буква в букву. Именно на этом
/// расхождении и попались — рукописная копия в `ensure_gentoo_overlay` шла
/// без `--quiet`, который есть и в таблице движка, и в шаблоне helper'а.
#[test]
fn overlay_tool_raw_path_is_the_engine_install_cmd() {
    let packages = vec![GENTOO_OVERLAY_TOOL_PACKAGE.to_string()];
    let mut want = vec!["pkexec".to_string()];
    want.extend(install_cmd(PkgFamily::Gentoo, &packages).expect("gentoo умеет ставить"));
    assert_eq!(
        gentoo_overlay_tool_install_cmd(PkgFamily::Gentoo, false),
        Some(want),
        "сырой откат разошёлся с install_cmd"
    );
}

/// Единственный ответ движка на «какой командой поставить эти пакеты».
///
/// Своих таблиц установки в продукте больше нет: и `wallpaper_engine`, и
/// `compliance` спрашивают эту функцию. Прежние копии знали три семейства из
/// одиннадцати и успели разойтись с движком — `pacman -S --noconfirm` без
/// `--needed` и `apt install -y` вместо
/// `apt-get install -y --no-install-recommends`.
///
/// ⛔ Ни одна команда не выполнялась: эталон записан руками, проверка ничего
/// не ставит.
#[test]
fn install_argv_matches_golden() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let cases = fx["install_argv"]["commands"].as_array().expect("commands[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");
    for case in cases {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let packages = str_vec(&case["packages"]);
        let avail = case["helper_available"].as_bool().unwrap();
        match install_argv(family, &packages, avail) {
            None => assert!(
                case["argv"].is_null(),
                "{label}: движок отказался, а голден ждёт команду"
            ),
            Some(argv) => assert_eq!(json!(argv), case["argv"], "{label}: argv"),
        }
    }
}

/// Сторож второй таблицы: для КАЖДОГО семейства сырой путь обязан совпасть с
/// `pkexec` + [`install_cmd`] буква в букву, а именованный — с
/// [`privileged_pkg_cmd`]. Ровно на этом расхождении и попались дважды, так
/// что проверка идёт не по списку из голдена, а по всем двенадцати значениям
/// перечисления: новое семейство мимо неё не проедет.
#[test]
fn install_argv_has_no_table_of_its_own() {
    // Язык — общий на процесс, а сравниваются ответы разных вызовов: без
    // закрепления соседняя сверка переключает язык между ними.
    let _lang = i18n_pin::en();
    let packages = vec!["mpv".to_string()];
    for family in [
        PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse,
        PkgFamily::Gentoo, PkgFamily::Alpine, PkgFamily::Void, PkgFamily::Nix,
        PkgFamily::Solus, PkgFamily::Clear, PkgFamily::Alt, PkgFamily::Unknown,
    ] {
        let fam = family.as_str();
        let Ok(base) = install_cmd(family, &packages) else {
            // UNKNOWN: команды нет ни при каком состоянии helper'а.
            assert_eq!(install_argv(family, &packages, false), None, "{fam}: сырой");
            assert_eq!(install_argv(family, &packages, true), None, "{fam}: именованный");
            continue;
        };
        // NixOS ставит в профиль пользователя — pkexec здесь был бы ошибкой.
        let want_raw = if family == PkgFamily::Nix {
            base.clone()
        } else {
            let mut v = vec!["pkexec".to_string()];
            v.extend(base.iter().cloned());
            v
        };
        assert_eq!(
            install_argv(family, &packages, false),
            Some(want_raw),
            "{fam}: сырой путь разошёлся с install_cmd"
        );
        let want_named = privileged_pkg_cmd(family, "install", &packages, true)
            .unwrap_or_else(|| install_argv(family, &packages, false).unwrap());
        assert_eq!(
            install_argv(family, &packages, true),
            Some(want_named),
            "{fam}: именованный путь разошёлся с privileged_pkg_cmd"
        );
        // И встроенный шаблон самого helper'а — из той же таблицы: семейство
        // либо в списке helper'а и идёт именованным путём, либо нет.
        assert_eq!(
            PKG_HELPER_FAMILIES.contains(&fam),
            privileged_pkg_cmd(family, "install", &packages, true).is_some(),
            "{fam}: список семейств helper'а и его же ответ разошлись"
        );
    }
}

/// «Поставить ничего» командой не выражается: `pacman -S --needed
/// --noconfirm` без единого имени вернул бы от имени root ошибку
/// использования. Пустой список — отказ при любом семействе и любом
/// состоянии helper'а.
#[test]
fn install_argv_refuses_empty_package_list() {
    for family in [
        PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse,
        PkgFamily::Gentoo, PkgFamily::Alpine, PkgFamily::Void, PkgFamily::Nix,
        PkgFamily::Solus, PkgFamily::Clear, PkgFamily::Alt, PkgFamily::Unknown,
    ] {
        for avail in [false, true] {
            assert_eq!(
                install_argv(family, &[], avail),
                None,
                "{}: пустой список дал команду (helper={avail})",
                family.as_str()
            );
        }
    }
}

#[test]
fn qi_field_matches_python() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["qi_field"].as_array().expect("qi_field[]") {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let qi = output_map(&case["qi"]);
        assert_eq!(
            json!(qi_field(
                family,
                case["package"].as_str().unwrap(),
                case["field"].as_str().unwrap(),
                &qi
            )),
            case["expected"],
            "{label}: qi_field"
        );
    }
}

#[test]
fn required_by_outside_matches_python() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["required_by_outside"].as_array().expect("required_by_outside[]") {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let candidate: HashSet<String> = str_vec(&case["candidate"]).into_iter().collect();
        let qi = output_map(&case["qi"]);
        assert_eq!(
            json!(required_by_outside(
                family,
                case["package"].as_str().unwrap(),
                &candidate,
                &qi
            )),
            case["expected"],
            "{label}: required_by_outside"
        );
    }
}

#[test]
fn filter_removable_matches_python() {
    // Язык — до разбора эталона: от него зависит `--lang` помощника
    // (`system_plan::retarget_helper_paths`).
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    for case in fx["filter_removable"].as_array().expect("filter_removable[]") {
        let label = case["label"].as_str().unwrap();
        let family = PkgFamily::from_value(case["family"].as_str().unwrap()).unwrap();
        let packages = str_vec(&case["packages"]);
        let qi = output_map(&case["qi"]);
        let (safe, skipped) = filter_removable_transitive(family, &packages, &qi);
        assert_eq!(json!(safe), case["safe"], "{label}: safe");
        assert_eq!(json!(skipped), case["skipped"], "{label}: skipped");
    }
}

/// Без pkexec молча не работает всё привилегированное — сообщение об этом
/// одно на весь проект и с именем пакета для текущего дистрибутива.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn polkit_missing_message_matches_python() {
    // Эталон — русские тексты оригинала: сверка через po/ru.po.
    let _lang = i18n_pin::ru();
    let fx = installer_fx();
    let c = &fx["polkit_missing"];
    assert_eq!(
        polkit_missing_message(&rexon_core::utils::tool_package_hint("pkexec", "suse")),
        c["with_hint"].as_str().unwrap()
    );
    assert_eq!(polkit_missing_message(""), c["no_hint"].as_str().unwrap());
}

// ============================================================================
// Какое окружение запущено
// ============================================================================

/// Автоопределение обязано узнавать ВСЕ 11 рабочих столов, которые Rexon
/// умеет ставить, — включая тайловые, ради которых оно и переписывалось.
/// Раньше три разных детектора отвечали по-разному, и sway/niri не знал ни
/// один из них.
#[test]
fn detects_every_installable_desktop() {
    use rexon_core::system::{detect_current_de, list_de_ids};

    // Как окружение представляется на самом деле: значение
    // XDG_CURRENT_DESKTOP из сеанса каждого рабочего стола.
    let sessions: &[(&str, &str)] = &[
        ("kde", "KDE"),
        ("gnome", "GNOME"),
        ("cinnamon-mint", "X-Cinnamon"),
        ("xfce", "XFCE"),
        ("mate", "MATE"),
        ("budgie", "Budgie:GNOME"),
        ("pantheon", "Pantheon"),
        // DDE v23/v25 ставит `DDE` (dde-session), deepin 20 — `Deepin`.
        ("deepin", "DDE"),
        ("lxqt", "LXQt"),
        ("cosmic", "COSMIC"),
        ("hyprland", "Hyprland"),
        ("sway", "sway"),
        ("niri", "niri"),
        // river-classic сам переменную не ставит — её кладёт стартовый init
        // Rexon; Wayfire ставит `wayfire` сам.
        ("river", "river"),
        ("wayfire", "wayfire"),
    ];
    for (de_id, xdg) in sessions {
        assert_eq!(
            detect_current_de(xdg, "", true).as_deref(),
            Some(*de_id),
            "сеанс {xdg} не опознан"
        );
    }
    // Pantheon узнаётся и без Wayland, и по имени сеанса (X11 «Classic
    // Session» — pantheon, Wayland «Secure Session» — pantheon-wayland).
    assert_eq!(detect_current_de("Pantheon", "pantheon", false).as_deref(), Some("pantheon"));
    assert_eq!(detect_current_de("", "pantheon-wayland", false).as_deref(), Some("pantheon"));
    // Deepin: deepin 20, Wayland-сеанс Treeland и имя сеанса X11.
    assert_eq!(detect_current_de("Deepin", "", false).as_deref(), Some("deepin"));
    assert_eq!(detect_current_de("Treeland", "treeland", true).as_deref(), Some("deepin"));
    assert_eq!(detect_current_de("", "deepin", false).as_deref(), Some("deepin"));
    // River из экрана входа без стартового init: `XDG_CURRENT_DESKTOP` пуст,
    // но экран входа называет сеанс по файлу `river.desktop`.
    assert_eq!(detect_current_de("", "river", true).as_deref(), Some("river"));
    // Ни один рабочий стол из базы не забыт.
    for de_id in list_de_ids() {
        assert!(
            sessions.iter().any(|(id, _)| *id == de_id),
            "{de_id} есть в базе, но не проверен автоопределением"
        );
    }
}

/// Регистр, составные значения и запасной путь по имени сеанса.
#[test]
fn detect_handles_case_and_session_fallback() {
    use rexon_core::system::detect_current_de;

    assert_eq!(detect_current_de("kde", "", false).as_deref(), Some("kde"));
    assert_eq!(detect_current_de("KDE:plasma", "", false).as_deref(), Some("kde"));
    assert_eq!(detect_current_de("ubuntu:GNOME", "", false).as_deref(), Some("gnome"));
    // Budgie объявляет себя вместе с GNOME — выиграть должен Budgie.
    assert_eq!(detect_current_de("Budgie:GNOME", "", false).as_deref(), Some("budgie"));
    // Имя сеанса как запасной путь (на Plasma это ровно "plasma").
    assert_eq!(detect_current_de("", "plasma", false).as_deref(), Some("kde"));
    assert_eq!(detect_current_de("", "gnome-xorg", false).as_deref(), Some("gnome"));
    assert_eq!(detect_current_de("", "cinnamon", false).as_deref(), Some("cinnamon-mint"));
    // Незнакомый композитор на Wayland — отдаём как есть, а не «не знаю».
    // (river для этого больше не годится: с 30.09 он в общей таблице.)
    assert_eq!(detect_current_de("hikari", "", true).as_deref(), Some("hikari"));
    assert_eq!(detect_current_de("hikari", "", false), None);
    // river, wayfire и labwc узнаются и без Wayland, и по имени сеанса —
    // их знал движок обоев, теперь знает общая таблица.
    assert_eq!(detect_current_de("river", "", false).as_deref(), Some("river"));
    assert_eq!(detect_current_de("labwc:wlroots", "", false).as_deref(), Some("labwc"));
    assert_eq!(detect_current_de("", "wayfire", false).as_deref(), Some("wayfire"));
    assert_eq!(detect_current_de("", "", false), None);
}

/// ⛔ Наборы столов на Arch не тянут чужое окружение и не ломают экран
/// входа соседнего стола.
///
/// Сверено 19.10.2026 по AUR RPC v5 (`info`) и `LC_ALL=C pacman -Si`:
///   * `vala-panel-appmenu` — общий пакет, зависит от `-budgie` (а тот от
///     всего `budgie-desktop`) и от `-valapanel` (тянет `vala-panel`);
///     глобальному меню MATE нужны только `-mate` и `-registrar`;
///   * `mint-artwork` объявляет `conflicts=(lightdm-gtk-greeter
///     lightdm-gtk-greeter-settings)` и сам кладёт
///     `/etc/lightdm/lightdm-gtk-greeter.conf`, а `lightdm-gtk-greeter` Rexon
///     ставит с XFCE и MATE: на такой машине pacman отказал бы в установке
///     Cinnamon, а без отказа снёс бы чужой экран входа.
/// Сторожатся все места, откуда Rexon ставит пакеты столов: база столов,
/// проверка соответствия MATE и пост-установка MATE.
#[test]
fn de_package_sets_do_not_pull_foreign_desktops() {
    use rexon_core::compliance::MATE_AUR_PACKAGES;
    use rexon_core::system_plan::MATE_EXTRA_PACKAGES;

    // Пакет → чем плох.
    const PULLS_FOREIGN_DESKTOP: &[(&str, &str)] = &[
        ("vala-panel-appmenu", "общий пакет: тянет budgie-desktop и vala-panel"),
        ("vala-panel-appmenu-budgie", "тянет budgie-desktop"),
        ("vala-panel-appmenu-valapanel", "тянет vala-panel"),
    ];
    const CONFLICTS_WITH_GTK_GREETER: &[&str] = &["mint-artwork"];

    let mut sets: Vec<(String, Vec<String>)> = Vec::new();
    for (de_id, entry) in DE_DATABASE {
        let mut all: Vec<String> = get_packages(de_id, PkgFamily::Arch);
        all.extend(entry.aur.iter().map(|p| p.to_string()));
        sets.push((format!("стол {de_id}"), all));
    }
    sets.push((
        "проверка соответствия MATE".to_string(),
        MATE_AUR_PACKAGES.iter().map(|p| p.to_string()).collect(),
    ));
    for (tag, _, pkgs) in MATE_EXTRA_PACKAGES {
        sets.push((
            format!("пост-установка MATE «{tag}»"),
            pkgs.iter().map(|p| p.to_string()).collect(),
        ));
    }

    // Экран входа GTK стоит хотя бы в одном наборе — значит, конфликт с
    // ним недопустим ни в одном (машина может нести оба стола).
    let gtk_greeter_used = sets
        .iter()
        .any(|(_, pkgs)| pkgs.iter().any(|p| p == "lightdm-gtk-greeter"));
    assert!(gtk_greeter_used, "в наборах нет lightdm-gtk-greeter — пересмотреть сторож");

    let mut bad = Vec::new();
    for (where_, pkgs) in &sets {
        for p in pkgs {
            if let Some((_, why)) = PULLS_FOREIGN_DESKTOP.iter().find(|(n, _)| n == p) {
                bad.push(format!("{where_}: {p} — {why}"));
            }
            if CONFLICTS_WITH_GTK_GREETER.contains(&p.as_str()) {
                bad.push(format!("{where_}: {p} — конфликтует с lightdm-gtk-greeter"));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");

    // Глобальное меню MATE — ровно своя часть и реестр меню.
    let mate_aur = get_aur_packages("mate", PkgFamily::Arch);
    for p in ["vala-panel-appmenu-mate", "vala-panel-appmenu-registrar"] {
        assert!(mate_aur.iter().any(|x| x == p), "MATE без {p}: {mate_aur:?}");
    }
    // Вид Mint остаётся: темы и значки на месте.
    let mint_aur = get_aur_packages("cinnamon-mint", PkgFamily::Arch);
    for p in ["mint-themes", "mint-y-icons", "mint-x-icons"] {
        assert!(mint_aur.iter().any(|x| x == p), "Cinnamon без {p}: {mint_aur:?}");
    }
}
