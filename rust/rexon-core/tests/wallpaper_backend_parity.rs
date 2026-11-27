//! Сверка выбора бэкенда обоев с тем, как его делает сам демон waywallen
//! (0.5.0: `src/plugin/display_registry.rs`, `builtin_display_defs`;
//! `src/wallframe/display/spawner.rs`, `detect_de` + `pick_backend`).
//!
//! Смысл сверки: если наше решение разойдётся с демоном, Rexon либо
//! начнёт ставить плагин там, где демон поднимает вывод сам (два пути
//! дерутся за экран), либо не поставит там, где демон ждёт внешнего
//! (обоев не будет вовсе).

use rexon_core::wallpaper_backend as wb;
use rexon_core::wallpaper_backend::WallpaperBackend as B;

/// Сеанс Wayland с заданным рабочим столом.
fn wl(desktop: &str) -> B {
    wb::pick(desktop, "wayland", "wayland-0", "")
}

#[test]
fn x11_session_always_goes_to_our_subprocess() {
    // Демон от X11 не зависит, но клиента вывода на X11 у него нет
    // (waywallen-layer-shell — Wayland). Plasma-X11 плагин бы потянул,
    // но этот шаг не принят (шапка модуля) — пока всё X11 рисуем сами.
    for de in ["XFCE", "MATE", "Budgie", "Pantheon", "X-Cinnamon", "KDE"] {
        assert_eq!(
            wb::pick(de, "x11", "", ""),
            B::X11Subprocess,
            "рабочий стол {de} на X11"
        );
    }
}

/// Pantheon на Wayland («Secure Session»): Gala — плагин Mutter, у неё нет
/// wlr-layer-shell (только свой протокол pantheon-desktop-shell) и нет
/// расширений оболочки, поэтому живых обоев там нет — честное Unsupported,
/// а не обещание. На X11 («Classic Session») — наш процесс, как везде.
#[test]
fn pantheon_wayland_is_honestly_unsupported() {
    assert_eq!(wl("Pantheon"), B::Unsupported);
    assert_eq!(wb::pick("Pantheon", "x11", "", ""), B::X11Subprocess);
}

#[test]
fn kde_is_a_hard_rule_and_never_falls_back() {
    // spawner.rs: «Hard rule: KDE sessions use their dedicated backend
    // and never fall back». Даже если в XDG_CURRENT_DESKTOP есть ещё
    // что-то, KDE перевешивает.
    assert_eq!(wl("KDE"), B::KdePlugin);
    // Демон узнаёт KDE только по токену `kde` (`DeCaps::is_kde`).
    assert_eq!(wl("plasma"), B::Unsupported);
    assert_eq!(wl("KDE:wayland"), B::KdePlugin);
    // KDE вперёд, даже когда рядом стоит wlr-композитор.
    assert_eq!(wl("KDE:sway"), B::KdePlugin);
}

#[test]
fn wlr_compositors_are_spawned_by_the_daemon_itself() {
    // Список — поле `de` определения layer-shell в builtin_display_defs.
    // river есть в коде демона, хотя в таблице README его нет; wayfire —
    // с 0.5.0.
    for de in ["Hyprland", "sway", "niri", "river", "COSMIC", "Wayfire"] {
        assert_eq!(wl(de), B::DaemonLayerShell, "композитор {de}");
    }
    // Rexon на этих сессиях не должен делать НИЧЕГО: демон сам найдёт
    // рядом с собой waywallen-layer-shell и запустит его.
    assert!(!B::DaemonLayerShell.needs_rexon_setup());
}

/// Wayfire `XDG_CURRENT_DESKTOP` сам не выставляет — демон узнаёт его по
/// `WAYFIRE_SOCKET`, но только когда переменная рабочего стола молчит
/// (пуста или одно `wayland`). Иначе прав рабочий стол.
#[test]
fn wayfire_is_recognised_by_its_socket_like_the_daemon_does() {
    let sock = "/run/user/1000/wayfire-wayland-1-.socket";
    let pick = |desktop: &str, socket: &str| wb::pick(desktop, "wayland", "wayland-1", socket);
    assert_eq!(pick("", sock), B::DaemonLayerShell);
    assert_eq!(pick("wayland", sock), B::DaemonLayerShell);
    assert_eq!(pick("Wayland", sock), B::DaemonLayerShell);
    // Без сокета — честное «не поддерживается», как у демона (headless).
    assert_eq!(pick("", ""), B::Unsupported);
    assert_eq!(pick("wayland", ""), B::Unsupported);
    // Рабочий стол назван — сокет ничего не меняет.
    assert_eq!(pick("KDE", sock), B::KdePlugin);
    assert_eq!(pick("GNOME", sock), B::ShellExtension);
    assert_eq!(pick("weston", sock), B::Unsupported);
    // X11 перевешивает всё.
    assert_eq!(wb::pick("", "x11", "", sock), B::X11Subprocess);

    assert_eq!(wb::session_desktops("", sock), "wayfire");
    assert_eq!(wb::session_desktops("wayland", sock), "wayland:wayfire");
    assert_eq!(wb::session_desktops("sway", sock), "sway");
    assert_eq!(wb::session_desktops("wayland", ""), "wayland");
}

/// Состав списка — ровно поле `de` определения layer-shell демона 0.5.0.
#[test]
fn daemon_layer_shell_list_matches_daemon_0_5_0() {
    assert_eq!(
        wb::DAEMON_LAYER_SHELL_DESKTOPS,
        &["hyprland", "sway", "niri", "river", "cosmic", "wayfire"]
    );
}

#[test]
fn only_kde_requires_setup_from_rexon() {
    // kde-plasma объявлен со spawn=External — демон ждёт, что плагин
    // поставит и выберет кто-то снаружи. Это единственный такой случай.
    assert!(B::KdePlugin.needs_rexon_setup());
    for b in [
        B::DaemonLayerShell,
        B::ShellExtension,
        B::X11Subprocess,
        B::Unsupported,
    ] {
        assert!(!b.needs_rexon_setup(), "{b:?} не требует настройки от нас");
    }
}

#[test]
fn mutter_shells_go_through_the_extension() {
    // Mutter и его форк Muffin не реализуют wlr-layer-shell, поэтому
    // кадры отдаём расширению оболочки через mmap.
    for de in ["GNOME", "ubuntu:GNOME", "Cinnamon", "Unity"] {
        assert_eq!(wl(de), B::ShellExtension, "оболочка {de}");
    }
}

#[test]
fn unknown_wayland_compositor_is_reported_honestly() {
    // Демон в этом случае уходит в headless (PickOutcome::None) —
    // врать пользователю, что обои будут, нельзя.
    assert_eq!(wl("weston"), B::Unsupported);
    assert_eq!(wl(""), B::Unsupported);
}

#[test]
fn wayland_detected_by_display_when_session_type_is_missing() {
    // Вложенные сеансы и запуск из TTY: XDG_SESSION_TYPE может быть
    // пуст, но WAYLAND_DISPLAY выставлен.
    assert!(wb::is_wayland_session("", "wayland-1"));
    assert!(wb::is_wayland_session("wayland", ""));
    assert!(!wb::is_wayland_session("", ""));
    assert!(!wb::is_wayland_session("x11", ""));
    assert_eq!(wb::pick("Hyprland", "", "wayland-1", ""), B::DaemonLayerShell);
}

#[test]
fn desktop_token_matching_is_case_insensitive_and_split_by_colon() {
    // XDG_CURRENT_DESKTOP бывает «pop:GNOME», «KDE», «Hyprland».
    assert_eq!(wl("pop:GNOME"), B::ShellExtension);
    assert_eq!(wl("HYPRLAND"), B::DaemonLayerShell);
    assert_eq!(wl(" niri "), B::DaemonLayerShell);
}

