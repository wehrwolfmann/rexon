//! Golden check of the `engine_update.rs` port against the original
//! `rexon/qt_wallpaper_engine/engine_update.py`.
//!
//! The golden file was captured from the LIVE `rexon.qt_wallpaper_engine.engine_update` by a one-off
//! generator: `_api_json` was mocked with a fixture (there were NO real requests to GitHub),
//! the FS is a temporary HOME (normalized to `/HOME` in the golden, the temporary
//! directory to `/TMP`), `_now_iso` frozen. Covered: paths and constants,
//! the `TARGETS` composition, `_ver_tuple`, the `_now_iso` format, busctl output parsing,
//! parsing Releases/Commits API responses, all branches of `check_target` together with
//! writing the report into the state, `render_changelog_text`, the state file format and
//! `mark_installed`/`mark_seen`, `installed_version`, reading `plugin.toml`,
//! the OWE compatibility gate, the `validate_appimage` verdict, the AppImage install
//! plan, `validate_plugin_zip` (on archives built by Python) and the plugin's
//! FS install plan.
//!
//! `golden/engine_update_cases.json` — the captured values.
//!
//! ⚠️ Deliberate divergences from the golden file (the auto-fix rule, details are
//! in the module docstring):
//! * the plugin zip is downloaded with permissions `0o644`, not `0o755` (the golden keeps
//!   the original `0o755` — the test checks the fixed value);
//! * `version.lstrip("v")` was replaced with `removeprefix("v")`;
//! * the TOML parse error text differs (Rust has its own parser) — only
//!   the message prefix is checked;
//! * the targets got an `asset_prefix` field, and the waywallen `asset_suffix` became
//!   `-x86_64.AppImage` instead of `.AppImage` — a fix of the original's defect 9
//!   (release asset choice, see the header of `engine_update.rs`). The golden was captured
//!   BEFORE the fix, so these two fields were edited in it by hand; the other
//!   check values did not change — the old rule and the new one choose
//!   THE SAME asset in the golden releases (there each release has one
//!   matching file), the divergence is visible only on the live releases
//!   v0.2.8 of OWE and v0.3.1+ of waywallen (test `asset_choice_survives_extra_files`).

use std::io::Write;

use rexon_core::engine_update as eu;
use serde_json::Value;

mod i18n_pin;

fn fx() -> Value {
    serde_json::from_str(include_str!("golden/engine_update_cases.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

const HOME: &str = "/HOME";

// ============================================================================
// Paths, constants, targets
// ============================================================================

#[test]
fn paths_and_constants() {
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(eu::managed_appimage(HOME), s(&g["managed_appimage"]));
    assert_eq!(eu::state_path(HOME), s(&g["state_path"]));
    assert_eq!(eu::OWE_PLUGIN_ID, s(&g["owe_plugin_id"]));
    let min: Vec<u128> = g["owe_min_waywallen"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u128)
        .collect();
    assert_eq!(eu::OWE_MIN_WAYWALLEN.to_vec(), min);
    assert_eq!(eu::FETCH_LIMIT as u64, g["fetch_limit"].as_u64().unwrap());
    assert_eq!(
        eu::UNKNOWN_BASE_LIMIT as u64,
        g["unknown_base_limit"].as_u64().unwrap()
    );
    assert_eq!(eu::USAGE_MARKER, s(&g["usage_marker"]));

    assert_eq!(eu::plugins_dir(None, HOME), s(&g["plugins_dir_default"]));
    assert_eq!(
        eu::plugins_dir(Some("/xdg/data"), HOME),
        s(&g["plugins_dir_xdg"])
    );
    // an empty environment variable = unset (`os.environ.get(...) or ...`)
    assert_eq!(
        eu::plugins_dir(Some(""), HOME),
        s(&g["plugins_dir_empty_xdg"])
    );
    assert_eq!(
        eu::owe_manifest_path(&eu::plugins_dir(None, HOME)),
        format!("{}/{}/plugin.toml", s(&g["plugins_dir_default"]), eu::OWE_PLUGIN_ID)
    );
    assert_eq!(
        eu::state_tmp_path(&eu::state_path(HOME)),
        format!("{}.tmp", s(&g["state_path"]))
    );
}

#[test]
fn targets_match_original() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let got = eu::targets(HOME);
    let want = g["targets"].as_array().unwrap();
    assert_eq!(got.len(), want.len());
    for (t, w) in got.iter().zip(want) {
        assert_eq!(t.key, s(&w["key"]), "key");
        assert_eq!(t.title, s(&w["title"]), "title {}", t.key);
        assert_eq!(t.repo, s(&w["repo"]), "repo {}", t.key);
        assert_eq!(t.asset_prefix, s(&w["asset_prefix"]), "prefix {}", t.key);
        assert_eq!(t.asset_suffix, s(&w["asset_suffix"]), "suffix {}", t.key);
        assert_eq!(t.managed_path, s(&w["managed_path"]), "managed {}", t.key);
        assert_eq!(
            t.installable,
            w["installable"].as_bool().unwrap(),
            "installable {}",
            t.key
        );
        assert_eq!(t.kind.as_str(), s(&w["kind"]), "kind {}", t.key);
        assert_eq!(t.note, s(&w["note"]), "note {}", t.key);
    }
    assert!(eu::target_by_key(HOME, "owe").is_some());
    assert!(eu::target_by_key(HOME, "нет-такой").is_none());
}

// ============================================================================
// Versions and time
// ============================================================================

#[test]
fn ver_tuple_matches_regex_findall() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for (v, want) in g["ver_tuple"].as_object().unwrap() {
        let want: Vec<u128> = want
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as u128)
            .collect();
        assert_eq!(eu::ver_tuple(v), want, "ver_tuple({v:?})");
    }
}

