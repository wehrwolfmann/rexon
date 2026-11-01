//! Сторож рисовальщика живых обоев: упал — поднять обои заново.
//!
//! ЗАЧЕМ. Сцены и веб-обои рисует отдельный процесс waywallen
//! (`waywallen-<вид>-renderer`). Если он аварийно завершается — сбой
//! драйвера видеокарты, ошибка в самой сцене, — демон только убирает его из
//! своих списков: выбранные обои за ним остаются (`CurrentWallpaperId`), а на
//! рабочем столе пусто, и человеку никто ничего не говорит (живой случай
//! 10.10.2026: сброс видеокарты → рисовальщик упал → стол без обоев).
//!
//! ЧТО ДЕЛАЕТ. Пока Rexon работает (значок в лотке), сторож раз в
//! [`POLL_SECS`] смотрит на процессы рисовальщиков. Пропал процесс —
//! спрашивает журнал системы, был ли это аварийный выход (запись
//! `systemd-coredump` с номером процесса). Был — просит демона применить те
//! же обои ещё раз и говорит об этом уведомлением.
//!
//! ЧЕГО НЕ ДЕЛАЕТ.
//! * Не трогает обои, остановленные намеренно: правила автопаузы демона
//!   («сеанс заблокирован — стоп») и смена обоев человеком тоже убирают
//!   процесс, но записи об аварии при этом нет — и сторож молчит.
//! * Не перезапускает по кругу: после [`MAX_RECOVERIES`] подъёмов тех же
//!   обоев за [`RECOVERY_WINDOW_SECS`] останавливается и говорит, что обои
//!   падают сами. Каждое такое падение может стоить сброса видеокарты.
//! * Не видит падений без записи об аварии (процесс убит по нехватке памяти,
//!   вышел с кодом ошибки) и не работает там, где нет `journalctl`/`busctl`.
//!
//! Решение — чистая функция [`Watch::tick`]; всё, что ходит наружу, — в
//! «живой» части внизу.

use std::collections::HashMap;

use crate::i18n::tr;

/// Как часто смотреть на процессы, секунды.
pub const POLL_SECS: u64 = 10;
/// Сколько проходов ждать записи об аварии после пропажи процесса:
/// `systemd-coredump` пишет её не мгновенно (большой образ памяти — секунды).
pub const CRASH_WAIT_TICKS: u32 = 3;
/// Окно, в котором считаются подъёмы одних и тех же обоев, секунды.
pub const RECOVERY_WINDOW_SECS: u64 = 600;
/// Сколько раз за окно поднимать одни и те же обои. Одного раза хватает на
/// случайный сбой; второй подряд — значит, обои падают сами.
pub const MAX_RECOVERIES: usize = 1;

/// Что делать после очередного прохода.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Ничего: всё работает либо вмешиваться не нужно.
    Nothing,
    /// Рисовальщик упал — применить обои `id` заново.
    Recover { id: String },
    /// Обои `id` падают повторно — больше не поднимать, сказать человеку.
    GiveUp { id: String },
}

/// Память сторожа между проходами.
#[derive(Debug, Default)]
pub struct Watch {
    /// Живые рисовальщики: номер процесса → обои, выбранные при нём.
    seen: HashMap<u32, String>,
    /// Пропавшие процессы, по которым ещё ждём записи об аварии:
    /// (номер процесса, обои, сколько проходов осталось ждать).
    pending: Vec<(u32, String, u32)>,
    /// Сделанные подъёмы: (время, обои).
    recoveries: Vec<(u64, String)>,
    /// Обои, которые решено больше не поднимать (пока человек их не сменит).
    given_up: Option<String>,
}