#[test]
fn waywallen_draws_on_kde_and_wlr_only() {
    assert!(B::KdePlugin.is_waywallen());
    assert!(B::DaemonLayerShell.is_waywallen());
    assert!(!B::ShellExtension.is_waywallen());
    assert!(!B::X11Subprocess.is_waywallen());
    assert!(!B::Unsupported.is_waywallen());
}

#[test]
fn every_backend_explains_itself_without_jargon() {
    // Правило #13: покупателю не показываем имена бинарей и протоколов.
    for b in [
        B::KdePlugin,
        B::DaemonLayerShell,
        B::ShellExtension,
        B::X11Subprocess,
        B::Unsupported,
    ] {
        let note = b.human_note();
        assert!(!note.is_empty(), "{b:?} без объяснения");
        for jargon in [
            "layer-shell",
            "layer_shell",
            "dmabuf",
            "dma-buf",
            "wlr",
            "kpackage",
            "mmap",
            "containment",
            "waywallen",
        ] {
            assert!(
                !note.to_lowercase().contains(jargon),
                "{b:?}: жаргон «{jargon}» в тексте для пользователя"
            );
        }
        assert!(!backend_id_is_empty(b));
    }
}

fn backend_id_is_empty(b: B) -> bool {
    wb::backend_id(b).is_empty()
}

// ── дисплей для демона, когда его нет (дефект 30.10.2026, MATE на X11) ──

/// Дисплей поднимает Rexon ровно там, где демон сам не может: KDE (плагин)
/// и X11 (хост-клиент вывода). Каждый бэкенд назван — новый вариант без
/// решения здесь не соберётся (правило #12).
#[test]
fn display_setup_per_backend() {
    use wb::DisplaySetup as D;
    for b in [B::KdePlugin, B::DaemonLayerShell, B::ShellExtension, B::X11Subprocess, B::Unsupported] {
        let want = match b {
            B::KdePlugin => D::KdePlugin,
            B::X11Subprocess => D::RexonHost,
            B::DaemonLayerShell | B::ShellExtension | B::Unsupported => D::None,
        };
        assert_eq!(b.display_setup(), want, "{b:?}");
        assert_eq!(b.display_setup().rexon_brings_display(), want != D::None, "{b:?}");
    }
    // Все X11-столы, которые перечисляет правило #12: MATE, Cinnamon X11, XFCE.
    for de in ["MATE", "X-Cinnamon", "XFCE", "Budgie:GNOME", "LXQt"] {
        assert_eq!(wb::pick(de, "x11", "", "").display_setup(), D::RexonHost, "{de}");
    }
}

