//! Golden check of the `distro_compat.rs` port against the original `rexon/distro_compat.py`.
//!
//! The golden file `golden/distro_compat_cases.json` was captured from Python itself: the
//! `DistroCompat` functions and methods were run over a battery of inputs (all known distros,
//! Astra SE by NAME/VARIANT_ID with and without ZPS, unknown distros of various
//! families) through `json.dumps(..., ensure_ascii=False)`. Any mismatch is
//! a bug in the port, not in the test.
//!
//! Checked structurally (serde_json::Value == Value): the tables `DISTRO_INFO` /
//! `ALT_DE_PACKAGES` / `ROSA_DE_PACKAGES` / `ROSA_BRANDING_PACKAGES` and their order,
//! the lists of regional distros, and for every case the whole public
//! API (`is_russian_distro`, `distro_name`, `is_known`, `has_de_repos`,
//! `get_info`, `get_warnings`, `get_compatibility_badge`, and on the DE battery
//! `get_de_packages`, `get_rosa_branding`, `check_install_possible`).
//!
//! ⚠️ A deliberate divergence from the Python original since 28.09.2026 — SteamOS
//! (Steam Deck and Steam Machine): installing desktops there is forbidden, not
//! "allowed with a warning" (a SteamOS update wipes everything
//! installed on top of `/usr`), and the texts were rewritten. The `steamos` entries in the golden file were edited by hand
//! one by one (the table, `get_info`, `get_warnings`, `check_install_possible`);
//! other distros are still checked against what was captured from Python.
//!
//! Since 30.09.2026 the warnings (`get_warnings`, the `DISTRO_INFO` table) and
//! the badge go through the translation catalog: in the golden file they are English (the source), and
//! the former Russian of the golden file captured from Python lies word for word in
//! `golden/distro_compat_ru_before.json` and is checked under the Russian catalog
//! (`warnings_in_russian_as_before`). The short string of
//! `check_install_possible` is a log line; it is Russian, as it was.

mod i18n_pin;

use rexon_core::distro_compat::{
    get_regional_distros, get_supported_regional_distros, get_supported_russian_distros,
    Detector, DistroCompat, PkgFamily, ALT_DE_PACKAGES, DISTRO_INFO, ROSA_BRANDING_PACKAGES,
    ROSA_DE_PACKAGES,
};
use serde_json::{json, Map, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!("golden/distro_compat_cases.json")).unwrap()
}

/// Option<Vec<String>> → Value (None → JSON null, like Python None).
fn opt_list(v: Option<Vec<String>>) -> Value {
    match v {
        Some(list) => Value::from(list),
        None => Value::Null,
    }
}

#[test]
fn tables_match_python() {
    let fx = fixture();
    let tables = &fx["tables"];

    // DISTRO_INFO: id → info, in insertion order.
    let mut di = Map::new();
    for (id, info) in DISTRO_INFO {
        di.insert((*id).to_string(), serde_json::to_value(info).unwrap());
    }
    assert_eq!(Value::Object(di), tables["distro_info"], "DISTRO_INFO");

    // ALT_DE_PACKAGES / ROSA_DE_PACKAGES.
    let mut alt = Map::new();
    for (id, pkgs) in ALT_DE_PACKAGES {
        alt.insert((*id).to_string(), serde_json::to_value(pkgs).unwrap());
    }
    assert_eq!(Value::Object(alt), tables["alt_de_packages"], "ALT_DE_PACKAGES");

    let mut rosa = Map::new();
    for (id, pkgs) in ROSA_DE_PACKAGES {
        rosa.insert((*id).to_string(), serde_json::to_value(pkgs).unwrap());
    }
    assert_eq!(Value::Object(rosa), tables["rosa_de_packages"], "ROSA_DE_PACKAGES");

    // ROSA_BRANDING_PACKAGES: id → [packages].
    let mut branding = Map::new();
    for (id, pkgs) in ROSA_BRANDING_PACKAGES {
        branding.insert((*id).to_string(), json!(pkgs));
    }
    assert_eq!(
        Value::Object(branding),
        tables["rosa_branding_packages"],
        "ROSA_BRANDING_PACKAGES"
    );

    // Table key order (matters for join(keys) in messages).
    let di_order: Vec<&str> = DISTRO_INFO.iter().map(|(k, _)| *k).collect();
    let alt_order: Vec<&str> = ALT_DE_PACKAGES.iter().map(|(k, _)| *k).collect();
    let rosa_order: Vec<&str> = ROSA_DE_PACKAGES.iter().map(|(k, _)| *k).collect();
    assert_eq!(json!(di_order), tables["distro_info_order"], "distro_info order");
    assert_eq!(json!(alt_order), tables["alt_order"], "alt order");
    assert_eq!(json!(rosa_order), tables["rosa_order"], "rosa order");
}

