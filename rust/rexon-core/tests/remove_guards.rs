//! Защиты снятия пакетов (28.10.2026): удаление стола не трогает текущий
//! стол и его экран входа, вечное не снимается ни одним путём, помощник
//! пакетов от root сам держит обе границы и ставит только из таблиц ядра.

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

/// Сидишь в Xfce — удаление Cinnamon не трогает lightdm и приветствие.
#[test]
fn удаление_cinnamon_из_xfce_не_трогает_lightdm() {
    let arch = PkgFamily::Arch;
    let set = removal(arch, "cinnamon-mint", "xfce", Some("lightdm"));
    assert!(!set.is_empty());
    assert!(set.iter().all(|p| p != "lightdm" && !system::is_lightdm_greeter(p)), "{set:?}");
    assert!(set.iter().any(|p| p.starts_with("cinnamon")), "{set:?}");
    // и без ответа о экране входа — lightdm в таблице Xfce
    let set = removal(arch, "cinnamon-mint", "xfce", None);
    assert!(!set.iter().any(|p| p == "lightdm"), "{set:?}");
}

/// Любая пара столов любого семейства: ни один пакет текущего стола не
/// попадает в набор удаления другого.
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

/// Активный экран входа держится, даже когда текущий стол неизвестен:
/// юнит и его пакет (plasmalogin → plasma-login-manager, на Debian gdm → gdm3).
#[test]
fn активный_экран_входа_не_снимается() {
    let all = s(&["plasma-login-manager", "sddm", "gdm", "gdm3", "foo"]);
    let keep = system::current_desktop_keep(PkgFamily::Arch, "unknown", Some("plasmalogin"));
    let left = system::without_current_desktop(PkgFamily::Arch, &all, &keep, &|_| None);
    assert_eq!(left, s(&["sddm", "gdm", "gdm3", "foo"]));
    let keep = system::current_desktop_keep(PkgFamily::Debian, "", Some("gdm"));
    let left = system::without_current_desktop(PkgFamily::Debian, &all, &keep, &|_| None);
    assert_eq!(left, s(&["plasma-login-manager", "sddm", "foo"]));
    // Gentoo: юнит lightdm держит атом x11-misc/lightdm и приветствие
    let all = s(&["x11-misc/lightdm", "x11-misc/lightdm-gtk-greeter", "gnome-extra/nemo"]);
    let keep = system::current_desktop_keep(PkgFamily::Gentoo, "", Some("lightdm"));
    let left = system::without_current_desktop(PkgFamily::Gentoo, &all, &keep, &|_| None);
    assert_eq!(left, s(&["gnome-extra/nemo"]));
}

/// Группы Arch раскрываются с обеих сторон: член группы текущего стола
/// остаётся, даже если удаляемый стол называет его по имени.
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

// ── Помощник пакетов в песочнице: подставные pacman, loginctl, systemctl ──

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

    /// Машина стенда: Arch, экран входа LightDM (ссылка display-manager на
    /// юнит, владелец юнита — `FAKE_OWNER`), один графический сеанс с
    /// `Desktop=$FAKE_DESKTOP`; группа `xfce4` = `xfce4-panel thunar`.
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

/// Вечное по семействам: ядра, драйверы, прошивки, микрокод — отказ кодом 2
/// и ни одного вызова пакетного менеджера; обычный пакет снимается.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
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

/// Текущий стол помощник знает сам: экран входа по ссылке
/// display-manager.service (имя юнита, владелец файла, приветствия LightDM)
/// и стол графического сеанса по `Desktop=` logind (группы раскрыты).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
fn помощник_не_снимает_текущий_стол_и_экран_входа() {
    let st = helper::stand("current");
    let xfce = [("FAKE_DESKTOP", "XFCE"), ("FAKE_OWNER", "lightdm-owner")];
    for held in ["lightdm", "lightdm-owner", "lightdm-slick-greeter", "xfce4-panel", "xfce4-goodies"] {
        let (code, out, err) = st.run(&["arch", "remove", held], &xfce);
        assert_eq!(code, 2, "{held} снят: {out}");
        assert!(err.contains(held), "{err}");
        assert!(!out.contains("STUB pacman"), "{held}: {out}");
    }
    // Чужой стол (Cinnamon) снимается; сеанс приветствия (Class greeter,
    // Desktop cinnamon) не в счёт.
    let (code, out, err) = st.run(&["arch", "remove", "cinnamon", "nemo"], &xfce);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("STUB pacman -Rns --noconfirm cinnamon nemo"), "{out}");
    // Сеанс Cinnamon: токен X-Cinnamon узнаётся той же таблицей, что у ядра.
    let (code, _, _) = st.run(&["arch", "remove", "cinnamon"], &[("FAKE_DESKTOP", "X-Cinnamon")]);
    assert_eq!(code, 2);
    // Не графический сеанс стол не держит.
    let (code, _, _) = st.run(&["arch", "remove", "xfce4-panel"], &[("FAKE_DESKTOP", "XFCE"), ("FAKE_TYPE", "tty")]);
    assert_eq!(code, 0);
}

/// Установка — только из таблиц ядра: чужое имя отвергнуто без вызова
/// pacman, имена каждой таблицы проходят.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужен системный bash")]
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

/// Белый список установки помощника держит каждую таблицу, по которой
/// живые места ставят пакеты через помощник: убрать таблицу из списка —
/// значит сломать честную установку (помощник откажет).
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
    // Вечное — правило ядра: точные имена и семейства.
    for v in ["linux-cachyos", "nvidia-utils", "mesa", "amd-ucode", "linux-image-amd64", "sys-kernel/gentoo-kernel"] {
        assert!(plan::is_vital_package(v), "{v}");
    }
    for ok in ["lightdm", "xfce4-panel", "polkit-gnome", "mpv"] {
        assert!(!plan::is_vital_package(ok), "{ok}");
    }
}
