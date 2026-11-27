//! Login screen: which login manager is the primary one, which are installed, whether
//! Rexon can switch them on this system and whether each one is ready to become the primary one.
//!
//! Only pure core functions (`rexon_core::system_plan`): not a single
//! subprocess, not a single file of the live machine. The texts of units and greeters in
//! `tests/data/display_managers/` were copied from Wehrwolfmann's machine on 20.10.2026
//! (Arch/CachyOS): plasmalogin, sddm, lightdm, greetd, cosmic-greeter,
//! cosmic-greeter-daemon, `/usr/share/xgreeters/*.desktop` and the output of
//! `lightdm --show-config`.

mod i18n_pin;

use std::cell::RefCell;

use rexon_core::system_plan::{
    active_login_manager_from, active_login_manager_opt, active_probe_units,
    dm_state_is_active, dm_units_from_unit_files, unit_exec_start, unit_values,
    KNOWN_DM_UNITS, LOGIN_MANAGER_PROBE_UNITS,
};

/// Units from Wehrwolfmann's machine (`/usr/lib/systemd/system`, 20.10.2026).
pub const UNIT_PLASMALOGIN: &str = include_str!("data/display_managers/plasmalogin.service");
pub const UNIT_SDDM: &str = include_str!("data/display_managers/sddm.service");
pub const UNIT_LIGHTDM: &str = include_str!("data/display_managers/lightdm.service");
pub const UNIT_GREETD: &str = include_str!("data/display_managers/greetd.service");
pub const UNIT_COSMIC_GREETER: &str = include_str!("data/display_managers/cosmic-greeter.service");
pub const UNIT_COSMIC_GREETER_DAEMON: &str =
    include_str!("data/display_managers/cosmic-greeter-daemon.service");

fn unit(name: &str, text: &str) -> (String, String) {
    (format!("/usr/lib/systemd/system/{name}.service"), text.to_string())
}

fn host_units() -> Vec<(String, String)> {
    vec![
        unit("cosmic-greeter-daemon", UNIT_COSMIC_GREETER_DAEMON),
        unit("cosmic-greeter", UNIT_COSMIC_GREETER),
        unit("greetd", UNIT_GREETD),
        unit("lightdm", UNIT_LIGHTDM),
        unit("plasmalogin", UNIT_PLASMALOGIN),
        unit("sddm", UNIT_SDDM),
    ]
}

const SDDM_UNIT_PATH: &str = "/usr/lib/systemd/system/sddm.service";

fn states<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |u: &str| {
        pairs
            .iter()
            .find(|(k, _)| *k == u)
            .map(|(_, v)| (*v).to_string())
    }
}

// ============================================================================
// Item 1. The active login screen without substituting "sddm"
// ============================================================================

#[test]
fn active_opt_live_symlink_wins_without_probes() {
    let probes = RefCell::new(0);
    let is_enabled = |_: &str| {
        *probes.borrow_mut() += 1;
        None
    };
    let got = active_login_manager_opt(
        Some("/usr/lib/systemd/system/plasmalogin.service"),
        true,
        true,
        &is_enabled,
    );
    assert_eq!(got.as_deref(), Some("plasmalogin"));
    assert_eq!(*probes.borrow(), 0, "живой симлинк решает без опроса юнитов");
}

#[test]
fn active_opt_dangling_symlink_is_none_not_sddm() {
    // After `pacman -R sddm` the link dangles, no unit is enabled:
    // the former function answers "sddm", the new one honestly says "we do not know".
    let nothing = |_: &str| Some("not-found".to_string());
    assert_eq!(
        active_login_manager_opt(Some(SDDM_UNIT_PATH), false, true, &nothing),
        None
    );
    assert_eq!(
        active_login_manager_from(Some(SDDM_UNIT_PATH), false, true, &nothing),
        "sddm",
        "прежний ответ для старых потребителей не изменился"
    );
}

#[test]
fn active_opt_without_symlink_takes_enabled_or_alias() {
    let enabled = states(&[("sddm", "disabled"), ("lightdm", "enabled")]);
    assert_eq!(
        active_login_manager_opt(None, false, true, &enabled).as_deref(),
        Some("lightdm")
    );
    let alias = states(&[("gdm", "alias")]);
    assert_eq!(
        active_login_manager_opt(None, false, true, &alias).as_deref(),
        Some("gdm")
    );
    let runtime = states(&[("sddm", "enabled-runtime")]);
    assert_eq!(
        active_login_manager_opt(None, false, true, &runtime).as_deref(),
        Some("sddm")
    );
}

#[test]
fn active_opt_static_and_indirect_are_not_main() {
    // `static`/`indirect` on a DM unit do not put it in the place of
    // display-manager.service — we will not call such a one primary.
    for state in ["static", "indirect", "disabled", "masked", "not-found", ""] {
        let is_enabled = |u: &str| (u == "lightdm").then(|| state.to_string());
        assert_eq!(
            active_login_manager_opt(None, false, true, &is_enabled),
            None,
            "состояние {state:?}"
        );
        assert!(!dm_state_is_active(state));
    }
    // And the former detection still accepts static (the Python golden).
    let st = states(&[("lightdm", "static")]);
    assert_eq!(active_login_manager_from(None, false, true, &st), "lightdm");
}

#[test]
fn active_opt_finds_units_outside_narrow_list() {
    // Debian without a symlink, gdm3 enabled: the narrow list does not know it, and
    // the former function answered "sddm". The broad query finds it.
    let st = states(&[("gdm3", "enabled")]);
    assert_eq!(
        active_login_manager_opt(None, false, true, &st).as_deref(),
        Some("gdm3")
    );
    assert_eq!(active_login_manager_from(None, false, true, &st), "sddm");
}

#[test]
fn active_opt_without_systemctl_is_none() {
    let probes = RefCell::new(0);
    let is_enabled = |_: &str| {
        *probes.borrow_mut() += 1;
        Some("enabled".to_string())
    };
    assert_eq!(active_login_manager_opt(None, false, false, &is_enabled), None);
    assert_eq!(*probes.borrow(), 0, "без systemctl опрашивать нечем");
    // A live symlink still answers in this case (a chroot without systemd).
    assert_eq!(
        active_login_manager_opt(Some(SDDM_UNIT_PATH), true, false, &is_enabled).as_deref(),
        Some("sddm")
    );
}

#[test]
fn active_probe_order_keeps_old_priority_then_rest() {
    let order = active_probe_units();
    assert_eq!(&order[..LOGIN_MANAGER_PROBE_UNITS.len()], LOGIN_MANAGER_PROBE_UNITS);
    assert!(!order.contains(&"display-manager"), "алиас — не экран входа");
    for unit in KNOWN_DM_UNITS.iter().filter(|u| **u != "display-manager") {
        assert_eq!(order.iter().filter(|u| *u == unit).count(), 1, "{unit}");
    }
}

#[test]
fn active_opt_probes_lazily_until_first_hit() {
    let probes = RefCell::new(Vec::<String>::new());
    let is_enabled = |u: &str| {
        probes.borrow_mut().push(u.to_string());
        (u == "sddm").then(|| "enabled".to_string())
    };
    assert_eq!(
        active_login_manager_opt(None, false, true, &is_enabled).as_deref(),
        Some("sddm")
    );
    assert_eq!(probes.into_inner(), vec!["plasmalogin", "sddm"]);
}

// ============================================================================
// Item 2. The list of installed ones by Alias=display-manager.service
// ============================================================================

