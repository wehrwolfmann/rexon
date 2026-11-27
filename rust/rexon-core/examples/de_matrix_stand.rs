//! The "setting × environment" bench: apply a Rexon setting by the same path as
//! the button — and nothing else. The result must be read with the environment's NATIVE
//! reader (gsettings, kreadconfig6, xfconf-query, lxqt/cosmic/hypr files…)
//! from outside, from the bench script.
//!
//! ⛔ Run ONLY with private HOME / XDG_CONFIG_HOME / XDG_RUNTIME_DIR and
//! its own bus (`dbus-run-session`), without WAYLAND_DISPLAY, SWAYSOCK,
//! HYPRLAND_INSTANCE_SIGNATURE and NIRI_SOCKET: the bench writes to the real settings
//! locations of the HOME it runs under.
//!
//! The environment is detected the same way as in the window: from `XDG_CURRENT_DESKTOP` /
//! `DESKTOP_SESSION` / `WAYLAND_DISPLAY` (`system::detect_current_de`).
//!
//! What is called:
//! * «Appearance» — `appearance_installer::LiveInstaller::apply` (the same
//!   executor as the «Apply» button) and `apply_cursor_size`;
//! * «Settings» (fonts, sounds, terminal) — core plans (`qt_settings::
//!   font_plan`, `sound_plan`, `defaults::set_terminal_plan`), executed
//!   the same way `rexon-gui/src/settings_actions.rs` does it
//!   (`apply_font`, `apply_sound_setting`, `run_terminal_plan`) — those functions
//!   live in the window crate and are not visible from here, so they are repeated here
//!   faithfully in meaning: first the companions, then the native store, a check by reading back.
//!
//! Run: `cargo run -p rexon-core --example de_matrix_stand -- <action> <value>`
//!   de                         — which environment was recognized
//!   categories                 — which «Appearance» sections are visible here
//!   gtk|icons|cursor|kvantum|widgetstyle|colors|sounds|wallpaper <name>
//!   cursor-size <N>
//!   font-system|font-mono|font-small|font-toolbar|font-menu|font-title "Family,size"
//!   font-render on|off <from-to|none> <subpixels> <hinting>   (antialiasing)
//!   font-render-state          — what the tab will show: native + fonts.conf
//!   font-dpi <N|off>           — forced font DPI
//!   font-list | font-install <file> | font-remove <family> — «Font management»
//!   sound-theme <name> | event-sounds true|false     (the «Settings» tab)
//!   terminal <id.desktop>
//!   dde-look-state             — the «Deepin windows» section (explanation only)
//!   file-manager <id.desktop>
//!   section <section> <name>   — a button of an environment section (xfwm4, marco,
//!                                gnomeshell, cinnamontheme, lxqttheme, tabbox,
//!                                kwinscripts, gnomeext, cinnamonapplets, nemoactions,
//!                                conky…): «Apply»/«Enable»/«Disable»/«Start»
//!   section-state <section>    — what is enabled now (`de_apply::active_names`)
//!   conky-autostart <widget> on|off — the «Start at login» switch
//!   system-apply sddm|plymouth <theme> <helper> — «Apply» of a system theme
//!                                with a FAKE pkexec first in PATH (no password)

use rexon_core::{appearance_installer, categories, defaults, gtk_sanitizer, qt_settings as qs, utils};

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

fn current_de() -> String {
    rexon_core::system::detect_current_de(
        &env("XDG_CURRENT_DESKTOP"),
        &env("DESKTOP_SESSION"),
        std::env::var("WAYLAND_DISPLAY").is_ok(),
    )
    .unwrap_or_default()
}

fn home() -> String {
    env("HOME")
}

fn config_home() -> String {
    let xdg = env("XDG_CONFIG_HOME");
    utils::config_home(if xdg.is_empty() { None } else { Some(&xdg) }, &home())
}

fn data_home() -> String {
    let xdg = env("XDG_DATA_HOME");
    if xdg.is_empty() { format!("{}/.local/share", home()) } else { xdg }
}

// ── «Settings» → font management: as settings_actions ───────────────