impl Watch {
    /// Один проход.
    ///
    /// `now` — время, секунды; `alive` — номера живых процессов
    /// рисовальщиков; `current_id` — обои, выбранные у демона (пусто — демон
    /// молчит либо обоев нет); `crashed(pid)` — есть ли в журнале системы
    /// запись об аварийном выходе этого процесса.
    pub fn tick(
        &mut self,
        now: u64,
        alive: &[u32],
        current_id: &str,
        crashed: &dyn Fn(u32) -> bool,
    ) -> Step {
        // Человек выбрал другие обои — прежний отказ к ним не относится.
        if self.given_up.as_deref().is_some_and(|id| id != current_id) {
            self.given_up = None;
        }

        // Пропавшие процессы — в ожидание записи об аварии.
        let gone: Vec<u32> = self.seen.keys().copied().filter(|pid| !alive.contains(pid)).collect();
        for pid in gone {
            if let Some(id) = self.seen.remove(&pid) {
                self.pending.push((pid, id, CRASH_WAIT_TICKS));
            }
        }
        // Новые процессы запоминаем с обоями, выбранными сейчас.
        for pid in alive {
            self.seen.entry(*pid).or_insert_with(|| current_id.to_string());
        }

        // Кто из пропавших упал на самом деле.
        let mut crashed_id: Option<String> = None;
        let mut still_waiting = Vec::new();
        for (pid, id, left) in std::mem::take(&mut self.pending) {
            if crashed(pid) {
                crashed_id = Some(id);
            } else if left > 1 {
                still_waiting.push((pid, id, left - 1));
            }
        }
        self.pending = still_waiting;

        let Some(id) = crashed_id else { return Step::Nothing };
        // Поднимаем только то, что человек всё ещё хочет видеть: обои те же,
        // рисовальщика нет (демон не поднял его сам, человек не выбрал
        // другое), и мы от этих обоев ещё не отказались.
        if id.is_empty() || id != current_id || !alive.is_empty() {
            return Step::Nothing;
        }
        if self.given_up.as_deref() == Some(id.as_str()) {
            return Step::Nothing;
        }
        self.recoveries.retain(|(at, _)| now.saturating_sub(*at) <= RECOVERY_WINDOW_SECS);
        let recent = self.recoveries.iter().filter(|(_, was)| *was == id).count();
        if recent >= MAX_RECOVERIES {
            self.given_up = Some(id.clone());
            return Step::GiveUp { id };
        }
        self.recoveries.push((now, id.clone()));
        Step::Recover { id }
    }
}

// ── Подписи ────────────────────────────────────────────────────────────

/// Заголовок уведомлений сторожа.
pub fn notice_title() -> String {
    tr("Live wallpaper")
}

/// Обои подняты после падения рисовальщика.
pub fn recovered_text() -> String {
    tr("The program that draws your wallpaper stopped unexpectedly. Rexon has started the wallpaper again.")
}

/// Обои упали повторно — больше не поднимаем.
pub fn gave_up_text() -> String {
    tr("Your wallpaper keeps stopping unexpectedly, so Rexon no longer restarts it. \
        Choose another wallpaper in the Wallpaper Engine tab.")
}

/// Поднять обои не получилось (демон отказал).
pub fn recover_failed_text() -> String {
    tr("The program that draws your wallpaper stopped unexpectedly, and Rexon couldn't start it again. \
        Apply the wallpaper again in the Wallpaper Engine tab.")
}

// ── Разбор ответов системы ─────────────────────────────────────────────

/// Процесс рисовальщика waywallen по первому слову его командной строки:
/// файл `waywallen-<вид>-renderer`.
pub fn is_renderer_exe(argv0: &str) -> bool {
    let name = argv0.rsplit('/').next().unwrap_or(argv0);
    name.starts_with("waywallen-") && name.ends_with("-renderer")
}

/// Значение строкового свойства из ответа `busctl get-property`:
/// `s "16352"` → `16352`. Не строка либо пусто — пустая строка.
pub fn busctl_string(output: &str) -> String {
    let line = output.trim();
    let Some(rest) = line.strip_prefix("s ") else { return String::new() };
    rest.trim().trim_matches('"').to_string()
}

/// Говорит ли ответ журнала (`journalctl -o json`, записи об авариях по
/// номеру процесса), что упал именно рисовальщик: в записи есть путь
/// программы `COREDUMP_EXE`, и это рисовальщик.
pub fn journal_names_renderer_crash(journal_json: &str) -> bool {
    journal_json.lines().any(|line| {
        serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|v| v["COREDUMP_EXE"].as_str().map(is_renderer_exe))
            .unwrap_or(false)
    })
}

// ── Живая часть ────────────────────────────────────────────────────────

/// Номер сообщения `systemd-coredump` в журнале («процесс сбросил образ
/// памяти») — постоянный идентификатор из каталога сообщений systemd.
const COREDUMP_MESSAGE_ID: &str = "fc2e22bc6ee647b6b90729ab34a250b1";
const DAEMON_BUS: &str = "org.waywallen.waywallen.Daemon";
const DAEMON_PATH: &str = "/org/waywallen/waywallen/Daemon";
const DAEMON_IFACE: &str = "org.waywallen.waywallen.Daemon1";

fn run_capture(argv: &[&str]) -> Option<(i32, String)> {
    let spec = crate::utils::RunSpec {
        argv: argv.iter().map(|s| s.to_string()).collect(),
        output: crate::utils::RunOutput::Capture,
        timeout_secs: Some(15),
        ..crate::utils::RunSpec::default()
    };
    let report = crate::utils::run(&spec, std::env::var("PATH").ok().as_deref());
    let code = report.returncode?;
    Some((code, String::from_utf8_lossy(&report.stdout).into_owned()))
}