/// Определение вывода читается демоном как его `DisplayManifest`: секция
/// `[display]`, `spawn = "daemon"`, наш хост в режиме клиента вывода, столы
/// без своего вывода waywallen — и ни одного стола, где вывод у демона свой.
#[test]
fn display_manifest_is_a_daemon_manifest() {
    let text = wb::rexon_display_manifest("/usr/bin/rexon-wallpaper-host");
    let v: toml::Value = text.parse().expect("TOML обязан разбираться");
    let d = v.get("display").expect("секция [display]");
    assert_eq!(d["name"].as_str(), Some(wb::REXON_DISPLAY_NAME));
    assert_eq!(d["bin"].as_str(), Some("/usr/bin/rexon-wallpaper-host"));
    assert_eq!(d["spawn"].as_str(), Some("daemon"));
    let args: Vec<&str> = d["extra_args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
    assert_eq!(args, ["--mode", "waywallen"]);
    let de: Vec<&str> = d["de"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
    for want in ["mate", "xfce", "x-cinnamon"] {
        assert!(de.contains(&want), "{want} в списке столов");
    }
    for own in ["kde", "gnome"].iter().chain(wb::DAEMON_LAYER_SHELL_DESKTOPS) {
        assert!(!de.contains(own), "{own}: у демона свой вывод");
    }
    // Выше встроенного `x11` (50) демона с waywallen#354: иначе на MATE,
    // Cinnamon и Xfce он выберет свой вывод вместо хоста Rexon.
    assert!(d["priority"].as_integer().unwrap() > 50, "выше встроенного x11");
    // Путь с кавычкой не ломает TOML.
    let odd = wb::rexon_display_manifest("/opt/a\"b/host");
    let v: toml::Value = odd.parse().unwrap();
    assert_eq!(v["display"]["bin"].as_str(), Some("/opt/a\"b/host"));
}

#[test]
fn display_manifest_paths_and_refresh() {
    assert_eq!(
        wb::rexon_display_manifest_path("/home/u/.local/share/"),
        "/home/u/.local/share/waywallen/displays/rexon-x11.toml"
    );
    assert_eq!(wb::rexon_host_lock_path("/run/user/1000"), "/run/user/1000/rexon-waywallen-display.lock");
    let host = "/usr/bin/rexon-wallpaper-host";
    assert!(wb::rexon_display_manifest_stale(None, host), "нет файла — писать");
    let cur = wb::rexon_display_manifest(host);
    assert!(!wb::rexon_display_manifest_stale(Some(&cur), host), "тот же — не трогать");
    assert!(wb::rexon_display_manifest_stale(Some(&cur), "/opt/rexon/host"), "хост переехал — переписать");
}

/// На Wayland и без `DISPLAY` хост-клиент вывода только ждёт.
#[test]
fn display_host_idles_off_x11() {
    assert!(!wb::rexon_host_should_idle("x11", "", ":0"));
    assert!(wb::rexon_host_should_idle("wayland", "wayland-0", ":0"), "Cinnamon/XFCE на Wayland");
    assert!(wb::rexon_host_should_idle("", "wayland-1", ""));
    assert!(wb::rexon_host_should_idle("x11", "", ""), "нет DISPLAY");
}

/// Rexon запускает хост той же строкой, что демон по определению вывода.
#[test]
fn display_host_argv_matches_daemon_spawn() {
    assert_eq!(
        wb::rexon_display_host_argv("/usr/bin/rexon-wallpaper-host", "/run/user/1000/waywallen/display.sock"),
        ["/usr/bin/rexon-wallpaper-host", "--socket", "/run/user/1000/waywallen/display.sock", "--mode", "waywallen"]
    );
    assert_eq!(wb::rexon_display_host_argv("/h", ""), ["/h", "--mode", "waywallen"]);
    assert_eq!(
        wb::rexon_display_host_sibling("/r/target-shared/release/rexon-main-window").as_deref(),
        Some("/r/target-shared/release/rexon-wallpaper-host")
    );
}

// ── значки рабочего стола под живыми обоями (X11) ─────────────────────────

use std::collections::HashMap;
use wb::DesktopIconsOwner as Owner;

/// Подставной стол: значения gsettings/xfconf, журнал записей и состояние.
#[derive(Default)]
struct FakeDesk {
    values: HashMap<String, String>,
    caja_keys: Vec<&'static str>,
    /// Умолчания схем для `gsettings reset` (нет — ключ после сброса пуст).
    defaults: HashMap<String, String>,
    /// Ключи схемы org.nemo.desktop; пусто — Nemo не установлен.
    nemo_keys: Vec<&'static str>,
    state: Option<String>,
    sets: Vec<String>,
    pauses: Vec<u64>,
    /// gsettings set отказывает.
    set_fails: bool,
}

impl FakeDesk {
    fn mate(icons: bool, has_key: bool) -> Self {
        let mut d = FakeDesk::default();
        d.put("org.mate.background show-desktop-icons", if icons { "true" } else { "false" });
        d.put("org.mate.session.required-components windowmanager", "'marco'");
        d.put("org.mate.Marco.general compositing-manager", "true");
        d.caja_keys = vec!["font", "home-icon-visible"];
        if has_key {
            d.caja_keys.push("transparent-background");
            d.put("org.mate.caja.desktop transparent-background", "false");
        }
        d
    }
    fn put(&mut self, k: &str, v: &str) {
        self.values.insert(k.to_string(), v.to_string());
    }
    fn get(&self, k: &str) -> &str {
        self.values.get(k).map(String::as_str).unwrap_or("")
    }
}

impl wb::DesktopIconsIo for FakeDesk {
    fn run(&mut self, argv: &[&str]) -> Option<String> {
        match argv {
            ["gsettings", "get", schema, key] => self.values.get(&format!("{schema} {key}")).map(|v| format!("{v}\n")),
            ["gsettings", "list-keys", "org.mate.caja.desktop"] => Some(self.caja_keys.join("\n")),
            ["gsettings", "list-keys", "org.nemo.desktop"] => {
                (!self.nemo_keys.is_empty()).then(|| self.nemo_keys.join("\n"))
            }
            ["gsettings", "set", schema, key, value] => {
                self.sets.push(format!("{schema} {key}={value}"));
                if self.set_fails {
                    return None;
                }
                self.put(&format!("{schema} {key}"), value);
                Some(String::new())
            }
            ["gsettings", "reset", schema, key] => {
                let k = format!("{schema} {key}");
                self.sets.push(format!("{k} reset"));
                match self.defaults.get(&k).cloned() {
                    Some(v) => self.values.insert(k, v),
                    None => self.values.remove(&k),
                };
                Some(String::new())
            }
            ["xfconf-query", "-c", "xfce4-desktop", "-p", "/desktop-icons/style"] => {
                self.values.get("xfce style").cloned()
            }
            other => panic!("неожиданная команда {other:?}"),
        }
    }
    fn read_state(&mut self) -> Option<String> {
        self.state.clone()
    }
    fn write_state(&mut self, content: Option<&str>) -> bool {
        self.state = content.map(str::to_string);
        true
    }
    fn pause_ms(&mut self, ms: u64) {
        self.pauses.push(ms);
    }
}

#[test]
fn desktop_icons_owner_only_on_x11_desktops_with_icon_windows() {
    assert_eq!(wb::desktop_icons_owner("mate", "x11"), Some(Owner::Caja));
    assert_eq!(wb::desktop_icons_owner("cinnamon", "x11"), Some(Owner::Nemo));
    assert_eq!(wb::desktop_icons_owner("xfce", "x11"), Some(Owner::Xfdesktop));
    assert_eq!(wb::desktop_icons_owner("mate", "wayland"), None);
    assert_eq!(wb::desktop_icons_owner("kde", "x11"), None);
    assert_eq!(
        wb::desktop_icons_state_path("", "/home/w"),
        "/home/w/.local/state/rexon/desktop-icons.state"
    );
    assert_eq!(wb::desktop_icons_state_path("/s/", "/home/w"), "/s/rexon/desktop-icons.state");
}

/// Caja без ключа (MATE 1.28 из коробки): честное предупреждение, значки
/// НЕ выключаются, ничего не пишется.
#[test]
fn caja_without_key_warns_and_touches_nothing() {
    let mut d = FakeDesk::mate(true, false);
    let notice = wb::prepare_desktop_icons(&mut d, Owner::Caja);
    assert_eq!(notice, Some(wb::desktop_icons_hidden_text(Owner::Caja)));
    assert!(d.sets.is_empty(), "значки и настройки не трогаются: {:?}", d.sets);
    assert_eq!(d.state, None);
    assert!(!wb::restore_desktop_icons(&mut d), "откатывать нечего");
}

/// Значки выключены человеком — молчим и ничего не делаем (ключ есть или нет).
#[test]
fn icons_off_means_nothing_to_say_or_do() {
    for has_key in [false, true] {
        let mut d = FakeDesk::mate(false, has_key);
        assert_eq!(wb::prepare_desktop_icons(&mut d, Owner::Caja), None);
        assert!(d.sets.is_empty());
    }
}

/// Caja с ключом: ключ включается, окно стола пересоздаётся переключением
/// значков (а не `caja -q`), прежнее записано; повтор ничего не трогает;
/// откат возвращает ключ и пересоздаёт окно.
#[test]
fn caja_with_key_turns_on_see_through_and_restores() {
    let mut d = FakeDesk::mate(true, true);
    let notice = wb::prepare_desktop_icons(&mut d, Owner::Caja);
    assert_eq!(notice, Some(wb::desktop_icons_kept_text(false)));
    assert_eq!(
        d.sets,
        [
            "org.mate.caja.desktop transparent-background=true",
            "org.mate.background show-desktop-icons=false",
            "org.mate.background show-desktop-icons=true",
        ]
    );
    assert_eq!(d.pauses, [wb::CAJA_DESKTOP_RECREATE_PAUSE_MS]);
    assert_eq!(d.get("org.mate.background show-desktop-icons"), "true", "значки остались включены");
    let saved = wb::DesktopIconsSaved::parse(d.state.as_deref().unwrap()).unwrap();
    assert!(saved.caja_transparent_was_off && !saved.marco_compositing_was_off);

    // Второе применение: всё уже на месте — молча.
    d.sets.clear();
    assert_eq!(wb::prepare_desktop_icons(&mut d, Owner::Caja), None);
    assert!(d.sets.is_empty());

    // «Остановить живые обои» — как было.
    assert!(wb::restore_desktop_icons(&mut d));
    assert_eq!(
        d.sets,
        [
            "org.mate.caja.desktop transparent-background=false",
            "org.mate.background show-desktop-icons=false",
            "org.mate.background show-desktop-icons=true",
        ]
    );
    assert_eq!(d.state, None);
    assert!(!wb::restore_desktop_icons(&mut d), "второй откат — пустой");
}

/// Композитор Marco выключен — включается и возвращается; прежнее
/// записывается только первым применением.
#[test]
fn marco_compositing_turned_on_and_back() {
    let mut d = FakeDesk::mate(true, true);
    d.put("org.mate.Marco.general compositing-manager", "false");
    let notice = wb::prepare_desktop_icons(&mut d, Owner::Caja).unwrap();
    assert_eq!(notice, wb::desktop_icons_kept_text(true));
    assert_eq!(d.sets[0], "org.mate.Marco.general compositing-manager=true");
    let first_state = d.state.clone();
    // Человек сам выключил композитор, применяет снова — прежнее не затирается.
    d.put("org.mate.Marco.general compositing-manager", "false");
    wb::prepare_desktop_icons(&mut d, Owner::Caja);
    assert_eq!(d.state, first_state);
    d.sets.clear();
    assert!(wb::restore_desktop_icons(&mut d));
    assert!(d.sets.contains(&"org.mate.Marco.general compositing-manager=false".to_string()), "{:?}", d.sets);
    assert!(d.sets.contains(&"org.mate.caja.desktop transparent-background=false".to_string()));
}

/// Не Marco (например, Compiz) — композитор не трогаем.
#[test]
fn other_window_manager_compositor_is_left_alone() {
    let mut d = FakeDesk::mate(true, true);
    d.put("org.mate.session.required-components windowmanager", "'compiz'");
    d.put("org.mate.Marco.general compositing-manager", "false");
    wb::prepare_desktop_icons(&mut d, Owner::Caja);
    assert!(!d.sets.iter().any(|s| s.contains("Marco")), "{:?}", d.sets);
}

/// gsettings не пишет — честное «значки скрыты», а не «остаются».
#[test]
fn failed_switch_falls_back_to_warning() {
    let mut d = FakeDesk::mate(true, true);
    d.set_fails = true;
    assert_eq!(
        wb::prepare_desktop_icons(&mut d, Owner::Caja),
        Some(wb::desktop_icons_hidden_text(Owner::Caja))
    );
}

/// Откат, когда ключ уже выключил сам человек, — окно стола не дёргается.
#[test]
fn restore_skips_key_already_off() {
    let mut d = FakeDesk::mate(true, true);
    wb::prepare_desktop_icons(&mut d, Owner::Caja);
    d.put("org.mate.caja.desktop transparent-background", "false");
    d.sets.clear();
    assert!(wb::restore_desktop_icons(&mut d));
    assert!(d.sets.is_empty(), "{:?}", d.sets);
}

/// xfdesktop: значки есть (стиль не 0 или не задан) — предупреждение;
/// стиль 0 — молчим. Nemo прозрачен всегда.
#[test]
fn xfdesktop_and_nemo() {
    let mut d = FakeDesk::default();
    assert_eq!(
        wb::prepare_desktop_icons(&mut d, Owner::Xfdesktop),
        Some(wb::desktop_icons_hidden_text(Owner::Xfdesktop)),
        "не задано — умолчание xfdesktop со значками"
    );
    d.put("xfce style", "2\n");
    assert!(wb::prepare_desktop_icons(&mut d, Owner::Xfdesktop).is_some());
    d.put("xfce style", "0\n");
    assert_eq!(wb::prepare_desktop_icons(&mut d, Owner::Xfdesktop), None);
    assert_eq!(wb::prepare_desktop_icons(&mut d, Owner::Nemo), None);
    assert!(d.sets.is_empty());
}

#[test]
fn saved_state_round_trips_and_rejects_foreign_text() {
    let saved = wb::DesktopIconsSaved {
        caja_recorded: true,
        caja_transparent_was_off: true,
        marco_compositing_was_off: false,
        nemo_ignored_before: None,
    };
    assert_eq!(wb::DesktopIconsSaved::parse(&saved.to_text()), Some(saved));
    let nemo = wb::DesktopIconsSaved {
        nemo_ignored_before: Some("['conky', 'csd-background']".into()),
        ..Default::default()
    };
    assert_eq!(wb::DesktopIconsSaved::parse(&nemo.to_text()), Some(nemo));
    // Файл прежней версии (только Caja) читается как был.
    let old = wb::DesktopIconsSaved::parse("owner=caja\ncaja-transparent-was-off=true\nmarco-compositing-was-off=false\n")
        .unwrap();
    assert!(old.caja_recorded && old.caja_transparent_was_off && old.nemo_ignored_before.is_none());
    assert_eq!(wb::DesktopIconsSaved::parse("мусор"), None);
    assert_eq!(wb::DesktopIconsSaved::parse("nemo-ignored-desktop-handlers-before=не массив"), None);
    let mut d = FakeDesk::mate(true, true);
    d.state = Some("мусор".into());
    assert!(!wb::restore_desktop_icons(&mut d));
    assert_eq!(d.state, None, "битый файл убран");
}

// ── nemo-desktop: окно обоев не «чужой стол» ─────────────────────────────

const NEMO_KEY: &str = "org.nemo.desktop ignored-desktop-handlers";

fn with_nemo(mut d: FakeDesk, value: &str) -> FakeDesk {
    d.nemo_keys = vec!["font", "ignored-desktop-handlers", "show-desktop-icons"];
    d.put(NEMO_KEY, value);
    d.defaults.insert(NEMO_KEY.into(), "['conky', 'csd-background']".into());
    d
}

/// Дефект 31.10.2026 (стенд Nemo): nemo-desktop, запущенный после хоста
/// обоев, выходил с «Desktop already managed». Rexon дописывает `rexon` к
/// чужим значениям, повтор ничего не трогает, «Остановить» возвращает
/// прежнее значение дословно.
#[test]
fn nemo_ignored_handlers_get_rexon_and_come_back() {
    let mut d = with_nemo(FakeDesk::default(), "['conky', 'csd-background']");
    assert_eq!(wb::prepare_desktop_icons(&mut d, Owner::Nemo), None, "Nemo прозрачен — сказать нечего");
    assert_eq!(d.sets, [format!("{NEMO_KEY}=['conky', 'csd-background', 'rexon']")]);
    let saved = wb::DesktopIconsSaved::parse(d.state.as_deref().unwrap()).unwrap();
    assert_eq!(saved.nemo_ignored_before.as_deref(), Some("['conky', 'csd-background']"));
    assert!(!saved.caja_recorded);

    d.sets.clear();
    wb::prepare_desktop_icons(&mut d, Owner::Nemo);
    assert!(d.sets.is_empty(), "уже дописано: {:?}", d.sets);

    // Прежнее совпадает с умолчанием схемы — сброс, без явной записи.
    assert!(wb::restore_desktop_icons(&mut d));
    assert_eq!(d.sets, [format!("{NEMO_KEY} reset")]);
    assert_eq!(d.get(NEMO_KEY), "['conky', 'csd-background']");
    assert_eq!(d.state, None);
    assert!(!wb::restore_desktop_icons(&mut d));
}

/// Человек правил список, пока шли обои, — откат убирает только наше.
#[test]
fn nemo_restore_keeps_foreign_edits() {
    let mut d = with_nemo(FakeDesk::default(), "@as []");
    wb::prepare_desktop_icons(&mut d, Owner::Nemo);
    assert_eq!(d.get(NEMO_KEY), "['rexon']");
    d.put(NEMO_KEY, "['rexon', 'my-dock']");
    d.sets.clear();
    assert!(wb::restore_desktop_icons(&mut d));
    assert_eq!(d.sets, [format!("{NEMO_KEY}=['my-dock']")]);

    // Пустой прежний список (не умолчание) — сброс не совпал, пишется явно.
    let mut d = with_nemo(FakeDesk::default(), "@as []");
    wb::prepare_desktop_icons(&mut d, Owner::Nemo);
    d.sets.clear();
    wb::restore_desktop_icons(&mut d);
    assert_eq!(d.sets, [format!("{NEMO_KEY} reset"), format!("{NEMO_KEY}=@as []")]);
    assert_eq!(d.get(NEMO_KEY), "@as []");
}

/// `rexon` уже в списке (вписал сам человек, любой регистр) — не трогаем
/// и не откатываем; нет схемы Nemo — не трогаем вовсе.
#[test]
fn nemo_left_alone_when_already_ignored_or_absent() {
    let mut d = with_nemo(FakeDesk::default(), "['conky', 'Rexon']");
    wb::prepare_desktop_icons(&mut d, Owner::Nemo);
    assert!(d.sets.is_empty());
    assert_eq!(d.state, None);

    let mut d = FakeDesk::default();
    d.put(NEMO_KEY, "['conky']");
    wb::prepare_desktop_icons(&mut d, Owner::Nemo);
    assert!(d.sets.is_empty(), "схемы нет — {:?}", d.sets);

    // Значение не разобрать — не пишем ничего (чужое не затираем).
    let mut d = with_nemo(FakeDesk::default(), "<странное>");
    wb::prepare_desktop_icons(&mut d, Owner::Nemo);
    assert!(d.sets.is_empty());
    assert_eq!(d.state, None);
}

/// nemo-desktop вне Cinnamon (стенд: на MATE с Caja): обе части пишутся в
/// один файл, ни одна не затирает другую; откат возвращает обе.
#[test]
fn nemo_and_caja_share_one_state_file() {
    let mut d = with_nemo(FakeDesk::mate(true, true), "['conky', 'csd-background']");
    assert_eq!(wb::prepare_desktop_icons(&mut d, Owner::Caja), Some(wb::desktop_icons_kept_text(false)));
    let saved = wb::DesktopIconsSaved::parse(d.state.as_deref().unwrap()).unwrap();
    assert!(saved.caja_recorded && saved.caja_transparent_was_off, "Caja записана после Nemo: {saved:?}");
    assert_eq!(saved.nemo_ignored_before.as_deref(), Some("['conky', 'csd-background']"));
    d.sets.clear();
    assert!(wb::restore_desktop_icons(&mut d));
    assert!(d.sets.contains(&"org.mate.caja.desktop transparent-background=false".to_string()), "{:?}", d.sets);
    assert!(d.sets.contains(&format!("{NEMO_KEY} reset")), "{:?}", d.sets);
    assert_eq!(d.get(NEMO_KEY), "['conky', 'csd-background']");
    assert_eq!(d.state, None);
}

#[test]
fn gvariant_string_arrays_round_trip() {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(wb::parse_gvariant_strv("['conky', 'csd-background']"), Some(v(&["conky", "csd-background"])));
    assert_eq!(wb::parse_gvariant_strv("@as []"), Some(vec![]));
    assert_eq!(wb::parse_gvariant_strv("[]\n"), Some(vec![]));
    assert_eq!(wb::parse_gvariant_strv(r#"["it's", 'a\'b']"#), Some(v(&["it's", "a'b"])));
    assert_eq!(wb::parse_gvariant_strv("conky"), None);
    assert_eq!(wb::parse_gvariant_strv("['open"), None);
    for items in [v(&[]), v(&["a"]), v(&["it's", "back\\slash", "rexon"])] {
        assert_eq!(wb::parse_gvariant_strv(&wb::format_gvariant_strv(&items)), Some(items.clone()));
    }
    assert_eq!(wb::format_gvariant_strv(&v(&["conky", "rexon"])), "['conky', 'rexon']");
}

/// Какие окна значков пересоздаются переключением значков (по `WM_CLASS`,
/// как на стенде xfwm4 31.10.2026).
#[test]
fn icons_window_toggle_by_wm_class() {
    assert_eq!(
        wb::icons_window_toggle("nemo-desktop", "Nemo-desktop"),
        Some(("org.nemo.desktop", "show-desktop-icons"))
    );
    assert_eq!(
        wb::icons_window_toggle("desktop_window", "Caja"),
        Some(("org.mate.background", "show-desktop-icons"))
    );
    assert_eq!(wb::icons_window_toggle("caja", "Caja"), None, "обычное окно Caja");
    assert_eq!(wb::icons_window_toggle("xfdesktop", "Xfdesktop"), None, "фон непрозрачный — не поможет");
}

/// Пересоздание окна значков: «выкл → пауза → вкл»; ключ выключен — ничего;
/// «вкл» не прошло — ещё попытка, значки не остаются выключенными.
#[test]
fn recreate_icons_window_toggles_and_restores() {
    let mut d = FakeDesk::default();
    d.put("org.nemo.desktop show-desktop-icons", "true");
    assert!(wb::recreate_icons_window(&mut d, "org.nemo.desktop", "show-desktop-icons"));
    assert_eq!(d.sets, ["org.nemo.desktop show-desktop-icons=false", "org.nemo.desktop show-desktop-icons=true"]);
    assert_eq!(d.pauses, [wb::CAJA_DESKTOP_RECREATE_PAUSE_MS]);
    assert_eq!(d.get("org.nemo.desktop show-desktop-icons"), "true");

    let mut off = FakeDesk::default();
    off.put("org.nemo.desktop show-desktop-icons", "false");
    assert!(!wb::recreate_icons_window(&mut off, "org.nemo.desktop", "show-desktop-icons"));
    assert!(off.sets.is_empty(), "значки выключены человеком — не трогаем");

    let mut none = FakeDesk::default();
    assert!(!wb::recreate_icons_window(&mut none, "org.nemo.desktop", "show-desktop-icons"), "нет схемы");
    assert!(none.sets.is_empty());

    let mut broken = FakeDesk::default();
    broken.put("org.mate.background show-desktop-icons", "true");
    broken.set_fails = true;
    assert!(!wb::recreate_icons_window(&mut broken, "org.mate.background", "show-desktop-icons"));
    assert_eq!(broken.sets, ["org.mate.background show-desktop-icons=false"], "«выкл» не прошло — дальше не идём");
}

// ── GNOME: расширение вывода waywallen (дефект 09.11.2026, чёрный стол) ──

/// Подставной сеанс: окружение, каталоги и ответы команд по первому слову.
struct GnomeFake {
    env: std::collections::BTreeMap<&'static str, &'static str>,
    dirs: Vec<String>,
    /// Ответ `GetExtensionInfo`; `None` — оболочка не ответила.
    info: Option<String>,
    /// Ответ оболочки на Enable/DisableExtension; `None` — шина молчит.
    toggle: Option<&'static str>,
    /// Удался ли запасной сценарий `sh -c`.
    script_ok: bool,
    ran: std::cell::RefCell<Vec<Vec<String>>>,
}

impl GnomeFake {
    fn new(desktop: &'static str, state: Option<f64>) -> GnomeFake {
        GnomeFake {
            env: [("XDG_CURRENT_DESKTOP", desktop), ("HOME", "/home/u")].into_iter().collect(),
            dirs: vec![format!("/home/u/.local/share/gnome-shell/extensions/{}", wb::WAYWALLEN_GNOME_EXT_ID)],
            info: state.map(|st| {
                format!("({{'uuid': <'{}'>, 'state': <{st:.1}>}},)", wb::WAYWALLEN_GNOME_EXT_ID)
            }),
            toggle: Some("(true,)"),
            script_ok: true,
            ran: Default::default(),
        }
    }
    fn scripts(&self) -> Vec<String> {
        self.ran.borrow().iter().filter(|a| a[0] == "sh").map(|a| a[2].clone()).collect()
    }
    /// Методы оболочки, которые звали, кроме вопроса о состоянии.
    fn toggles(&self) -> Vec<String> {
        self.ran
            .borrow()
            .iter()
            .filter(|a| a[0] == "gdbus" && !a[8].ends_with("GetExtensionInfo"))
            .map(|a| a[8].rsplit('.').next().unwrap().to_string())
            .collect()
    }
}

impl wb::GnomeExtIo for GnomeFake {
    fn env(&self, name: &str) -> String {
        self.env.get(name).copied().unwrap_or_default().to_string()
    }
    fn is_dir(&self, path: &str) -> bool {
        self.dirs.iter().any(|d| d == path)
    }
    fn run(&self, argv: &[String]) -> Option<String> {
        self.ran.borrow_mut().push(argv.to_vec());
        match argv[0].as_str() {
            "gdbus" => {
                assert_eq!(argv.last().map(String::as_str), Some(wb::WAYWALLEN_GNOME_EXT_ID));
                if argv[8].ends_with("GetExtensionInfo") {
                    self.info.clone()
                } else {
                    self.toggle.map(str::to_string)
                }
            }
            "sh" => self.script_ok.then(String::new),
            other => panic!("неожиданная команда {other}"),
        }
    }
}

#[test]
fn waywallen_gnome_display_switches_only_what_is_needed() {
    use wb::GnomeDisplaySwitch as R;
    // Включено → выключает сама оболочка; своей правки списка нет.
    let io = GnomeFake::new("GNOME", Some(1.0));
    assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Switched);
    assert_eq!(io.toggles(), vec!["DisableExtension"]);
    assert!(io.scripts().is_empty(), "{:?}", io.scripts());
    // Включено и просят включить — ничего не зовём.
    let io = GnomeFake::new("GNOME", Some(1.0));
    assert_eq!(wb::set_waywallen_gnome_display(&io, true), R::Already);
    assert!(io.toggles().is_empty() && io.scripts().is_empty());
    // Выключено → включаем.
    let io = GnomeFake::new("ubuntu:GNOME", Some(2.0));
    assert_eq!(wb::set_waywallen_gnome_display(&io, true), R::Switched);
    assert_eq!(io.toggles(), vec!["EnableExtension"]);
    // Выключено и просят выключить; загружено, но не включалось (6);
    // ошибка; не та версия.
    for st in [2.0, 6.0, 3.0, 4.0] {
        let io = GnomeFake::new("GNOME", Some(st));
        assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Already, "{st}");
        assert!(io.toggles().is_empty() && io.scripts().is_empty(), "{st}");
    }
    // Ошибка в расширении: включаем, но дисплея не будет — придержка не ждёт.
    let io = GnomeFake::new("GNOME", Some(3.0));
    assert_eq!(wb::set_waywallen_gnome_display(&io, true), R::Failed);
    // Шина не ответила на переключение — запасной путь: список настроек
    // без чужих uuid.
    let mut io = GnomeFake::new("GNOME", Some(1.0));
    io.toggle = None;
    assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Switched);
    let s = io.scripts();
    assert_eq!(s.len(), 1, "{s:?}");
    assert!(s[0].contains("grep -vF 'org.waywallen.gnome@waywallen.io'"), "{}", s[0]);
    assert!(s[0].contains("gsettings set org.gnome.shell enabled-extensions"), "{}", s[0]);
    let mut io = GnomeFake::new("GNOME", Some(2.0));
    io.toggle = Some("(false,)");
    assert_eq!(wb::set_waywallen_gnome_display(&io, true), R::Switched);
    assert!(io.scripts()[0].contains("'org.waywallen.gnome@waywallen.io'"), "{:?}", io.scripts());
    // И запасной путь не удался.
    let mut io = GnomeFake::new("GNOME", Some(1.0));
    io.toggle = None;
    io.script_ok = false;
    assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Failed);
}

