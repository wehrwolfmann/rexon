//! «Остановить все» в разделе «Виджеты Conky»: все процессы conky на
//! машине — свои, найденные на диске и запущенные человеком мимо Rexon.
//!
//! ⛔ Никакого `killall conky` / `pkill conky`: гасится только то, что
//! Rexon перед этим нашёл в `/proc` и показал человеку списком, — по номеру
//! процесса. Процесс считается conky, только если и имя программы в его
//! командной строке, и имя процесса ядра (`/proc/<pid>/comm`) — `conky`:
//! сценарий с «conky» в аргументах (`sh start.sh conky`) или чужая программа
//! с таким словом в пути не задеваются. Перед `kill` каждый номер
//! проверяется ещё раз — за время вопроса процесс мог завершиться, а номер
//! достаться другой программе.

use crate::animations::LookIo;
use crate::de_apply::Ctx;
use crate::i18n::{ntr, tr, trf};

/// Запущенный процесс conky.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConkyProc {
    pub pid: String,
    /// Файл настроек из командной строки; `None` — conky без `-c` (тогда он
    /// читает `~/.config/conky/conky.conf` или `~/.conkyrc`).
    pub conf: Option<String>,
}

/// conky ли это: программа `conky` в командной строке и `conky` — имя
/// процесса у ядра.
fn is_conky(cmdline: &str, comm: Option<&str>) -> bool {
    let prog = cmdline.split('\0').next().unwrap_or("");
    prog.rsplit('/').next() == Some("conky") && comm.map(str::trim) == Some("conky")
}

fn proc_of(io: &dyn LookIo, pid: &str) -> Option<ConkyProc> {
    let cmd = io.read(&format!("/proc/{pid}/cmdline"))?;
    let comm = io.read(&format!("/proc/{pid}/comm"));
    if !is_conky(&cmd, comm.as_deref()) {
        return None;
    }
    // Копия виджета на невидимом экране снимков (`conky_preview`) — не
    // виджет на столе: ни в «На столе сейчас», ни в «Остановить все», ни в
    // «запущен ли» она не попадает.
    if crate::de_apply::conky_launch_of_cmdline(&cmd).is_some_and(|l| crate::conky_preview::is_preview_launch(&l)) {
        return None;
    }
    Some(ConkyProc { pid: pid.to_string(), conf: crate::de_apply::conky_conf_of_cmdline(&cmd) })
}

/// Все процессы conky сейчас, по возрастанию номера.
pub fn all(io: &dyn LookIo) -> Vec<ConkyProc> {
    let mut pids: Vec<u64> = io.list_dir("/proc").iter().filter_map(|p| p.parse().ok()).collect();
    pids.sort_unstable();
    pids.into_iter().filter_map(|p| proc_of(io, &p.to_string())).collect()
}

/// Как назвать процесс человеку: имя виджета Rexon, имя найденного виджета
/// с пометкой «не из Rexon», имя файла настроек либо «conky без файла
/// настроек».
pub fn label(ctx: &Ctx, extra: &[String], p: &ConkyProc) -> String {
    let Some(conf) = p.conf.as_deref() else {
        return tr("conky with its default settings file");
    };
    let root = format!("{}/", crate::de_apply::conky_root(ctx));
    if let Some((id, _)) = conf.strip_prefix(&root).and_then(|r| r.split_once('/')) {
        return id.to_string();
    }
    let owner = crate::conky_scan::owner_of_conf(ctx, extra, conf).unwrap_or_else(|| conf.to_string());
    let name = owner.rsplit('/').next().unwrap_or(&owner).to_string();
    // TRANSLATORS: %1 is a Conky widget name.
    trf("%1 (not from Rexon)", &[&name])
}

/// Вопрос перед «Остановить все»: `(заголовок, текст)` со списком того, что
/// будет остановлено. `None` — останавливать нечего.
pub fn stop_all_question(ctx: &Ctx, extra: &[String], procs: &[ConkyProc]) -> Option<(String, String)> {
    if procs.is_empty() {
        return None;
    }
    let mut names: Vec<String> = Vec::new();
    for p in procs {
        let l = label(ctx, extra, p);
        if !names.contains(&l) {
            names.push(l);
        }
    }
    let list = names.iter().map(|n| format!("• {n}")).collect::<Vec<_>>().join("\n");
    Some((
        tr("Stop all Conky widgets?"),
        trf(
            // TRANSLATORS: %1 is a bulleted list of widget names, one per line.
            "Every running Conky widget will be stopped, including the ones not started from Rexon:\n\n%1",
            &[&list],
        ),
    ))
}