#[test]
fn installed_from_host_unit_files() {
    // Wehrwolfmann's machine: five login screens, the cosmic-greeter-daemon service
    // (without Alias) does not get into the list. The order follows the table of known ones.
    assert_eq!(
        dm_units_from_unit_files(&host_units()),
        vec!["plasmalogin", "sddm", "lightdm", "greetd", "cosmic-greeter"]
    );
}

#[test]
fn unit_without_install_or_alias_is_not_listed() {
    let files = vec![
        unit("foo", "[Unit]\nDescription=x\n[Service]\nExecStart=/usr/bin/foo\n"),
        unit("bar", "[Install]\nWantedBy=multi-user.target\n"),
        // Alias outside [Install] does not count.
        unit("baz", "[Unit]\nAlias=display-manager.service\n"),
    ];
    assert!(dm_units_from_unit_files(&files).is_empty());
}

#[test]
fn alias_alias_template_and_legacy_are_filtered() {
    let alias = "[Install]\nAlias=display-manager.service\n";
    let files = vec![
        unit("display-manager", alias),
        unit("display-manager-legacy", alias),
        unit("ly@", alias),
        unit("ly@tty2", alias),
        unit("xdm-legacy", alias),
        unit("emptty", alias),
    ];
    assert_eq!(dm_units_from_unit_files(&files), vec!["emptty"]);
}

#[test]
fn unknown_dm_with_alias_is_listed_after_known() {
    let alias = "[Install]\nAlias=display-manager.service\n";
    let files = vec![
        unit("lemurs", alias),
        unit("entrance", alias),
        unit("sddm", UNIT_SDDM),
        unit("abc-dm", "[Install]\nAlias=foo.service display-manager.service\n"),
    ];
    assert_eq!(
        dm_units_from_unit_files(&files),
        vec!["sddm", "entrance", "abc-dm", "lemurs"]
    );
}

#[test]
fn etc_override_wins_over_package_unit() {
    // A user's own file in /etc without Alias overrides the package one — systemd reads it.
    let files = vec![
        ("/etc/systemd/system/sddm.service".to_string(), "[Service]\nExecStart=/usr/bin/sddm\n".to_string()),
        unit("sddm", UNIT_SDDM),
    ];
    assert!(dm_units_from_unit_files(&files).is_empty());
}

#[test]
fn empty_alias_assignment_resets_list() {
    let text = "[Install]\nAlias=display-manager.service\nAlias=\n";
    assert!(unit_values(text, "Install", "Alias").is_empty());
    assert!(dm_units_from_unit_files(&[unit("x", text)]).is_empty());
}

#[test]
fn exec_start_of_host_units() {
    assert_eq!(unit_exec_start(UNIT_PLASMALOGIN).unwrap(), vec!["/usr/bin/plasmalogin"]);
    assert_eq!(unit_exec_start(UNIT_SDDM).unwrap(), vec!["/usr/bin/sddm"]);
    assert_eq!(unit_exec_start(UNIT_LIGHTDM).unwrap(), vec!["/usr/bin/lightdm"]);
    assert_eq!(unit_exec_start(UNIT_GREETD).unwrap(), vec!["greetd"]);
    assert_eq!(
        unit_exec_start(UNIT_COSMIC_GREETER).unwrap(),
        vec!["greetd", "--config", "/etc/greetd/cosmic-greeter.toml"]
    );
    // ExecStartPre is not confused with ExecStart, systemd prefixes are stripped.
    assert_eq!(
        unit_exec_start("[Service]\nExecStartPre=-/bin/true\nExecStart=-@/usr/sbin/gdm3 gdm3\n").unwrap(),
        vec!["/usr/sbin/gdm3", "gdm3"]
    );
    assert_eq!(unit_exec_start("[Unit]\nExecStart=/x\n"), None);
}

// ============================================================================
// Item 3. The login screen table
// ============================================================================

#[test]
fn spec_table_covers_known_units_in_order() {
    use rexon_core::system_plan::LOGIN_MANAGER_SPECS;
    let table: Vec<&str> = LOGIN_MANAGER_SPECS.iter().map(|s| s.unit).collect();
    let known: Vec<&str> = KNOWN_DM_UNITS
        .iter()
        .copied()
        .filter(|u| *u != "display-manager" && *u != "tbsm")
        .collect();
    assert_eq!(table, known);
    for s in LOGIN_MANAGER_SPECS {
        assert!(!s.label.is_empty() && !s.binary.is_empty() && !s.pam.is_empty(), "{}", s.unit);
    }
}

#[test]
fn labels_keep_old_names_and_add_new() {
    use rexon_core::categories::login_manager_label;
    for (unit, label) in [
        ("plasmalogin", "Plasma Login Manager"),
        ("sddm", "SDDM"),
        ("gdm", "GDM"),
        ("gdm3", "GDM"),
        ("lightdm", "LightDM"),
        ("lxdm", "LXDM"),
        ("greetd", "greetd"),
        ("ly", "Ly"),
        ("cosmic-greeter", "COSMIC Greeter"),
        ("emptty", "emptty"),
        ("slim", "SLiM"),
        (" sddm \n", "SDDM"),
        ("lemurs", ""),
        ("", ""),
    ] {
        assert_eq!(login_manager_label(unit), label, "{unit:?}");
    }
}

#[test]
fn debian_gnome_gets_gdm3_enabled_when_no_login_screen() {
    use rexon_core::system::PkgFamily;
    use rexon_core::system_plan::dm_enable_step;
    let pkgs: Vec<String> = ["gnome", "gnome-tweaks", "gdm3"].iter().map(|s| s.to_string()).collect();
    let (unit, cmd) = dm_enable_step(PkgFamily::Debian, &pkgs, false, false, false)
        .expect("gdm3 из списка пакетов включается");
    assert_eq!(unit, "gdm3");
    assert_eq!(cmd.argv, vec!["pkexec", "systemctl", "enable", "gdm3.service"]);
    // A login screen already exists — we do not touch someone else's.
    assert!(dm_enable_step(PkgFamily::Debian, &pkgs, false, false, true).is_none());
}

// ============================================================================
// Item 5. The switching mechanism by family and probes
// ============================================================================

mod backend {
    use rexon_core::system::PkgFamily;
    use rexon_core::system_plan::{
        dm_manual_switch_command, dm_switch_backend, other_init_name, DmBackendProbe,
        DmSwitchBackend,
    };

    fn systemd() -> DmBackendProbe {
        DmBackendProbe { systemd_running: true, has_systemctl: true, ..Default::default() }
    }

    #[test]
    fn arch_and_fedora_are_plain_systemd() {
        for family in [PkgFamily::Arch, PkgFamily::Fedora, PkgFamily::Unknown] {
            let b = dm_switch_backend(family, &systemd());
            assert_eq!(b, DmSwitchBackend::Systemd, "{family:?}");
            assert!(b.switch_supported());
        }
    }

    #[test]
    fn debian_with_default_dm_file() {
        let p = DmBackendProbe { x11_default_dm_file: true, ..systemd() };
        let b = dm_switch_backend(PkgFamily::Debian, &p);
        assert_eq!(b, DmSwitchBackend::SystemdDebian);
        assert!(b.switch_supported());
        // No file — the choice is only by the link.
        assert_eq!(dm_switch_backend(PkgFamily::Debian, &systemd()), DmSwitchBackend::Systemd);
        // On Arch this file decides nothing.
        assert_eq!(dm_switch_backend(PkgFamily::Arch, &p), DmSwitchBackend::Systemd);
    }