#[test]
fn waywallen_gnome_display_skips_foreign_sessions() {
    use wb::GnomeDisplaySwitch as R;
    // Не GNOME Shell — даже не спрашиваем.
    for de in ["KDE", "X-Cinnamon", "Budgie:GNOME", "Unity", "Pantheon:GNOME", "Hyprland", ""] {
        let io = GnomeFake::new(de, Some(1.0));
        assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Skipped, "{de}");
        assert!(io.ran.borrow().is_empty(), "{de}: {:?}", io.ran.borrow());
    }
    // Расширения нет — тоже.
    let mut io = GnomeFake::new("GNOME", Some(1.0));
    io.dirs.clear();
    assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Skipped);
    assert!(io.ran.borrow().is_empty());
    // Оболочка не ответила (нет gnome-shell на шине).
    let io = GnomeFake::new("GNOME", None);
    assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Skipped);
    assert!(io.scripts().is_empty());
    // Системный каталог из XDG_DATA_DIRS находится так же.
    let mut io = GnomeFake::new("GNOME", Some(1.0));
    io.dirs = vec![format!("/opt/share/gnome-shell/extensions/{}", wb::WAYWALLEN_GNOME_EXT_ID)];
    io.env.insert("XDG_DATA_DIRS", "/opt/share/:/usr/share");
    assert_eq!(wb::set_waywallen_gnome_display(&io, false), R::Switched);
}

