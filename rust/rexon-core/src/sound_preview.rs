//! Sound theme preview — the core of the «listen» buttons in the «Sounds» section.
//!
//! Three parts:
//! * **five theme sounds** ([`PREVIEW_SLOTS`]) — the same set as the KDE
//!   «System Sounds» module: window bell, warning, new message,
//!   battery, device plugged in; each button has its own fallback list
//!   of events (`dialog-warning` → `dialog-information` etc.);
//! * **finding an event file per the freedesktop sound-theme spec**
//!   ([`find_event_file`]): theme directories from `Directories` with the
//!   `stereo` profile first, extensions `.disabled`/`.oga`/`.ogg`/`.wav`, the
//!   `Inherits` chain, then the `freedesktop` theme; the event name is shortened at
//!   the last `-` (`message-new-instant` → `message-new` → `message`);
//! * **previewing a Store theme before installing**
//!   ([`prepare_store_preview`]): the archive is downloaded into a temporary cache
//!   `~/.cache/rexon/sound-preview/<id>/`, unpacked, and the theme root is
//!   found by the same helper the installer uses
//!   ([`crate::appearance_installer::find_theme_roots`]) — nothing is installed.
//!
//! Playback — [`play_argv`]: an installed theme is played by libcanberra by
//! event (exactly what the system will sound like), otherwise the file is played
//! by the first suitable player ([`crate::utils::sound_play_argv`]).
//!
//! ⚠️ Locale subdirectories from the spec (`stereo/<language>/`) are not scanned:
//! none were seen in themes on the machine or in the Store; a file in the root of
//! the profile directory is found as usual.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::i18n::{tr, trf};

/// Preview button: key (for the icon in QML) and events in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewSlot {
    pub key: &'static str,
    pub events: &'static [&'static str],
}

/// Five theme sounds — the set and fallback events as in the KDE
/// «System Sounds» module (`kcm_soundtheme`).
pub const PREVIEW_SLOTS: &[PreviewSlot] = &[
    PreviewSlot { key: "bell", events: &["bell-window-system"] },
    PreviewSlot { key: "warning", events: &["dialog-warning", "dialog-information"] },
    PreviewSlot { key: "message", events: &["message-new-instant", "message-new-email"] },
    PreviewSlot { key: "battery", events: &["battery-caution", "battery-low"] },
    PreviewSlot { key: "device", events: &["device-added", "device-removed"] },
];

/// «Theme demo» (on clicking the theme itself) — the same search order.
pub const DEMO_EVENTS: &[&str] = &["theme-demo", "desktop-login", "service-login"];

/// Theme the search falls back to when neither the theme nor its parents gave a sound.
pub const FALLBACK_THEME: &str = "freedesktop";

/// Extensions per the spec; `disabled` means «this event's sound is turned off».
const LOOKUP_EXTS: &[&str] = &["disabled", "oga", "ogg", "wav"];

/// Parsed `index.theme` of a sound theme.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoundThemeIndex {
    pub inherits: Vec<String>,
    /// `(subdirectory, OutputProfile)` in `Directories` order.
    pub dirs: Vec<(String, String)>,
}

/// Parse `index.theme`; `None` — no `[Sound Theme]` section (not a theme).
pub fn parse_sound_index(text: &str) -> Option<SoundThemeIndex> {
    let mut section = String::new();
    let mut is_theme = false;
    let mut directories: Vec<String> = Vec::new();
    let mut inherits: Vec<String> = Vec::new();
    let mut profiles: std::collections::HashMap<String, String> = Default::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_string();
            if section == "Sound Theme" {
                is_theme = true;
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), value.trim());
        let list = || -> Vec<String> {
            value.split(',').map(str::trim).filter(|v| !v.is_empty()).map(str::to_string).collect()
        };
        if section == "Sound Theme" {
            match key {
                "Directories" => directories = list(),
                "Inherits" => inherits = list(),
                _ => {}
            }
        } else if key == "OutputProfile" {
            profiles.insert(section.clone(), value.to_string());
        }
    }
    if !is_theme {
        return None;
    }
    let dirs = directories
        .into_iter()
        .map(|d| {
            let p = profiles.get(&d).cloned().unwrap_or_else(|| "stereo".to_string());
            (d, p)
        })
        .collect();
    Some(SoundThemeIndex { inherits, dirs })
}

