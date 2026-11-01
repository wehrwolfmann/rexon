//! Исполнитель связки «композитор + оболочка» (шаг 3 поддержки оболочек).
//!
//! Решения — [`crate::shell_bundle`]; здесь только порядок шагов поверх того
//! же [`DeOpsIo`], что у установки столов. Пакеты идут ТЕМ ЖЕ путём, что у
//! всего продукта: `system_plan::install_plan` (именованный помощник
//! `rexon-pkg-helper`, один пароль на сеанс) и `pkg_remove` исполнителя
//! столов с его защитами. Своих команд пакетного менеджера здесь нет.
//!
//! ⭐ Обратимость: «Убрать» снимает ровно наш управляемый блок (общий
//! помощник `compositor_autostart::remove_text`) — файл возвращается
//! бит-в-бит, а заведённый нами удаляется. Сам композитор не трогается.

use super::*;
use crate::compositor_autostart::TextEdit;
use crate::shell_bundle as sb;
use crate::system::DesktopShell;

fn installing_packages(list: &str) -> String {
    // TRANSLATORS: %1 is a list of package names.
    trf("📦 Installing packages: %1", &[&list])
}

fn removing_packages(list: &str) -> String {
    // TRANSLATORS: %1 is a list of package names.
    trf("🗑️ Removing packages: %1", &[&list])
}

fn adding_autostart(shell: &str, wm: &str) -> String {
    // TRANSLATORS: %1 is a desktop shell, %2 a compositor (niri, sway…).
    trf("📝 Adding %1 to the startup of %2", &[&shell, &wm])
}

fn removing_autostart(shell: &str, wm: &str) -> String {
    // TRANSLATORS: %1 is a desktop shell, %2 a compositor.
    trf("📝 Taking %1 out of the startup of %2", &[&shell, &wm])
}

fn shell_installed(shell: &str, wm: &str) -> String {
    // TRANSLATORS: %1 is a desktop shell, %2 a compositor.
    trf("✅ %1 is set up for %2", &[&shell, &wm])
}

fn shell_removed(shell: &str, wm: &str) -> String {
    // TRANSLATORS: %1 is a desktop shell, %2 a compositor that stays installed.
    trf("✅ %1 is removed — %2 stays as it was", &[&shell, &wm])
}

fn config_write_failed(path: &str, why: &str) -> String {
    trf("Couldn't change the settings file %1: %2", &[&path, &why])
}

fn config_rejected(wm: &str) -> String {
    // TRANSLATORS: %1 is a compositor.
    trf(
        "%1 didn't accept its settings file after the change — Rexon put the file back as it was.",
        &[&wm],
    )
}

fn no_config(wm: &str) -> String {
    // TRANSLATORS: %1 is a compositor.
    trf(
        "%1 has no settings file yet. Log in to %1 once — it creates the file itself — and install the shell again.",
        &[&wm],
    )
}

fn config_refused(path: &str, why: &str) -> String {
    // TRANSLATORS: %1 is a settings file path, %2 the reason.
    trf("Couldn't add the startup line to %1: %2.", &[&path, &why])
}

fn shell_not_set_up(shell: &str, wm: &str) -> String {
    // TRANSLATORS: %1 is a desktop shell, %2 a compositor.
    trf("%1 isn't set up for %2 yet — install it first.", &[&shell, &wm])
}

fn marker_failed(why: &str) -> String {
    trf("Couldn't remember which shell to start: %1", &[&why])
}

fn running_shell_hint(shell: &str) -> String {
    // TRANSLATORS: %1 is a desktop shell.
    trf("%1 keeps running until you log out.", &[&shell])
}

impl<'a, IO: DeOpsIo + ?Sized> DeOpsExec<'a, IO> {
    fn state_home(&self) -> Option<String> {
        self.env_opt("XDG_STATE_HOME")
    }