#[test]
fn gnome_display_setup_needs_installed_waywallen_extension() {
    use wb::DisplaySetup as D;
    let gnome = wb::pick("GNOME", "wayland", "wayland-0", "");
    assert_eq!(wb::display_setup_for(gnome, "GNOME", true), D::GnomeExtension);
    assert!(D::GnomeExtension.rexon_brings_display());
    assert_eq!(wb::display_setup_for(gnome, "GNOME", false), D::None);
    // Cinnamon на Wayland — тот же бэкенд, но расширение waywallen не его.
    let cin = wb::pick("X-Cinnamon", "wayland", "wayland-0", "");
    assert_eq!(wb::display_setup_for(cin, "X-Cinnamon", true), D::None);
    // Остальные бэкенды — как без расширения.
    for de in ["KDE", "Hyprland"] {
        let b = wb::pick(de, "wayland", "wayland-0", "");
        assert_eq!(wb::display_setup_for(b, de, true), b.display_setup(), "{de}");
    }
    assert_eq!(wb::display_setup_for(wb::pick("GNOME", "x11", "", ""), "GNOME", true), D::RexonHost);
}

// ── Cinnamon с layer-shell (Muffin 6.8+) ────────────────────────────────

/// Признак — протокол композитора; без ответа композитора — служба фона.
/// Номер версии не смотрим; вне Cinnamon на Wayland — всегда нет.
#[test]
fn cinnamon_layer_shell_by_protocol_then_service() {
    let ls = vec!["wl_compositor".to_string(), wb::LAYER_SHELL_GLOBAL.to_string()];
    let plain = vec!["wl_compositor".to_string()];
    assert!(wb::cinnamon_layer_shell("X-Cinnamon", "wayland", "wayland-0", Some(&ls), false));
    assert!(!wb::cinnamon_layer_shell("X-Cinnamon", "wayland", "wayland-0", Some(&plain), true), "протокол главнее службы");
    assert!(wb::cinnamon_layer_shell("X-Cinnamon", "wayland", "wayland-0", None, true));
    assert!(!wb::cinnamon_layer_shell("X-Cinnamon", "wayland", "wayland-0", None, false));
    assert!(!wb::cinnamon_layer_shell("X-Cinnamon", "x11", "", Some(&ls), true), "X11 — прежний путь окном");
    assert!(!wb::cinnamon_layer_shell("GNOME", "wayland", "wayland-0", Some(&ls), true));
}