fn font_library() -> Vec<qs::InstalledFont> {
    qs::parse_fc_list_fonts(&run(&qs::fc_list_fonts_argv(), true).1, &qs::font_user_dirs(&home(), &data_home()))
}

fn install_font(source: &str) -> qs::FontInstallOutcome {
    let name = std::path::Path::new(source)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.to_string());
    let families = qs::parse_fc_query_families(&run(&qs::fc_query_families_argv(source), true).1);
    if families.is_empty() {
        return qs::FontInstallOutcome::NotAFont(name);
    }
    let Some(dest) = qs::font_install_dest(&data_home(), source) else {
        return qs::FontInstallOutcome::CopyFailed(name);
    };
    if std::path::Path::new(&dest).exists() {
        let same = std::fs::read(&dest).ok().zip(std::fs::read(source).ok()).is_some_and(|(a, b)| a == b);
        return if same {
            qs::FontInstallOutcome::AlreadyInstalled(name)
        } else {
            qs::FontInstallOutcome::NameTaken(name)
        };
    }
    let dir = qs::font_install_dir(&data_home());
    if std::fs::create_dir_all(&dir).is_err() || std::fs::copy(source, &dest).is_err() {
        return qs::FontInstallOutcome::CopyFailed(name);
    }
    let _ = run(&qs::fc_cache_argv(&dir), false);
    qs::FontInstallOutcome::Installed(families)
}

fn tool(name: &str) -> String {
    utils::which(name, None).unwrap_or_default()
}

fn run(argv: &[String], lang_c: bool) -> (bool, String) {
    if argv.is_empty() {
        return (false, String::new());
    }
    let report = utils::run(
        &utils::RunSpec {
            argv: argv.to_vec(),
            output: utils::RunOutput::Capture,
            timeout_secs: Some(10),
            lang_c,
            ..Default::default()
        },
        None,
    );
    eprintln!("  $ {}  → {:?}", argv.join(" "), report.returncode);
    (report.returncode == Some(0), utils::decode_text(&report.stdout))
}

fn write_file(path: &str, text: &str) -> bool {
    if let Some(dir) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    utils::atomic_write(path, text).is_ok()
}

// ── «Settings» → fonts: as settings_actions::apply_font ────────────────

fn font_file_path(file: &qs::FontFile) -> String {
    format!("{}/{}", config_home().trim_end_matches('/'), file.rel_path())
}

fn write_font_store(store: &qs::FontStore, family: &str, size: i32, face: qs::FontFace) -> bool {
    match store {
        qs::FontStore::Cmd(_) => match qs::font_store_write_argv(store, family, size, &tool("kwriteconfig6")) {
            Some(argv) => run(&argv, false).0,
            None => false,
        },
        qs::FontStore::File(file) => {
            let base = config_home();
            let Some(writes) = file.file_writes_face(&base, &ConfigFiles, family, size, face) else {
                eprintln!("  (файл {} не тронут: его нет и заводить нельзя, либо блок разорван)", font_file_path(file));
                return false;
            };
            writes.iter().all(|w| {
                let path = format!("{}/{}", base.trim_end_matches('/'), w.rel_path);
                eprintln!("  > {path}{}", if w.replace_link { " (ссылка заменена файлом)" } else { "" });
                if w.replace_link && std::fs::remove_file(&path).is_err() {
                    return false;
                }
                write_file(&path, &w.text)
            })
        }
    }
}

/// As `settings_actions::ConfigFiles`.
struct ConfigFiles;

impl qs::FontFileIo for ConfigFiles {
    fn read(&self, rel_path: &str) -> Option<String> {
        std::fs::read_to_string(format!("{}/{rel_path}", config_home().trim_end_matches('/'))).ok()
    }

    fn link_target(&self, rel_path: &str) -> Option<String> {
        let path = format!("{}/{rel_path}", config_home().trim_end_matches('/'));
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if !meta.file_type().is_symlink() {
            return None;
        }
        std::fs::canonicalize(&path).ok().map(|p| p.to_string_lossy().into_owned())
    }
}