#[test]
fn now_iso_format() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for (ts, want) in g["now_iso_samples"].as_object().unwrap() {
        let ts: i64 = ts.parse().unwrap();
        assert_eq!(eu::now_iso(ts), s(want), "now_iso({ts})");
    }
}

#[test]
fn busctl_output_parsing() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for (out, want) in g["busctl"].as_object().unwrap() {
        assert_eq!(eu::parse_busctl_version(out), s(want), "busctl {out:?}");
    }
    // argv — exactly as in the original
    assert_eq!(
        eu::busctl_version_argv(),
        vec![
            "busctl",
            "--user",
            "get-property",
            "org.waywallen.waywallen.Daemon",
            "/org/waywallen/waywallen/Daemon",
            "org.waywallen.waywallen.Daemon1",
            "Version"
        ]
    );
}

#[test]
fn current_version_priority() {
    let _lang = i18n_pin::ru();
    // the binary beats the live daemon, the daemon beats busctl, busctl beats the state
    assert_eq!(
        eu::current_version(Some("0.3.7"), Some("0.3.5"), Some("s \"0.2.5\""), "0.2.1"),
        "0.3.7"
    );
    assert_eq!(
        eu::current_version(None, Some("0.2.9"), Some("s \"0.2.5\""), "0.2.1"),
        "0.2.9"
    );
    assert_eq!(
        eu::current_version(Some(""), Some("0.2.9"), Some("s \"0.2.5\""), "0.2.1"),
        "0.2.9"
    );
    assert_eq!(
        eu::current_version(None, None, Some("s \"0.2.5\""), "0.2.1"),
        "0.2.5"
    );
    assert_eq!(eu::current_version(None, None, Some("мусор"), "0.2.1"), "0.2.1");
    assert_eq!(eu::current_version(None, None, None, ""), "");
}

/// Defect of 27.08.2026: the installed version was "remembered" from the moment of installation.
/// The first source must be `--version` of the binary — the very one that
/// is actually launched; everything else (daemon, busctl, state) goes stale.
#[test]
fn installed_version_comes_from_binary() {
    let _lang = i18n_pin::ru();
    // parsing the binary's answer
    assert_eq!(eu::parse_binary_version("waywallen 0.3.7\n"), "0.3.7");
    assert_eq!(eu::parse_binary_version("waywallen v0.3.7"), "0.3.7");
    assert_eq!(eu::parse_binary_version("0.2.4-x86_64"), "0.2.4");
    assert_eq!(eu::parse_binary_version("waywallen 0.3.7+git1"), "0.3.7");
    // an old build prints usage — no version at all
    assert_eq!(
        eu::parse_binary_version("usage: waywallen [OPTIONS] --fps 60"),
        ""
    );
    // numbers without a dot are not considered a version
    assert_eq!(eu::parse_binary_version("waywallen build 60"), "");
    assert_eq!(eu::parse_binary_version("   "), "");
    // argv is built around the found binary
    assert_eq!(
        eu::binary_version_argv("/home/u/.local/bin/waywallen"),
        vec!["/home/u/.local/bin/waywallen", "--version"]
    );

    // the live layout of the defect: 0.3.7 on disk, the daemon reports 0.3.5,
    // the state has had 0.2.6 since 26.07 — "our version" must be 0.3.7
    let binary = eu::parse_binary_version("waywallen 0.3.7\n");
    let live = eu::current_version(Some(&binary), Some("0.3.5"), None, "0.2.6");
    assert_eq!(live, "0.3.7");
    assert_eq!(
        eu::installed_version("waywallen", &live, "", "0.2.6", ""),
        "0.3.7"
    );
    // the binary is silent (an old build) → the former chain of sources is intact
    let live_old = eu::current_version(Some(""), Some("0.3.5"), None, "0.2.6");
    assert_eq!(live_old, "0.3.5");
}

#[test]
fn installed_version_priority() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for (key, want) in g["installed_version"].as_object().unwrap() {
        // key: "<target>|<state json>|<version from plugin.toml or None>"
        let parts: Vec<&str> = key.splitn(3, '|').collect();
        let ts: Value = serde_json::from_str(parts[1]).unwrap();
        let fs_ver = if parts[2] == "None" { "" } else { parts[2] };
        let field = |n: &str| ts.get(n).and_then(Value::as_str).unwrap_or("");
        assert_eq!(
            eu::installed_version(
                parts[0],
                "",
                fs_ver,
                field("installed_version"),
                field("seen_version")
            ),
            s(want),
            "installed_version {key}"
        );
    }
    // waywallen always takes the live engine version
    assert_eq!(
        eu::installed_version("waywallen", "0.2.6", "x", "0.1.0", "0.1.1"),
        "0.2.6"
    );
}

#[test]
fn owe_version_from_plugin_toml() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for (name, case) in g["owe_version_from_manifest"].as_object().unwrap() {
        assert_eq!(
            eu::owe_version_from_manifest(s(&case["text"])),
            s(&case["version"]),
            "манифест {name}"
        );
    }
}

// ============================================================================
// Parsing GitHub API responses
// ============================================================================

fn entries_from_golden(v: &Value) -> Vec<eu::ChangelogEntry> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|r| eu::ChangelogEntry {
            tag: s(&r["tag"]).to_string(),
            name: s(&r["name"]).to_string(),
            published_at: s(&r["published_at"]).to_string(),
            body: s(&r["body"]).to_string(),
            assets: r["assets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| eu::Asset {
                    name: s(&a["name"]).to_string(),
                    url: s(&a["url"]).to_string(),
                })
                .collect(),
        })
        .collect()
}