    #[test]
    fn suse_alternatives_only_with_real_unit_and_alternative() {
        let both = DmBackendProbe {
            dm_service_is_regular_file: true,
            suse_dm_alternative: true,
            ..systemd()
        };
        let b = dm_switch_backend(PkgFamily::Suse, &both);
        assert_eq!(b, DmSwitchBackend::SystemdSuseAlternatives);
        assert!(!b.switch_supported(), "openSUSE — после стенда");
        let only_alt = DmBackendProbe { suse_dm_alternative: true, ..systemd() };
        assert_eq!(dm_switch_backend(PkgFamily::Suse, &only_alt), DmSwitchBackend::Systemd);
    }

    #[test]
    fn nixos_is_declarative_even_with_systemd() {
        let b = dm_switch_backend(PkgFamily::Nix, &systemd());
        assert_eq!(b, DmSwitchBackend::NixosDeclarative);
        assert!(!b.switch_supported());
    }

    #[test]
    fn without_systemd_unsupported_with_init_name() {
        let p = DmBackendProbe { other_init: "OpenRC".into(), ..Default::default() };
        let b = dm_switch_backend(PkgFamily::Arch, &p);
        assert_eq!(b, DmSwitchBackend::Unsupported("OpenRC".into()));
        assert!(!b.switch_supported());
        // systemctl exists, but PID 1 is not systemd (Artix) — not supported either.
        let artix = DmBackendProbe { has_systemctl: true, other_init: "runit".into(), ..Default::default() };
        assert_eq!(dm_switch_backend(PkgFamily::Arch, &artix), DmSwitchBackend::Unsupported("runit".into()));
        // systemd is running, but there is no systemctl — we do not touch it.
        let no_ctl = DmBackendProbe { systemd_running: true, ..Default::default() };
        assert_eq!(dm_switch_backend(PkgFamily::Fedora, &no_ctl), DmSwitchBackend::Unsupported(String::new()));
    }

    #[test]
    fn init_names() {
        let none = |_: &str| false;
        assert_eq!(other_init_name(&|p| p == "/run/openrc", &none), "OpenRC");
        assert_eq!(other_init_name(&none, &|n| n == "rc-update"), "OpenRC");
        assert_eq!(other_init_name(&|p| p == "/run/runit", &none), "runit");
        assert_eq!(other_init_name(&none, &|n| n == "s6-rc"), "s6");
        assert_eq!(other_init_name(&none, &|n| n == "dinitctl"), "dinit");
        assert_eq!(other_init_name(&|p| p == "/etc/inittab", &none), "SysVinit");
        assert_eq!(other_init_name(&none, &none), "");
    }

    #[test]
    fn manual_commands_per_init() {
        assert_eq!(
            dm_manual_switch_command("OpenRC", PkgFamily::Gentoo, "sddm").unwrap(),
            "sudo sed -i 's/^DISPLAYMANAGER=.*/DISPLAYMANAGER=\"sddm\"/' /etc/conf.d/display-manager && sudo rc-update add display-manager default"
        );
        assert_eq!(
            dm_manual_switch_command("runit", PkgFamily::Void, "lightdm").unwrap(),
            "sudo ln -s /etc/sv/lightdm /var/service/"
        );
        assert_eq!(
            dm_manual_switch_command("runit", PkgFamily::Arch, "lightdm").unwrap(),
            "sudo ln -s /etc/runit/sv/lightdm /run/runit/service/"
        );
        assert_eq!(dm_manual_switch_command("dinit", PkgFamily::Arch, "sddm").unwrap(), "sudo dinitctl enable sddm");
        assert_eq!(
            dm_manual_switch_command("SysVinit", PkgFamily::Debian, "lightdm").unwrap(),
            "sudo dpkg-reconfigure lightdm"
        );
        assert_eq!(dm_manual_switch_command("s6", PkgFamily::Arch, "sddm"), None);
        assert_eq!(dm_manual_switch_command("", PkgFamily::Arch, "sddm"), None);
        // A name with dangerous characters does not get into the command.
        assert_eq!(dm_manual_switch_command("dinit", PkgFamily::Arch, "sddm; rm -rf /"), None);
    }
}

// ============================================================================
// Item 4 (LightDM). The greeter: is it set, is it installed, what to offer
// ============================================================================

mod lightdm {
    use super::i18n_pin;
    use rexon_core::system::PkgFamily;
    use rexon_core::system_plan::{
        lightdm_default_greeter, lightdm_greeter_from_show_config, lightdm_greeter_readiness,
        xgreeters_from, DmGreeter, DmReadiness,
    };

    const SHOW_CONFIG_HOST: &str = include_str!("data/display_managers/lightdm-show-config.txt");
    const GTK: &str = include_str!("data/display_managers/lightdm-gtk-greeter.desktop");
    const SLICK: &str = include_str!("data/display_managers/lightdm-slick-greeter.desktop");

    fn host_greeters() -> Vec<DmGreeter> {
        let files = vec![
            ("/usr/share/xgreeters/lightdm-slick-greeter.desktop".to_string(), SLICK.to_string()),
            ("/usr/share/xgreeters/lightdm-gtk-greeter.desktop".to_string(), GTK.to_string()),
        ];
        xgreeters_from(&files, &|_| true)
    }

    #[test]
    fn show_config_on_host_has_no_greeter_session() {
        assert_eq!(lightdm_greeter_from_show_config(SHOW_CONFIG_HOST), None);
        let set = "   [Seat:*]\nB  greeter-session=lightdm-slick-greeter\nA  session-wrapper=/etc/lightdm/Xsession\n";
        assert_eq!(lightdm_greeter_from_show_config(set).as_deref(), Some("lightdm-slick-greeter"));
        // A key in another section does not count.
        let other = "[LightDM]\nA  greeter-session=x\n";
        assert_eq!(lightdm_greeter_from_show_config(other), None);
        let legacy = "[SeatDefaults]\ngreeter-session=unity-greeter\n";
        assert_eq!(lightdm_greeter_from_show_config(legacy).as_deref(), Some("unity-greeter"));
    }

    #[test]
    fn default_greeter_per_family() {
        assert_eq!(lightdm_default_greeter(PkgFamily::Arch), Some("lightdm-gtk-greeter"));
        assert_eq!(lightdm_default_greeter(PkgFamily::Debian), Some("lightdm-greeter"));
        assert_eq!(lightdm_default_greeter(PkgFamily::Gentoo), Some("example-gtk-gnome"));
        assert_eq!(lightdm_default_greeter(PkgFamily::Fedora), None, "не подтверждено");
    }

    #[test]
    fn xgreeters_from_host_files() {
        assert_eq!(
            host_greeters(),
            vec![
                DmGreeter { id: "lightdm-gtk-greeter".into(), name: "LightDM GTK Greeter".into(), exec: "lightdm-gtk-greeter".into() },
                DmGreeter { id: "lightdm-slick-greeter".into(), name: "Slick Greeter".into(), exec: "slick-greeter".into() },
            ]
        );
        // No program — the greeter is not installed.
        let files = vec![("/usr/share/xgreeters/lightdm-gtk-greeter.desktop".to_string(), GTK.to_string())];
        assert!(xgreeters_from(&files, &|_| false).is_empty());
    }

    #[test]
    fn debian_alternative_is_not_listed_twice() {
        let files = vec![
            ("/usr/share/xgreeters/lightdm-greeter.desktop".to_string(), GTK.to_string()),
            ("/usr/share/xgreeters/lightdm-gtk-greeter.desktop".to_string(), GTK.to_string()),
            ("/usr/share/xgreeters/hidden.desktop".to_string(), "[Desktop Entry]\nExec=x\nHidden=true\n".to_string()),
        ];
        let ids: Vec<String> = xgreeters_from(&files, &|_| true).into_iter().map(|g| g.id).collect();
        assert_eq!(ids, vec!["lightdm-gtk-greeter"]);
    }