/// Result of a search in one theme.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Lookup {
    Found(String),
    /// `<event>.disabled`: the sound is turned off by the theme — stop searching.
    Disabled,
    NotFound,
}

/// Search for `name` in the theme itself (without parents): `stereo` profile
/// directories first, then the rest; without `Directories` — the theme root.
fn lookup_in_theme(root: &Path, index: &SoundThemeIndex, name: &str) -> Lookup {
    let mut subdirs: Vec<&str> = index
        .dirs
        .iter()
        .filter(|(_, p)| p == "stereo")
        .map(|(d, _)| d.as_str())
        .collect();
    subdirs.extend(index.dirs.iter().filter(|(_, p)| p != "stereo").map(|(d, _)| d.as_str()));
    if subdirs.is_empty() {
        subdirs.push("");
    }
    for sub in subdirs {
        let dir = if sub.is_empty() { root.to_path_buf() } else { root.join(sub) };
        for ext in LOOKUP_EXTS {
            let p = dir.join(format!("{name}.{ext}"));
            if p.is_file() {
                return if *ext == "disabled" {
                    Lookup::Disabled
                } else {
                    Lookup::Found(p.to_string_lossy().into_owned())
                };
            }
        }
    }
    Lookup::NotFound
}

/// The theme and its parents (`Inherits`, depth-first, no repeats).
fn lookup_chain(
    resolve: &dyn Fn(&str) -> Option<PathBuf>,
    theme: &str,
    name: &str,
    visited: &mut Vec<String>,
) -> Lookup {
    if visited.iter().any(|v| v == theme) || visited.len() > 16 {
        return Lookup::NotFound;
    }
    visited.push(theme.to_string());
    let Some(root) = resolve(theme) else { return Lookup::NotFound };
    let Some(index) = std::fs::read_to_string(root.join("index.theme"))
        .ok()
        .and_then(|t| parse_sound_index(&t))
    else {
        return Lookup::NotFound;
    };
    match lookup_in_theme(&root, &index, name) {
        Lookup::NotFound => {}
        hit => return hit,
    }
    for parent in &index.inherits {
        match lookup_chain(resolve, parent, name, visited) {
            Lookup::NotFound => {}
            hit => return hit,
        }
    }
    Lookup::NotFound
}

/// Event names from exact to generic: `a-b-c`, `a-b`, `a`.
pub fn event_name_fallbacks(event: &str) -> Vec<String> {
    let mut out = vec![event.to_string()];
    let mut cur = event;
    while let Some((head, _)) = cur.rsplit_once('-') {
        if head.is_empty() {
            break;
        }
        out.push(head.to_string());
        cur = head;
    }
    out
}

/// File of event `event` in theme `theme` per the spec; `None` — no sound
/// or the theme turned it off (`.disabled`). `resolve` — theme directory by name.
pub fn find_event_file(
    resolve: &dyn Fn(&str) -> Option<PathBuf>,
    theme: &str,
    event: &str,
) -> Option<String> {
    for name in event_name_fallbacks(event) {
        let mut visited = Vec::new();
        let mut hit = lookup_chain(resolve, theme, &name, &mut visited);
        if hit == Lookup::NotFound && !visited.iter().any(|v| v == FALLBACK_THEME) {
            hit = lookup_chain(resolve, FALLBACK_THEME, &name, &mut visited);
        }
        match hit {
            Lookup::Found(p) => return Some(p),
            Lookup::Disabled => return None,
            Lookup::NotFound => {}
        }
    }
    None
}