/// Остановить процессы `pids` — только те, что и сейчас conky (номер мог
/// перейти к другой программе, пока человек читал вопрос). `(получилось,
/// сообщение)`.
pub fn stop_all(io: &dyn LookIo, pids: &[String]) -> (bool, String) {
    let alive: Vec<String> = pids.iter().filter(|p| proc_of(io, p).is_some()).cloned().collect();
    if alive.is_empty() {
        return (true, tr("⏹️ No Conky widgets are running"));
    }
    let mut cmd = vec!["kill".to_string()];
    cmd.extend(alive.iter().cloned());
    let (ok, out) = io.run(&cmd);
    if !ok {
        return (false, trf("Couldn't stop the Conky widgets: %1", &[&out.trim().to_string()]));
    }
    (true, ntr("⏹️ Stopped %n Conky widget", "⏹️ Stopped %n Conky widgets", alive.len() as u64))
}

// ════════════════════════════════════════════════════════════════════════════
//  Запуск и остановка частей виджета
// ════════════════════════════════════════════════════════════════════════════

/// Файлы настроек всех запущенных conky; у conky без `-c` — файл, который он
/// читает сам ([`crate::conky_scan::default_conf`]).
pub fn running_confs(io: &dyn LookIo, ctx: &Ctx) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in all(io) {
        let conf = p.conf.unwrap_or_else(|| crate::conky_scan::default_conf(io, ctx));
        if !out.contains(&conf) {
            out.push(conf);
        }
    }
    out
}

/// Номера процессов conky, чей файл настроек проходит `belongs`.
pub fn pids_where(io: &dyn LookIo, ctx: &Ctx, belongs: &dyn Fn(&str) -> bool) -> Vec<String> {
    all(io)
        .into_iter()
        .filter(|p| {
            let conf = p.conf.clone().unwrap_or_else(|| crate::conky_scan::default_conf(io, ctx));
            belongs(&conf)
        })
        .map(|p| p.pid)
        .collect()
}

/// Остановить процессы `pids` виджета `title`. `(получилось, сообщение)`.
pub fn stop_pids(io: &dyn LookIo, title: &str, pids: Vec<String>) -> (bool, String) {
    if !pids.is_empty() {
        let mut cmd = vec!["kill".to_string()];
        cmd.extend(pids);
        let (ok, out) = io.run(&cmd);
        if !ok {
            return (false, trf("Couldn't stop “%1”: %2", &[&title, &out.trim().to_string()]));
        }
    }
    (true, trf("⏹️ Stopped: %1", &[&title]))
}

/// Запустить conky с файлом `conf` (часть виджета `title`): из папки файла,
/// с выводом ошибок в журнал ([`crate::conky_log`]). Правда — процесс в
/// системе через секунду; нет его — причина из журнала словами conky.
pub fn start_conf(io: &dyn LookIo, ctx: &Ctx, title: &str, conf: &str) -> (bool, String) {
    if !io.have("conky") {
        return (false, crate::de_apply::conky_missing_refusal());
    }
    let log = crate::conky_log::live_log_path(ctx, conf);
    let started = start_with(io, ctx, "conky", title, conf, &log);
    // Первый запуск — заодно снимок виджета для плитки (в фоне, X11).
    if started.0 {
        crate::conky_preview::capture_later(ctx, conf, &log);
    }
    started
}

/// [`start_conf`] с заданной программой и журналом (сверки подставляют
/// свою программу вместо conky).
pub fn start_with(io: &dyn LookIo, ctx: &Ctx, program: &str, title: &str, conf: &str, log: &str) -> (bool, String) {
    // Человек менял настройки виджета — conky идёт с накладкой Rexon, а
    // рабочий каталог остаётся папкой файла автора.
    let launch = crate::conky_overlay::launch_conf(io, ctx, &crate::conky_overlay::live_paths(ctx, conf), conf);
    let (argv, _) = crate::de_apply::conky_start_command(&launch);
    let dir = crate::de_apply::conky_work_dir(conf).to_string();
    // Вывод — в файл, не в канал: conky с `-d` уходит в фон и держит
    // унаследованный вывод, с каналом кнопка ждала бы его конца.
    let stderr = match crate::conky_log::open_for_start(log) {
        Some(f) => std::process::Stdio::from(f),
        None => std::process::Stdio::null(),
    };
    let started = std::process::Command::new(program)
        .args(&argv[1..])
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(stderr)
        .status();
    if let Err(e) = started {
        return (false, trf("Couldn't start “%1”: %2", &[&title, &e.to_string()]));
    }
    let running = (0..10).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        running_confs(io, ctx).iter().any(|c| c == conf)
    });
    if !running {
        let text = std::fs::read(log).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let why = match crate::conky_log::failure_reason(&text) {
            Some(reason) => trf("conky closed right after starting: %1", &[&reason]),
            None => tr("conky closed right after starting — the widget's settings file may be broken"),
        };
        return (false, trf("Couldn't start “%1”: %2", &[&title, &why]));
    }
    let mut msg = trf("▶️ Started: %1", &[&title]);
    // Версия conky нужна оговорке только в сеансе Wayland.
    let version = if ctx.wayland { crate::conky_overlay::conky_version(io) } else { None };
    if let Some(note) = crate::de_apply::conky_wayland_note_for(&ctx.de, ctx.wayland, version) {
        msg.push_str(" — ");
        msg.push_str(&note);
    }
    (true, msg)
}