    #[test]
    fn owner_case_unset_with_gtk_and_slick_needs_choice_slick_suggested() {
        let greeters = host_greeters();
        let r = lightdm_greeter_readiness(None, lightdm_default_greeter(PkgFamily::Arch), &greeters);
        assert_eq!(r, DmReadiness::NeedsGreeterChoice(greeters.clone()));
        assert_eq!(r.suggested_greeter().unwrap().id, "lightdm-slick-greeter");
    }

    #[test]
    fn set_and_installed_is_ready() {
        let greeters = host_greeters();
        assert_eq!(
            lightdm_greeter_readiness(Some("lightdm-gtk-greeter"), None, &greeters),
            DmReadiness::Ready
        );
        assert_eq!(DmReadiness::Ready.suggested_greeter(), None);
    }

    #[test]
    fn set_but_missing_needs_choice() {
        let greeters = host_greeters();
        let r = lightdm_greeter_readiness(Some("unity-greeter"), None, &greeters);
        assert_eq!(r.suggested_greeter().unwrap().id, "lightdm-slick-greeter");
        // Without Slick the first one is offered.
        let gtk_only = vec![greeters[0].clone()];
        let r = lightdm_greeter_readiness(Some("unity-greeter"), None, &gtk_only);
        assert_eq!(r.suggested_greeter().unwrap().id, "lightdm-gtk-greeter");
    }

    #[test]
    fn unset_single_default_is_ready_unconfirmed_needs_choice() {
        let gtk_only = vec![host_greeters()[0].clone()];
        assert_eq!(
            lightdm_greeter_readiness(None, Some("lightdm-gtk-greeter"), &gtk_only),
            DmReadiness::Ready
        );
        assert!(matches!(
            lightdm_greeter_readiness(None, None, &gtk_only),
            DmReadiness::NeedsGreeterChoice(_)
        ));
        assert!(matches!(
            lightdm_greeter_readiness(None, Some("example-gtk-gnome"), &gtk_only),
            DmReadiness::NeedsGreeterChoice(_)
        ));
    }

    #[test]
    fn no_greeter_is_blocked() {
        let _lang = i18n_pin::en();
        assert_eq!(
            lightdm_greeter_readiness(None, Some("lightdm-gtk-greeter"), &[]),
            DmReadiness::Blocked(
                "LightDM has no greeter installed, so it can't show a login screen.".into()
            )
        );
    }
}

// ============================================================================
// Item 4. greetd, SDDM, common checks and the readiness verdict
// ============================================================================

mod readiness {
    use super::i18n_pin;
    use rexon_core::system_plan::{
        dm_from_init_files, dm_readiness, dm_warnings, greetd_command_kind, greetd_config_path,
        greetd_session_user, login_manager_spec, sddm_theme_ready, sddm_uses_x11,
        sddm_wayland_compositor, sysusers_declares, passwd_has_user, unit_exec_start,
        DmFacts, DmReadiness, DmSwitchBackend, GreetdCommand, GreetdFacts, SddmFacts,
        SddmThemeState,
    };

    const GREETD_CONFIG_HOST: &str = include_str!("data/display_managers/greetd-config.toml");
    const COSMIC_CONFIG_HOST: &str = include_str!("data/display_managers/cosmic-greeter.toml");
    const SDDM_DEFAULT: &str = include_str!("data/display_managers/sddm-default.conf");
    const SDDM_GENERAL: &str = include_str!("data/display_managers/sddm-general.conf");
    const SDDM_WAYLAND: &str = include_str!("data/display_managers/sddm-zz-wayland.conf");
    const SDDM_KDE: &str = include_str!("data/display_managers/sddm-kde_settings.conf");
    const BREEZE_META: &str = include_str!("data/display_managers/sddm-breeze-metadata.desktop");

    /// Everything in place — from here on every check breaks one thing.
    fn good() -> DmFacts {
        DmFacts {
            binary_exists: true,
            user: Some("x".into()),
            user_ok: true,
            pam: "x".into(),
            pam_exists: true,
            xorg: true,
            x_sessions: 3,
            wayland_sessions: 2,
            ..Default::default()
        }
    }

    fn ready(unit: &str, facts: &DmFacts) -> DmReadiness {
        dm_readiness(login_manager_spec(unit), &DmSwitchBackend::Systemd, facts)
    }

    fn reason(r: DmReadiness) -> String {
        match r {
            DmReadiness::Blocked(s) => s,
            other => panic!("ждали Blocked, пришло {other:?}"),
        }
    }

    #[test]
    fn greetd_command_kinds() {
        assert_eq!(greetd_command_kind(GREETD_CONFIG_HOST), GreetdCommand::Text("agreety".into()));
        assert_eq!(
            greetd_command_kind(COSMIC_CONFIG_HOST),
            GreetdCommand::Graphical("cosmic-greeter-start".into())
        );
        let tui = "[default_session]\ncommand = \"tuigreet --cmd sway\"\nuser = \"greeter\"\n";
        assert_eq!(greetd_command_kind(tui), GreetdCommand::Graphical("tuigreet".into()));
        let cage = "[default_session]\ncommand = 'env GTK_THEME=Adwaita cage -s -- regreet'\n";
        assert_eq!(greetd_command_kind(cage), GreetdCommand::Graphical("cage".into()));
        let shell = "[default_session]\ncommand = \"/bin/bash\"\n";
        assert_eq!(greetd_command_kind(shell), GreetdCommand::Text("/bin/bash".into()));
        assert_eq!(greetd_command_kind("[terminal]\nvt = 1\n"), GreetdCommand::Missing);
        assert_eq!(greetd_command_kind("[default_session]\ncommand = \"\"\n"), GreetdCommand::Missing);
        assert_eq!(greetd_session_user(GREETD_CONFIG_HOST).as_deref(), Some("greeter"));
        assert_eq!(greetd_session_user(COSMIC_CONFIG_HOST).as_deref(), Some("cosmic-greeter"));
    }

    #[test]
    fn greetd_config_path_from_unit() {
        let greetd = unit_exec_start(super::UNIT_GREETD).unwrap();
        assert_eq!(greetd_config_path(&greetd), "/etc/greetd/config.toml");
        let cosmic = unit_exec_start(super::UNIT_COSMIC_GREETER).unwrap();
        assert_eq!(greetd_config_path(&cosmic), "/etc/greetd/cosmic-greeter.toml");
        let eq: Vec<String> = ["greetd", "--config=/etc/x.toml"].iter().map(|s| s.to_string()).collect();
        assert_eq!(greetd_config_path(&eq), "/etc/x.toml");
        let short: Vec<String> = ["greetd", "-c", "/etc/y.toml"].iter().map(|s| s.to_string()).collect();
        assert_eq!(greetd_config_path(&short), "/etc/y.toml");
    }