fn apply_font(de: &str, target: &str, family: &str, size: i32, face: qs::FontFace) -> bool {
    if !qs::font_target_available(de, target) {
        eprintln!("  (цель {target} в {de} не показывается)");
        return false;
    }
    let plan = qs::font_plan(de, target);
    for argv in &plan.extra {
        let _ = run(argv, false);
    }
    let ok = write_font_store(&plan.primary, family, size, face);
    for store in &plan.also {
        let _ = write_font_store(store, family, size, face);
    }
    if plan.gtk_ini {
        let spec = qs::gnome_font_spec(family, size);
        for path in qs::gtk_font_ini_paths(&config_home()) {
            let text = gtk_sanitizer::write_ini_setting_in(
                std::fs::read_to_string(&path).ok().as_deref(),
                "Settings",
                qs::GTK_FONT_KEY,
                &spec,
            );
            let _ = write_file(&path, &text);
        }
        for (dir, ini) in qs::stray_gtk_font_dirs(&config_home()) {
            let Ok(read) = std::fs::read_dir(&dir) else { continue };
            let entries: Vec<String> =
                read.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            if qs::stray_gtk_font_dir_removable(&entries, std::fs::read_to_string(&ini).ok().as_deref()) {
                eprintln!("  - снят мусорный {dir}");
                let _ = std::fs::remove_file(&ini);
                let _ = std::fs::remove_dir(&dir);
            }
        }
    }
    ok
}

// ── «Settings» → antialiasing: as settings_actions::apply_rendering ───────

fn fontconfig_path() -> String {
    format!("{}/{}", config_home().trim_end_matches('/'), qs::FONTCONFIG_REL_PATH)
}

fn apply_rendering(de: &str, r: &qs::FontRendering) -> qs::RenderingOutcome {
    let path = fontconfig_path();
    let existing = std::fs::read_to_string(&path).ok();
    let dpi = existing.as_deref().and_then(|t| qs::fontconfig_rendering(t).dpi);
    let text = match qs::fontconfig_with_block(existing.as_deref(), &qs::fontconfig_block(r, dpi)) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("  (fonts.conf не тронут: {e:?})");
            return qs::RenderingOutcome::FontconfigRefused(path);
        }
    };
    eprintln!("  > {path}");
    if !write_file(&path, &text) {
        return qs::RenderingOutcome::FontconfigFailed(path);
    }
    let plan = qs::rendering_plan(de, r, &tool("kwriteconfig6"), !tool("gsettings").is_empty());
    let mut ok = true;
    for argv in &plan.primary {
        ok &= run(argv, false).0;
    }
    for argv in &plan.also {
        let _ = run(argv, false);
    }
    if legacy_render_available() {
        let mut legacy_ok = true;
        for argv in qs::legacy_rendering_argv(r) {
            legacy_ok &= run(&argv, false).0;
        }
        if legacy_ok && qs::legacy_render_is_native(de) {
            ok = true;
        }
    }
    if ok {
        qs::RenderingOutcome::Saved
    } else {
        qs::RenderingOutcome::NativeFailed
    }
}

/// As `settings_actions::legacy_render_available`.
fn legacy_render_available() -> bool {
    !tool("gsettings").is_empty() && qs::legacy_render_available(&run(&qs::legacy_render_probe_argv(), true).1)
}

/// As `settings_actions::apply_xresources_dpi` (the live X database is not touched:
/// the bench runs without DISPLAY).
fn apply_xresources_dpi(dpi: Option<i32>) -> bool {
    let path = format!("{}/{}", home().trim_end_matches('/'), qs::XRESOURCES_REL_PATH);
    let existing = std::fs::read_to_string(&path).ok();
    if existing.is_none() && dpi.is_none() {
        return true;
    }
    match qs::xresources_with_dpi(existing.as_deref(), dpi) {
        Some(text) => {
            eprintln!("  > {path}");
            write_file(&path, &text)
        }
        None => false,
    }
}

