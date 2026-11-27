//! Package removal protections (28.10.2026): removing a desktop does not touch the current
//! desktop and its login screen, the permanent things are not removed by any path, the root
//! package helper holds both boundaries itself and installs only from the core's tables.

use rexon_core::system::{self, PkgFamily};

fn s(list: &[&str]) -> Vec<String> {
    list.iter().map(|x| x.to_string()).collect()
}

const FAMILIES: &[PkgFamily] = &[
    PkgFamily::Arch, PkgFamily::Debian, PkgFamily::Fedora, PkgFamily::Suse,
    PkgFamily::Gentoo, PkgFamily::Alpine, PkgFamily::Void, PkgFamily::Solus,
    PkgFamily::Clear, PkgFamily::Alt,
];

fn removal(family: PkgFamily, de: &str, current: &str, dm: Option<&str>) -> Vec<String> {
    let keep = system::current_desktop_keep(family, current, dm);
    system::without_current_desktop(family, &system::desktop_packages(de, family), &keep, &|_| None)
}

/// Sitting in Xfce — removing Cinnamon does not touch lightdm and the greeter.
#[test]
fn удаление_cinnamon_из_xfce_не_трогает_lightdm() {
    let arch = PkgFamily::Arch;
    let set = removal(arch, "cinnamon-mint", "xfce", Some("lightdm"));
    assert!(!set.is_empty());
    assert!(set.iter().all(|p| p != "lightdm" && !system::is_lightdm_greeter(p)), "{set:?}");
    assert!(set.iter().any(|p| p.starts_with("cinnamon")), "{set:?}");
    // and without an answer about the login screen — lightdm is in the Xfce table
    let set = removal(arch, "cinnamon-mint", "xfce", None);
    assert!(!set.iter().any(|p| p == "lightdm"), "{set:?}");
}

/// Any pair of desktops of any family: not a single package of the current desktop
/// gets into the removal set of the other one.
#[test]
fn ни_одна_пара_столов_не_снимает_пакеты_текущего() {
    let base = |p: &str| p.rsplit('/').next().unwrap_or(p).to_string();
    let ids = system::list_de_ids();
    for fam in FAMILIES {
        for cur in &ids {
            let cur_pkgs: Vec<String> =
                system::desktop_packages(cur, *fam).iter().map(|p| base(p)).collect();
            for other in ids.iter().filter(|o| *o != cur) {
                for p in removal(*fam, other, cur, None) {
                    assert!(
                        !cur_pkgs.contains(&base(&p)),
                        "{}: удаление {other} из {cur} снимает {p}",
                        fam.as_str()
                    );
                }
            }
        }
    }
}

/// The active login screen is kept even when the current desktop is unknown:
/// the unit and its package (plasmalogin → plasma-login-manager, on Debian gdm → gdm3).
#[test]
fn активный_экран_входа_не_снимается() {
    let all = s(&["plasma-login-manager", "sddm", "gdm", "gdm3", "foo"]);
    let keep = system::current_desktop_keep(PkgFamily::Arch, "unknown", Some("plasmalogin"));
    let left = system::without_current_desktop(PkgFamily::Arch, &all, &keep, &|_| None);
    assert_eq!(left, s(&["sddm", "gdm", "gdm3", "foo"]));
    let keep = system::current_desktop_keep(PkgFamily::Debian, "", Some("gdm"));
    let left = system::without_current_desktop(PkgFamily::Debian, &all, &keep, &|_| None);
    assert_eq!(left, s(&["plasma-login-manager", "sddm", "foo"]));
    // Gentoo: the lightdm unit keeps the x11-misc/lightdm atom and the greeter
    let all = s(&["x11-misc/lightdm", "x11-misc/lightdm-gtk-greeter", "gnome-extra/nemo"]);
    let keep = system::current_desktop_keep(PkgFamily::Gentoo, "", Some("lightdm"));
    let left = system::without_current_desktop(PkgFamily::Gentoo, &all, &keep, &|_| None);
    assert_eq!(left, s(&["gnome-extra/nemo"]));
}

/// Arch groups are expanded on both sides: a member of the current desktop's group
/// stays, even if the desktop being removed names it by name.
#[test]
fn член_группы_текущего_стола_остаётся() {
    let qg = |p: &str| -> Option<String> {
        match p {
            "xfce4" => Some("xfce4 xfce4-panel\nxfce4 thunar\n".into()),
            "other-group" => Some("other-group thunar\nother-group other-app\n".into()),
            _ => None,
        }
    };
    let keep = s(&["xfce4", "lightdm"]);
    let all = s(&["other-group", "xfce4-panel", "mine"]);
    let left = system::without_current_desktop(PkgFamily::Arch, &all, &keep, &qg);
    assert_eq!(left, s(&["other-app", "mine"]));
}