/// Номера живых процессов рисовальщиков этого пользователя.
fn live_renderer_pids() -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
    let mut pids = Vec::new();
    for entry in dir.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else { continue };
        let argv0 = cmdline.split(|b| *b == 0).next().unwrap_or_default();
        if is_renderer_exe(&String::from_utf8_lossy(argv0)) {
            pids.push(pid);
        }
    }
    pids
}

/// Обои, выбранные у демона; пусто — демон не отвечает либо обоев нет.
fn live_current_id() -> String {
    run_capture(&["busctl", "--user", "get-property", DAEMON_BUS, DAEMON_PATH, DAEMON_IFACE, "CurrentWallpaperId"])
        .filter(|(code, _)| *code == 0)
        .map(|(_, out)| busctl_string(&out))
        .unwrap_or_default()
}

/// Есть ли в журнале запись об аварийном выходе процесса `pid`.
fn live_crashed(pid: u32) -> bool {
    let by_id = format!("MESSAGE_ID={COREDUMP_MESSAGE_ID}");
    let by_pid = format!("COREDUMP_PID={pid}");
    run_capture(&["journalctl", "--user", "--no-pager", "-o", "json", "--since", "-15min", &by_id, &by_pid])
        .map(|(_, out)| journal_names_renderer_crash(&out))
        .unwrap_or(false)
}