#[test]
fn releases_and_commits_parsing() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let raw_releases = serde_json::to_string(&g["raw"]["releases"]).unwrap();
    assert_eq!(
        eu::parse_releases(&raw_releases).unwrap(),
        entries_from_golden(&g["fetch_releases"])
    );
    let raw_owe = serde_json::to_string(&g["raw"]["owe_releases"]).unwrap();
    assert_eq!(
        eu::parse_releases(&raw_owe).unwrap(),
        entries_from_golden(&g["fetch_releases_owe"])
    );
    let raw_commits = serde_json::to_string(&g["raw"]["commits"]).unwrap();
    assert_eq!(
        eu::parse_commits(&raw_commits).unwrap(),
        entries_from_golden(&g["fetch_commits"])
    );

    // a non-array at the top level — an empty list (like `if isinstance(...)`)
    assert!(eu::parse_releases("{}").unwrap().is_empty());
    assert!(eu::parse_commits("null").unwrap().is_empty());
    // a broken body — an error (in Python the json.loads exception was caught by check_target)
    assert!(eu::parse_releases("не json").is_err());

    // ⚠️ fix: an explicit null in a field must not crash the parse
    let nulls = r#"[{"tag_name":null,"name":null,"published_at":null,
                     "body":null,"assets":[{"name":null,
                     "browser_download_url":null}]}]"#;
    let got = eu::parse_releases(nulls).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].tag, "");
    assert_eq!(got[0].assets[0].name, "");
}

#[test]
fn api_urls_and_headers() {
    let _lang = i18n_pin::ru();
    let g = fx();
    assert_eq!(
        eu::releases_url("waywallen/waywallen"),
        s(&g["api_urls"]["releases"])
    );
    assert_eq!(
        eu::commits_url("waywallen/waywallen", ""),
        s(&g["api_urls"]["commits_plain"])
    );
    assert_eq!(
        eu::commits_url("waywallen/waywallen", "2026-07-01T00:00:00Z"),
        s(&g["api_urls"]["commits_since"])
    );
    assert_eq!(
        eu::api_headers(),
        vec![
            ("Accept".to_string(), "application/vnd.github+json".to_string()),
            ("User-Agent".to_string(), "Rexon-dev".to_string()),
        ]
    );
}

/// Defect of 27.08.2026, the second half: "up to date" with 0.2.6 installed
/// and the newest tag v0.3.5. The comparison must be NUMERIC and must not trip
/// over the `v` prefix (a string comparison `"0.2.6" >= "0.3.5"` would give exactly that falsehood).
/// We also pin that the check puts into the state the version it
/// compared with — the file no longer shows two different "our versions".
#[test]
fn older_installed_against_v_prefixed_tag_is_an_update() {
    let _lang = i18n_pin::ru();
    let target = eu::targets(HOME)[0].clone();
    let rel = |tag: &str| eu::ChangelogEntry {
        tag: tag.to_string(),
        name: format!("waywallen {tag}"),
        published_at: "2026-08-25T10:00:00Z".into(),
        body: String::new(),
        assets: vec![eu::Asset {
            name: format!("waywallen-{}-x86_64.AppImage", &tag[1..]),
            url: "https://example/w.AppImage".into(),
        }],
    };
    let fetch = eu::FetchOutcome::Releases(vec![rel("v0.3.5")]);

    let st = eu::check_target(&target, "0.2.6", &fetch);
    assert!(st.update_available, "0.2.6 против v0.3.5 — это обновление");
    assert_eq!(st.note, "доступна v0.3.5 (у нас 0.2.6)");
    assert_eq!(st.latest, "v0.3.5");
    assert!(st.asset_url.ends_with("w.AppImage"));

    // digits and the prefix: where the string comparison lies
    assert!(eu::ver_tuple("v0.3.5") > eu::ver_tuple("0.2.6"));
    assert!(eu::ver_tuple("v0.10.0") > eu::ver_tuple("0.9.9"));
    assert!(eu::ver_tuple("v0.3.7") > eu::ver_tuple("0.3.5"));

    // the same version as the tag → "up to date", no update
    let same = eu::check_target(&target, "0.3.5", &fetch);
    assert!(!same.update_available);
    assert_eq!(same.note, "актуально (0.3.5)");

    // state after the check: installed_version = what was compared with
    let mut state = eu::load_state(None);
    eu::store_report(&mut state, &target.key, &st, "2026-08-27T00:00:00Z");
    assert_eq!(
        eu::state_field(&state, "waywallen", "installed_version"),
        "0.2.6"
    );
    assert!(eu::state_field(&state, "waywallen", "note").starts_with("доступна"));

    // the version could not be determined → the former install record is not overwritten
    let unknown = eu::check_target(&target, "", &fetch);
    let mut state2 = eu::load_state(Some(
        r#"{"targets": {"waywallen": {"installed_version": "0.2.6"}}}"#,
    ));
    eu::store_report(&mut state2, &target.key, &unknown, "2026-08-27T00:00:00Z");
    assert_eq!(
        eu::state_field(&state2, "waywallen", "installed_version"),
        "0.2.6"
    );
}

// ============================================================================
// check_target
// ============================================================================

fn extra_target(key: &str, title: &str, repo: &str, suffix: &str, installable: bool) -> eu::UpstreamTarget {
    eu::UpstreamTarget {
        key: key.into(),
        title: title.into(),
        repo: repo.into(),
        // an "outside" target: we do not know the artifact name, the old
        // "suffix only" rule works
        asset_prefix: String::new(),
        asset_suffix: suffix.into(),
        managed_path: String::new(),
        installable,
        kind: if installable {
            eu::InstallKind::AppImage
        } else {
            eu::InstallKind::None
        },
        note: "n".into(),
    }
}