// ════════════════════════════════════════════════════════════════════════════
//  Для страницы: части виджета, журнал, подписи раздела
// ════════════════════════════════════════════════════════════════════════════

/// Часть виджета для подсписка в карточке.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Part {
    pub conf: String,
    /// Путь файла от каталога виджета (`variants/small.conf`).
    pub label: String,
    pub running: bool,
}

/// Части виджета `name` (имя плитки): главная — первой.
pub fn parts(io: &dyn LookIo, ctx: &Ctx, name: &str) -> Vec<Part> {
    let primary = if crate::conky_scan::is_found_name(name) {
        None
    } else {
        crate::de_apply::conky_conf(io, ctx, name)
    };
    let base = crate::conky_scan::widget_path(ctx, name);
    let running = running_confs(io, ctx);
    crate::conky_scan::widget_confs(io, ctx, name, primary)
        .into_iter()
        .map(|conf| Part {
            label: conf
                .strip_prefix(&format!("{base}/"))
                .map(str::to_string)
                .unwrap_or_else(|| crate::conky_scan::widget_title(&conf)),
            running: running.contains(&conf),
            conf,
        })
        .collect()
}

/// Кнопка части: запущена — остановить её процессы, нет — запустить.
/// Чужой файл (не часть этого виджета) — отказ без действия.
pub fn press_part(io: &dyn LookIo, ctx: &Ctx, name: &str, conf: &str) -> (bool, String) {
    let Some(part) = parts(io, ctx, name).into_iter().find(|p| p.conf == conf) else {
        return (false, tr("This file isn't part of the widget."));
    };
    let title = format!("{} — {}", crate::conky_scan::widget_title(name), part.label);
    if part.running {
        stop_pids(io, &title, pids_where(io, ctx, &|c| c == conf))
    } else if let Some(why) = crate::conky_safety::start_refusal(io, ctx, name) {
        // Часть виджета — тот же виджет: без разрешения не запускается.
        (false, why)
    } else {
        start_conf(io, ctx, &title, conf)
    }
}

/// Последние строки журнала виджета для карточки; пусто — Rexon его ещё
/// не запускал.
pub fn log_tail(io: &dyn LookIo, ctx: &Ctx, name: &str) -> String {
    let confs: Vec<String> = parts(io, ctx, name).into_iter().map(|p| p.conf).collect();
    crate::conky_log::widget_log(ctx, &confs).map(|(_, text)| text).unwrap_or_default()
}

/// Подписи раздела «Виджеты Conky» для страницы.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SectionTexts {
    pub running_chip: String,
    pub running_chip_tooltip: String,
    pub stop_all: String,
    pub stop_all_tooltip: String,
    pub parts_title: String,
    pub part_start: String,
    pub part_stop: String,
    pub log_title: String,
    pub ribbon_title: String,
    pub ribbon_open_tooltip: String,
    pub ribbon_stop_tooltip: String,
}