fn read_rendering(de: &str) -> qs::FontRendering {
    let native = match qs::rendering_read_argv(de, &tool("kreadconfig6")) {
        Some((dialect, reads)) => {
            let answers: Vec<(qs::RenderKey, String)> =
                reads.into_iter().map(|(k, argv)| (k, run(&argv, true).1)).collect();
            qs::parse_render_answers(dialect, &answers)
        }
        None => Default::default(),
    };
    let native = if qs::rendering_needs_legacy(&native, legacy_render_available()) {
        let (dialect, reads) = qs::legacy_rendering_read_argv();
        let answers: Vec<(qs::RenderKey, String)> =
            reads.into_iter().map(|(k, argv)| (k, run(&argv, true).1)).collect();
        qs::parse_render_answers(dialect, &answers)
    } else {
        native
    };
    let fc = std::fs::read_to_string(fontconfig_path())
        .map(|t| qs::fontconfig_rendering(&t))
        .unwrap_or_default();
    qs::merge_rendering(&native, &fc)
}

// ── «Settings» → sounds: as settings_actions::apply_sound_setting ─────────

fn apply_sound(de: &str, setting: qs::SoundSetting, value: &str) -> bool {
    let plan = qs::sound_plan(
        de,
        setting,
        value,
        &tool("kwriteconfig6"),
        &tool("kreadconfig6"),
        !tool("gsettings").is_empty(),
    );
    let Some(primary) = plan.write.first() else { return false };
    let primary_ok = run(primary, false).0;
    for argv in plan.write.iter().skip(1) {
        let _ = run(argv, false);
    }
    match plan.verify {
        Some(verify) => {
            let raw = run(&verify, true).1;
            match setting {
                qs::SoundSetting::Theme => qs::parse_sound_theme(&raw) == value,
                qs::SoundSetting::EventSounds => qs::parse_event_sounds(&raw) == (value == "true"),
            }
        }
        None => primary_ok,
    }
}

// ── «Settings» → terminal: as settings_actions::run_terminal_plan ────────

fn apply_terminal(desktop_id: &str) -> bool {
    run_steps(&defaults::set_terminal_plan(desktop_id, &env("XDG_CURRENT_DESKTOP"), &home()));
    true
}

fn run_steps(plan: &[defaults::TermStep]) {
    let mut env_written = false;
    for step in plan {
        match step {
            defaults::TermStep::WriteFile { path, content } => {
                eprintln!("  > {path}");
                if write_file(path, content) {
                    env_written |= rexon_core::compositor_autostart::is_environment_d_path(path);
                }
            }
            defaults::TermStep::MakeDirs { path } => {
                let _ = std::fs::create_dir_all(path);
            }
            defaults::TermStep::Run { argv } => {
                let _ = run(argv, false);
            }
            defaults::TermStep::SetIni { path, section, key, value } => {
                eprintln!("  > {path} {section} {key}={value}");
                let existing = std::fs::read_to_string(path).ok();
                let _ = write_file(path, &defaults::set_ini_value(existing.as_deref(), section, key, value));
            }
        }
    }
    if env_written {
        for (path, r) in rexon_core::compositor_autostart::sync_session_env_live() {
            eprintln!("  ~ {path}: {r:?}");
        }
    }
}

// ── «Settings» → file manager: as settings_actions::set_file_manager_default

fn apply_file_manager(desktop_id: &str) -> bool {
    let ok = run(&rexon_core::appdefaults::xdg_mime_default_cmd(desktop_id), false).0;
    let steps = rexon_core::appdefaults::file_manager_home_steps(
        desktop_id,
        &env("XDG_CURRENT_DESKTOP"),
        &config_home(),
        &defaults::xfce_system_helper_exists,
    );
    run_steps(&steps);
    ok
}

// ── «Appearance»: LiveInstaller, as for the button ─────────────────────────────

fn category_id(action: &str) -> Option<i64> {
    Some(match action {
        "gtk" => 135,
        "icons" => 132,
        "cursor" => 107,
        "kvantum" => 123,
        "widgetstyle" => 901,
        "colors" => 112,
        "sounds" => 316,
        "wallpaper" => 299,
        "lockwallpaper" => 904,
        _ => return None,
    })
}