#[test]
fn regional_lists_match_python() {
    let fx = fixture();
    let reg = &fx["regional"];
    assert_eq!(
        serde_json::to_value(get_supported_russian_distros()).unwrap(),
        reg["russian"],
        "russian"
    );
    assert_eq!(
        serde_json::to_value(get_supported_regional_distros(None)).unwrap(),
        reg["all_regional"],
        "all_regional"
    );
    assert_eq!(
        serde_json::to_value(get_regional_distros("CN")).unwrap(),
        reg["cn"],
        "cn"
    );
    assert_eq!(
        serde_json::to_value(get_regional_distros("IN")).unwrap(),
        reg["in"],
        "in"
    );
    assert_eq!(
        serde_json::to_value(get_regional_distros("US")).unwrap(),
        reg["us"],
        "us"
    );
}

#[test]
fn cases_match_python() {
    // Warnings and the badge go through the translation catalog: the English
    // source is checked, whatever the machine's locale and whatever catalog is installed
    // on the system.
    let _lang = i18n_pin::en();
    let fx = fixture();
    let cases = fx["cases"].as_array().expect("cases[]");
    assert!(!cases.is_empty(), "фикстура кейсов пуста");

    for case in cases {
        let label = case["label"].as_str().unwrap();
        let d = &case["detector"];
        let zps = case["zps"].as_bool().unwrap();
        let detector = Detector {
            distro_id: d["distro_id"].as_str().unwrap().to_string(),
            family: PkgFamily::from_value(d["family"].as_str().unwrap())
                .unwrap_or_else(|| panic!("{label}: неизвестное семейство")),
            pretty_name: d["pretty_name"].as_str().unwrap().to_string(),
            variant_id: d["variant_id"].as_str().unwrap().to_string(),
            name: d["name"].as_str().unwrap().to_string(),
        };
        let c = DistroCompat::new(detector);
        let exp = &case["expected"];

        assert_eq!(
            json!(c.is_russian_distro()),
            exp["is_russian_distro"],
            "{label}: is_russian_distro"
        );
        assert_eq!(json!(c.distro_name()), exp["distro_name"], "{label}: distro_name");
        assert_eq!(json!(c.is_known()), exp["is_known"], "{label}: is_known");
        assert_eq!(json!(c.has_de_repos()), exp["has_de_repos"], "{label}: has_de_repos");
        assert_eq!(
            serde_json::to_value(c.get_info()).unwrap(),
            exp["get_info"],
            "{label}: get_info"
        );
        assert_eq!(
            json!(c.get_warnings(zps)),
            exp["get_warnings"],
            "{label}: get_warnings"
        );
        assert_eq!(
            json!(c.get_compatibility_badge()),
            exp["get_compatibility_badge"],
            "{label}: get_compatibility_badge"
        );

        // DE battery — we check exactly the keys the oracle captured.
        let per_de = exp["per_de"].as_object().expect("per_de{}");
        for (de, want) in per_de {
            assert_eq!(
                opt_list(c.get_de_packages(de)),
                want["get_de_packages"],
                "{label}/{de}: get_de_packages"
            );
            assert_eq!(
                json!(c.get_rosa_branding(de)),
                want["get_rosa_branding"],
                "{label}/{de}: get_rosa_branding"
            );
            let (ok, msg) = c.check_install_possible(de, zps);
            assert_eq!(
                json!([ok, msg]),
                want["check_install_possible"],
                "{label}/{de}: check_install_possible"
            );
        }
    }
}

