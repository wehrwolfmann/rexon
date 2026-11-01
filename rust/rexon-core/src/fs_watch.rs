//! Общая слежка inotify за каталогами: «что-то поменялось на диске» без
//! опроса.
//!
//! ЗАЧЕМ ОДНА. Своя слежка была у трёх мест: раздел «Виджеты Conky»
//! (`conky_scan`), сторож пакета движка обоев (`we_socket` лотка: пакет Rexon
//! положил новый плагин — живой демон держит старый) и сторож оформления
//! (`theme_watch`: тему сменили в настройках стола). Каждая копия разбора
//! событий ядра — копия ошибок в нём (правило #12), поэтому слежка живёт
//! здесь, а места отдают только свои цели и набор событий.
//!
//! КАК. Следим за КАТАЛОГАМИ, а не файлами: пакетные менеджеры, редакторы и
//! программы настроек заменяют файл (удалить + создать, `rename` поверх), и
//! слежка за самим файлом после первой же замены глохнет. Каталога ещё нет —
//! слежка за ним ставится, когда он появится (его родитель тоже должен быть
//! в целях; иначе — [`Watch::rescan`] по сроку). Каталог удалён или переехал
//! (`IN_DELETE_SELF`, `IN_MOVE_SELF`) — слежка снимается и встанет заново на
//! тот же путь; изменение ли это, решает набор событий пользователя.
//! Переполнение очереди — изменение всегда: что пропущено, уже не узнать.

use std::collections::HashMap;
use std::path::Path;

/// Каталог для слежки и имена, появление, исчезновение или запись которых в
/// нём что-то значат (пусто — любое имя).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchTarget {
    pub dir: String,
    pub names: Vec<String>,
}

/// Слежка inotify за набором [`WatchTarget`].
pub struct Watch {
    fd: i32,
    targets: Vec<WatchTarget>,
    /// Какие события ядра значат «поменялось» ([`ANY_CHANGE`] или свой набор).
    events: u32,
    /// Номер слежки → индекс цели; цели без слежки (каталога нет) — нет в карте.
    watches: HashMap<i32, usize>,
}

/// Чем проснулось ожидание [`Watch::wait_wake`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// В целях что-то поменялось (или очередь событий переполнилась).
    Relevant,
    /// Проснулись от постороннего события: чужое имя, служебное событие.
    Other,
    /// Срок вышел, событий не было.
    Timeout,
}

/// Набор событий по умолчанию: появление, исчезновение, запись и переезд
/// имени, а также уход самого каталога.
pub const ANY_CHANGE: u32 = libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_MOVED_TO
    | libc::IN_MOVED_FROM
    | libc::IN_CLOSE_WRITE
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF;

/// Уход самого каталога. Ядру эти события нужны всегда — без них слежка
/// после переезда каталога продолжала бы смотреть на него по новому месту.
const SELF_GONE: u32 = libc::IN_DELETE_SELF | libc::IN_MOVE_SELF;

/// Значит ли событие ядра «смотреть заново» при наборе [`ANY_CHANGE`].
/// `names` — имена цели (пусто — любое), `name` — имя из события (у событий
/// самого каталога пусто). Переполнение очереди и уход самого каталога
/// значат «да» всегда: что пропущено, уже не узнать.
pub fn event_matters(mask: u32, names: &[String], name: &str) -> bool {
    matters(ANY_CHANGE, mask, names, name)
}

/// То же для набора `events`: уход каталога — изменение, только если он в
/// наборе; переполнение — всегда.
fn matters(events: u32, mask: u32, names: &[String], name: &str) -> bool {
    if mask & libc::IN_Q_OVERFLOW != 0 {
        return true;
    }
    if mask & SELF_GONE != 0 {
        return events & mask & SELF_GONE != 0;
    }
    if mask & libc::IN_IGNORED != 0 {
        return false;
    }
    names.is_empty() || names.iter().any(|n| n == name)
}