/// Find a theme by name in the `dirs` directories (the first with a real theme).
pub fn resolver_for(dirs: Vec<String>) -> impl Fn(&str) -> Option<PathBuf> {
    move |name: &str| {
        if name.is_empty() || name.contains('/') {
            return None;
        }
        dirs.iter()
            .map(|d| Path::new(d).join(name))
            .find(|p| crate::qt_settings::is_sound_theme_dir(p))
    }
}

/// The sound of one button: which event was found and where the file is (`""` — nothing to play).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewSound {
    pub key: String,
    pub event: String,
    pub file: String,
}

/// Five theme sounds. A button without a sound has an empty `file`, and `event` is
/// the first of its events (so canberra at least tries).
pub fn preview_sounds(resolve: &dyn Fn(&str) -> Option<PathBuf>, theme: &str) -> Vec<PreviewSound> {
    PREVIEW_SLOTS
        .iter()
        .map(|slot| {
            for ev in slot.events {
                if let Some(file) = find_event_file(resolve, theme, ev) {
                    return PreviewSound { key: slot.key.into(), event: (*ev).into(), file };
                }
            }
            PreviewSound {
                key: slot.key.into(),
                event: slot.events[0].into(),
                file: String::new(),
            }
        })
        .collect()
}

/// Playback command.
///
/// An installed theme (`installed`), when `canberra-gtk-play` exists, is played
/// BY EVENT with an explicit theme name — that is how the system sounds. The tool
/// has no `--theme` flag; the theme is set via the `canberra.xdg-theme.name`
/// property. Otherwise (a Store theme before installing, or no canberra) the file
/// is played by the first suitable player. `None` — nothing to play with.
pub fn play_argv(
    available: &[String],
    installed: bool,
    theme: &str,
    event: &str,
    file: &str,
) -> Option<Vec<String>> {
    if installed && !event.is_empty() && available.iter().any(|b| b == "canberra-gtk-play") {
        return Some(vec![
            "canberra-gtk-play".to_string(),
            "--id".to_string(),
            event.to_string(),
            format!("--property=canberra.xdg-theme.name={theme}"),
        ]);
    }
    if file.is_empty() {
        return None;
    }
    crate::utils::sound_play_argv(available, file)
}

/// Players from [`crate::utils::SOUND_PLAYERS`] found in `PATH`.
pub fn live_available_players() -> Vec<String> {
    let path = std::env::var("PATH").unwrap_or_default();
    crate::utils::SOUND_PLAYERS
        .iter()
        .map(|(b, _, _)| *b)
        .filter(|b| path.split(':').any(|d| !d.is_empty() && Path::new(d).join(b).is_file()))
        .map(str::to_string)
        .collect()
}

/// Start the player and do not wait: the process plays its stream to the end.
pub fn live_play(argv: &[String]) -> Result<(), String> {
    let Some((bin, args)) = argv.split_first() else {
        return Err(tr("Nothing to play"));
    };
    let mut child = std::process::Command::new(bin)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| trf("Couldn't start the player: %1", &[&e]))?;
    // Leave no zombies: wait for completion on the side.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

// ── previewing a Store theme before installing ──────────────────────────────

/// Size cap of the archive downloaded for preview.
pub const PREVIEW_MAX_ARCHIVE: i64 = 64 * 1024 * 1024;
/// Cap on unpacked size (protection against an archive bomb).
pub const PREVIEW_MAX_EXTRACTED: u64 = 256 * 1024 * 1024;
/// How many recently previewed themes to keep in the cache.
pub const PREVIEW_KEEP: usize = 6;
/// Download timeout, seconds.
pub const PREVIEW_TIMEOUT_SECS: u64 = 120;

/// Preview cache: `<cache>/rexon/sound-preview`.
pub fn preview_cache_root(cache_home: &str) -> String {
    format!("{}/rexon/sound-preview", cache_home.trim_end_matches('/'))
}

/// Cache subdirectory name for an item: safe characters only.
pub fn preview_slot_name(content_id: &str) -> String {
    let s: String = content_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(64)
        .collect();
    if s.is_empty() || s.chars().all(|c| c == '_') {
        "item".to_string()
    } else {
        s
    }
}