#[test]
fn cinnamon_bg_service_files_follow_xdg_order() {
    let files = wb::cinnamon_bg_service_files("", "/home/u", "/usr/local/share:/usr/share/");
    assert_eq!(
        files,
        vec![
            "/home/u/.local/share/dbus-1/services/org.Cinnamon.Background.service",
            "/usr/local/share/dbus-1/services/org.Cinnamon.Background.service",
            "/usr/share/dbus-1/services/org.Cinnamon.Background.service",
        ]
    );
}

/// Cinnamon с layer-shell: сцены выводит хост Rexon (он поднимает штатный
/// клиент layer-shell), а не расширение оболочки — «Применить» доступна.
#[test]
fn cinnamon_layer_shell_display_is_rexon_host() {
    use wb::{DisplaySetup, WallpaperBackend as B};
    assert_eq!(wb::display_setup_in(B::ShellExtension, "X-Cinnamon", false, true), DisplaySetup::RexonHost);
    assert_eq!(wb::display_setup_in(B::ShellExtension, "X-Cinnamon", false, false), DisplaySetup::None);
    assert_eq!(wb::display_setup_in(B::ShellExtension, "GNOME", true, true), DisplaySetup::GnomeExtension, "GNOME не трогаем");
    assert_eq!(wb::display_setup_in(B::X11Subprocess, "X-Cinnamon", false, true), DisplaySetup::RexonHost);
}

/// Хост на Wayland: layer-shell и найденный клиент — поднимать клиента;
/// иначе ждать. Сокет демона передаётся как есть, пустой — клиент найдёт сам.
#[test]
fn wayland_host_delegates_only_with_layer_shell_and_client() {
    let ls = vec![wb::LAYER_SHELL_GLOBAL.to_string()];
    assert_eq!(
        wb::wayland_host_role(&ls, Some("/opt/ww/waywallen-layer-shell"), "/run/user/1/waywallen/display.sock"),
        wb::WaylandHostRole::Delegate(vec![
            "/opt/ww/waywallen-layer-shell".to_string(),
            "--socket".to_string(),
            "/run/user/1/waywallen/display.sock".to_string(),
            "--name".to_string(),
            "rexon".to_string(),
        ])
    );
    assert_eq!(
        wb::wayland_host_role(&ls, Some("/b/waywallen-layer-shell"), ""),
        wb::WaylandHostRole::Delegate(vec!["/b/waywallen-layer-shell".into(), "--name".into(), "rexon".into()])
    );
    assert_eq!(wb::wayland_host_role(&ls, None, ""), wb::WaylandHostRole::Idle);
    assert_eq!(wb::wayland_host_role(&[], Some("/b/waywallen-layer-shell"), ""), wb::WaylandHostRole::Idle);
}