    fn wm_name(wm: &str) -> &'static str {
        system::DE_META
            .iter()
            .find(|m| m.id == wm)
            .map(|m| m.display_name)
            .unwrap_or("")
    }

    /// Режим композитора `wm`: `Some(shell)` — входить с этой оболочкой
    /// (её отметка кладётся, отметки других снимаются), `None` — стол сам по
    /// себе (все отметки снимаются). Отсутствующую отметку не трогаем вовсе:
    /// у композитора без оболочек переключение не делает ни одного лишнего
    /// шага.
    pub(super) fn set_shell_mode(&self, wm: &str, shell: Option<DesktopShell>) -> Step<()> {
        if !sb::SHELL_WMS.contains(&wm) {
            return Ok(());
        }
        let home = self.home();
        let state = self.state_home();
        for s in DesktopShell::all() {
            let marker = sb::marker_path(wm, *s, &home, state.as_deref());
            if Some(*s) == shell {
                if self.io.is_file(&marker) {
                    continue;
                }
                let dir = sb::marker_dir(&home, state.as_deref());
                self.io
                    .make_dirs(&dir)
                    .and_then(|_| self.io.write_text(&marker, ""))
                    .map_err(|e| Stop::Failed(marker_failed(&e)))?;
            } else if self.io.is_file(&marker) {
                self.io.remove_file(&marker);
            }
        }
        Ok(())
    }

    fn wm_ready(&self, wm: &str) -> bool {
        let bins = system::de_installed_binaries(wm);
        !bins.is_empty() && bins.iter().all(|b| self.io.which(b))
    }

    /// Проверка конфига самим композитором (тем же argv, что у записи
    /// курсора). `None` — проверять нечем.
    fn wm_config_valid(&self, wm: &str, path: &str) -> Option<bool> {
        let cw = crate::themes::CursorWm::from_de(wm)?;
        let mut argv =
            crate::themes::wm_cursor_config_plan(cw, "default", None, &self.home(), None).validate;
        if !argv.first().is_some_and(|b| self.io.which(b)) {
            return None;
        }
        argv.push(path.to_string());
        Some(self.io.run(&argv, Some(30), true).rc() == Some(0))
    }

    /// Поставить недостающие пакеты оболочки — общим планом продукта.
    fn shell_pkg_install(&self, family: PkgFamily, packages: &[String]) -> Step<()> {
        let ip = plan::install_plan(family, packages, false, None, self.sys.pkg_helper_available())
            .map_err(Stop::Failed)?;
        if let Some((ok, msg)) = ip.early_result {
            return if ok { Ok(()) } else { Err(Stop::Failed(msg)) };
        }
        let (free, msg) = self.wait_pkg_lock(&self.io.pkg_lock_paths(family), 300)?;
        if !free {
            return Err(Stop::Failed(msg));
        }
        for step in &ip.steps {
            if let PlanStep::Run { id, cmd, .. } = step {
                self.point_of_no_return()?;
                // Установка из таблицы оболочек — сеанс прав окна; его нет
                // (или шаг не от root) — прежняя команда плана.
                let session = (*id == plan::RunStepId::PkgInstall
                    && cmd.argv.first().map(String::as_str) == Some("pkexec"))
                .then(|| PrivOp::PkgInstall { family, packages: packages.to_vec(), source: PkgSource::Shell })
                .and_then(|op| self.io.run_priv(&op, &[], None));
                let r = session.unwrap_or_else(|| self.io.run(&cmd.argv, cmd.timeout_secs, cmd.lang_c));
                if r.rc() == Some(0) {
                    continue;
                }
                let text = format!("{}{}", r.stdout, r.stderr);
                if r.returncode == Some(126) || text.contains("Not authorized") {
                    return Err(Stop::Failed(ip.sudo_hint.clone()));
                }
                return Err(Stop::Failed(run_why(&r)));
            }
        }
        Ok(())
    }

    /// Установка оболочки в композитор: пакеты (если их нет) и строка
    /// автозапуска управляемым блоком.
    pub fn shell_install(&self, wm: &str, shell: DesktopShell) -> DeOpOutcome {
        let result = self.shell_install_steps(wm, shell);
        self.finish(result)
    }

    fn shell_install_steps(&self, wm: &str, shell: DesktopShell) -> Step<String> {
        let name = Self::wm_name(wm);
        if name.is_empty() {
            return Err(Stop::Failed(unknown_de_message(wm)));
        }
        self.set_cancellable(true);
        // Отказы — до первого побочного действия и до пароля.
        if let Some(reason) = dc::SteamOsBlock::Desktops.on(self.on_steamos()) {
            return Err(Stop::Failed(reason));
        }
        if !self.wm_ready(wm) {
            return Err(Stop::Failed(sb::install_desktop_first(name)));
        }
        let family = self.sys.family();
        let packages = sb::packages(shell, wm, family)
            .map_err(|u| Stop::Failed(sb::unavailable_text(u, shell, name)))?;
        let home = self.home();
        let cfg = self.env_opt("XDG_CONFIG_HOME");
        let state = self.state_home();
        let read = |p: &str| self.io.read_text(p);
        let edit = sb::enable_plan(wm, shell, &home, cfg.as_deref(), state.as_deref(), &read);
        match &edit {
            sb::EnablePlan::NoConfig => return Err(Stop::Failed(no_config(name))),
            sb::EnablePlan::Refused { path, reason } => {
                return Err(Stop::Failed(config_refused(path, &reason.text())))
            }
            _ => {}
        }
        self.checkpoint()?;

        let missing: Vec<String> = packages
            .iter()
            .filter(|p| !self.sys.is_pkg_installed(p))
            .map(|p| p.to_string())
            .collect();
        if !missing.is_empty() {
            self.emit("install", &installing_packages(&missing.join(" ")), Some(0.10));
            self.shell_pkg_install(family, &missing)?;
        }
        self.set_cancellable(false);

        self.emit("config", &adding_autostart(shell.label(), name), Some(0.85));
        if let sb::EnablePlan::Write { path, content, created } = edit {
            let before = if created { None } else { self.io.read_text(&path) };
            let was_valid = match &before {
                Some(_) => self.wm_config_valid(wm, &path),
                None => None,
            };
            if let Some(dir) = std::path::Path::new(&path).parent().and_then(|d| d.to_str()) {
                self.io
                    .make_dirs(dir)
                    .map_err(|e| Stop::Failed(config_write_failed(&path, &e)))?;
            }
            self.io
                .write_text(&path, &content)
                .map_err(|e| Stop::Failed(config_write_failed(&path, &e)))?;
            // Конфиг был исправен (или заведён нами), а после правки
            // композитор его не принимает — вернуть как было.
            let must_hold = created || was_valid == Some(true);
            if must_hold && self.wm_config_valid(wm, &path) == Some(false) {
                match before {
                    Some(text) => {
                        let _ = self.io.write_text(&path, &text);
                    }
                    None => self.io.remove_file(&path),
                }
                return Err(Stop::Failed(config_rejected(name)));
            }
        }
        self.emit("done", &shell_installed(shell.label(), name), Some(1.0));
        Ok(de_progress::MSG_OK.to_string())
    }

    /// «Переключиться» у оболочки: отметка режима и вход в композитор с ней.
    pub fn shell_switch(&self, wm: &str, shell: DesktopShell, current_de: &str) -> DeOpOutcome {
        let result = self.shell_switch_steps(wm, shell, current_de);
        self.finish(result)
    }

    fn shell_switch_steps(&self, wm: &str, shell: DesktopShell, current_de: &str) -> Step<String> {
        let name = Self::wm_name(wm);
        if name.is_empty() {
            return Err(Stop::Failed(unknown_de_message(wm)));
        }
        let home = self.home();
        let cfg = self.env_opt("XDG_CONFIG_HOME");
        let read = |p: &str| self.io.read_text(p);
        if !self.io.which(sb::binary(shell))
            || !sb::block_installed(wm, shell, &home, cfg.as_deref(), &read)
        {
            return Err(Stop::Failed(shell_not_set_up(shell.label(), name)));
        }
        if current_de != wm {
            // Обычный путь смены стола (копии, приложения по умолчанию,
            // выход), но отметка кладётся, а не снимается.
            return self.switch_steps_with(wm, current_de, Some(shell));
        }
        // Мы уже в этом композиторе: перезайти в него с оболочкой.
        self.set_cancellable(true);
        self.point_of_no_return()?;
        self.set_shell_mode(wm, Some(shell))?;
        self.emit("done", &logout_message(), Some(1.0));
        self.logout_session()
    }

    /// «Убрать» у оболочки: её пакеты (если больше никому не нужны), отметку
    /// и ровно наш блок в конфиге. Композитор остаётся.
    pub fn shell_remove(&self, wm: &str, shell: DesktopShell, current_de: &str) -> DeOpOutcome {
        let result = self.shell_remove_steps(wm, shell, current_de);
        self.finish(result)
    }

    fn shell_remove_steps(&self, wm: &str, shell: DesktopShell, current_de: &str) -> Step<String> {
        let name = Self::wm_name(wm);
        if name.is_empty() {
            return Err(Stop::Failed(unknown_de_message(wm)));
        }
        if let Some(reason) = dc::SteamOsBlock::Desktops.on(self.on_steamos()) {
            return Err(Stop::Failed(reason));
        }
        self.set_cancellable(true);
        let family = self.sys.family();
        let home = self.home();
        let cfg = self.env_opt("XDG_CONFIG_HOME");
        let read = |p: &str| self.io.read_text(p);
        let edit = sb::disable_plan(wm, shell, &home, cfg.as_deref(), &read);
        // Кто ещё держит эту оболочку: тем её общие пакеты нужны дальше.
        let still: Vec<&str> = sb::SHELL_WMS
            .iter()
            .copied()
            .filter(|other| *other != wm)
            .filter(|other| sb::block_installed(other, shell, &home, cfg.as_deref(), &read))
            .collect();
        let packages: Vec<String> = sb::packages_to_remove(shell, wm, family, &still)
            .into_iter()
            .filter(|p| self.sys.is_pkg_installed(p))
            .map(str::to_string)
            .collect();
        self.checkpoint()?;

        // Пакеты — первыми: отказ в пароле не меняет ничего.
        if !packages.is_empty() {
            self.emit("remove", &removing_packages(&packages.join(" ")), Some(0.10));
            let (ok, message) = self.pkg_remove(family, &packages)?;
            if !ok {
                return Err(Stop::Failed(message));
            }
        }
        self.set_cancellable(false);

        self.emit("config", &removing_autostart(shell.label(), name), Some(0.85));
        let marker = sb::marker_path(wm, shell, &home, self.state_home().as_deref());
        let was_on = self.io.is_file(&marker);
        if was_on {
            self.io.remove_file(&marker);
        }
        match edit.edit {
            TextEdit::Unchanged => {}
            TextEdit::Delete => self.io.remove_file(&edit.path),
            TextEdit::Write(content) => {
                self.io
                    .write_text(&edit.path, &content)
                    .map_err(|e| Stop::Failed(config_write_failed(&edit.path, &e)))?;
            }
        }
        if was_on && current_de == wm {
            self.raw(&running_shell_hint(shell.label()));
        }
        self.emit("done", &shell_removed(shell.label(), name), Some(1.0));
        Ok(de_progress::MSG_OK.to_string())
    }
}

