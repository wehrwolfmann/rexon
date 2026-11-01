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