/// Клиент вывода ищется рядом с демоном-родителем (AppImage), потом рядом с
/// демоном пакета; чужой родитель (не демон) в счёт не идёт.
#[test]
fn layer_shell_client_found_next_to_daemon() {
    let c = wb::layer_shell_client_candidates(Some("/tmp/.mount_ww/usr/bin/waywallen"), &[]);
    assert_eq!(c[0], "/tmp/.mount_ww/usr/bin/waywallen-layer-shell");
    let c = wb::layer_shell_client_candidates(Some("/usr/bin/bash"), &[]);
    assert!(!c.iter().any(|p| p.starts_with("/usr/bin/bash")), "{c:?}");
    assert!(c.iter().all(|p| p.ends_with("/waywallen-layer-shell")));
}

#[test]
fn layer_shell_restart_backs_off_with_ceiling() {
    assert_eq!(wb::layer_shell_restart_delay_ms(0), 2000);
    assert_eq!(wb::layer_shell_restart_delay_ms(1), 4000);
    assert_eq!(wb::layer_shell_restart_delay_ms(3), 16_000);
    assert_eq!(wb::layer_shell_restart_delay_ms(9), 30_000);
}

#[test]
fn display_off_marker_lives_in_state_dir() {
    assert_eq!(wb::display_off_marker_path("", "/home/u"), "/home/u/.local/state/rexon/waywallen-display.off");
    assert_eq!(wb::display_off_marker_path("/s/", "/home/u"), "/s/rexon/waywallen-display.off");
}

#[test]
fn gdbus_answers_are_parsed() {
    assert!(wb::parse_gdbus_bool("(true,)\n"));
    assert!(!wb::parse_gdbus_bool("(false,)\n"));
    assert_eq!(wb::parse_gdbus_uint("(uint32 4321,)\n"), Some(4321));
    assert_eq!(wb::parse_gdbus_uint("()"), None);
}

/// Подставная шина и systemd для перестановки фона.
struct BgIo {
    owner: std::cell::RefCell<Vec<bool>>,
    unit_active: bool,
    restart_ok: bool,
    ran: std::cell::RefCell<Vec<String>>,
}

impl BgIo {
    fn new(owner: &[bool], unit_active: bool, restart_ok: bool) -> Self {
        BgIo { owner: std::cell::RefCell::new(owner.to_vec()), unit_active, restart_ok, ran: Default::default() }
    }
}

impl wb::CinnamonBgIo for BgIo {
    fn run(&self, argv: &[String]) -> Option<String> {
        let line = argv.join(" ");
        self.ran.borrow_mut().push(line.clone());
        if line.contains("NameHasOwner") {
            let mut o = self.owner.borrow_mut();
            let v = if o.len() > 1 { o.remove(0) } else { o[0] };
            return Some(format!("({v},)\n"));
        }
        if line.contains("is-active") {
            return self.unit_active.then(|| "active\n".to_string());
        }
        if line.contains("systemctl --user restart") {
            return self.restart_ok.then(String::new);
        }
        if line.contains("GetConnectionUnixProcessID") {
            return Some("(uint32 777,)\n".to_string());
        }
        Some(String::new())
    }
    fn sleep_ms(&self, _ms: u64) {}
}

/// Службы нет — ничего не трогаем; есть юнит systemd — `restart` (лимит
/// менеджера фона Cinnamon нам не мешает); без systemd — сигнал владельцу
/// имени и свой `Start()` (активация D-Bus), ждём возвращения имени.
#[test]
fn restack_restarts_background_service_safely() {
    let io = BgIo::new(&[false], true, true);
    assert_eq!(wb::restack_cinnamon_background(&io), wb::BgRestack::NotRunning);
    assert!(!io.ran.borrow().iter().any(|l| l.contains("restart") || l.contains("kill")));

    let io = BgIo::new(&[true], true, true);
    assert_eq!(wb::restack_cinnamon_background(&io), wb::BgRestack::Restarted);
    assert!(io.ran.borrow().iter().any(|l| l == "systemctl --user restart cinnamon-background-daemon.service"));
    assert!(!io.ran.borrow().iter().any(|l| l.starts_with("kill")));

    let io = BgIo::new(&[true], true, false);
    assert_eq!(wb::restack_cinnamon_background(&io), wb::BgRestack::Failed);

    let io = BgIo::new(&[true, false, true], false, false);
    assert_eq!(wb::restack_cinnamon_background(&io), wb::BgRestack::Restarted);
    let ran = io.ran.borrow();
    let kill = ran.iter().position(|l| l == "kill -TERM 777").expect("сигнал владельцу имени");
    let start = ran.iter().position(|l| l.contains("org.Cinnamon.Background.Start")).expect("свой Start()");
    assert!(kill < start);
}

/// Защита от круга перезапусков: не чаще лимита за окно; наш же перезапуск
/// (свежая метка) наблюдатель шины падением службы не считает; перестановку,
/// которую уже сделал другой процесс Rexon, не повторяем.
#[test]
fn restack_guards_against_loops() {
    let now = 1_000_000;
    assert!(wb::bg_restack_allowed(&[], now));
    let full: Vec<u64> = (0..wb::BG_RESTACK_LIMIT as u64).map(|i| now - 1000 * i).collect();
    assert!(!wb::bg_restack_allowed(&full, now));
    let old: Vec<u64> = full.iter().map(|t| t - wb::BG_RESTACK_WINDOW_MS).collect();
    assert!(wb::bg_restack_allowed(&old, now), "старые перезапуски не в счёт");

    assert!(wb::bg_owner_change_is_ours(Some(now - 1000), now));
    assert!(!wb::bg_owner_change_is_ours(Some(now - wb::BG_OWN_RESTART_WINDOW_MS - 1), now));
    assert!(!wb::bg_owner_change_is_ours(None, now));

    assert!(wb::bg_restack_done_by_other(Some(now + 3000), now, 2500));
    assert!(!wb::bg_restack_done_by_other(Some(now + 1000), now, 2500), "раньше нашей поверхности — не в счёт");
    assert!(!wb::bg_restack_done_by_other(None, now, 2500));
    assert_eq!(wb::bg_restack_stamp_path("/run/user/1/"), "/run/user/1/rexon-cinnamon-bg-restack.stamp");
    assert_eq!(wb::bg_restack_lock_path("/run/user/1"), "/run/user/1/rexon-cinnamon-bg-restack.stamp.lock");
}

/// Метка помнит начало и конец перезапуска: «уже сделано другим» судится по
/// НАЧАЛУ (перезапуск, начатый до показа нашей поверхности, её не покрыл,
/// даже если кончился после), «наш ли перезапуск» для наблюдателя шины — по
/// последнему моменту. Метка прежнего вида (одно число) читается как начало.
#[test]
fn restack_stamp_keeps_start_and_end() {
    let s = wb::BgRestackStamp { started: 100, finished: Some(250) };
    assert_eq!(wb::parse_bg_restack_stamp(&s.to_text()), Some(s));
    assert_eq!(s.latest(), 250);
    let running = wb::BgRestackStamp { started: 100, finished: None };
    assert_eq!(wb::parse_bg_restack_stamp("100\n"), Some(running));
    assert_eq!(running.latest(), 100);
    assert_eq!(wb::parse_bg_restack_stamp(""), None);
    assert_eq!(wb::parse_bg_restack_stamp("мусор"), None);
    // Показ в 200: перезапуск 100→250 его не покрыл, 210→300 — покрыл.
    assert!(!wb::bg_restack_done_by_other(Some(100), 200, 0));
    assert!(wb::bg_restack_done_by_other(Some(210), 200, 0));
}