/// Сколько ещё ждать конца пачки записей: прошло `elapsed_ms` с первой из
/// них. `None` — пачка тянется дольше `max_ms`, хватит ждать (смотреть
/// сейчас). Иначе — пауза тишины `quiet_ms`, но не дальше потолка.
pub fn burst_wait_ms(elapsed_ms: u64, quiet_ms: i32, max_ms: u64) -> Option<i32> {
    if elapsed_ms >= max_ms {
        return None;
    }
    let left = (max_ms - elapsed_ms).min(i32::MAX as u64) as i32;
    Some(quiet_ms.max(0).min(left))
}

impl Watch {
    /// Слежка с набором событий [`ANY_CHANGE`].
    pub fn new(targets: Vec<WatchTarget>) -> Option<Self> {
        Self::with_events(targets, ANY_CHANGE)
    }

    /// Слежка, у которой «поменялось» значат только события `events`
    /// (флаги `IN_*`); остальные будят [`Self::wait_wake`] как [`Wake::Other`].
    pub fn with_events(targets: Vec<WatchTarget>, events: u32) -> Option<Self> {
        // SAFETY: обычный системный вызов без указателей.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return None;
        }
        let mut me = Watch { fd, targets, events, watches: HashMap::new() };
        me.rescan();
        Some(me)
    }

    /// Поставить слежку за каталогами, у которых её ещё нет (появились).
    /// После каждого пробуждения от событий это делается само; без событий
    /// (родителя каталога нет в целях) — звать по сроку.
    pub fn rescan(&mut self) {
        let mask = self.events | SELF_GONE;
        for (i, t) in self.targets.iter().enumerate() {
            if self.watches.values().any(|v| *v == i) || !Path::new(&t.dir).is_dir() {
                continue;
            }
            let Ok(path) = std::ffi::CString::new(t.dir.as_str()) else { continue };
            // SAFETY: `path` — живая C-строка до конца вызова.
            let wd = unsafe { libc::inotify_add_watch(self.fd, path.as_ptr(), mask) };
            if wd >= 0 {
                self.watches.insert(wd, i);
            }
        }
    }

    /// Ждать до `timeout_ms` (−1 — без срока). `true` — в целях что-то
    /// поменялось (или очередь событий переполнилась — тогда тоже смотреть).
    pub fn wait(&mut self, timeout_ms: i32) -> bool {
        self.wait_wake(timeout_ms) == Wake::Relevant
    }

    /// Как [`Self::wait`], но отличает постороннее событие от вышедшего срока.
    pub fn wait_wake(&mut self, timeout_ms: i32) -> Wake {
        let mut pfd = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: один дескриптор, структура живёт до конца вызова.
        let n = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if n <= 0 {
            return Wake::Timeout;
        }
        let mut changed = false;
        let mut buf = [0u8; 8192];
        loop {
            // SAFETY: буфер наш, длина — его размер.
            let len = unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) };
            if len <= 0 {
                break;
            }
            let len = len as usize;
            let head = std::mem::size_of::<libc::inotify_event>();
            let mut off = 0usize;
            while off + head <= len {
                // SAFETY: ядро кладёт события целиком; выравнивание не
                // гарантировано — читаем копией.
                let ev: libc::inotify_event = unsafe { std::ptr::read_unaligned(buf.as_ptr().add(off).cast()) };
                let name_len = ev.len as usize;
                let raw = &buf[off + head..(off + head + name_len).min(len)];
                let name = String::from_utf8_lossy(raw.split(|b| *b == 0).next().unwrap_or(&[])).into_owned();
                off += head + name_len;
                changed |= self.on_event(ev.wd, ev.mask, &name);
            }
        }
        // Появился каталог-место (или пропал) — переставить слежку.
        self.rescan();
        if changed { Wake::Relevant } else { Wake::Other }
    }

    /// Учесть одно событие ядра; `true` — это изменение в целях.
    fn on_event(&mut self, wd: i32, mask: u32, name: &str) -> bool {
        if mask & libc::IN_Q_OVERFLOW != 0 {
            return true;
        }
        let names = self.watches.get(&wd).and_then(|i| self.targets.get(*i)).map(|t| t.names.as_slice());
        let relevant = names.is_some_and(|n| matters(self.events, mask, n, name));
        if mask & libc::IN_IGNORED != 0 {
            self.watches.remove(&wd);
        } else if mask & libc::IN_MOVE_SELF != 0 {
            // Слежка ушла вместе с каталогом на новое место; на прежний
            // путь её надо ставить заново.
            // SAFETY: дескриптор и номер слежки наши.
            unsafe { libc::inotify_rm_watch(self.fd, wd) };
            self.watches.remove(&wd);
        }
        relevant
    }

    /// Дождаться изменения и конца пачки записей: после первого изменения
    /// ждать, пока `quiet_ms` подряд не будет новых (пакетный менеджер пишет
    /// десятки файлов подряд), но не дольше `max_ms` от первого.
    pub fn wait_burst(&mut self, quiet_ms: i32, max_ms: u64) {
        while !self.wait(-1) {}
        self.settle(quiet_ms, max_ms);
    }

    /// Изменение уже пришло — дождаться конца пачки (как в [`Self::wait_burst`]).
    pub fn settle(&mut self, quiet_ms: i32, max_ms: u64) {
        let start = std::time::Instant::now();
        while let Some(ms) = burst_wait_ms(start.elapsed().as_millis() as u64, quiet_ms, max_ms) {
            if !self.wait(ms) {
                break;
            }
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: дескриптор наш и закрывается один раз.
        unsafe { libc::close(self.fd) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-fswatch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn target(dir: &Path, names: &[&str]) -> WatchTarget {
        WatchTarget { dir: dir.to_string_lossy().into_owned(), names: names.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn events_are_filtered_by_name() {
        let toml = vec!["plugin.toml".to_string()];
        assert!(event_matters(libc::IN_CLOSE_WRITE, &toml, "plugin.toml"));
        assert!(event_matters(libc::IN_MOVED_TO, &toml, "plugin.toml"));
        assert!(!event_matters(libc::IN_CLOSE_WRITE, &toml, "libowe.so"));
        assert!(event_matters(libc::IN_CREATE | libc::IN_ISDIR, &[], "new-plugin"));
        // Уход самого каталога и переполнение — всегда повод смотреть.
        assert!(event_matters(libc::IN_DELETE_SELF, &toml, ""));
        assert!(event_matters(libc::IN_MOVE_SELF, &toml, ""));
        assert!(event_matters(libc::IN_Q_OVERFLOW, &toml, ""));
        assert!(!event_matters(libc::IN_IGNORED, &[], ""));
    }

    #[test]
    fn burst_waits_for_quiet_but_not_forever() {
        assert_eq!(burst_wait_ms(0, 8000, 120_000), Some(8000));
        assert_eq!(burst_wait_ms(115_000, 8000, 120_000), Some(5000));
        assert_eq!(burst_wait_ms(120_000, 8000, 120_000), None);
        assert_eq!(burst_wait_ms(200_000, 8000, 120_000), None);
    }

    #[test]
    fn wait_burst_returns_after_quiet() {
        let d = tmp("burst");
        let mut w = Watch::new(vec![target(&d, &[])]).expect("inotify");
        let dir = d.clone();
        let writer = std::thread::spawn(move || {
            for i in 0..5 {
                std::fs::write(dir.join(format!("f{i}")), "x").unwrap();
                std::thread::sleep(std::time::Duration::from_millis(40));
            }
        });
        let start = std::time::Instant::now();
        w.wait_burst(300, 5000);
        writer.join().unwrap();
        // Пачка ~200 мс + тишина 300 мс; до потолка далеко.
        let took = start.elapsed().as_millis();
        assert!((300..4000).contains(&took), "{took} мс");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Каталог удалили и создали заново (так пакетный менеджер может
    /// заменить папку плагина): уход замечен, а слежка встаёт на новый.
    #[test]
    fn deleted_and_recreated_dir_is_watched_again() {
        let d = tmp("recreate");
        let sub = d.join("plugins");
        std::fs::create_dir_all(&sub).unwrap();
        let mut w = Watch::new(vec![target(&d, &["plugins"]), target(&sub, &["plugin.toml"])]).expect("inotify");
        std::fs::write(sub.join("other"), "x").unwrap();
        assert!(!w.wait(200), "постороннее имя — не повод");
        std::fs::remove_dir_all(&sub).unwrap();
        assert!(w.wait(500));
        std::fs::create_dir_all(&sub).unwrap();
        assert!(w.wait(500));
        // Слежка встала на новый каталог — запись в него слышна.
        while w.wait(100) {}
        std::fs::write(sub.join("plugin.toml"), "x").unwrap();
        assert!(w.wait(500));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Каталог подменили через `rename` (новый рядом, потом поверх).
    #[test]
    fn dir_replaced_by_rename_is_noticed() {
        let d = tmp("rename");
        let sub = d.join("plugins");
        std::fs::create_dir_all(&sub).unwrap();
        let mut w = Watch::new(vec![target(&d, &["plugins"]), target(&sub, &["plugin.toml"])]).expect("inotify");
        let fresh = d.join("plugins.new");
        std::fs::create_dir_all(&fresh).unwrap();
        assert!(!w.wait(200), "чужое имя в родителе — не повод");
        std::fs::rename(&sub, d.join("plugins.old")).unwrap();
        assert!(w.wait(500));
        std::fs::rename(&fresh, &sub).unwrap();
        assert!(w.wait(500));
        while w.wait(100) {}
        std::fs::write(sub.join("plugin.toml"), "x").unwrap();
        assert!(w.wait(500), "слежка за новым каталогом по прежнему пути");
        let _ = std::fs::remove_dir_all(&d);
    }
    /// Набор событий сторожа оформления (`theme_watch`): запись и появление
    /// имени, без исчезновения.
    const WRITES: u32 = libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_CREATE;

    /// Постороннее имя будит как [`Wake::Other`], а не как вышедший срок:
    /// сторож оформления на постороннем не обрывает паузу затишья.
    #[test]
    fn wake_tells_unrelated_write_from_timeout() {
        let d = tmp("wake");
        let mut w = Watch::with_events(vec![target(&d, &["a"])], WRITES).expect("inotify");
        assert_eq!(w.wait_wake(50), Wake::Timeout);
        std::fs::write(d.join("b"), "x").unwrap();
        assert_eq!(w.wait_wake(500), Wake::Other);
        std::fs::write(d.join("a"), "x").unwrap();
        assert_eq!(w.wait_wake(500), Wake::Relevant);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Атомарная запись редактора и dconf: временный файл рядом, потом
    /// `rename` поверх. Слышно с любым набором событий, и слежка после
    /// замены не глохнет.
    #[test]
    fn atomic_save_by_rename_is_heard_every_time() {
        let d = tmp("atomic");
        std::fs::write(d.join("settings.ini"), "a").unwrap();
        for events in [ANY_CHANGE, WRITES] {
            let mut w = Watch::with_events(vec![target(&d, &["settings.ini"])], events).expect("inotify");
            for round in 0..3 {
                std::fs::write(d.join(".settings.ini.tmp"), format!("{round}")).unwrap();
                std::fs::rename(d.join(".settings.ini.tmp"), d.join("settings.ini")).unwrap();
                assert_eq!(w.wait_wake(500), Wake::Relevant, "замена №{round}, набор {events:#x}");
                while w.wait_wake(100) != Wake::Timeout {}
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Удаление файла — изменение для набора по умолчанию и постороннее
    /// событие для набора без `IN_DELETE`.
    #[test]
    fn deletion_counts_only_when_asked_for() {
        let d = tmp("delete");
        std::fs::write(d.join("a"), "x").unwrap();
        let mut any = Watch::new(vec![target(&d, &["a"])]).expect("inotify");
        let mut writes = Watch::with_events(vec![target(&d, &["a"])], WRITES | libc::IN_DELETE).expect("inotify");
        let mut quiet = Watch::with_events(vec![target(&d, &["a"])], WRITES).expect("inotify");
        std::fs::remove_file(d.join("a")).unwrap();
        assert_eq!(any.wait_wake(500), Wake::Relevant);
        assert_eq!(writes.wait_wake(500), Wake::Relevant);
        // Ядро не шлёт того, о чём не просили: срок выходит.
        assert_eq!(quiet.wait_wake(200), Wake::Timeout);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Каталог цели исчез и появился снова (его родитель тоже в целях): для
    /// набора без ухода каталога это посторонние события, но слежка встаёт
    /// на новый каталог сама, без [`Watch::rescan`] по сроку.
    #[test]
    fn vanished_dir_is_rewatched_with_custom_events() {
        let d = tmp("vanish");
        let sub = d.join("gtk-3.0");
        std::fs::create_dir_all(&sub).unwrap();
        let mut w = Watch::with_events(vec![target(&d, &["kdeglobals"]), target(&sub, &["settings.ini"])], WRITES)
            .expect("inotify");
        std::fs::remove_dir_all(&sub).unwrap();
        assert_eq!(w.wait_wake(500), Wake::Other);
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(w.wait_wake(500), Wake::Other);
        std::fs::write(sub.join("settings.ini"), "x").unwrap();
        assert_eq!(w.wait_wake(500), Wake::Relevant);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Каталог переехал: слежка не уходит с ним, а остаётся на прежнем пути.
    #[test]
    fn moved_away_dir_is_not_followed() {
        let d = tmp("moved");
        let sub = d.join("dconf");
        std::fs::create_dir_all(&sub).unwrap();
        let mut w =
            Watch::with_events(vec![target(&d, &[]), target(&sub, &["user"])], WRITES).expect("inotify");
        std::fs::rename(&sub, d.join("dconf.old")).unwrap();
        while w.wait_wake(200) != Wake::Timeout {}
        // Запись в уехавший каталог — уже не наша цель.
        std::fs::write(d.join("dconf.old").join("user"), "x").unwrap();
        assert_ne!(w.wait_wake(200), Wake::Relevant, "слежка уехала вместе с каталогом");
        std::fs::create_dir_all(&sub).unwrap();
        while w.wait_wake(200) != Wake::Timeout {}
        std::fs::write(sub.join("user"), "x").unwrap();
        assert_eq!(w.wait_wake(500), Wake::Relevant);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Каталога не было при запуске, а родителя нет в целях: событий не
    /// будет, слежку ставит [`Watch::rescan`].
    #[test]
    fn rescan_picks_up_dir_without_watched_parent() {
        let d = tmp("rescan");
        let sub = d.join("later");
        let mut w = Watch::with_events(vec![target(&sub, &["a"])], WRITES).expect("inotify");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(w.wait_wake(100), Wake::Timeout);
        w.rescan();
        std::fs::write(sub.join("a"), "x").unwrap();
        assert_eq!(w.wait_wake(500), Wake::Relevant);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Переполнение очереди ядра — изменение при любом наборе, даже без
    /// номера слежки (ядро шлёт его с `wd = -1`); уход каталога — только
    /// если о нём просили.
    #[test]
    fn overflow_always_matters() {
        let d = tmp("overflow");
        for events in [ANY_CHANGE, WRITES] {
            let mut w = Watch::with_events(vec![target(&d, &["a"])], events).expect("inotify");
            assert!(w.on_event(-1, libc::IN_Q_OVERFLOW, ""));
        }
        assert!(matters(WRITES, libc::IN_Q_OVERFLOW, &["a".to_string()], ""));
        assert!(!matters(WRITES, libc::IN_DELETE_SELF, &[], ""));
        assert!(!matters(WRITES, libc::IN_MOVE_SELF, &[], ""));
        assert!(matters(ANY_CHANGE, libc::IN_MOVE_SELF, &["a".to_string()], ""));
        let _ = std::fs::remove_dir_all(&d);
    }
}