    #[test]
    fn owner_case_greetd_agreety_blocked_cosmic_not() {
        let _lang = i18n_pin::en();
        // greetd per /etc/greetd/config.toml — agreety.
        let greetd = DmFacts {
            greetd: Some(GreetdFacts { command: greetd_command_kind(GREETD_CONFIG_HOST), program_exists: false }),
            ..good()
        };
        assert_eq!(
            reason(ready("greetd", &greetd)),
            "greetd is set up for text sign-in — the desktop won't start."
        );
        // cosmic-greeter is the same greetd, but with its own config: agreety from
        // config.toml does not concern it.
        let cosmic = DmFacts {
            greetd: Some(GreetdFacts { command: greetd_command_kind(COSMIC_CONFIG_HOST), program_exists: true }),
            ..good()
        };
        assert_eq!(ready("cosmic-greeter", &cosmic), DmReadiness::Ready);
        // Its greeter is gone — a blocker with the program name.
        let gone = DmFacts {
            greetd: Some(GreetdFacts { command: greetd_command_kind(COSMIC_CONFIG_HOST), program_exists: false }),
            ..good()
        };
        assert_eq!(
            reason(ready("cosmic-greeter", &gone)),
            "The greeter cosmic-greeter-start that greetd is set up to start isn't installed."
        );
        let missing = DmFacts {
            greetd: Some(GreetdFacts { command: GreetdCommand::Missing, program_exists: false }),
            ..good()
        };
        assert!(matches!(ready("greetd", &missing), DmReadiness::Blocked(_)));
    }

    #[test]
    fn sddm_config_merge_on_host() {
        // The host's reading order: package conf.d by name, then the user's own.
        let confs = [SDDM_DEFAULT, SDDM_GENERAL, SDDM_WAYLAND, SDDM_KDE];
        assert!(!sddm_uses_x11(&confs), "zz-wayland.conf переводит SDDM на Wayland");
        assert_eq!(sddm_wayland_compositor(&confs), "kwin_wayland");
        assert!(sddm_uses_x11(&[SDDM_DEFAULT]));
        assert_eq!(sddm_wayland_compositor(&[]), "weston");
        assert_eq!(
            rexon_core::system_plan::ini_last_value(&confs, "Theme", "Current").as_deref(),
            Some("breeze")
        );
    }

    #[test]
    fn sddm_theme_states() {
        let all = |_: &str| true;
        let none = |_: &str| false;
        assert_eq!(sddm_theme_ready(None, None, &none), SddmThemeState::Builtin);
        assert_eq!(sddm_theme_ready(Some(" "), None, &none), SddmThemeState::Builtin);
        assert_eq!(
            sddm_theme_ready(Some("breeze"), Some(BREEZE_META), &all),
            SddmThemeState::Ready { theme: "breeze".into() }
        );
        assert_eq!(
            sddm_theme_ready(Some("gone"), None, &all),
            SddmThemeState::Missing { theme: "gone".into() }
        );
        // A Qt 6 theme without a Qt 6 greeter.
        assert_eq!(
            sddm_theme_ready(Some("breeze"), Some(BREEZE_META), &|g| g == "sddm-greeter"),
            SddmThemeState::NeedsGreeter { theme: "breeze".into(), greeter: "sddm-greeter-qt6".into() }
        );
        // A theme without QtVersion is for Qt 5.
        let qt5 = "[SddmGreeterTheme]\nName=Old\nMainScript=Main.qml\n";
        assert_eq!(
            sddm_theme_ready(Some("old"), Some(qt5), &|g| g == "sddm-greeter-qt6"),
            SddmThemeState::NeedsGreeter { theme: "old".into(), greeter: "sddm-greeter".into() }
        );
    }

    #[test]
    fn sddm_readiness_and_warning() {
        let _lang = i18n_pin::en();
        let missing_theme = DmFacts {
            sddm: Some(SddmFacts { theme: SddmThemeState::Missing { theme: "gone".into() }, wayland_compositor: None }),
            ..good()
        };
        assert_eq!(ready("sddm", &missing_theme), DmReadiness::Ready, "запасная тема — не блок");
        assert_eq!(
            dm_warnings(&missing_theme),
            vec!["The SDDM theme “gone” isn't installed — SDDM will show its built-in look."]
        );
        let no_greeter = DmFacts {
            sddm: Some(SddmFacts {
                theme: SddmThemeState::NeedsGreeter { theme: "old".into(), greeter: "sddm-greeter".into() },
                wayland_compositor: None,
            }),
            ..good()
        };
        assert_eq!(
            reason(ready("sddm", &no_greeter)),
            "The SDDM theme “old” needs sddm-greeter, which isn't installed."
        );
        let no_compositor = DmFacts {
            needs_xorg: false,
            sddm: Some(SddmFacts {
                theme: SddmThemeState::Builtin,
                wayland_compositor: Some(("weston".into(), false)),
            }),
            ..good()
        };
        assert_eq!(
            reason(ready("sddm", &no_compositor)),
            "SDDM is set to start on Wayland with weston, which isn't installed."
        );
        assert!(dm_warnings(&good()).is_empty());
    }

    #[test]
    fn common_checks_in_order() {
        let _lang = i18n_pin::en();
        assert_eq!(ready("plasmalogin", &good()), DmReadiness::Ready);
        let cases: Vec<(DmFacts, &str)> = vec![
            (DmFacts { masked: true, ..good() }, "This login screen is switched off in the system (the service is masked)."),
            (DmFacts { binary_exists: false, ..good() }, "The login screen program plasmalogin is missing."),
            (DmFacts { pam_exists: false, ..good() }, "The sign-in settings file (PAM) “x” is missing."),
            (DmFacts { user_ok: false, ..good() }, "The system user “x” that the login screen runs as doesn't exist."),
            (DmFacts { x_sessions: 0, wayland_sessions: 0, ..good() }, "No desktop sessions are installed — there would be nothing to sign in to."),
            (DmFacts { needs_xorg: true, xorg: false, ..good() }, "This login screen needs Xorg, and Xorg isn't installed."),
        ];
        for (facts, want) in cases {
            assert_eq!(reason(ready("plasmalogin", &facts)), want);
        }
        // No user to check — not a blocker; Xorg not needed — not a blocker.
        assert_eq!(ready("plasmalogin", &DmFacts { user: None, user_ok: false, ..good() }), DmReadiness::Ready);
        assert_eq!(ready("plasmalogin", &DmFacts { xorg: false, ..good() }), DmReadiness::Ready);
        // Only Wayland sessions — there is a way to log in.
        assert_eq!(ready("plasmalogin", &DmFacts { x_sessions: 0, ..good() }), DmReadiness::Ready);
    }

    #[test]
    fn unsupported_backends_and_listed_only_dms() {
        for backend in [
            DmSwitchBackend::Unsupported("OpenRC".into()),
            DmSwitchBackend::NixosDeclarative,
            DmSwitchBackend::SystemdSuseAlternatives,
        ] {
            assert_eq!(
                dm_readiness(login_manager_spec("sddm"), &backend, &good()),
                DmReadiness::Unsupported,
                "{backend:?}"
            );
        }
        assert_eq!(
            dm_readiness(login_manager_spec("sddm"), &DmSwitchBackend::SystemdDebian, &good()),
            DmReadiness::Ready
        );
        // ly (a template unit), lxdm and unknown ones — only in the list.
        assert_eq!(ready("ly", &good()), DmReadiness::Unsupported);
        assert_eq!(ready("lxdm", &good()), DmReadiness::Unsupported);
        assert_eq!(dm_readiness(None, &DmSwitchBackend::Systemd, &good()), DmReadiness::Unsupported);
    }

    #[test]
    fn users_and_sysusers() {
        let passwd = "root:x:0:0::/root:/bin/bash\nsddm:x:964:964:SDDM:/var/lib/sddm:/usr/bin/nologin\n";
        assert!(passwd_has_user(passwd, "sddm"));
        assert!(!passwd_has_user(passwd, "sdd"));
        assert!(sysusers_declares("u greeter - \"greetd greeter user\" - /bin/bash\nm greeter video\n", "greeter"));
        assert!(sysusers_declares("u! lightdm - \"Light Display Manager\" /var/lib/lightdm\n", "lightdm"));
        assert!(!sysusers_declares("m greeter video\ng greeter -\n", "greeter"));
    }