/// Scenario: target, our version, how the trip to the API ended.
fn scenario(g: &Value, name: &str) -> (eu::UpstreamTarget, String, eu::FetchOutcome) {
    let t = eu::targets(HOME);
    let releases = |k: &str| {
        eu::FetchOutcome::Releases(
            eu::parse_releases(&serde_json::to_string(&g["raw"][k]).unwrap()).unwrap(),
        )
    };
    let commits = eu::FetchOutcome::Commits(
        eu::parse_commits(&serde_json::to_string(&g["raw"]["commits"]).unwrap()).unwrap(),
    );
    match name {
        "ww_known_base" => (t[0].clone(), "0.2.4".into(), releases("releases")),
        "ww_up_to_date" => (t[0].clone(), "0.2.5".into(), releases("releases")),
        "ww_unknown_base" => (t[0].clone(), String::new(), releases("releases")),
        "owe_known_base" => (t[1].clone(), "0.1.10".into(), releases("owe_releases")),
        "noasset" => (
            extra_target("noasset", "Без ассета", "x/y", ".AppImage", true),
            "1.0.0".into(),
            releases("noasset_releases"),
        ),
        "report_only" => (
            extra_target("ro", "Только отчёт", "x/z", "", false),
            "0.2.4".into(),
            releases("releases"),
        ),
        "releases_error" => (
            t[0].clone(),
            "0.2.4".into(),
            eu::FetchOutcome::ReleasesFailed("HTTP Error 403: rate limit".into()),
        ),
        "no_releases_commits" => (t[0].clone(), "0.2.4".into(), commits),
        "no_releases_no_commits" => (
            t[0].clone(),
            "0.2.4".into(),
            eu::FetchOutcome::Commits(Vec::new()),
        ),
        "commits_error" => (
            t[0].clone(),
            "0.2.4".into(),
            eu::FetchOutcome::CommitsFailed("connection reset".into()),
        ),
        other => panic!("неизвестный сценарий {other}"),
    }
}

#[test]
fn check_target_all_branches() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let mut ordered: Vec<eu::TargetStatus> = Vec::new();
    for name in [
        "ww_known_base",
        "ww_up_to_date",
        "ww_unknown_base",
        "owe_known_base",
        "noasset",
        "report_only",
        "releases_error",
        "no_releases_commits",
        "no_releases_no_commits",
    ] {
        let want = &g["check"][name]["status"];
        let (target, installed, fetch) = scenario(&g, name);
        let st = eu::check_target(&target, &installed, &fetch);
        assert_eq!(st.key, s(&want["key"]), "{name} key");
        assert_eq!(st.title, s(&want["title"]), "{name} title");
        assert_eq!(st.installed, s(&want["installed"]), "{name} installed");
        assert_eq!(st.latest, s(&want["latest"]), "{name} latest");
        assert_eq!(
            st.update_available,
            want["update_available"].as_bool().unwrap(),
            "{name} update_available"
        );
        assert_eq!(st.asset_url, s(&want["asset_url"]), "{name} asset_url");
        assert_eq!(
            st.installable,
            want["installable"].as_bool().unwrap(),
            "{name} installable"
        );
        assert_eq!(st.note, s(&want["note"]), "{name} note");
        assert_eq!(st.ok, want["ok"].as_bool().unwrap(), "{name} ok");
        let tags: Vec<String> = st.changelog.iter().map(|c| c.tag.clone()).collect();
        let want_tags: Vec<String> = want["changelog"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| s(&c["tag"]).to_string())
            .collect();
        assert_eq!(tags, want_tags, "{name} changelog");
        ordered.push(st);
    }

    // the "no releases, commits unavailable" branch is checked separately: in
    // the original it was built outside the common run
    let want = &g["check"]["commits_error"]["status"];
    let (target, installed, fetch) = scenario(&g, "commits_error");
    let st = eu::check_target(&target, &installed, &fetch);
    assert_eq!(st.note, s(&want["note"]));
    assert!(!st.ok);

    // the "copy everything" report text — by the same statuses and in the same order
    assert_eq!(eu::render_changelog_text(&ordered), s(&g["render_changelog"]));
    assert_eq!(eu::render_changelog_text(&[]), s(&g["render_changelog_empty"]));
}