/// Deterministic parts of the side-effect functions (port of `_is_zps_enabled`,
/// `is_apt_rpm_system`, `detect_alt_variant`) — a pointwise logic check.
#[test]
fn side_effect_pure_parts() {
    use rexon_core::distro_compat::{detect_alt_variant_from, is_apt_rpm, zps_enabled_from};

    // is_apt_rpm: apt+rpm without dpkg.
    assert!(is_apt_rpm(true, true, false));
    assert!(!is_apt_rpm(true, true, true));
    assert!(!is_apt_rpm(false, true, false));
    assert!(!is_apt_rpm(true, false, false));

    // detect_alt_variant_from: check priority as in Python (top to bottom).
    assert_eq!(detect_alt_variant_from("NAME=\"ALT Workstation\""), "workstation");
    assert_eq!(detect_alt_variant_from("Рабочая станция"), "workstation");
    assert_eq!(detect_alt_variant_from("ALT Education 10"), "education");
    assert_eq!(detect_alt_variant_from("Образование"), "education");
    assert_eq!(detect_alt_variant_from("ALT Server 10"), "server");
    assert_eq!(detect_alt_variant_from("Сервер"), "server");
    assert_eq!(detect_alt_variant_from("Simply Linux 10"), "simply");
    assert_eq!(detect_alt_variant_from("ALT Starterkit"), "starterkit");
    assert_eq!(detect_alt_variant_from("Something else"), "unknown");
    // server before simply: a string with both gives server.
    assert_eq!(detect_alt_variant_from("simply server"), "server");

    // zps_enabled_from: a kernel module OR the config.
    assert!(zps_enabled_from(Some("... digsig_verif ..."), None));
    assert!(zps_enabled_from(None, Some("DIGSIG_ELF_MODE=1")));
    assert!(!zps_enabled_from(Some("no such module"), Some("DIGSIG_ELF_MODE=0")));
    assert!(!zps_enabled_from(None, None));
}

/// The Russian for SteamOS lives in `po/ru.po` — and says what is promised
/// to the buyer: what works and which desktops are installed as images (since 25.10.2026),
/// without dates and without commands.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn steamos_texts_in_russian_catalog() {
    use rexon_core::distro_compat as dc;
    let po = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../po/ru.po");
    let out = std::env::temp_dir().join(format!("rexon-steamos-ru-{}.mo", std::process::id()));
    let status = std::process::Command::new("msgfmt")
        .arg("--check")
        .arg("-o")
        .arg(&out)
        .arg(&po)
        .status()
        .expect("msgfmt (пакет gettext) запускается");
    assert!(status.success(), "msgfmt отверг {}", po.display());
    let ru = rexon_core::i18n::Catalog::load(&out).expect("каталог читается");
    let _ = std::fs::remove_file(&out);
    let t = |msgid: &str| ru.get(None, msgid).unwrap_or_else(|| panic!("нет перевода «{msgid}»")).to_string();

    assert_eq!(
        t(dc::STEAMOS_SUMMARY_WORKS),
        "Оформление, живые обои и видеозаставка при входе работают на SteamOS и переживают обновления системы."
    );
    assert_eq!(
        t(dc::STEAMOS_SUMMARY_DESKTOPS),
        "На SteamOS можно установить Xfce, LXQt и Hyprland: Rexon собирает их системными расширениями и сам пересобирает после каждого обновления системы."
    );
    let everything = [
        dc::STEAMOS_SUMMARY_WORKS,
        dc::STEAMOS_SUMMARY_DESKTOPS,
        dc::STEAMOS_DESKTOPS_TITLE,
    ]
    .iter()
    .map(|m| t(m))
    .collect::<Vec<_>>()
    .join(" ");
    for banned in ["steamos-readonly", "pacman", "readonly", "скоро"] {
        assert!(!everything.contains(banned), "«{banned}» в тексте для покупателя: {everything}");
    }
}

/// Warnings and the badge in Russian — word for word what was captured from Python
/// before switching to the translation catalog.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn warnings_in_russian_as_before() {
    let before: Value =
        serde_json::from_str(include_str!("golden/distro_compat_ru_before.json")).unwrap();
    let fx = fixture();
    let _lang = i18n_pin::ru();
    let mut seen = 0;
    for case in fx["cases"].as_array().expect("cases[]") {
        let label = case["label"].as_str().unwrap();
        let Some(want) = before.get(label) else { continue };
        let d = &case["detector"];
        let c = DistroCompat::new(Detector {
            distro_id: d["distro_id"].as_str().unwrap().to_string(),
            family: PkgFamily::from_value(d["family"].as_str().unwrap()).unwrap(),
            pretty_name: d["pretty_name"].as_str().unwrap().to_string(),
            variant_id: d["variant_id"].as_str().unwrap().to_string(),
            name: d["name"].as_str().unwrap().to_string(),
        });
        let zps = case["zps"].as_bool().unwrap();
        assert_eq!(json!(c.get_warnings(zps)), want["get_warnings"], "{label}: get_warnings");
        assert_eq!(
            json!(c.get_compatibility_badge()),
            want["get_compatibility_badge"],
            "{label}: get_compatibility_badge"
        );
        seen += 1;
    }
    assert_eq!(seen, before.as_object().unwrap().len(), "не все случаи сверены");
}