    #[test]
    fn active_without_systemd_from_init_files() {
        assert_eq!(dm_from_init_files(Some("# x\nDISPLAYMANAGER=\"lightdm\"\n"), None).as_deref(), Some("lightdm"));
        assert_eq!(dm_from_init_files(None, Some("/usr/sbin/gdm3\n")).as_deref(), Some("gdm3"));
        assert_eq!(dm_from_init_files(None, Some("/usr/bin/plasmalogin\n")).as_deref(), Some("plasmalogin"));
        assert_eq!(dm_from_init_files(Some("DISPLAYMANAGER=\"\"\n"), None), None);
        assert_eq!(dm_from_init_files(None, None), None);
    }
}

// ============================================================================
// Item 6. The helper's switch-dm operation and the rollback service
// ============================================================================

mod switch_dm {
    use super::i18n_pin;
    use rexon_core::system::PkgFamily;
    use rexon_core::system_plan::{
        dm_fallback_unit, dm_rollback_command, dm_switch_units, pkg_helper_script, switch_dm_cmd,
        DmSwitchBackend, DM_FALLBACK_DIR, DM_FALLBACK_DROPIN, DM_FALLBACK_SCRIPT,
        DM_FALLBACK_SCRIPT_PATH, DM_FALLBACK_SERVICE, LIGHTDM_GREETER_CONF, PKG_HELPER_OPS,
        PREVIOUS_DM_FILE,
    };

    /// The helper's switch-dm branch WITHOUT the embedded texts of the rollback service.
    fn switch_branch(script: &str) -> String {
        let start = script.find("if [ \"$op\" = \"switch-dm\" ]").expect("ветка switch-dm");
        let end = start + script[start..].find("# install / remove").unwrap();
        let mut body = script[start..end].to_string();
        for (open, close) in [
            ("<<'REXON_DM_FALLBACK'\n", "\nREXON_DM_FALLBACK\n"),
            ("<<'REXON_DM_FALLBACK_UNIT'\n", "\nREXON_DM_FALLBACK_UNIT\n"),
        ] {
            let a = body.find(open).expect(open) + open.len();
            let b = a + body[a..].find(close).expect(close);
            body.replace_range(a..b, "");
        }
        body
    }

    #[test]
    fn helper_knows_switch_dm_and_has_no_eval() {
        let _lang = i18n_pin::en();
        let s = pkg_helper_script();
        assert!(PKG_HELPER_OPS.contains(&"switch-dm"));
        assert!(s.contains("install|remove|update|enable-dm|switch-dm|build-deps|install-built) : ;;"));
        // `eval` is mentioned only in the header comments — nowhere as a command.
        for line in s.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
            assert!(
                !(line.starts_with("eval ") || line.contains("; eval ") || line.contains("$(eval")),
                "eval в помощнике запрещён: {line}"
            );
        }
        // The embedded texts are in place and complete.
        assert!(s.contains(DM_FALLBACK_SCRIPT.trim_end()));
        assert!(s.contains(dm_fallback_unit().trim_end()));
        for path in [DM_FALLBACK_SCRIPT_PATH, PREVIOUS_DM_FILE, LIGHTDM_GREETER_CONF, DM_FALLBACK_DIR] {
            assert!(s.contains(path), "{path}");
        }
        assert!(!s.contains("__"), "незаполненная метка шаблона");
    }

    #[test]
    fn switch_branch_never_starts_or_stops_services() {
        let _lang = i18n_pin::en();
        let body = switch_branch(&pkg_helper_script());
        for banned in [
            "--now", "systemctl start", "systemctl stop", "systemctl restart", "try-restart",
            "isolate", "systemctl reload", "systemctl kill", "daemon-reload", "systemctl reboot",
            "eval ",
        ] {
            assert!(!body.contains(banned), "в switch-dm нашлось «{banned}»");
        }
        assert!(body.contains("systemctl enable --force \"$new.service\""));
        // "reboot" only in the hint text for a person, not as a command.
        for line in body.lines().map(str::trim) {
            assert!(!line.starts_with("reboot") && !line.starts_with("shutdown"), "{line}");
        }
        // On Debian the package script cannot start the service.
        assert!(body.contains("/usr/sbin/policy-rc.d"));
    }

    #[test]
    fn switch_branch_allowlist_is_switchable_units() {
        let _lang = i18n_pin::en();
        let s = pkg_helper_script();
        let units = dm_switch_units();
        assert_eq!(units, vec!["plasmalogin", "sddm", "gdm", "gdm3", "lightdm", "greetd", "cosmic-greeter", "ddm"]);
        assert!(s.contains(&format!("        {}) : ;;\n        *) die bad_dm \"$new\" ;;", units.join("|"))));
    }

    #[test]
    fn fallback_script_runs_once_and_only_on_boot_failure() {
        let f = DM_FALLBACK_SCRIPT;
        assert!(f.starts_with("#!/bin/bash\n"));
        // The only start is the login screen itself, without waiting.
        assert_eq!(f.matches("systemctl start").count(), 1);
        assert!(f.contains("systemctl start --no-block display-manager.service"));
        for banned in ["--now", "isolate", "reboot", "eval", "source ", ". /"] {
            assert!(!f.contains(banned), "{banned}");
        }
        // The "already rolled back" mark and removal of the drop-in — protection against a loop.
        assert!(f.contains("[ ! -e \"$stamp\" ] || exit 0"));
        assert!(f.find("date > \"$stamp\"").unwrap() < f.find("systemctl enable --force").unwrap());
        assert!(f.contains(&format!("{}/dm-fallback.done", DM_FALLBACK_DIR)));
        assert!(f.contains(&format!("$new.service.d/{DM_FALLBACK_DROPIN}")));
        assert!(f.contains(LIGHTDM_GREETER_CONF));
        assert!(f.contains("stopping) exit 0"), "при выключении не срабатывает");
        assert!(dm_fallback_unit().contains(&format!("ExecStart={DM_FALLBACK_SCRIPT_PATH}\n")));
        assert!(dm_fallback_unit().contains("\nStartLimitIntervalSec=0\n"));
        assert!(DM_FALLBACK_SERVICE.ends_with(".service"));
    }

    #[test]
    fn switch_dm_command_allowlist() {
        let ok = |u: &str, g: Option<&str>| switch_dm_cmd(PkgFamily::Arch, u, g, true);
        let cmd = ok("sddm", None).unwrap();
        assert_eq!(cmd.first().map(String::as_str), Some("pkexec"));
        assert_eq!(&cmd[cmd.len() - 3..], ["arch", "switch-dm", "sddm"]);
        let cmd = ok("lightdm", Some("lightdm-slick-greeter")).unwrap();
        assert_eq!(&cmd[cmd.len() - 2..], ["lightdm", "lightdm-slick-greeter"]);
        for (u, g) in [
            ("sddm", Some("lightdm-gtk-greeter")),
            ("lightdm", Some("../../x")),
            ("lightdm", Some("-x")),
            ("lightdm", Some("")),
            ("ly", None),
            ("lxdm", None),
            ("evil;rm", None),
            ("../sddm", None),
            ("", None),
        ] {
            assert_eq!(ok(u, g), None, "{u} {g:?}");
        }
        assert_eq!(switch_dm_cmd(PkgFamily::Arch, "sddm", None, false), None, "без помощника — нет сырого pkexec");
        assert_eq!(switch_dm_cmd(PkgFamily::Nix, "sddm", None, true), None);
    }

    #[test]
    fn rollback_command_matches_helper_text() {
        let _lang = i18n_pin::en();
        let s = pkg_helper_script();
        assert_eq!(
            dm_rollback_command("plasmalogin", &DmSwitchBackend::Systemd).unwrap(),
            "sudo systemctl enable --force plasmalogin.service && sudo reboot"
        );
        assert!(s.contains("back=\"sudo systemctl enable --force $old.service && sudo reboot\""));
        assert_eq!(
            dm_rollback_command("gdm", &DmSwitchBackend::SystemdDebian).unwrap(),
            "sudo dpkg-reconfigure gdm3 && sudo systemctl enable --force gdm.service && sudo reboot"
        );
        assert!(s.contains("back=\"sudo dpkg-reconfigure $old_pkg && sudo systemctl enable --force $old.service && sudo reboot\""));
        assert!(s.contains("            gdm) printf '%s' gdm3 ;;"));
        assert_eq!(dm_rollback_command("x; rm", &DmSwitchBackend::Systemd), None);
        assert_eq!(dm_rollback_command("sddm", &DmSwitchBackend::NixosDeclarative), None);
    }
}