/// Release asset choice must not break because of extra files in a release.
///
/// ⚠️ The golden file does not fit here: it was captured from the original, and the original was
/// wrong exactly here (defect 9). The asset names are LIVE — `gh api` over the releases of both
/// targets, 31.08.2026. Before the fix the FIRST one matching the suffix was chosen, i.e.
/// `debug-symbols-linux-x86_64.zip` for OWE and `…-aarch64.AppImage` for
/// waywallen.
#[test]
fn asset_choice_survives_extra_files() {
    let _lang = i18n_pin::ru();
    let t = eu::targets(HOME);
    let (ww, owe) = (&t[0], &t[1]);

    // ── names as in the live releases ────────────────────────────────
    let owe_v028 = [
        "debug-symbols-linux-aarch64.zip",
        "debug-symbols-linux-x86_64.zip",
        "org.waywallen.open-wallpaper-engine-0.2.8-linux-aarch64.zip",
        "org.waywallen.open-wallpaper-engine-0.2.8-linux-x86_64.zip",
    ];
    let picked: Vec<&str> = owe_v028
        .iter()
        .copied()
        .filter(|n| eu::asset_matches(n, &owe.asset_prefix, &owe.asset_suffix))
        .collect();
    assert_eq!(
        picked,
        ["org.waywallen.open-wallpaper-engine-0.2.8-linux-x86_64.zip"],
        "OWE v0.2.8: выбран не плагин"
    );

    // since v0.4.0 the archive is named after the project, not the plugin id
    let owe_v041 = [
        "debug-symbols-linux-aarch64.zip",
        "debug-symbols-linux-x86_64.zip",
        "open-wallpaper-engine-0.4.1-linux-aarch64.zip",
        "open-wallpaper-engine-0.4.1-linux-x86_64.zip",
    ];
    let picked: Vec<&str> = owe_v041
        .iter()
        .copied()
        .filter(|n| eu::asset_matches(n, &owe.asset_prefix, &owe.asset_suffix))
        .collect();
    assert_eq!(picked, ["open-wallpaper-engine-0.4.1-linux-x86_64.zip"], "OWE v0.4.1: выбран не плагин");

    let ww_v037 = [
        "waywallen-0.3.7-aarch64.AppImage",
        "waywallen-0.3.7-x86_64.AppImage",
    ];
    let picked: Vec<&str> = ww_v037
        .iter()
        .copied()
        .filter(|n| eu::asset_matches(n, &ww.asset_prefix, &ww.asset_suffix))
        .collect();
    assert_eq!(
        picked,
        ["waywallen-0.3.7-x86_64.AppImage"],
        "waywallen v0.3.7: выбрана чужая архитектура"
    );

    // historical waywallen names with an addition between the version and the architecture
    for name in [
        "waywallen-0.1.4-g8233394-x86_64.AppImage",
        "waywallen-0.1.7-1-x86_64.AppImage",
        "waywallen-0.2.5-x86_64.AppImage",
    ] {
        assert!(
            eu::asset_matches(name, &ww.asset_prefix, &ww.asset_suffix),
            "{name} обязан подходить"
        );
    }

    // side files that were not yet in the releases but appear
    // in any project: the rule rests on what the artifact IS
    for name in [
        "sha256sums.txt",
        "waywallen-0.3.7-x86_64.AppImage.sha256",
        "sources-x86_64.AppImage",
        "waywallen-x86_64.AppImage",
        "waywallen-src.tar.gz",
    ] {
        assert!(
            !eu::asset_matches(name, &ww.asset_prefix, &ww.asset_suffix),
            "{name} не должен считаться артефактом"
        );
    }
    for name in [
        "sources.zip",
        "debug-symbols-0.2.8-linux-x86_64.zip",
        "org.waywallen.open-wallpaper-engine-debug-linux-x86_64.zip",
    ] {
        assert!(
            !eu::asset_matches(name, &owe.asset_prefix, &owe.asset_suffix),
            "{name} не должен считаться плагином"
        );
    }

    // empty asset_prefix = the former behaviour (suffix only)
    assert!(eu::asset_matches("что-угодно.AppImage", "", ".AppImage"));

    // ── and the same through the full check_target ───────────────────
    let release = |tag: &str, names: &[&str]| {
        eu::FetchOutcome::Releases(vec![eu::ChangelogEntry {
            tag: tag.into(),
            name: tag.into(),
            published_at: "2026-08-30T10:00:00Z".into(),
            body: String::new(),
            assets: names
                .iter()
                .map(|n| eu::Asset {
                    name: (*n).into(),
                    url: format!("https://example/{n}"),
                })
                .collect(),
        }])
    };
    let st = eu::check_target(owe, "0.2.5", &release("v0.2.8", &owe_v028));
    assert_eq!(
        st.asset_url,
        "https://example/org.waywallen.open-wallpaper-engine-0.2.8-linux-x86_64.zip"
    );
    let st = eu::check_target(ww, "0.3.5", &release("v0.3.7", &ww_v037));
    assert_eq!(st.asset_url, "https://example/waywallen-0.3.7-x86_64.AppImage");

    // not a single own file in the release — the former addition to note
    let st = eu::check_target(ww, "0.3.5", &release("v0.3.7", &["notes.txt"]));
    assert!(st.asset_url.is_empty());
    assert!(st.note.ends_with("; в релизе нет подходящего ассета"));
}

#[test]
fn check_target_stores_report_in_state() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let now = "2026-07-27T12:34:56Z";
    for name in [
        "ww_known_base",
        "ww_unknown_base",
        "no_releases_commits",
        "no_releases_no_commits",
    ] {
        let Some(want) = g["check"][name]["state_after"].as_str() else {
            continue;
        };
        let (target, installed, fetch) = scenario(&g, name);
        let st = eu::check_target(&target, &installed, &fetch);
        // "no_releases_commits" started with last_check already written
        let mut state = if name == "no_releases_commits" {
            eu::load_state(Some(
                r#"{"targets": {"waywallen": {"last_check": "2026-07-01T00:00:00Z"}}}"#,
            ))
        } else {
            eu::load_state(None)
        };
        eu::store_report(&mut state, &target.key, &st, now);
        assert_eq!(eu::serialize_state(&state), want, "стейт после {name}");
    }
}

// ============================================================================
// State
// ============================================================================

#[test]
fn state_marks_and_serialization() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let now = "2026-07-27T12:34:56Z";
    let mut state = eu::load_state(None);
    eu::mark_installed(&mut state, "waywallen", "0.2.5", now);
    eu::mark_seen(&mut state, "owe", "v0.1.12", now);
    assert_eq!(eu::serialize_state(&state), s(&g["state_after_marks"]));

    // re-reading the same text gives the same object
    let reloaded = eu::load_state(Some(s(&g["state_after_marks"])));
    assert_eq!(
        eu::state_field(&reloaded, "waywallen", "installed_version"),
        "0.2.5"
    );
    assert_eq!(eu::state_field(&reloaded, "owe", "seen_version"), "v0.1.12");
    assert_eq!(eu::state_field(&reloaded, "нет", "installed_version"), "");
}

#[test]
fn broken_state_is_survived() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for (name, case) in g["broken_state"].as_object().unwrap() {
        let text = match name.as_str() {
            "not_json" => "не json",
            "list_top" => "[1,2]",
            "targets_list" => r#"{"targets": [1,2]}"#,
            "entry_str" => r#"{"targets": {"owe": "x"}}"#,
            other => panic!("неизвестный кейс {other}"),
        };
        let state = eu::load_state(Some(text));
        assert_eq!(
            Value::Object(state.clone()),
            case["load"],
            "load_state {name}"
        );
        // ⚠️ fix: the original crashed with AttributeError on a non-object inside
        // "targets"; here any entry is normalized, and the marks are applied
        let mut state = state;
        eu::mark_seen(&mut state, "owe", "1", "T");
        assert_eq!(eu::state_field(&state, "owe", "seen_version"), "1");
    }
}