/// Заказы сливаются: просьбы по сроку только отодвигают исполнение и
/// помнят первый заказ; весть «первый кадр на экране» подтягивает срок к
/// почти-сейчас и сама становится точкой отсчёта — без запаса.
#[test]
fn restack_orders_merge_and_first_frame_pulls_in() {
    let fallback = wb::merge_bg_restack_order(None, 1000, wb::BG_RESTACK_FALLBACK_MS, false);
    assert_eq!(fallback, wb::BgRestackOrder { due: 1000 + wb::BG_RESTACK_FALLBACK_MS, requested: 1000, slack: wb::BG_RESTACK_FALLBACK_MS });
    let later = wb::merge_bg_restack_order(Some(fallback), 1500, wb::BG_RESTACK_FALLBACK_MS, false);
    assert_eq!(later.due, 1500 + wb::BG_RESTACK_FALLBACK_MS, "вторая просьба отодвинула срок");
    assert_eq!(later.requested, 1000, "точка отсчёта — первая просьба");
    let shorter = wb::merge_bg_restack_order(Some(later), 1600, 100, false);
    assert_eq!(shorter.due, later.due, "короткая просьба по сроку не подтягивает");

    let shown = wb::merge_bg_restack_order(Some(later), 1700, 0, true);
    assert_eq!(shown, wb::BgRestackOrder { due: 1700 + wb::BG_RESTACK_SHOWN_MS, requested: 1700, slack: 0 });
    assert!(shown.due < later.due, "первый кадр не ждёт запасного срока");
    assert!(wb::BG_RESTACK_SHOWN_MS < 500 && wb::BG_RESTACK_FALLBACK_MS > wb::BG_RESTACK_DELAY_MS);
}

/// Когда ставить подложку — по путям вывода: после показа там, где смена
/// картинки стола нашу поверхность не перекрывает (X11-окно, расширение
/// GNOME/Cinnamon, Cinnamon 6.8 с layer-shell, KDE, Treeland); ДО показа там,
/// где картинку держит отдельная поверхность слоя фона (wlroots, COSMIC,
/// нераспознанный Wayland). Не ставить — нигде: на Cinnamon с layer-shell
/// без подложки экран блокировки показывал устаревшую картинку (16.11.2026).
#[test]
fn backdrop_timing_follows_the_output_path() {
    use rexon_core::wallpaper_engine::{self as wpe, BackdropTiming::*};
    for de in ["mate", "xfce", "cinnamon", "cinnamon-mint", "budgie", "gnome", "lxqt", "kde", "deepin", "generic"] {
        assert_eq!(wpe::backdrop_timing(de, "x11"), AfterShow, "{de} x11");
    }
    for de in ["gnome", "cinnamon", "cinnamon-mint"] {
        assert_eq!(
            wpe::backdrop_timing(de, "wayland"),
            AfterShow,
            "{de}: расширение оболочки или (Cinnamon 6.8) слой под перезапущенной службой фона"
        );
    }
    assert_eq!(wpe::backdrop_timing("kde", "wayland"), AfterShow);
    assert_eq!(wpe::backdrop_timing("deepin", "wayland"), AfterShow, "Treeland");
    for de in ["sway", "hyprland", "niri", "river", "labwc", "wayfire", "cosmic", "budgie", "lxqt", "generic"] {
        assert_eq!(wpe::backdrop_timing(de, "wayland"), BeforeShow, "{de}: своя поверхность фона");
    }
}

/// Кеш кадра подложки: тот же ролик и тот же экран — тот же файл (повторное
/// применение не зовёт ffmpeg); другой экран, правка файла или другой файл —
/// другой кадр. Кадр масштабируется под самый большой выход.
#[test]
fn video_still_cache_key_and_scaling() {
    use rexon_core::wallpaper_engine as wpe;
    let screen = Some((2560, 1440));
    let key = wpe::video_still_key("/home/u/волны.mp4", 4096, 100, screen);
    assert_eq!(key, wpe::video_still_key("/home/u/волны.mp4", 4096, 100, screen));
    assert_ne!(key, wpe::video_still_key("/home/u/волны.mp4", 4096, 100, Some((1920, 1080))));
    assert_ne!(key, wpe::video_still_key("/home/u/волны.mp4", 4096, 101, screen));
    assert_ne!(key, wpe::video_still_key("/home/u/волны.mp4", 4097, 100, screen));
    assert_ne!(key, wpe::video_still_key("/home/u/лес.mp4", 4096, 100, screen));
    assert_ne!(key, wpe::video_still_key("/home/u/волны.mp4", 4096, 100, None));
    assert_eq!(
        wpe::video_still_path("/home/u/.cache/", &key),
        format!("/home/u/.cache/rexon/video-stills/{key}.jpg")
    );
    assert_eq!(wpe::still_screen_size(&[(1920, 1080), (2560, 1440), (1080, 1920)]), Some((2560, 1440)));
    assert_eq!(wpe::still_screen_size(&[]), None);
    let scaled = wpe::video_still_argv("/v.mp4", "/o.jpg", 3.0, screen).join(" ");
    assert!(scaled.contains("scale=2560:1440:force_original_aspect_ratio=increase"), "{scaled}");
    assert!(scaled.ends_with("/o.jpg") && scaled.contains("-frames:v 1"), "{scaled}");
    let native = wpe::video_still_argv("/v.mp4", "/o.jpg", 3.0, None).join(" ");
    assert!(!native.contains("scale="), "{native}");
    assert_eq!(wpe::backdrop_ready_log_line(420), "подложка готова через 420 мс после видео");
}

/// Предел кеша кадров: в пределах — ничего не трогается; сверх числа или
/// байт — уходят старейшие, ставящийся кадр — никогда; недописанные
/// `*.part.jpg` — всегда.
#[test]
fn video_stills_cache_is_bounded() {
    use rexon_core::wallpaper_engine as wpe;
    let f = |i: usize, mtime: i64, bytes: u64| (format!("/c/{i:02}.jpg"), mtime, bytes);
    let small: Vec<_> = (0..5).map(|i| f(i, i as i64, 1000)).collect();
    assert!(wpe::video_still_evictions(&small, "/c/04.jpg").is_empty());

    let n = wpe::VIDEO_STILLS_MAX_FILES + 3;
    let many: Vec<_> = (0..n).map(|i| f(i, 100 - i as i64, 1000)).collect();
    let keep = format!("/c/{:02}.jpg", n - 1); // самый старый — но он ставится
    let gone = wpe::video_still_evictions(&many, &keep);
    assert_eq!(gone.len(), 3, "{gone:?}");
    assert!(!gone.contains(&keep));
    assert_eq!(gone[0], format!("/c/{:02}.jpg", n - 2), "старейшие — первыми: {gone:?}");

    let big = wpe::VIDEO_STILLS_MAX_BYTES / 4 + 1;
    let heavy: Vec<_> = (0..5).map(|i| f(i, i as i64, big)).collect();
    let gone = wpe::video_still_evictions(&heavy, "/c/04.jpg");
    assert_eq!(gone, vec!["/c/00.jpg".to_string(), "/c/01.jpg".to_string()]);

    let mut part = small.clone();
    part.push(("/c/x.jpg.part.jpg".to_string(), 50, 10));
    assert_eq!(wpe::video_still_evictions(&part, "/c/04.jpg"), vec!["/c/x.jpg.part.jpg".to_string()]);
}

/// Строка замера «первый кадр на экране»: от «применяю» и от запуска хоста;
/// без момента применения — только от запуска. Мусор и время из будущего в
/// переменной — не замер.
#[test]
fn first_frame_measure_line() {
    use rexon_core::wallpaper_engine as wpe;
    assert_eq!(wpe::since_apply_ms("1000", 2500), Some(1500));
    assert_eq!(wpe::since_apply_ms(" 1000\n", 1000), Some(0));
    assert_eq!(wpe::since_apply_ms("", 2500), None);
    assert_eq!(wpe::since_apply_ms("abc", 2500), None);
    assert_eq!(wpe::since_apply_ms("3000", 2500), None);
    assert_eq!(wpe::since_apply_ms("0", 2500), None);
    assert_eq!(
        wpe::first_frame_log_line("видео", Some(1500), 900),
        "первый кадр (видео) на экране через 1500 мс после «применяю» (хост — 900 мс от запуска)"
    );
    assert_eq!(wpe::first_frame_log_line("Flash", None, 700), "первый кадр (Flash) на экране через 700 мс от запуска хоста");
}