/// A theme from the archive, ready for preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThemePreview {
    pub theme: String,
    pub root: String,
    pub sounds: Vec<PreviewSound>,
}

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// Archive unpacked into `slot_dir/extracted` → sound themes with their five
/// sounds. Theme parents (`Inherits`) are looked up among the themes of the same
/// archive, then among the installed ones (`installed_dirs`).
pub fn previews_from_extracted(extracted: &str, installed_dirs: &[String]) -> Result<Vec<ThemePreview>, String> {
    let snap = crate::appearance_installer::install_tree_snapshot(extracted, &[]);
    let roots = crate::appearance_installer::find_theme_roots(&snap, extracted, "sounds");
    if roots.is_empty() {
        return Err(crate::appearance_installer::no_sound_theme_message());
    }
    let local: Vec<(String, PathBuf)> = roots
        .iter()
        .map(|r| (r.rsplit('/').next().unwrap_or("").to_string(), PathBuf::from(r)))
        .collect();
    let installed = resolver_for(installed_dirs.to_vec());
    let resolve = |name: &str| -> Option<PathBuf> {
        local.iter().find(|(n, _)| n == name).map(|(_, p)| p.clone()).or_else(|| installed(name))
    };
    Ok(local
        .iter()
        .map(|(name, root)| ThemePreview {
            theme: name.clone(),
            root: root.to_string_lossy().into_owned(),
            sounds: preview_sounds(&resolve, name),
        })
        .collect())
}

/// Unpack an already downloaded `archive` into the `slot_dir` cache and find themes.
/// The archive is deleted after unpacking; exceeding the size limit is a refusal with cleanup.
pub fn preview_from_archive(
    slot_dir: &str,
    archive: &str,
    installed_dirs: &[String],
) -> Result<Vec<ThemePreview>, String> {
    let extracted = format!("{slot_dir}/extracted");
    let _ = std::fs::remove_dir_all(&extracted);
    let res = crate::appearance_installer::live_safe_extract(archive, &extracted)
        .map_err(|e| trf("Couldn't unpack the archive: %1", &[&e]));
    let _ = std::fs::remove_file(archive);
    res?;
    if dir_size(Path::new(&extracted)) > PREVIEW_MAX_EXTRACTED {
        let _ = std::fs::remove_dir_all(slot_dir);
        return Err(tr("The archive is too large to preview"));
    }
    previews_from_extracted(&extracted, installed_dirs)
}

/// Keep no more than `keep` most recent items in the cache (by modification
/// time); do not touch `current`.
pub fn prune_preview_cache(root: &str, keep: usize, current: &str) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    let mut slots: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| e.file_name().to_string_lossy() != current)
        .map(|e| {
            let t = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            (t, e.path())
        })
        .collect();
    slots.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in slots.into_iter().skip(keep.saturating_sub(1)) {
        let _ = std::fs::remove_dir_all(p);
    }
}