pub fn section_texts() -> SectionTexts {
    SectionTexts {
        running_chip: tr("▶️ Running"),
        running_chip_tooltip: tr("Show only the widgets that are running now"),
        stop_all: tr("⏹️ Stop all"),
        stop_all_tooltip: tr("Stop every running Conky widget, including the ones not started from Rexon"),
        parts_title: tr("Parts of the widget"),
        // Те же слова, что на кнопке плитки (`appearance_page::card_local_button_for`).
        part_start: tr("▶️ Start"),
        part_stop: tr("⏹️ Stop"),
        log_title: tr("What Conky reported at the last start"),
        ribbon_title: tr("On the desktop now"),
        ribbon_open_tooltip: tr("Open the widget's card: settings, parts, log"),
        ribbon_stop_tooltip: tr("Remove from the desktop (the widget stays installed)"),
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Лента «На столе сейчас»
// ════════════════════════════════════════════════════════════════════════════

/// Запущенный виджет в ленте над сеткой: как гаджеты на столе Windows —
/// видно сразу, что стоит, щелчок открывает карточку, крестик убирает.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RibbonItem {
    /// Номер плитки в разделе (`local:conky:<имя или путь>`).
    pub item_id: String,
    pub title: String,
    /// Картинка: своя у виджета либо снимок Rexon; пусто — знак 📊.
    pub picture: String,
    /// Запущенные файлы настроек этого виджета.
    pub confs: Vec<String>,
}

/// Запущенные виджеты, по одному на виджет (части — вместе), по порядку
/// запуска. Процесс conky без файла настроек — тоже виджет (файл, который
/// conky читает сам).
pub fn ribbon(io: &dyn LookIo, ctx: &Ctx, extra: &[String]) -> Vec<RibbonItem> {
    let mut out: Vec<RibbonItem> = Vec::new();
    for conf in running_confs(io, ctx) {
        let name = crate::conky_sets::widget_name_of(ctx, extra, &conf);
        let id = format!("local:conky:{name}");
        if let Some(it) = out.iter_mut().find(|i| i.item_id == id) {
            it.confs.push(conf);
            continue;
        }
        out.push(RibbonItem { item_id: id, title: crate::conky_scan::widget_title(&name), picture: String::new(), confs: vec![conf] });
    }
    for it in &mut out {
        let name = it.item_id.trim_start_matches("local:conky:").to_string();
        let path = crate::conky_scan::widget_path(ctx, &name);
        let mut confs = it.confs.clone();
        for c in crate::conky_scan::widget_confs(io, ctx, &name, None) {
            if !confs.contains(&c) {
                confs.push(c);
            }
        }
        it.picture = crate::conky_scan::widget_picture(io, &path, &confs)
            .or_else(|| crate::conky_preview::cached(ctx, &confs))
            .unwrap_or_default();
    }
    out
}

/// Крестик ленты: погасить ровно эти файлы настроек (по номерам
/// процессов, чужие не задеваются).
pub fn stop_confs(io: &dyn LookIo, ctx: &Ctx, title: &str, confs: &[String]) -> (bool, String) {
    stop_pids(io, title, pids_where(io, ctx, &|c| confs.iter().any(|x| x == c)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct FakeProc {
        files: HashMap<String, String>,
        ran: RefCell<Vec<Vec<String>>>,
    }

    impl LookIo for FakeProc {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(argv.to_vec());
            (true, String::new())
        }
        fn have(&self, _: &str) -> bool {
            true
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.get(path).cloned()
        }
        fn write(&self, _: &str, _: &str) -> bool {
            false
        }
        fn list_dir(&self, path: &str) -> Vec<String> {
            if path != "/proc" {
                return Vec::new();
            }
            let mut v: Vec<String> = self
                .files
                .keys()
                .filter_map(|k| k.strip_prefix("/proc/")?.split_once('/').map(|(p, _)| p.to_string()))
                .collect();
            v.push("self".into());
            v.sort();
            v.dedup();
            v
        }
    }

    fn proc(files: &mut HashMap<String, String>, pid: u32, cmd: &[&str], comm: &str) {
        files.insert(format!("/proc/{pid}/cmdline"), cmd.join("\0") + "\0");
        files.insert(format!("/proc/{pid}/comm"), format!("{comm}\n"));
    }

    fn ctx() -> Ctx {
        Ctx { home: "/h".into(), xdg_config_home: None, wayland: false, de: String::new() }
    }

    fn fixture() -> FakeProc {
        let mut f = HashMap::new();
        proc(&mut f, 120, &["conky", "-d", "-c", "/h/.config/conky/Jhalak/Jhalak.conf"], "conky");
        proc(&mut f, 99, &["/usr/bin/conky", "-c", "/h/.conky/Gotham/Gotham"], "conky");
        proc(&mut f, 300, &["conky"], "conky");
        // Копия Jhalak на невидимом экране снимков — не виджет на столе.
        proc(
            &mut f,
            130,
            &["conky", "-c", "/run/user/1000/rexon/conky-preview/h+s.config+sconky+sJhalak+sJhalak.conf.preview.conf"],
            "conky",
        );
        // Не conky: сценарий запуска с «conky» в аргументах, редактор файла.
        proc(&mut f, 400, &["sh", "start.sh", "conky"], "sh");
        proc(&mut f, 401, &["/usr/bin/vim", "/h/.config/conky/x/conky.conf"], "vim");
        // Имя программы подменено, а ядро знает его как другое.
        proc(&mut f, 402, &["conky"], "python3");
        FakeProc { files: f, ran: RefCell::new(Vec::new()) }
    }

    #[test]
    fn only_real_conky_processes_are_listed() {
        let io = fixture();
        let got = all(&io);
        let pids: Vec<&str> = got.iter().map(|p| p.pid.as_str()).collect();
        assert_eq!(pids, ["99", "120", "300"]);
        assert_eq!(got[2].conf, None);
    }

    #[test]
    fn question_lists_own_and_foreign_widgets() {
        crate::i18n::set_language(Some("en"));
        let io = fixture();
        let (_, text) = stop_all_question(&ctx(), &[], &all(&io)).unwrap();
        assert!(text.contains("• Gotham (not from Rexon)"), "{text}");
        assert!(text.contains("• Jhalak\n"), "{text}");
        assert!(text.contains("• conky with its default settings file"), "{text}");
        assert!(stop_all_question(&ctx(), &[], &[]).is_none());
    }

    /// conky упал сразу — сообщение называет причину из его вывода.
    #[test]
    fn failed_start_tells_the_reason_from_log() {
        use std::os::unix::fs::PermissionsExt;
        crate::i18n::set_language(Some("en"));
        let dir = std::env::temp_dir().join(format!("rexon-conky-start-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("w")).unwrap();
        let fake = dir.join("conky");
        std::fs::write(
            &fake,
            "#!/bin/sh\necho \"[2026-10-19 00:21:46.200] [error] [src/lua/llua.cc:339] lua script './x.lua' not found\" >&2\npwd >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let conf = dir.join("w/conky.conf").to_string_lossy().into_owned();
        std::fs::write(&conf, "conky.text = [[x]]").unwrap();
        let log = dir.join("log/w.log").to_string_lossy().into_owned();
        let io = FakeProc { files: HashMap::new(), ran: RefCell::new(Vec::new()) };
        let (ok, msg) = start_with(&io, &ctx(), &fake.to_string_lossy(), "w", &conf, &log);
        assert!(!ok);
        assert!(msg.contains("lua script './x.lua' not found"), "{msg}");
        // Запуск — из папки виджета, вывод — в журнал.
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains(&dir.join("w").to_string_lossy().into_owned()), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn running_confs_cover_default_config() {
        let io = fixture();
        let confs = running_confs(&io, &ctx());
        assert!(confs.contains(&"/h/.conkyrc".to_string()), "{confs:?}");
        assert!(confs.contains(&"/h/.conky/Gotham/Gotham".to_string()));
        let pids = pids_where(&io, &ctx(), &|c| c.starts_with("/h/.conky/"));
        assert_eq!(pids, ["99"]);
    }

    /// Гасятся по номеру только показанные процессы, которые и сейчас conky:
    /// номер 401 за время вопроса достался редактору — его не трогаем.
    #[test]
    fn stop_all_kills_by_pid_and_rechecks() {
        let io = fixture();
        let (ok, _) = stop_all(&io, &["99".into(), "120".into(), "401".into(), "777".into()]);
        assert!(ok);
        assert_eq!(*io.ran.borrow(), vec![vec!["kill".to_string(), "99".into(), "120".into()]]);
        let io2 = fixture();
        assert!(stop_all(&io2, &["401".into()]).0);
        assert!(io2.ran.borrow().is_empty(), "некого гасить — kill не зовётся");
    }

    /// Лента: один пункт на виджет, по порядку запуска; крестик гасит только
    /// процессы своего виджета.
    #[test]
    fn ribbon_lists_running_widgets_and_stops_only_theirs() {
        let f = fixture();
        let r = ribbon(&f, &ctx(), &[]);
        let ids: Vec<&str> = r.iter().map(|i| i.item_id.as_str()).collect();
        assert_eq!(ids, ["local:conky:/h/.conky/Gotham", "local:conky:Jhalak", "local:conky:/h/.conkyrc"]);
        assert_eq!(r[1].title, "Jhalak");
        assert_eq!(r[1].confs, ["/h/.config/conky/Jhalak/Jhalak.conf"]);
        let (ok, _) = stop_confs(&f, &ctx(), "Jhalak", &r[1].confs);
        assert!(ok);
        assert_eq!(*f.ran.borrow(), [vec!["kill".to_string(), "120".to_string()]]);
    }
}