// ============================================================================
// Item 7. Texts: confirmation, how to go back, results
// ============================================================================

mod texts {
    use super::i18n_pin;
    use rexon_core::system_plan::{
        login_manager_needs_helper, login_manager_rollback_hint, login_manager_switch_confirm,
        login_manager_switch_title, login_manager_unsupported_note, switch_dm_outcome, DmGreeter,
        DmSwitchBackend,
    };

    #[test]
    fn confirm_names_greeter_restart_and_way_back() {
        let _lang = i18n_pin::en();
        let slick = DmGreeter { id: "lightdm-slick-greeter".into(), name: "Slick Greeter".into(), exec: "slick-greeter".into() };
        assert_eq!(login_manager_switch_title("lightdm"), "Make LightDM the login screen?");
        assert_eq!(
            login_manager_switch_confirm(Some("plasmalogin"), "lightdm", Some(&slick), &DmSwitchBackend::Systemd),
            "LightDM will appear after a restart. Your current session won't be interrupted.\n\n\
             Greeter: Slick Greeter.\n\n\
             If LightDM doesn't start, Rexon brings back Plasma Login Manager by itself on that boot.\n\n\
             If there's no login screen after the restart: press Ctrl+Alt+F3, sign in with your name and password and run:\n\
             sudo systemctl enable --force plasmalogin.service && sudo reboot\n\
             The same is written in /etc/rexon/previous-display-manager."
        );
        // The previous one is unknown — neither rollback nor hint.
        assert_eq!(
            login_manager_switch_confirm(None, "sddm", None, &DmSwitchBackend::Systemd),
            "SDDM will appear after a restart. Your current session won't be interrupted."
        );
        assert!(login_manager_rollback_hint("sddm", &DmSwitchBackend::NixosDeclarative).is_none());
    }

    #[test]
    fn outcome_from_helper() {
        let _lang = i18n_pin::en();
        assert_eq!(
            switch_dm_outcome(Some(0), "rexon-pkg-helper: lightdm is now the login screen\n", "", "lightdm", "x"),
            (true, "Done: LightDM will be the login screen after a restart.".to_string())
        );
        assert_eq!(
            switch_dm_outcome(Some(2), "", "rexon-pkg-helper: the greeter slick-greeter is not installed\n", "lightdm", "x"),
            (false, "Couldn't change the login screen: the greeter slick-greeter is not installed".to_string())
        );
        let (ok, text) = switch_dm_outcome(Some(126), "", "", "lightdm", "pkexec x");
        // No terminal command for the person (27.11.2026).
        assert!(!ok && !text.contains("pkexec x"), "{text}");
        assert_eq!(
            switch_dm_outcome(Some(1), "", "", "sddm", "x"),
            (false, "Couldn't change the login screen.".to_string())
        );
        assert!(login_manager_needs_helper().contains("Rexon helper"));
    }

    #[test]
    fn notes_instead_of_button() {
        let _lang = i18n_pin::en();
        assert_eq!(login_manager_unsupported_note(&DmSwitchBackend::Systemd, None), None);
        assert_eq!(login_manager_unsupported_note(&DmSwitchBackend::SystemdDebian, None), None);
        assert_eq!(
            login_manager_unsupported_note(&DmSwitchBackend::Unsupported("OpenRC".into()), Some("sudo rc-update add display-manager default")).unwrap(),
            // No terminal command for the person (27.11.2026).
            "Here the login screen is switched by OpenRC, and Rexon can't do it yet."
        );
        assert_eq!(
            login_manager_unsupported_note(&DmSwitchBackend::Unsupported(String::new()), None).unwrap(),
            "Here the login screen is switched by its own init system, and Rexon can't do it yet."
        );
        assert!(login_manager_unsupported_note(&DmSwitchBackend::NixosDeclarative, None).unwrap().contains("configuration.nix"));
        assert!(login_manager_unsupported_note(&DmSwitchBackend::SystemdSuseAlternatives, None).unwrap().contains("openSUSE"));
    }
}

// ============================================================================
// Item 6. Which login screen each desktop needs, DDM and the section's warnings
// ============================================================================

mod desktop_needs {
    use super::i18n_pin;
    use rexon_core::system::PkgFamily;
    use rexon_core::system_exec::{login_screen_view, LoginScreenFacts};
    use rexon_core::system_plan::{
        desktop_dm_harm, desktop_dm_need, dm_compat_issues, dm_readiness, dm_warnings,
        login_manager_spec, login_manager_switch_confirm, sysusers_declares, unit_exec_start,
        unit_values, DmFacts, DmHarm, DmNeed, DmReadiness, DmSwitchBackend, LoginManagerInfo,
        DESKTOP_DM_RULES,
    };

    /// Unit and sysusers of the ddm 0.4.0 package from Wehrwolfmann's machine (15.11.2026).
    const UNIT_DDM: &str = include_str!("data/display_managers/ddm.service");
    const SYSUSERS_DDM: &str = include_str!("data/display_managers/ddm-sysusers.conf");

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn table_gnome_gdm_deepin_ddm_rest_any() {
        assert_eq!(desktop_dm_need("gnome"), DmNeed::Required(&["gdm", "gdm3"]));
        assert_eq!(desktop_dm_need("deepin"), DmNeed::Required(&["ddm"]));
        for de in ["kde", "cinnamon-mint", "mate", "xfce"] {
            assert_eq!(desktop_dm_need(de), DmNeed::Avoid(&["gdm", "gdm3"]), "{de}");
        }
        for de in ["hyprland", "sway", "niri", "lxqt", "budgie", "cosmic", "nothing"] {
            assert_eq!(desktop_dm_need(de), DmNeed::Any, "{de}");
        }
        // Required — critical under any other one, under its own — nothing.
        assert_eq!(desktop_dm_harm("gnome", "plasmalogin"), Some(DmHarm::Critical));
        assert_eq!(desktop_dm_harm("gnome", "gdm3"), None);
        assert_eq!(desktop_dm_harm("deepin", "sddm"), Some(DmHarm::Critical));
        assert_eq!(desktop_dm_harm("deepin", "ddm"), None);
        assert_eq!(desktop_dm_harm("kde", "gdm"), Some(DmHarm::Minor));
        assert_eq!(desktop_dm_harm("kde", "ddm"), None);
        assert_eq!(desktop_dm_harm("sway", "gdm"), None);
        // Every rule has a marker program and a reason text with the name of
        // the login screen; the desktop is in the core's database.
        let known = rexon_core::system::list_de_ids();
        for r in DESKTOP_DM_RULES {
            assert!(!r.binaries.is_empty() && r.harm_text.contains("%1"), "{}", r.de_id);
            assert!(known.contains(&r.de_id), "{} нет в базе столов", r.de_id);
        }
        // Deepin is recognized by Treeland itself: ordinary Deepin on X11 does not need DDM.
        let deepin = DESKTOP_DM_RULES.iter().find(|r| r.de_id == "deepin").unwrap();
        assert_eq!(deepin.binaries, &["treeland"]);
    }