fn installer(de: &str) -> appearance_installer::LiveInstaller {
    appearance_installer::LiveInstaller::new(de, Box::new(|line: &str| eprintln!("  [журнал] {line}")))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let action = args.first().map(String::as_str).unwrap_or("de");
    let value = args.get(1).cloned().unwrap_or_default();
    let de = current_de();
    eprintln!("окружение: {de:?} (XDG_CURRENT_DESKTOP={:?})", env("XDG_CURRENT_DESKTOP"));

    let (ok, message) = match action {
        "de" => (true, de.clone()),
        "categories" => {
            let home = std::path::PathBuf::from(home());
            let mut out = Vec::new();
            for cat in categories::categories(&home) {
                let shown = categories::category_available(&cat.key, &de);
                let cond = categories::category_condition(&cat.key, &de).unwrap_or_default();
                out.push(format!("{}={}{}", cat.key, if shown { "показан" } else { "скрыт" },
                    if cond.is_empty() { String::new() } else { format!(" [{cond}]") }));
            }
            (true, out.join("\n"))
        }
        "cursor-size" => installer(&de).apply_cursor_size(value.parse().unwrap_or(24)),
        "font-system" | "font-mono" | "font-small" | "font-toolbar" | "font-menu" | "font-title" => {
            let target = &action[5..];
            let (family, size) = qs::parse_font_spec(&value);
            (apply_font(&de, target, &family, if size > 0 { size } else { 10 }, qs::parse_font_face(&value)), String::new())
        }
        "font-render" => {
            let exclude = args.get(2).and_then(|r| r.split_once('-')).and_then(|(a, b)| {
                Some((a.parse().ok()?, b.parse().ok()?))
            });
            let r = qs::FontRendering {
                antialias: value == "on",
                exclude,
                subpixel: args.get(3).cloned().unwrap_or_default(),
                hinting: args.get(4).cloned().unwrap_or_default(),
            }
            .normalized();
            let outcome = apply_rendering(&de, &r);
            (outcome == qs::RenderingOutcome::Saved, qs::rendering_status(&outcome))
        }
        "font-render-state" => (true, format!("{:?}", read_rendering(&de))),
        "font-list" => {
            let fonts = font_library();
            let groups = qs::group_font_families(&fonts);
            let mine: Vec<String> = groups
                .iter()
                .filter(|g| g.has_user())
                .map(|g| format!("{} [{}]", g.family, g.styles.iter().map(|f| f.style.clone()).collect::<Vec<_>>().join(", ")))
                .collect();
            (true, format!("{} | свои: {}", qs::font_manager_summary(groups.len(), mine.len()), mine.join("; ")))
        }
        "font-install" => {
            let outcome = install_font(&value);
            (matches!(outcome, qs::FontInstallOutcome::Installed(_)), qs::font_install_status(&outcome))
        }
        "font-remove" => {
            // As settings_actions::remove_font, without the question window.
            let fonts = font_library();
            let dirs = qs::font_user_dirs(&home(), &data_home());
            let mut files: Vec<String> = fonts
                .iter()
                .filter(|f| f.family == value && f.user && qs::font_file_is_user(&f.file, &dirs))
                .map(|f| f.file.clone())
                .collect();
            files.sort();
            files.dedup();
            let mut removed = 0;
            let mut failed = 0;
            for f in &files {
                if std::fs::remove_file(f).is_ok() {
                    removed += 1;
                } else {
                    failed += 1;
                }
            }
            if removed > 0 {
                let _ = run(&qs::fc_cache_argv(&qs::font_install_dir(&data_home())), false);
            }
            (failed == 0 && removed > 0, qs::font_remove_status(&value, removed, failed))
        }
        "font-dpi" => {
            let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
            let dpi = value.parse::<i32>().ok().filter(|d| *d > 0);
            match qs::font_dpi_store(&de, wayland) {
                None => (false, qs::font_dpi_note_for(&de, None)),
                Some(qs::FontDpiStore::Xresources) => {
                    let ok = apply_xresources_dpi(dpi);
                    let back = std::fs::read_to_string(format!("{}/{}", home(), qs::XRESOURCES_REL_PATH))
                        .ok()
                        .and_then(|t| qs::xresources_dpi(&t));
                    (ok, format!("{} | прочитано обратно: {back:?}", qs::font_dpi_status(ok, dpi)))
                }
                Some(qs::FontDpiStore::Fontconfig) => {
                    let path = fontconfig_path();
                    let existing = std::fs::read_to_string(&path).ok();
                    let block = qs::fontconfig_block(&read_rendering(&de), dpi);
                    let ok = qs::fontconfig_with_block(existing.as_deref(), &block)
                        .map(|t| write_file(&path, &t))
                        .unwrap_or(false);
                    if ok && qs::font_dpi_xresources_also(&de, wayland) {
                        let _ = apply_xresources_dpi(dpi);
                    }
                    (ok, qs::font_dpi_status(ok, dpi))
                }
                Some(store) => {
                    let ok = qs::font_dpi_write_argv(&store, dpi, &tool("kwriteconfig6"))
                        .map(|a| run(&a, false).0)
                        .unwrap_or(false);
                    let back = qs::font_dpi_read_argv(&store, &tool("kreadconfig6"))
                        .map(|a| qs::parse_font_dpi(&store, &run(&a, true).1));
                    (ok, format!("{} | прочитано обратно: {back:?}", qs::font_dpi_status(ok, dpi)))
                }
            }
        }
        "sound-theme" => (apply_sound(&de, qs::SoundSetting::Theme, &value), String::new()),
        "event-sounds" => (apply_sound(&de, qs::SoundSetting::EventSounds, &value), String::new()),
        "terminal" => (apply_terminal(&value), String::new()),
        "dde-look-state" => (
            true,
            serde_json::to_string(&rexon_core::dde::read_state(&de, rexon_core::dde::treeland_session_live()))
                .unwrap_or_default(),
        ),
        "file-manager" => (apply_file_manager(&value), String::new()),
        "section" | "section-state" => {
            let home = std::path::PathBuf::from(home());
            let Some(cat) = categories::get_category(&home, &value) else {
                return println!("ИТОГ ok=false нет раздела {value}");
            };
            if action == "section-state" {
                let kind = categories::category_kind(&value).unwrap_or("");
                let names = rexon_core::de_apply::active_names(
                    &rexon_core::animations::SystemIo,
                    &rexon_core::de_apply::Ctx::live(),
                    kind,
                );
                (true, format!("включено: {names:?}"))
            } else {
                let name = args.get(2).cloned().unwrap_or_default();
                let item = rexon_core::models::ThemeItem { category_id: cat.id, name: name.clone(), ..Default::default() };
                installer(&de).apply(&item, &name)
            }
        }
        "conky-autostart" => rexon_core::de_apply::set_conky_autostart(
            &rexon_core::animations::SystemIo,
            &rexon_core::de_apply::Ctx::live(),
            &value,
            args.get(2).map(String::as_str) == Some("on"),
        ),
        "system-apply" => {
            let home = std::path::PathBuf::from(home());
            let cat = categories::get_category(&home, &value).expect("sddm|plymouth");
            let theme = args.get(2).cloned().unwrap_or_default();
            let helper = args.get(3).cloned().unwrap_or_default();
            let env = appearance_installer::RootEnv {
                is_plasmalogin: false,
                has_pkexec: true,
                system_helper_path: &helper,
                system_helper_exists: true,
                is_steamos_or_immutable: false,
                is_steamos: false,
                family_is_debian: false,
            };
            appearance_installer::system_theme_apply_with(
                &env,
                &cat,
                &theme,
                &[],
                &|p| {
                    eprintln!("  ? каталог темы {p}");
                    true
                },
                &|argv, timeout| {
                    eprintln!("  > {} (таймаут {timeout} с)", argv.join(" "));
                    utils::run(
                        &utils::RunSpec {
                            argv: argv.to_vec(),
                            output: utils::RunOutput::Capture,
                            timeout_secs: Some(timeout),
                            ..Default::default()
                        },
                        std::env::var("PATH").ok().as_deref(),
                    )
                },
            )
        }
        other => match category_id(other) {
            Some(id) => {
                let item = rexon_core::models::ThemeItem { category_id: id, name: value.clone(), ..Default::default() };
                installer(&de).apply(&item, &value)
            }
            None => (false, format!("незнакомое действие {other}")),
        },
    };
    println!("ИТОГ ok={ok} {message}");
}