/// Попросить демона применить обои `id` заново.
fn live_apply(id: &str) -> bool {
    run_capture(&["busctl", "--user", "call", DAEMON_BUS, DAEMON_PATH, DAEMON_IFACE, "ApplyById", "s", id])
        .is_some_and(|(code, _)| code == 0)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Кнопка уведомления «Открыть Wallpaper Engine».
pub const ACTION_OPEN: &str = "open";

/// Кнопки уведомления, которое просит выбрать или применить обои заново.
pub fn open_actions() -> Vec<(String, String)> {
    vec![(ACTION_OPEN.to_string(), tr("Open Wallpaper Engine"))]
}

/// Уведомление `text` с кнопкой «Открыть Wallpaper Engine»: текст просит
/// человека выбрать или применить обои во вкладке — кнопка ведёт прямо туда.
/// Ответа ждём отдельным потоком: сторож тем временем сторожит дальше.
fn ask_open_wallpaper_engine(text: String) {
    std::thread::spawn(move || {
        let answer = crate::utils::notify_ask_live(&notice_title(), &text, &open_actions(), "", 24 * 3600);
        if answer.as_deref() == Some(ACTION_OPEN) {
            let section = format!("{}wallpaper_engine", crate::cli::SECTION_FLAG);
            if let Err(e) = crate::installer::open_rexon_live(&[&section]) {
                crate::log_warn!("wallpaper", "окно Wallpaper Engine не открылось: {e}");
            }
        }
    });
}

/// Сторожить, пока жив процесс. Звать в отдельном потоке.
///
/// Без `journalctl` или `busctl` сторожу не на что опереться — он говорит
/// это в журнал один раз и уходит.
pub fn run_forever() {
    let path = std::env::var("PATH").ok();
    for tool in ["journalctl", "busctl"] {
        if crate::utils::which(tool, path.as_deref()).is_none() {
            crate::log_info!("wallpaper", "сторож рисовальщика не запущен: нет {tool}");
            return;
        }
    }
    crate::log_info!("wallpaper", "сторож рисовальщика запущен: проход раз в {POLL_SECS} с");
    let mut watch = Watch::default();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(POLL_SECS));
        let alive = live_renderer_pids();
        // Демона спрашиваем, только когда есть о чём: рисовальщики живы
        // либо недавно пропали. В простое сторож не будит никого.
        if alive.is_empty() && watch.seen.is_empty() && watch.pending.is_empty() {
            continue;
        }
        let current = live_current_id();
        match watch.tick(now_secs(), &alive, &current, &live_crashed) {
            Step::Nothing => {}
            Step::Recover { id } => {
                crate::log_warn!("wallpaper", "рисовальщик обоев {id} аварийно завершился — применяю обои заново");
                let ok = live_apply(&id);
                crate::log_info!("wallpaper", "повторное применение обоев {id}: {}", if ok { "принято демоном" } else { "демон отказал" });
                if ok {
                    crate::utils::notify_live(&notice_title(), &recovered_text());
                } else {
                    ask_open_wallpaper_engine(recover_failed_text());
                }
            }
            Step::GiveUp { id } => {
                crate::log_warn!("wallpaper", "рисовальщик обоев {id} упал повторно — больше не перезапускаю");
                ask_open_wallpaper_engine(gave_up_text());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crashed_set(pids: &'static [u32]) -> impl Fn(u32) -> bool {
        move |pid| pids.contains(&pid)
    }

    #[test]
    fn crash_of_the_chosen_wallpaper_is_recovered_once() {
        let mut w = Watch::default();
        let none = crashed_set(&[]);
        assert_eq!(w.tick(0, &[100], "16352", &none), Step::Nothing);
        // Процесс пропал, записи об аварии ещё нет — ждём.
        assert_eq!(w.tick(10, &[], "16352", &none), Step::Nothing);
        // Запись появилась на следующем проходе — поднимаем.
        assert_eq!(w.tick(20, &[], "16352", &crashed_set(&[100])), Step::Recover { id: "16352".into() });
        // Демон поднял новый процесс; он упал снова в том же окне — отказ.
        assert_eq!(w.tick(30, &[200], "16352", &none), Step::Nothing);
        assert_eq!(w.tick(40, &[], "16352", &crashed_set(&[200])), Step::GiveUp { id: "16352".into() });
        // После отказа те же обои больше не трогаем, даже если их применили
        // руками и они упали ещё раз.
        assert_eq!(w.tick(50, &[300], "16352", &none), Step::Nothing);
        assert_eq!(w.tick(60, &[], "16352", &crashed_set(&[300])), Step::Nothing);
        // Другие обои — счёт заново.
        assert_eq!(w.tick(70, &[400], "777", &none), Step::Nothing);
        assert_eq!(w.tick(80, &[], "777", &crashed_set(&[400])), Step::Recover { id: "777".into() });
    }

    #[test]
    fn deliberate_stop_and_wallpaper_change_are_left_alone() {
        // Процесс пропал без записи об аварии (автопауза, выход демона).
        let mut w = Watch::default();
        let none = crashed_set(&[]);
        w.tick(0, &[100], "1", &none);
        for at in 1..=5 {
            assert_eq!(w.tick(at * 10, &[], "1", &none), Step::Nothing);
        }
        assert!(w.pending.is_empty(), "ожидание записи об аварии не кончилось");

        // Упал, но человек уже выбрал другие обои — не возвращаем старые.
        let mut w = Watch::default();
        w.tick(0, &[100], "1", &none);
        assert_eq!(w.tick(10, &[], "2", &crashed_set(&[100])), Step::Nothing);

        // Упал, но другой рисовальщик уже работает — демон справился сам.
        let mut w = Watch::default();
        w.tick(0, &[100], "1", &none);
        assert_eq!(w.tick(10, &[101], "1", &crashed_set(&[100])), Step::Nothing);

        // Демон молчит (обои неизвестны) — применять нечего.
        let mut w = Watch::default();
        w.tick(0, &[100], "", &none);
        assert_eq!(w.tick(10, &[], "", &crashed_set(&[100])), Step::Nothing);
    }

    #[test]
    fn old_recoveries_fall_out_of_the_window() {
        let mut w = Watch::default();
        let none = crashed_set(&[]);
        w.tick(0, &[1], "9", &none);
        assert_eq!(w.tick(10, &[], "9", &crashed_set(&[1])), Step::Recover { id: "9".into() });
        // Следующее падение — через час: это уже не «подряд».
        let later = 10 + RECOVERY_WINDOW_SECS + 3000;
        w.tick(later, &[2], "9", &none);
        assert_eq!(w.tick(later + 10, &[], "9", &crashed_set(&[2])), Step::Recover { id: "9".into() });
    }

    #[test]
    fn system_answers_are_parsed() {
        assert!(is_renderer_exe("/home/u/.local/share/waywallen/plugins/x/bin/waywallen-wescene-renderer"));
        assert!(is_renderer_exe("waywallen-weweb-renderer"));
        assert!(!is_renderer_exe("/tmp/.mount_x/usr/bin/waywallen"));
        assert!(!is_renderer_exe("rexon-wallpaper-host"));
        assert_eq!(busctl_string("s \"16352\"\n"), "16352");
        assert_eq!(busctl_string("s \"\"\n"), "");
        assert_eq!(busctl_string("u 5\n"), "");
        let entry = r#"{"COREDUMP_PID":"729862","COREDUMP_EXE":"/home/u/p/bin/waywallen-wescene-renderer","COREDUMP_SIGNAL_NAME":"SIGABRT"}"#;
        assert!(journal_names_renderer_crash(entry));
        assert!(!journal_names_renderer_crash(r#"{"COREDUMP_PID":"5","COREDUMP_EXE":"/usr/bin/firefox"}"#));
        assert!(!journal_names_renderer_crash(""));
    }
}