/// Установка оболочки на живой машине одним вызовом.
pub fn shell_install_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    wm: &str,
    shell: DesktopShell,
) -> DeOpOutcome {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).shell_install(wm, shell)
}

/// Вход в композитор с оболочкой на живой машине.
pub fn shell_switch_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    wm: &str,
    shell: DesktopShell,
    current_de: &str,
) -> DeOpOutcome {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).shell_switch(wm, shell, current_de)
}

/// Снятие оболочки на живой машине.
pub fn shell_remove_live(
    sink: Arc<dyn DeOpSink>,
    cancel: CancelToken,
    wm: &str,
    shell: DesktopShell,
    current_de: &str,
) -> DeOpOutcome {
    let io = LiveDeOpsIo::new();
    DeOpsExec::new(&io, sink, cancel).shell_remove(wm, shell, current_de)
}

#[cfg(test)]
mod tests {
    use super::super::tests::{healthy, out, FakeIo, Recorder};
    use super::*;

    const NIRI_CFG: &str = "/home/t/.config/niri/config.kdl";
    const NIRI_TEXT: &str = "input {\n    keyboard { }\n}\nspawn-at-startup \"waybar\"";
    const MARKER: &str = "/home/t/.local/state/rexon/shells/niri-noctalia";

    /// Машина: Arch, стоят niri и sway, конфиг niri есть. `installed` —
    /// какие пакеты уже стоят (ответ `pacman -Q`).
    fn machine(installed: &'static [&'static str]) -> FakeIo {
        let mut io = FakeIo::arch(Box::new(move |argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Q", p] if installed.contains(p) => out(0, &format!("{p} 1.0-1\n"), ""),
                _ => healthy(argv),
            }
        }));
        for b in ["niri", "sway", "noctalia", "dms"] {
            if b == "noctalia" || b == "dms" {
                if installed.iter().any(|p| p.starts_with(if b == "dms" { "dms-shell" } else { b })) {
                    io.bins.insert(b.to_string());
                }
            } else {
                io.bins.insert(b.to_string());
            }
        }
        io.files.borrow_mut().insert(NIRI_CFG.to_string(), NIRI_TEXT.to_string());
        io
    }

    fn exec(io: &FakeIo) -> (DeOpsExec<'_, FakeIo>, Arc<Recorder>) {
        let rec = Arc::new(Recorder::default());
        let sink: Arc<dyn DeOpSink> = rec.clone();
        (DeOpsExec::new(io, sink, CancelToken::new()), rec)
    }

    /// Установка: один запуск пакетного менеджера тем же путём, что у всего
    /// продукта, и ровно наш блок в конфиге niri. Юнит `dms.service` и
    /// `systemctl --user enable` не трогаются вовсе.
    #[test]
    fn install_puts_packages_and_one_block() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = machine(&[]);
        let (ex, rec) = exec(&io);
        let outcome = ex.shell_install("niri", DesktopShell::Noctalia);
        assert!(outcome.ok, "{outcome:?}");
        let runs = io.runs();
        let pkexec: Vec<_> = runs.iter().filter(|r| r.starts_with("pkexec")).collect();
        assert_eq!(pkexec, vec!["pkexec pacman -S --needed --noconfirm noctalia"], "{runs:#?}");
        assert!(!runs.iter().any(|r| r.contains("systemctl")), "{runs:#?}");
        let text = io.files.borrow().get(NIRI_CFG).cloned().unwrap();
        assert!(text.starts_with(NIRI_TEXT), "{text}");
        assert!(
            text.contains(&format!(
                "spawn-at-startup \"sh\" \"-c\" \"test -e {MARKER} && exec noctalia\""
            )),
            "{text}"
        );
        // Отметку ставит только «Переключиться» — установка режим не меняет.
        assert!(!io.files.borrow().contains_key(MARKER));
        assert_eq!(rec.stages().last().map(String::as_str), Some("done"));
    }

    /// Б9: сеанс прав есть — пакеты оболочки уходят помощнику по таблице
    /// оболочек (`Shell`), и имена её проходят; `pkexec` нет.
    #[test]
    fn install_goes_through_privilege_session() {
        let mut io = machine(&[]);
        io.session = Some(out(0, "", ""));
        let (ex, _rec) = exec(&io);
        assert!(ex.shell_install("niri", DesktopShell::Noctalia).ok);
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")), "{:#?}", io.runs());
        let ops = io.priv_ops.borrow();
        let [PrivOp::PkgInstall { family: PkgFamily::Arch, packages, source: PkgSource::Shell }] = ops.as_slice() else {
            panic!("{ops:?}");
        };
        assert_eq!(packages, &vec!["noctalia".to_string()]);
        assert!(crate::privilege::validate::check_install(
            PkgFamily::Arch,
            packages,
            crate::privilege::validate::InstallSource::Shell
        )
        .is_ok());
    }

    /// Пакет уже стоит — пароль не спрашивается вовсе.
    #[test]
    fn install_without_missing_packages_asks_no_password() {
        let io = machine(&["noctalia"]);
        let (ex, _rec) = exec(&io);
        assert!(ex.shell_install("niri", DesktopShell::Noctalia).ok);
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")), "{:#?}", io.runs());
    }

    /// DMS под sway: отказ ДО пароля и до записи файлов.
    #[test]
    fn dms_for_sway_is_refused_before_anything() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let io = machine(&[]);
        let (ex, _rec) = exec(&io);
        let outcome = ex.shell_install("sway", DesktopShell::DankMaterialShell);
        assert!(!outcome.ok);
        assert!(outcome.message.contains("only together with niri or Hyprland"), "{}", outcome.message);
        assert!(io.index_of("write").is_none());
        assert!(!io.runs().iter().any(|r| r.starts_with("pkexec")));
    }

    /// Композитора нет — «сначала установите стол», без пароля.
    #[test]
    fn missing_compositor_is_refused() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let mut io = machine(&[]);
        io.bins.remove("niri");
        let (ex, _rec) = exec(&io);
        let outcome = ex.shell_install("niri", DesktopShell::Noctalia);
        assert!(!outcome.ok);
        assert_eq!(outcome.message, sb::install_desktop_first("niri"));
        assert!(io.runs().iter().all(|r| !r.starts_with("pkexec")));
    }

    /// Композитор отверг конфиг после правки — файл возвращается бит-в-бит.
    #[test]
    fn rejected_config_is_rolled_back() {
        let io = FakeIo::arch(Box::new(|argv: &[String]| {
            let a: Vec<&str> = argv.iter().map(String::as_str).collect();
            match a.as_slice() {
                ["pacman", "-Q", "noctalia"] => out(0, "noctalia 5.2\n", ""),
                // До правки конфиг исправен, после — нет.
                ["niri", "validate", "--config", _] => {
                    out(if VALIDATED.fetch_add(1, Ordering::SeqCst) == 0 { 0 } else { 1 }, "", "")
                }
                _ => healthy(argv),
            }
        }));
        static VALIDATED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let mut io = io;
        io.bins.insert("niri".into());
        io.bins.insert("noctalia".into());
        io.files.borrow_mut().insert(NIRI_CFG.to_string(), NIRI_TEXT.to_string());
        let (ex, _rec) = exec(&io);
        let outcome = ex.shell_install("niri", DesktopShell::Noctalia);
        assert!(!outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get(NIRI_CFG).map(String::as_str), Some(NIRI_TEXT));
    }

    /// Полный круг: установить → переключиться → «Переключиться» у самого
    /// стола → убрать. Конфиг возвращается бит-в-бит, отметка снята, пакет
    /// снят одной командой.
    #[test]
    fn full_cycle_is_reversible_bit_for_bit() {
        let io = machine(&["noctalia"]);
        let (ex, _rec) = exec(&io);
        assert!(ex.shell_install("niri", DesktopShell::Noctalia).ok);
        let installed = io.files.borrow().get(NIRI_CFG).cloned().unwrap();
        assert_ne!(installed, NIRI_TEXT);

        // Мы в sway → обычная смена стола, но с отметкой.
        let r = ex.shell_switch("niri", DesktopShell::Noctalia, "sway");
        assert!(io.files.borrow().contains_key(MARKER), "{r:?} {:#?}", io.calls.borrow());

        // Левая «Переключиться» на niri снимает отметку.
        let _ = ex.switch_de("niri", "sway");
        assert!(!io.files.borrow().contains_key(MARKER));

        // Уже в niri: «Переключиться» у оболочки — отметка и выход.
        let _ = ex.shell_switch("niri", DesktopShell::Noctalia, "niri");
        assert!(io.files.borrow().contains_key(MARKER));

        let outcome = ex.shell_remove("niri", DesktopShell::Noctalia, "niri");
        assert!(outcome.ok, "{outcome:?}");
        assert_eq!(io.files.borrow().get(NIRI_CFG).map(String::as_str), Some(NIRI_TEXT));
        assert!(!io.files.borrow().contains_key(MARKER));
        let removes: Vec<_> = io.runs().into_iter().filter(|r| r.contains("-Rns")).collect();
        assert_eq!(removes.len(), 1, "{:#?}", io.runs());
        assert!(removes[0].ends_with("noctalia"), "{removes:?}");
    }

    /// Оболочка стоит и в niri, и в sway: «Убрать» из niri не снимает общий
    /// пакет и не трогает конфиг sway.
    #[test]
    fn remove_keeps_package_used_by_another_desktop() {
        let io = machine(&["noctalia"]);
        let sway_cfg = "/home/t/.config/sway/config";
        io.files.borrow_mut().insert(sway_cfg.to_string(), "bindsym $mod+q kill\n".to_string());
        let (ex, _rec) = exec(&io);
        assert!(ex.shell_install("niri", DesktopShell::Noctalia).ok);
        assert!(ex.shell_install("sway", DesktopShell::Noctalia).ok);
        let sway_with = io.files.borrow().get(sway_cfg).cloned().unwrap();
        assert!(ex.shell_remove("niri", DesktopShell::Noctalia, "kde").ok);
        assert!(!io.runs().iter().any(|r| r.contains("-Rns")), "{:#?}", io.runs());
        assert_eq!(io.files.borrow().get(sway_cfg), Some(&sway_with));
    }

    /// sway без личного конфига: файл заводится с `include` системного, а
    /// «Убрать» его удаляет — машина как до нас.
    #[test]
    fn sway_file_created_by_us_is_deleted_on_remove() {
        let io = machine(&["noctalia"]);
        io.files.borrow_mut().insert("/etc/sway/config".to_string(), "# system\n".to_string());
        let path = "/home/t/.config/sway/config";
        let (ex, _rec) = exec(&io);
        assert!(ex.shell_install("sway", DesktopShell::Noctalia).ok);
        let text = io.files.borrow().get(path).cloned().unwrap();
        assert!(text.contains("include /etc/sway/config"), "{text}");
        assert!(ex.shell_remove("sway", DesktopShell::Noctalia, "kde").ok);
        assert!(!io.files.borrow().contains_key(path));
    }

    /// Переключение БЕЗ оболочек не делает лишних шагов: ни записи, ни
    /// удаления отметок, которых нет.
    #[test]
    fn plain_switch_without_shells_touches_no_markers() {
        let io = machine(&[]);
        let (ex, _rec) = exec(&io);
        let _ = ex.switch_de("niri", "kde");
        assert!(
            !io.calls.borrow().iter().any(|c| c.contains("rexon/shells")),
            "{:#?}",
            io.calls.borrow()
        );
    }
}