/// Download the archive of item `content_id` from `url` into the cache, unpack,
/// find themes and their five sounds. An item unpacked earlier is not downloaded
/// again. Nothing is installed.
pub fn prepare_store_preview<IO: crate::net::NetIo + ?Sized>(
    io: &mut IO,
    cache_root: &str,
    content_id: &str,
    url: &str,
    installed_dirs: &[String],
) -> Result<Vec<ThemePreview>, String> {
    let slot = preview_slot_name(content_id);
    let slot_dir = format!("{cache_root}/{slot}");
    let extracted = format!("{slot_dir}/extracted");
    if Path::new(&extracted).is_dir() {
        if let Ok(found) = previews_from_extracted(&extracted, installed_dirs) {
            return Ok(found);
        }
    }
    if url.trim().is_empty() {
        return Err(tr("This item has no download link"));
    }
    let _ = std::fs::remove_dir_all(&slot_dir);
    std::fs::create_dir_all(&slot_dir).map_err(|e| e.to_string())?;
    let archive = format!("{slot_dir}/archive");
    // Download progress goes through `NetIo::progress`: the page shows it while
    // the archive arrives (without progress a large theme looked frozen).
    if !crate::ocs::http_download(io, url, &archive, true, PREVIEW_TIMEOUT_SECS, PREVIEW_MAX_ARCHIVE) {
        let _ = std::fs::remove_dir_all(&slot_dir);
        return Err(tr("Couldn't download the theme to preview it"));
    }
    let res = preview_from_archive(&slot_dir, &archive, installed_dirs);
    if res.is_err() {
        let _ = std::fs::remove_dir_all(&slot_dir);
    }
    prune_preview_cache(cache_root, PREVIEW_KEEP, &slot);
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rexon_sndprev_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn put(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    const IDX: &str = "[Sound Theme]\nName=T\nInherits=Base\nDirectories=stereo,surround\n\n[stereo]\nOutputProfile=stereo\n\n[surround]\nOutputProfile=5.1\n";

    /// Directories: `T` (inherits `Base`), `Base`, `freedesktop`.
    fn themes(base: &Path) -> Vec<String> {
        let d = base.join("sounds");
        put(&d.join("T/index.theme"), IDX);
        put(&d.join("T/surround/bell-window-system.oga"), "5.1");
        put(&d.join("T/stereo/bell-window-system.ogg"), "stereo-ogg");
        put(&d.join("T/stereo/message.wav"), "общий message");
        put(&d.join("T/stereo/battery-caution.disabled"), "");
        put(&d.join("Base/index.theme"), "[Sound Theme]\nName=B\nDirectories=stereo\n[stereo]\nOutputProfile=stereo\n");
        put(&d.join("Base/stereo/device-added.oga"), "base");
        put(&d.join("freedesktop/index.theme"), "[Sound Theme]\nName=Default\nDirectories=stereo\n[stereo]\nOutputProfile=stereo\n");
        put(&d.join("freedesktop/stereo/dialog-information.oga"), "fd");
        put(&d.join("freedesktop/stereo/battery-low.oga"), "fd");
        put(&d.join("freedesktop/stereo/message-new-instant.oga"), "fd точный");
        vec![d.to_string_lossy().into_owned()]
    }

    #[test]
    fn five_sounds_follow_spec_lookup() {
        let base = tmp("spec");
        let resolve = resolver_for(themes(&base));
        let got = preview_sounds(&resolve, "T");
        let short: Vec<(String, String)> = got
            .iter()
            .map(|s| (s.event.clone(), s.file.rsplit("/sounds/").next().unwrap_or("").to_string()))
            .collect();
        assert_eq!(
            short,
            [
                // stereo before 5.1, even though .oga is «better» than .ogg
                ("bell-window-system".into(), "T/stereo/bell-window-system.ogg".into()),
                // dialog-warning exists nowhere — the button's fallback event, from freedesktop
                ("dialog-information".into(), "freedesktop/stereo/dialog-information.oga".into()),
                // the exact name in freedesktop beats the shortened «message» in the theme
                ("message-new-instant".into(), "freedesktop/stereo/message-new-instant.oga".into()),
                // battery-caution is turned off by the theme (.disabled) → fallback battery-low
                ("battery-low".into(), "freedesktop/stereo/battery-low.oga".into()),
                // device-added — in the parent via Inherits
                ("device-added".into(), "Base/stereo/device-added.oga".into()),
            ]
        );
        assert_eq!(find_event_file(&resolve, "T", "message-new-email").as_deref().map(|p| p.ends_with("T/stereo/message.wav")), Some(true), "укорачивание имени");
        assert_eq!(find_event_file(&resolve, "T", "battery-caution"), None, ".disabled глушит");
    }

    #[test]
    fn play_argv_prefers_canberra_for_installed_theme() {
        let all: Vec<String> = ["pw-play", "canberra-gtk-play"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            play_argv(&all, true, "Ocean", "bell-window-system", "/x/bell.oga").unwrap(),
            ["canberra-gtk-play", "--id", "bell-window-system", "--property=canberra.xdg-theme.name=Ocean"]
        );
        // A Store theme is not installed — canberra will not find it, so we play the file.
        assert_eq!(play_argv(&all, false, "X", "bell", "/x/bell.oga").unwrap(), ["pw-play", "/x/bell.oga"]);
        assert_eq!(play_argv(&[], false, "X", "bell", "/x/bell.oga"), None);
        assert_eq!(play_argv(&all, false, "X", "bell", ""), None);
    }

    /// Fake player: writes its arguments to a file — real sound is not touched.
    #[test]
    fn live_play_runs_given_player() {
        let base = tmp("player");
        let out = base.join("args.txt");
        let script = base.join("fake-player");
        put(&script, &format!("#!/bin/sh\nprintf '%s' \"$*\" > '{}'\n", out.display()));
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        live_play(&[script.to_string_lossy().into_owned(), "/x/bell.oga".into()]).unwrap();
        for _ in 0..100 {
            if out.is_file() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "/x/bell.oga");
    }

    /// Archive like a real item: two themes deep, wallpapers next to them. Nothing
    /// is installed, the archive is deleted after unpacking, five sounds are found.
    #[test]
    fn store_archive_preview_without_install() {
        let base = tmp("store");
        let src = base.join("src");
        for t in ["Zorin", "Zorin2"] {
            put(&src.join(format!("zorinos2/sounds/{t}/index.theme")), "[Sound Theme]\nName=Z\nDirectories=stereo\n[stereo]\nOutputProfile=stereo\n");
            put(&src.join(format!("zorinos2/sounds/{t}/stereo/bell-window-system.oga")), "z");
        }
        put(&src.join("zorinos2/backgrounds/a.jpg"), "jpg");
        let slot = base.join("cache/12345");
        std::fs::create_dir_all(&slot).unwrap();
        let archive = slot.join("archive");
        let st = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&src)
            .arg("zorinos2")
            .status()
            .unwrap();
        assert!(st.success());
        let installed = themes(&base.join("inst"));
        let got = preview_from_archive(&slot.to_string_lossy(), &archive.to_string_lossy(), &installed).unwrap();
        assert!(!archive.exists(), "архив после распаковки удалён");
        let names: Vec<&str> = got.iter().map(|t| t.theme.as_str()).collect();
        assert_eq!(names, ["Zorin", "Zorin2"]);
        assert!(got[0].sounds[0].file.ends_with("Zorin/stereo/bell-window-system.oga"));
        // What the theme lacks comes from the installed freedesktop.
        assert!(got[0].sounds[1].file.ends_with("freedesktop/stereo/dialog-information.oga"));
        // Without a sound theme — an honest refusal.
        let empty = base.join("cache/empty");
        put(&empty.join("x/readme"), "r");
        let a2 = base.join("cache/e.tar.gz");
        assert!(std::process::Command::new("tar").arg("-czf").arg(&a2).arg("-C").arg(&empty).arg("x").status().unwrap().success());
        let err = preview_from_archive(&empty.to_string_lossy(), &a2.to_string_lossy(), &installed).unwrap_err();
        assert_eq!(err, crate::appearance_installer::no_sound_theme_message());
    }

    #[test]
    fn cache_is_pruned_and_slot_names_are_safe() {
        let base = tmp("prune");
        for i in 0..9 {
            std::fs::create_dir_all(base.join(format!("s{i}"))).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        prune_preview_cache(&base.to_string_lossy(), 3, "s0");
        let mut left: Vec<String> = std::fs::read_dir(&base).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["s0", "s7", "s8"]);
        assert_eq!(preview_slot_name("../../etc"), "______etc");
        assert_eq!(preview_slot_name(""), "item");
        assert_eq!(event_name_fallbacks("message-new-instant"), ["message-new-instant", "message-new", "message"]);
    }
}
