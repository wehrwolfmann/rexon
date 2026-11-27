//! Shared inotify watching of directories: "something changed on disk" without
//! polling.
//!
//! WHY A SINGLE ONE. Three places had their own watching: the "Conky widgets"
//! section (`conky_scan`), the wallpaper engine package guard (the tray's
//! `we_socket`: the Rexon package installed a new plugin while the running
//! daemon holds the old one) and the appearance guard (`theme_watch`: the theme
//! was changed in the desktop's settings). Every copy of parsing kernel events
//! is a copy of its bugs (rule #12), so watching lives here, and the places
//! only provide their targets and event set.
//!
//! HOW. We watch DIRECTORIES, not files: package managers, editors and settings
//! programs replace a file (delete + create, `rename` over it), and a watch on
//! the file itself goes deaf after the very first replacement. If a directory
//! does not exist yet, a watch is put on it when it appears (its parent must
//! also be among the targets; otherwise — [`Watch::rescan`] on a timer). If a
//! directory is deleted or moved (`IN_DELETE_SELF`, `IN_MOVE_SELF`), the watch
//! is removed and will be set again on the same path; whether this is a change
//! is decided by the user's event set. A queue overflow is always a change:
//! what was missed can no longer be known.

use std::collections::HashMap;
use std::path::Path;

/// A directory to watch, and the names whose appearance, disappearance or
/// writes in it mean something (empty — any name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchTarget {
    pub dir: String,
    pub names: Vec<String>,
}

/// An inotify watch over a set of [`WatchTarget`].
pub struct Watch {
    fd: i32,
    targets: Vec<WatchTarget>,
    /// Which kernel events mean "changed" ([`ANY_CHANGE`] or a custom set).
    events: u32,
    /// Watch number → target index; targets without a watch (no directory) are not
    /// in the map.
    watches: HashMap<i32, usize>,
}

/// What woke up the wait in [`Watch::wait_wake`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wake {
    /// Something changed in the targets (or the event queue overflowed).
    Relevant,
    /// Woken by an unrelated event: a foreign name, a service event.
    Other,
    /// The timeout expired, there were no events.
    Timeout,
}

/// Default event set: appearance, disappearance, writing and moving of a name,
/// as well as the directory itself going away.
pub const ANY_CHANGE: u32 = libc::IN_CREATE
    | libc::IN_DELETE
    | libc::IN_MOVED_TO
    | libc::IN_MOVED_FROM
    | libc::IN_CLOSE_WRITE
    | libc::IN_DELETE_SELF
    | libc::IN_MOVE_SELF;

/// The directory itself going away. The core always needs these events —
/// without them, after the directory moved, the watch would keep looking at it
/// in its new location.
const SELF_GONE: u32 = libc::IN_DELETE_SELF | libc::IN_MOVE_SELF;

/// Whether a kernel event means "look again" with the [`ANY_CHANGE`] set.
/// `names` — the target's names (empty — any), `name` — the name from the event
/// (empty for events of the directory itself). A queue overflow and the
/// directory itself going away always mean "yes": what was missed can no longer
/// be known.
pub fn event_matters(mask: u32, names: &[String], name: &str) -> bool {
    matters(ANY_CHANGE, mask, names, name)
}

/// The same for the `events` set: the directory going away is a change only if
/// it is in the set; an overflow — always.
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

/// How much longer to wait for the end of a burst of writes: `elapsed_ms` have
/// passed since the first of them. `None` — the burst drags on longer than
/// `max_ms`, enough waiting (look now). Otherwise — a quiet pause of
/// `quiet_ms`, but not beyond the ceiling.
pub fn burst_wait_ms(elapsed_ms: u64, quiet_ms: i32, max_ms: u64) -> Option<i32> {
    if elapsed_ms >= max_ms {
        return None;
    }
    let left = (max_ms - elapsed_ms).min(i32::MAX as u64) as i32;
    Some(quiet_ms.max(0).min(left))
}

impl Watch {
    /// A watch with the [`ANY_CHANGE`] event set.
    pub fn new(targets: Vec<WatchTarget>) -> Option<Self> {
        Self::with_events(targets, ANY_CHANGE)
    }

    /// A watch for which only the `events` events (`IN_*` flags) mean "changed";
    /// the rest wake [`Self::wait_wake`] as [`Wake::Other`].
    pub fn with_events(targets: Vec<WatchTarget>, events: u32) -> Option<Self> {
        // SAFETY: an ordinary system call without pointers.
        let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if fd < 0 {
            return None;
        }
        let mut me = Watch { fd, targets, events, watches: HashMap::new() };
        me.rescan();
        Some(me)
    }

    /// Set watches on directories that do not have one yet (that have appeared).
    /// After each wake-up from events this happens by itself; without events (the
    /// directory's parent is not among the targets) call it on a timer.
    pub fn rescan(&mut self) {
        let mask = self.events | SELF_GONE;
        for (i, t) in self.targets.iter().enumerate() {
            if self.watches.values().any(|v| *v == i) || !Path::new(&t.dir).is_dir() {
                continue;
            }
            let Ok(path) = std::ffi::CString::new(t.dir.as_str()) else { continue };
            // SAFETY: `path` is a live C string until the end of the call.
            let wd = unsafe { libc::inotify_add_watch(self.fd, path.as_ptr(), mask) };
            if wd >= 0 {
                self.watches.insert(wd, i);
            }
        }
    }

    /// Wait up to `timeout_ms` (−1 — no timeout). `true` — something changed in the
    /// targets (or the event queue overflowed — then look as well).
    pub fn wait(&mut self, timeout_ms: i32) -> bool {
        self.wait_wake(timeout_ms) == Wake::Relevant
    }