    #[test]
    fn compat_issues_critical_first_with_reasons() {
        let _lang = i18n_pin::en();
        let all = ids(&["kde", "gnome", "deepin", "hyprland"]);
        let plm = dm_compat_issues("plasmalogin", &all);
        assert_eq!(plm.iter().map(|i| i.de_id.as_str()).collect::<Vec<_>>(), ["gnome", "deepin"]);
        assert!(plm.iter().all(|i| i.harm == DmHarm::Critical));
        assert_eq!(
            plm[0].text,
            "With Plasma Login Manager as the login screen, GNOME can't lock the screen: Super+L, \
             locking when idle and before sleep do nothing. GNOME needs GDM."
        );
        assert!(
            plm[1].text.starts_with("With Plasma Login Manager as the login screen, Deepin doesn't start"),
            "{}",
            plm[1].text
        );
        // Under GDM: Deepin does not start (critical — first), Plasma loses minor things.
        let gdm = dm_compat_issues("gdm", &all);
        assert_eq!(
            gdm.iter().map(|i| (i.de_id.as_str(), i.harm)).collect::<Vec<_>>(),
            [("deepin", DmHarm::Critical), ("kde", DmHarm::Minor)]
        );
        assert!(gdm[1].text.contains("KWallet"), "{}", gdm[1].text);
        // Under DDM — only GNOME; desktops that are not installed do not count.
        let ddm = dm_compat_issues("ddm", &all);
        assert_eq!(ddm.iter().map(|i| i.de_id.as_str()).collect::<Vec<_>>(), ["gnome"]);
        assert!(ddm[0].text.starts_with("With DDM as the login screen"), "{}", ddm[0].text);
        assert!(dm_compat_issues("sddm", &ids(&["kde", "mate", "hyprland"])).is_empty());
        assert!(dm_compat_issues("", &all).is_empty());
    }

    #[test]
    fn ddm_spec_from_package_files() {
        let _lang = i18n_pin::en();
        let spec = login_manager_spec("ddm").expect("DDM в таблице");
        assert_eq!(
            (spec.label, spec.binary, spec.package, spec.user, spec.pam),
            ("DDM", "ddm", "ddm", Some("dde"), "ddm")
        );
        assert!(spec.switchable && !spec.needs_xorg);
        // Check against the package files: program, seat service, user.
        assert_eq!(unit_exec_start(UNIT_DDM).unwrap(), vec!["/usr/bin/ddm"]);
        assert_eq!(unit_values(UNIT_DDM, "Unit", "Requires"), vec!["dde-seatd.service"]);
        assert_eq!(unit_values(UNIT_DDM, "Install", "Alias"), vec!["display-manager.service"]);
        assert!(sysusers_declares(SYSUSERS_DDM, "dde"));
        // Ready, but with an honest note «not tested with other desktops».
        let good = DmFacts {
            unit: "ddm".into(),
            binary_exists: true,
            user: Some("dde".into()),
            user_ok: true,
            pam: "ddm".into(),
            pam_exists: true,
            wayland_sessions: 2,
            ..Default::default()
        };
        assert_eq!(dm_readiness(Some(spec), &DmSwitchBackend::Systemd, &good), DmReadiness::Ready);
        let warnings = dm_warnings(&good);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("hasn't checked yet whether it starts other desktops"), "{warnings:?}");
        // No seat service — a refusal before the password, with its name.
        let no_seatd = DmFacts { missing_units: vec!["dde-seatd.service".into()], ..good.clone() };
        assert_eq!(
            dm_readiness(Some(spec), &DmSwitchBackend::Systemd, &no_seatd),
            DmReadiness::Blocked(
                "The login screen needs the system service dde-seatd.service, and it's missing or switched off."
                    .into()
            )
        );
        // The confirmation window says the same before the password.
        let confirm = login_manager_switch_confirm(Some("plasmalogin"), "ddm", None, &DmSwitchBackend::Systemd);
        assert!(confirm.starts_with("DDM will appear after a restart."), "{confirm}");
        assert!(confirm.contains("hasn't checked yet"), "{confirm}");
        // Other login screens do not get the note.
        assert!(dm_warnings(&DmFacts { unit: "sddm".into(), ..good }).is_empty());
    }

    fn info(unit: &str, label: &str, active: bool) -> LoginManagerInfo {
        LoginManagerInfo {
            unit: unit.into(),
            label: label.into(),
            binary: Some(format!("/usr/bin/{unit}")),
            present: true,
            active,
            readiness: DmReadiness::Ready,
            warnings: Vec::new(),
        }
    }

    fn facts(desktops: &[&str]) -> LoginScreenFacts {
        LoginScreenFacts {
            family: PkgFamily::Arch,
            helper: true,
            immutable: false,
            running: vec!["plasmalogin".into()],
            previous: None,
            desktops: ids(desktops),
            autologin_notes: Vec::new(),
        }
    }

    #[test]
    fn section_warns_about_installed_desktops() {
        let _lang = i18n_pin::en();
        let list = vec![
            info("plasmalogin", "Plasma Login Manager", true),
            info("gdm", "GDM", false),
            info("ddm", "DDM", false),
        ];
        let view = login_screen_view(&list, &DmSwitchBackend::Systemd, &facts(&["kde", "gnome", "deepin"]));
        // The primary one — as a common line of the section: GNOME without locking, Deepin does not start.
        assert_eq!(view.compat.len(), 2, "{:?}", view.compat);
        assert!(view.compat[0].contains("GNOME can't lock the screen"), "{:?}", view.compat);
        assert!(view.compat[1].contains("Deepin doesn't start"), "{:?}", view.compat);
        let row = |u: &str| view.managers.iter().find(|m| m.unit == u).unwrap();
        assert!(row("plasmalogin").warnings.is_empty(), "у основного — только общая строка");
        // GDM: it fixes GNOME, Deepin is broken anyway — only Plasma is added.
        let gdm = row("gdm");
        assert_eq!(gdm.warnings.len(), 1, "{:?}", gdm.warnings);
        assert!(gdm.warnings[0].contains("KWallet"));
        assert!(gdm.confirm.contains("KWallet"), "подтверждение говорит то же до пароля: {}", gdm.confirm);
        // DDM: the button is there, no new harm (GNOME is already without locking).
        assert!(row("ddm").can_switch);
        assert!(row("ddm").warnings.is_empty());
        // Only Plasma — silent for the primary one and for DDM, KWallet for GDM.
        let quiet = login_screen_view(&list, &DmSwitchBackend::Systemd, &facts(&["kde"]));
        assert!(quiet.compat.is_empty());
        let warns = |u: &str| quiet.managers.iter().find(|m| m.unit == u).unwrap().warnings.len();
        assert_eq!((warns("plasmalogin"), warns("ddm"), warns("gdm")), (0, 0, 1));
    }
}