// ── The package helper in a sandbox: fake pacman, loginctl, systemctl ────

mod helper {
    use rexon_core::system_plan as plan;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn exec(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    pub struct Stand {
        root: PathBuf,
        script: PathBuf,
    }

    /// The bench machine: Arch, the LightDM login screen (the display-manager link to
    /// the unit, the unit's owner is `FAKE_OWNER`), one graphical session with
    /// `Desktop=$FAKE_DESKTOP`; the group `xfce4` = `xfce4-panel thunar`.
    pub fn stand(tag: &str) -> Stand {
        let root = std::env::temp_dir().join(format!("rexon-pkg-guards-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let stub = root.join("stub");
        std::fs::create_dir_all(&stub).unwrap();
        exec(
            &stub.join("pacman"),
            "#!/bin/bash\n\
             case \"$1\" in\n\
             -Qqo) [ -n \"${FAKE_OWNER:-}\" ] && echo \"$FAKE_OWNER\"; exit 0 ;;\n\
             -Qqg) [ \"$3\" = xfce4 ] && { echo xfce4-panel; echo thunar; exit 0; }; exit 1 ;;\n\
             esac\n\
             echo \"STUB pacman $*\"\nexit 0\n",
        );
        exec(
            &stub.join("loginctl"),
            "#!/bin/bash\n\
             case \"$1\" in\n\
             list-sessions) echo \"  7 1000 u seat0 tty2\"; echo \"  c1 120 lightdm seat0 tty7\" ;;\n\
             show-session) case \"$2|$4\" in\n\
                 7\\|Type) echo \"${FAKE_TYPE:-x11}\" ;;\n\
                 7\\|Class) echo user ;;\n\
                 7\\|Desktop) echo \"${FAKE_DESKTOP:-}\" ;;\n\
                 c1\\|Type) echo x11 ;;\n\
                 c1\\|Class) echo greeter ;;\n\
                 c1\\|Desktop) echo cinnamon ;;\n\
                 esac ;;\n\
             esac\nexit 0\n",
        );
        exec(&stub.join("systemctl"), "#!/bin/bash\necho disabled\nexit 1\n");
        let units = root.join("units");
        std::fs::create_dir_all(&units).unwrap();
        std::fs::write(units.join("lightdm.service"), "[Unit]\n").unwrap();
        let link = root.join("display-manager.service");
        symlink(units.join("lightdm.service"), &link).unwrap();
        let osr = root.join("os-release");
        std::fs::write(&osr, "ID=arch\n").unwrap();
        let mut text = plan::pkg_helper_script();
        for (from, to) in [
            ("export PATH=/usr/sbin:/usr/bin:/sbin:/bin", format!("export PATH={}:/usr/bin:/bin", stub.display())),
            ("for f in /etc/os-release /usr/lib/os-release; do", format!("for f in {}; do", osr.display())),
        ] {
            assert!(text.contains(from), "строка помощника изменилась: {from}");
            text = text.replacen(from, &to, 1);
        }
        text = text.replace(plan::DM_SYMLINK, &link.display().to_string());
        let script = root.join("rexon-pkg-helper");
        exec(&script, &text);
        Stand { root, script }
    }

    impl Stand {
        pub fn run(&self, args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
            let mut cmd = Command::new("bash");
            cmd.arg(&self.script).args(args).env_remove("PKEXEC_UID");
            for (k, v) in env {
                cmd.env(k, v);
            }
            let out = cmd.output().unwrap();
            (
                out.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&out.stdout).into_owned(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
            )
        }
    }

    impl Drop for Stand {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// Permanent things per family: kernels, drivers, firmware, microcode — a refusal with code 2
/// and not a single package manager call; an ordinary package is removed.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs the system bash")]
fn помощник_не_снимает_вечное_по_семействам() {
    let st = helper::stand("vital");
    for v in [
        "linux-cachyos", "linux-image-amd64", "nvidia-utils", "mesa", "amd-ucode",
        "vulkan-radeon", "sof-firmware", "mkinitcpio", "systemd-libs", "linux",
    ] {
        let (code, out, _) = st.run(&["arch", "remove", v], &[]);
        assert_eq!(code, 2, "{v} снят");
        assert!(!out.contains("STUB pacman"), "{v}: {out}");
    }
    let (code, out, err) = st.run(&["arch", "remove", "some-app"], &[]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("STUB pacman -Rns --noconfirm some-app"), "{out}");
}

/// The helper knows the current desktop by itself: the login screen by the
/// display-manager.service link (unit name, file owner, LightDM greeters)
/// and the graphical session's desktop by logind's `Desktop=` (groups expanded).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs the system bash")]
fn помощник_не_снимает_текущий_стол_и_экран_входа() {
    let st = helper::stand("current");
    let xfce = [("FAKE_DESKTOP", "XFCE"), ("FAKE_OWNER", "lightdm-owner")];
    for held in ["lightdm", "lightdm-owner", "lightdm-slick-greeter", "xfce4-panel", "xfce4-goodies"] {
        let (code, out, err) = st.run(&["arch", "remove", held], &xfce);
        assert_eq!(code, 2, "{held} снят: {out}");
        assert!(err.contains(held), "{err}");
        assert!(!out.contains("STUB pacman"), "{held}: {out}");
    }
    // Another desktop (Cinnamon) is removed; the greeter session (Class greeter,
    // Desktop cinnamon) does not count.
    let (code, out, err) = st.run(&["arch", "remove", "cinnamon", "nemo"], &xfce);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("STUB pacman -Rns --noconfirm cinnamon nemo"), "{out}");
    // A Cinnamon session: the X-Cinnamon token is recognized by the same table as the core's.
    let (code, _, _) = st.run(&["arch", "remove", "cinnamon"], &[("FAKE_DESKTOP", "X-Cinnamon")]);
    assert_eq!(code, 2);
    // A non-graphical session does not hold a desktop.
    let (code, _, _) = st.run(&["arch", "remove", "xfce4-panel"], &[("FAKE_DESKTOP", "XFCE"), ("FAKE_TYPE", "tty")]);
    assert_eq!(code, 0);
}

/// Installing — only from the core's tables: a foreign name is rejected without calling
/// pacman, the names of every table pass.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "needs the system bash")]
fn помощник_ставит_только_из_таблиц() {
    let st = helper::stand("install");
    let (code, out, _) = st.run(&["arch", "install", "evil-tool"], &[]);
    assert_eq!(code, 2);
    assert!(!out.contains("STUB pacman"), "{out}");
    let (code, out, _) = st.run(&["arch", "install", "mpv", "evil-tool"], &[]);
    assert_eq!(code, 2, "{out}");
    let (code, out, err) = st.run(&["arch", "install", "mpv", "yt-dlp", "sddm", "conky", "cinnamon"], &[]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("STUB pacman -S --needed --noconfirm mpv yt-dlp sddm conky cinnamon"), "{out}");
}

/// The helper's install allow-list holds every table by which
/// the live places install packages through the helper: removing a table from the list
/// means breaking an honest install (the helper will refuse).
#[test]
fn белый_список_покрывает_все_таблицы() {
    use rexon_core::system_plan as plan;
    for fam in FAMILIES {
        let f = fam.as_str();
        let allow = plan::pkg_helper_install_allowlist(*fam);
        let has = |p: &str| allow.iter().any(|a| a == p);
        let mut want: Vec<String> = Vec::new();
        for id in system::list_de_ids() {
            want.extend(system::desktop_packages(id, *fam));
        }
        want.extend(rexon_core::themes::known_set_packages(f).iter().map(|s| s.to_string()));
        want.extend(rexon_core::mate_panel::all_part_packages(f).iter().map(|s| s.to_string()));
        want.extend(rexon_core::shell_bundle::all_packages(*fam).iter().map(|s| s.to_string()));
        want.extend(rexon_core::utils::tool_packages_for(f).iter().map(|s| s.to_string()));
        want.extend(rexon_core::distro_looks::builtin_package_names(*fam));
        want.extend(rexon_core::compliance::FIX_PACKAGES.iter().map(|s| s.to_string()));
        want.extend(rexon_core::compliance::AYATANA_PACKAGES.iter().map(|s| s.to_string()));
        for session in ["wayland", "x11"] {
            want.extend(rexon_core::wallpaper_engine::install_deps_packages(f, session));
        }
        want.push(rexon_core::youtube_wallpaper::YTDLP_PACKAGE.to_string());
        if let Some(p) = rexon_core::de_apply::conky_package(f) {
            want.push(p.to_string());
        }
        for p in want.iter().filter(|p| plan::is_safe_pkg_arg(*fam, p)) {
            assert!(has(p), "{f}: {p} не в белом списке помощника");
        }
    }
    // Permanent things are a core rule: exact names and families.
    for v in ["linux-cachyos", "nvidia-utils", "mesa", "amd-ucode", "linux-image-amd64", "sys-kernel/gentoo-kernel"] {
        assert!(plan::is_vital_package(v), "{v}");
    }
    for ok in ["lightdm", "xfce4-panel", "polkit-gnome", "mpv"] {
        assert!(!plan::is_vital_package(ok), "{ok}");
    }
}