    /// Like [`Self::wait`], but distinguishes an unrelated event from an expired timeout.
    pub fn wait_wake(&mut self, timeout_ms: i32) -> Wake {
        let mut pfd = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
        // SAFETY: a single descriptor, the structure lives until the end of the call.
        let n = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
        if n <= 0 {
            return Wake::Timeout;
        }
        let mut changed = false;
        let mut buf = [0u8; 8192];
        loop {
            // SAFETY: the buffer is ours, the length is its size.
            let len = unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) };
            if len <= 0 {
                break;
            }
            let len = len as usize;
            let head = std::mem::size_of::<libc::inotify_event>();
            let mut off = 0usize;
            while off + head <= len {
                // SAFETY: the kernel writes whole events; alignment is not
                // guaranteed — we read via a copy.
                let ev: libc::inotify_event = unsafe { std::ptr::read_unaligned(buf.as_ptr().add(off).cast()) };
                let name_len = ev.len as usize;
                let raw = &buf[off + head..(off + head + name_len).min(len)];
                let name = String::from_utf8_lossy(raw.split(|b| *b == 0).next().unwrap_or(&[])).into_owned();
                off += head + name_len;
                changed |= self.on_event(ev.wd, ev.mask, &name);
            }
        }
        // A target directory appeared (or vanished) — re-set the watch.
        self.rescan();
        if changed { Wake::Relevant } else { Wake::Other }
    }

    /// Account for one kernel event; `true` — it is a change in the targets.
    fn on_event(&mut self, wd: i32, mask: u32, name: &str) -> bool {
        if mask & libc::IN_Q_OVERFLOW != 0 {
            return true;
        }
        let names = self.watches.get(&wd).and_then(|i| self.targets.get(*i)).map(|t| t.names.as_slice());
        let relevant = names.is_some_and(|n| matters(self.events, mask, n, name));
        if mask & libc::IN_IGNORED != 0 {
            self.watches.remove(&wd);
        } else if mask & libc::IN_MOVE_SELF != 0 {
            // The watch went away together with the directory to its new place; it has to
            // be set on the old path again.
            // SAFETY: the descriptor and the watch number are ours.
            unsafe { libc::inotify_rm_watch(self.fd, wd) };
            self.watches.remove(&wd);
        }
        relevant
    }

    /// Wait for a change and the end of the burst of writes: after the first change,
    /// wait until there are no new ones for `quiet_ms` in a row (a package manager
    /// writes dozens of files in a row), but no longer than `max_ms` from the first.
    pub fn wait_burst(&mut self, quiet_ms: i32, max_ms: u64) {
        while !self.wait(-1) {}
        self.settle(quiet_ms, max_ms);
    }

    /// A change has already arrived — wait for the end of the burst (as in [`Self::wait_burst`]).
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
        // SAFETY: the descriptor is ours and is closed once.
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
        // The directory itself going away and an overflow are always a reason to look.
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
        // A burst of ~200 ms + 300 ms of quiet; far from the ceiling.
        let took = start.elapsed().as_millis();
        assert!((300..4000).contains(&took), "{took} мс");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The directory was deleted and created again (this is how a package manager
    /// may replace a plugin folder): the removal is noticed, and the watch is set on
    /// the new one.
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
        // The watch is set on the new directory — writes to it are heard.
        while w.wait(100) {}
        std::fs::write(sub.join("plugin.toml"), "x").unwrap();
        assert!(w.wait(500));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The directory was replaced via `rename` (a new one next to it, then over it).
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
    /// The event set of the appearance guard (`theme_watch`): writing and
    /// appearance of a name, without disappearance.
    const WRITES: u32 = libc::IN_CLOSE_WRITE | libc::IN_MOVED_TO | libc::IN_CREATE;

    /// An unrelated name wakes as [`Wake::Other`], not as an expired timeout: the
    /// appearance guard does not cut its quiet pause short on unrelated events.
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

    /// Atomic write by an editor and dconf: a temporary file next to it, then
    /// `rename` over it. Heard with any event set, and the watch does not go deaf
    /// after the replacement.
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

    /// Deleting a file is a change for the default set and an unrelated event for a
    /// set without `IN_DELETE`.
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
        // The kernel does not send what was not asked for: the timeout expires.
        assert_eq!(quiet.wait_wake(200), Wake::Timeout);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The target directory vanished and appeared again (its parent is also among
    /// the targets): for a set without directory removal these are unrelated
    /// events, but the watch is set on the new directory by itself, without a timed
    /// [`Watch::rescan`].
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

    /// The directory moved: the watch does not go with it but stays on the old path.
    #[test]
    fn moved_away_dir_is_not_followed() {
        let d = tmp("moved");
        let sub = d.join("dconf");
        std::fs::create_dir_all(&sub).unwrap();
        let mut w =
            Watch::with_events(vec![target(&d, &[]), target(&sub, &["user"])], WRITES).expect("inotify");
        std::fs::rename(&sub, d.join("dconf.old")).unwrap();
        while w.wait_wake(200) != Wake::Timeout {}
        // Writing to the moved directory is no longer our target.
        std::fs::write(d.join("dconf.old").join("user"), "x").unwrap();
        assert_ne!(w.wait_wake(200), Wake::Relevant, "слежка уехала вместе с каталогом");
        std::fs::create_dir_all(&sub).unwrap();
        while w.wait_wake(200) != Wake::Timeout {}
        std::fs::write(sub.join("user"), "x").unwrap();
        assert_eq!(w.wait_wake(500), Wake::Relevant);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The directory did not exist at start-up and its parent is not among the
    /// targets: there will be no events, the watch is set by [`Watch::rescan`].
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

    /// A kernel queue overflow is a change with any set, even without a watch number
    /// (the kernel sends it with `wd = -1`); the directory going away — only if it
    /// was asked for.
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