// ============================================================================
// AppImage
// ============================================================================

#[test]
fn appimage_validation_verdicts() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let v = &g["validate_appimage"];
    let check = |key: &str, got: (bool, String)| {
        let want = v[key].as_array().unwrap();
        assert_eq!(got.0, want[0].as_bool().unwrap(), "{key} ok");
        assert_eq!(got.1, s(&want[1]), "{key} msg");
    };
    check("missing", eu::validate_appimage_verdict(false, false, None, ""));
    check("not_exec", eu::validate_appimage_verdict(true, false, None, ""));
    check(
        "ok_stdout",
        eu::validate_appimage_verdict(true, true, None, "usage: waywallen [OPTIONS]"),
    );
    check(
        "ok_stderr",
        eu::validate_appimage_verdict(true, true, None, "usage: waywallen\nblah"),
    );
    check(
        "bad_output",
        eu::validate_appimage_verdict(true, true, None, "garbagemore garbage"),
    );
    check(
        "spawn_error",
        eu::validate_appimage_verdict(true, true, Some("Exec format error"), ""),
    );

    assert_eq!(
        eu::appimage_probe_argv("/TMP/good"),
        g["probe_argv"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| s(x).to_string())
            .collect::<Vec<_>>()
    );
}

#[test]
fn appimage_install_plan_and_messages() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let plan = eu::appimage_install_plan(HOME, "v0.2.5");
    assert_eq!(plan.dest, s(&g["managed_appimage"]));
    assert_eq!(plan.dest_dir, format!("{HOME}/.local/share/waywallen"));
    assert_eq!(plan.bak, format!("{}.bak-update", plan.dest));
    assert_eq!(plan.tmp, format!("{}.new-update", plan.dest));
    assert_eq!(plan.mode, 0o755);
    // ⚠️ fix: removeprefix("v") instead of lstrip("v")
    assert_eq!(plan.state_version, "0.2.5");
    assert_eq!(eu::appimage_install_plan(HOME, "").state_version, "");
    assert_eq!(eu::version_tag_to_state("vv1"), "v1");
    // the variant from an explicit path gives the same plan
    assert_eq!(
        eu::appimage_install_plan_for_dest(s(&g["managed_appimage"]), "v0.2.5"),
        plan
    );

    let fresh = &g["install_appimage"]["fresh"];
    assert_eq!(
        eu::appimage_install_ok_message(s(&g["managed_appimage"])),
        s(&fresh["msg"])
    );
    let fail = s(&g["install_appimage"]["missing_source"]["msg"]);
    assert!(
        fail.starts_with("ошибка установки, откат: "),
        "неожиданный текст ошибки: {fail}"
    );
    assert_eq!(
        eu::appimage_install_fail_message("бум"),
        "ошибка установки, откат: бум"
    );

    // the state after installation — the state with the same version
    let mut state = eu::load_state(None);
    eu::mark_installed(&mut state, "waywallen", &plan.state_version, "2026-07-27T12:34:56Z");
    assert_eq!(eu::serialize_state(&state), s(&fresh["state"]));
}

// ============================================================================
// OWE plugin
// ============================================================================

#[test]
fn owe_min_gate() {
    let _lang = i18n_pin::ru();
    let g = fx();
    for (v, want) in g["owe_min"].as_object().unwrap() {
        let want = want.as_array().unwrap();
        let (ok, msg) = eu::waywallen_meets_owe_min(v);
        assert_eq!(ok, want[0].as_bool().unwrap(), "owe_min({v})");
        assert_eq!(msg, s(&want[1]), "owe_min({v}) msg");
    }
    let want = g["owe_min_unknown"].as_array().unwrap();
    let (ok, msg) = eu::waywallen_meets_owe_min("");
    assert_eq!(ok, want[0].as_bool().unwrap());
    assert_eq!(msg, s(&want[1]));
}

/// Build a zip with the same entries the golden generator captured.
fn build_zip(dir: &std::path::Path, name: &str, entries: &[(String, String)]) -> std::path::PathBuf {
    let path = dir.join(format!("{name}.zip"));
    let file = std::fs::File::create(&path).unwrap();
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (n, data) in entries {
        zw.start_file(n.clone(), opts).unwrap();
        zw.write_all(data.as_bytes()).unwrap();
    }
    zw.finish().unwrap();
    path
}

#[test]
fn plugin_zip_validation() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let dir = std::env::temp_dir().join(format!("rexon-eu-parity-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    for (name, case) in g["validate_plugin_zip"].as_object().unwrap() {
        let want_ok = case["ok"].as_bool().unwrap();
        let want_msg = s(&case["msg"]);
        let got = match name.as_str() {
            "missing" => eu::validate_plugin_zip(&dir.join("absent.zip")),
            "not_a_zip" => {
                let p = dir.join("notzip.zip");
                std::fs::write(&p, b"not a zip at all").unwrap();
                eu::validate_plugin_zip(&p)
            }
            _ => {
                let entries: Vec<(String, String)> = case["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| {
                        let pair = e.as_array().unwrap();
                        (s(&pair[0]).to_string(), s(&pair[1]).to_string())
                    })
                    .collect();
                let p = build_zip(&dir, name, &entries);
                eu::validate_plugin_zip(&p)
            }
        };
        assert_eq!(got.ok, want_ok, "{name} ok (msg: {})", got.msg);
        if name == "broken_toml" {
            // Rust has its own parser error text — we compare only the prefix
            assert!(
                got.msg.starts_with("архив не читается: "),
                "{name}: {}",
                got.msg
            );
        } else {
            assert_eq!(got.msg, want_msg, "{name} msg");
        }
        let want_info = &case["info"];
        assert_eq!(
            got.info.id,
            want_info.get("id").and_then(Value::as_str).unwrap_or(""),
            "{name} id"
        );
        assert_eq!(
            got.info.name,
            want_info.get("name").and_then(Value::as_str).unwrap_or(""),
            "{name} name"
        );
        assert_eq!(
            got.info.version,
            want_info.get("version").and_then(Value::as_str).unwrap_or(""),
            "{name} version"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn plugin_install_plan_and_messages() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let fresh = &g["install_plugin_zip_fs"]["fresh"];
    let root = "/TMP/plugins";
    let ok_validation = eu::PluginValidation {
        ok: true,
        msg: String::new(),
        info: eu::PluginInfo {
            id: eu::OWE_PLUGIN_ID.to_string(),
            name: "Wallpaper Engine".into(),
            version: "0.2.0".into(),
        },
    };
    let plan = eu::plugin_install_plan(&ok_validation, "/DEFAULT", root, "v0.2.0");
    assert!(plan.ok);
    assert_eq!(plan.root, root);
    assert_eq!(plan.target, format!("{root}/{}", eu::OWE_PLUGIN_ID));
    assert_eq!(plan.bak, format!("{}.bak-update", plan.target));
    assert_eq!(plan.staging_prefix, ".rexon-plugin-install-");
    assert_eq!(plan.extract_subdir, "archive");
    assert_eq!(plan.state_version, "0.2.0");
    assert_eq!(
        eu::plugin_install_ok_message(&plan.info.id, &plan.target),
        s(&fresh["msg"])
    );
    // empty plugins_root → the default directory
    assert_eq!(
        eu::plugin_install_plan(&ok_validation, "/DEFAULT", "", "").root,
        "/DEFAULT"
    );

    // validation failed
    let bad = eu::PluginValidation {
        ok: false,
        msg: "в корне архива нет plugin.toml".into(),
        info: eu::PluginInfo::default(),
    };
    let plan = eu::plugin_install_plan(&bad, "/DEFAULT", root, "1");
    assert!(!plan.ok);
    assert_eq!(
        plan.message,
        s(&g["install_plugin_zip_fs"]["invalid"]["msg"])
    );
    assert_eq!(
        eu::plugin_install_fail_message("бум"),
        "ошибка установки плагина, откат: бум"
    );

    // the state after installation
    let mut state = eu::load_state(None);
    eu::mark_installed(&mut state, "owe", "0.2.0", "2026-07-27T12:34:56Z");
    assert_eq!(eu::serialize_state(&state), s(&fresh["state"]));
}

#[test]
fn zip_modes_and_rollback() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let modes = &g["install_plugin_zip_fs"]["zip_modes"];
    assert_eq!(
        eu::zip_entry_mode(0o644 << 16),
        modes["0o644"].as_u64().unwrap() as u32
    );
    assert_eq!(
        eu::zip_entry_mode(0o755 << 16),
        modes["0o755"].as_u64().unwrap() as u32
    );
    assert_eq!(eu::zip_entry_mode(0), modes["zero"].as_u64().unwrap() as u32);

    // ⚠️ fix: target is removed only if we created it ourselves —
    // otherwise an unpack failure wiped a working installed plugin
    assert_eq!(
        eu::plugin_rollback_plan(false, false),
        eu::PluginRollbackPlan {
            remove_target: false,
            restore_bak: false
        }
    );
    assert_eq!(
        eu::plugin_rollback_plan(true, true),
        eu::PluginRollbackPlan {
            remove_target: true,
            restore_bak: true
        }
    );
    assert_eq!(
        eu::plugin_rollback_plan(true, false),
        eu::PluginRollbackPlan {
            remove_target: false,
            restore_bak: true
        }
    );
}

// ============================================================================
// Download
// ============================================================================

#[test]
fn download_specs() {
    let _lang = i18n_pin::ru();
    let g = fx();
    let d = &g["download_asset"];
    let app = eu::download_spec(eu::DownloadKind::AppImage);
    assert_eq!(app.prefix, s(&d["local_copy"]["basename_prefix"]));
    assert_eq!(app.suffix, s(&d["local_copy"]["suffix"]));
    assert_eq!(format!("0o{:o}", app.mode), s(&d["local_copy"]["mode"]));
    assert_eq!(app.chunk, 1 << 20);
    assert_eq!(app.timeout_secs, 30);

    let zipd = eu::download_spec(eu::DownloadKind::PluginZip);
    assert_eq!(zipd.prefix, s(&d["plugin_local_copy"]["basename_prefix"]));
    assert_eq!(zipd.suffix, s(&d["plugin_local_copy"]["suffix"]));
    // ⚠️ fix: the original set 0o755 on the archive too (the golden keeps it)
    assert_eq!(s(&d["plugin_local_copy"]["mode"]), "0o755");
    assert_eq!(zipd.mode, 0o644);

    assert_eq!(eu::progress_mib(3 << 20, 200 << 20), (3, 200));
    assert_eq!(eu::progress_mib(0, 0), (0, 0));
}

// ============================================================================
// Language: the «Разработчик» page in English and in Russian
// ============================================================================

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

fn one_release(tag: &str, asset: &str) -> eu::FetchOutcome {
    eu::FetchOutcome::Releases(vec![eu::ChangelogEntry {
        tag: tag.into(),
        name: tag.into(),
        published_at: "2026-08-30T10:00:00Z".into(),
        body: String::new(),
        assets: vec![eu::Asset { name: asset.into(), url: format!("https://example/{asset}") }],
    }])
}

/// Everything the update core tells the person on the «Разработчик» page,
/// in English — without a single Russian letter.
#[test]
fn english_engine_update_texts_have_no_russian() {
    let _lang = i18n_pin::en();
    let targets = eu::targets(HOME);
    let ww = &targets[0];
    let mut all: Vec<String> = Vec::new();
    for t in &targets {
        all.push(t.title.clone());
        all.push(t.note.clone());
    }
    let statuses = vec![
        eu::check_target(ww, "0.3.5", &eu::FetchOutcome::ReleasesFailed("timeout".into())),
        eu::check_target(ww, "0.3.5", &eu::FetchOutcome::CommitsFailed("timeout".into())),
        eu::check_target(ww, "0.3.5", &eu::FetchOutcome::Commits(vec![])),
        eu::check_target(ww, "", &one_release("v0.3.7", "notes.txt")),
        eu::check_target(ww, "0.3.5", &one_release("v0.3.7", "notes.txt")),
        eu::check_target(ww, "0.3.7", &one_release("v0.3.7", "waywallen-0.3.7-x86_64.AppImage")),
    ];
    for st in &statuses {
        all.push(st.note.clone());
    }
    all.push(eu::render_changelog_text(&statuses));
    all.push(eu::validate_appimage_verdict(false, false, None, "").1);
    all.push(eu::validate_appimage_verdict(true, true, Some("x"), "").1);
    all.push(eu::validate_appimage_verdict(true, true, None, "").1);
    all.push(eu::appimage_install_ok_message("/d"));
    all.push(eu::appimage_install_fail_message("x"));
    all.push(eu::waywallen_meets_owe_min("").1);
    all.push(eu::waywallen_meets_owe_min("0.1.0").1);
    all.push(eu::waywallen_meets_owe_min("9.9.9").1);
    all.push(eu::plugin_install_ok_message("id", "/t"));
    all.push(eu::plugin_install_fail_message("x"));
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    all.push(eu::validate_plugin_entries(&names(&["../x"]), None).msg);
    all.push(eu::validate_plugin_entries(&names(&["a"]), None).msg);
    all.push(eu::validate_plugin_entries(&names(&["plugin.toml"]), Some("[plugin]\nid = \"a\"\n")).msg);
    all.push(eu::validate_plugin_entries(&names(&["plugin.toml"]), Some("[plugin]\nid = \"\"\n")).msg);
    all.push(eu::validate_plugin_zip(std::path::Path::new("/nonexistent.zip")).msg);
    for m in &all {
        assert!(!m.is_empty());
        assert!(!has_cyrillic(m), "русский в английском: {m}");
    }
    assert_eq!(statuses[4].note, "v0.3.7 available (we have 0.3.5); the release has no matching asset");
    assert_eq!(statuses[5].note, "up to date (0.3.7)");
    assert_eq!(eu::waywallen_meets_owe_min("9.9.9").1, "waywallen 9.9.9 is compatible");
}

/// In Russian — verbatim as before the translation (a snapshot of the former strings).
#[test]
fn russian_engine_update_texts_read_as_before() {
    let _lang = i18n_pin::ru();
    let targets = eu::targets(HOME);
    assert_eq!(targets[0].title, "waywallen (приложение-интерфейс)");
    assert_eq!(targets[1].title, "open-wallpaper-engine (движок рендеров)");
    assert_eq!(
        targets[1].note,
        "плагин-zip к waywallen (v0.2.3+), установка через демон или в ~/.local/share/waywallen/plugins/"
    );
    let ww = &targets[0];
    let st = eu::check_target(ww, "0.3.5", &one_release("v0.3.7", "notes.txt"));
    assert_eq!(st.note, "доступна v0.3.7 (у нас 0.3.5); в релизе нет подходящего ассета");
    let st = eu::check_target(ww, "", &one_release("v0.3.7", "notes.txt"));
    assert!(st.note.starts_with("наша версия неизвестна — показано последних выпусков: 1, самый свежий — v0.3.7"));
    assert_eq!(
        eu::check_target(ww, "1", &eu::FetchOutcome::Commits(vec![])).note,
        "релизов нет, новых коммитов нет"
    );
    let text = eu::render_changelog_text(&[st]);
    assert!(text.contains("═══ waywallen (приложение-интерфейс) — (неизвестна) → v0.3.7 ═══"), "{text}");
    assert!(text.contains("Статус: ") && text.contains("(без описания)"), "{text}");
    assert_eq!(
        eu::appimage_install_ok_message("/d"),
        "установлено в /d; работающий демон доигрывает старую версию — перезапустите движок обоев, чтобы подхватилась новая"
    );
    assert_eq!(
        eu::waywallen_meets_owe_min("").1,
        "версия waywallen неизвестна (демон не запущен) — не могу проверить совместимость плагина"
    );
    let names = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        eu::validate_plugin_entries(&names(&["plugin.toml"]), Some("[plugin]\nid = \"a\"\nname = \"N\"\nversion = \"1\"\n")).msg,
        "плагин «N» 1 (id a), файлов: 1; ⚠️ нет files.txt (демон выдаст warning)"
    );
    assert_eq!(
        eu::validate_plugin_zip(std::path::Path::new("/nonexistent.zip")).msg,
        "файл не найден"
    );
}

/// A target installed as a distro package is not downloaded by the «Разработчик» page:
/// a second copy in the home directory would shadow the packaged one and get stuck
/// outside system updates. The refusal is per target.
#[test]
fn system_package_targets_are_not_downloaded() {
    assert_eq!(eu::system_update_refusal("waywallen", false, false), None);
    assert_eq!(eu::system_update_refusal("owe", false, false), None);
    let engine = eu::system_update_refusal("waywallen", true, false).expect("движок пакета");
    assert!(engine.contains("waywallen"), "{engine}");
    assert_eq!(eu::system_update_refusal("owe", true, false), None, "плагин не из пакета — можно");
    let owe = eu::system_update_refusal("owe", false, true).expect("плагин пакета");
    assert!(owe.contains("open-wallpaper-engine"), "{owe}");
    assert_ne!(engine, owe);
    assert_eq!(eu::system_update_refusal("waywallen", false, true), None);
    assert_eq!(eu::system_update_refusal("неизвестно", true, true), None);
}
