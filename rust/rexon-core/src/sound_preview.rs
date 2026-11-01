//! Прослушивание темы звуков — ядро кнопок «послушать» раздела «Звуки».
//!
//! Три части:
//! * **пять звуков темы** ([`PREVIEW_SLOTS`]) — тот же набор, что у модуля
//!   KDE «Системные звуки»: звонок окна, предупреждение, новое сообщение,
//!   батарея, подключение устройства; у каждой кнопки свой запасной список
//!   событий (`dialog-warning` → `dialog-information` и т.д.);
//! * **поиск файла события по спецификации freedesktop sound-theme**
//!   ([`find_event_file`]): каталоги темы из `Directories` с профилем
//!   `stereo` первыми, расширения `.disabled`/`.oga`/`.ogg`/`.wav`, цепочка
//!   `Inherits`, затем тема `freedesktop`; имя события укорачивается по
//!   последнему `-` (`message-new-instant` → `message-new` → `message`);
//! * **прослушивание темы из Магазина до установки**
//!   ([`prepare_store_preview`]): архив скачивается во временный кеш
//!   `~/.cache/rexon/sound-preview/<id>/`, распаковывается, корень темы
//!   ищет тот же помощник, что у установки
//!   ([`crate::appearance_installer::find_theme_roots`]), — ничего не ставится.
//!
//! Проигрывание — [`play_argv`]: установленную тему играет libcanberra по
//! событию (ровно то, что услышит система), иначе — файл первым подходящим
//! проигрывателем ([`crate::utils::sound_play_argv`]).
//!
//! ⚠️ Подкаталоги локалей спецификации (`stereo/<язык>/`) не просматриваются:
//! у тем на машине и в Магазине их не встречено; файл в корне каталога
//! профиля находится как обычно.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::i18n::{tr, trf};

/// Кнопка прослушивания: ключ (для значка в QML) и события по порядку.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreviewSlot {
    pub key: &'static str,
    pub events: &'static [&'static str],
}

/// Пять звуков темы — набор и запасные события как у модуля KDE
/// «Системные звуки» (`kcm_soundtheme`).
pub const PREVIEW_SLOTS: &[PreviewSlot] = &[
    PreviewSlot { key: "bell", events: &["bell-window-system"] },
    PreviewSlot { key: "warning", events: &["dialog-warning", "dialog-information"] },
    PreviewSlot { key: "message", events: &["message-new-instant", "message-new-email"] },
    PreviewSlot { key: "battery", events: &["battery-caution", "battery-low"] },
    PreviewSlot { key: "device", events: &["device-added", "device-removed"] },
];

/// «Демонстрация темы» (по нажатию на саму тему) — тот же порядок поиска.
pub const DEMO_EVENTS: &[&str] = &["theme-demo", "desktop-login", "service-login"];

/// Тема, в которую уходит поиск, когда ни тема, ни её родители звука не дали.
pub const FALLBACK_THEME: &str = "freedesktop";

/// Расширения по спецификации; `disabled` — «звук этого события выключен».
const LOOKUP_EXTS: &[&str] = &["disabled", "oga", "ogg", "wav"];

/// Разобранный `index.theme` звуковой темы.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoundThemeIndex {
    pub inherits: Vec<String>,
    /// `(подкаталог, OutputProfile)` в порядке `Directories`.
    pub dirs: Vec<(String, String)>,
}

/// Разбор `index.theme`; `None` — нет раздела `[Sound Theme]` (не тема).
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

/// Итог поиска в одной теме.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Lookup {
    Found(String),
    /// `<событие>.disabled`: звук выключен темой — дальше не искать.
    Disabled,
    NotFound,
}

/// Поиск `name` в самой теме (без родителей): каталоги профиля `stereo`
/// первыми, затем остальные; без `Directories` — корень темы.
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

/// Тема и её родители (`Inherits`, вглубь, без повторов).
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

/// Имена события от точного к общему: `a-b-c`, `a-b`, `a`.
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

/// Файл события `event` темы `theme` по спецификации; `None` — звука нет
/// либо тема его выключила (`.disabled`). `resolve` — каталог темы по имени.
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

/// Поиск темы по имени в каталогах `dirs` (первый с настоящей темой).
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

/// Звук одной кнопки: какое событие нашлось и где файл (`""` — нечего играть).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreviewSound {
    pub key: String,
    pub event: String,
    pub file: String,
}

/// Пять звуков темы. У кнопки без звука `file` пустой, `event` — первое из
/// её событий (чтобы canberra хотя бы попробовала).
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

/// Команда проигрывания.
///
/// Установленная тема (`installed`) при наличии `canberra-gtk-play` играется
/// ПО СОБЫТИЮ с явным именем темы — так звучит система. Ключа `--theme` у
/// утилиты нет; тема задаётся свойством `canberra.xdg-theme.name`. Иначе
/// (тема из Магазина до установки, либо нет canberra) — файл первым
/// подходящим проигрывателем. `None` — играть нечем.
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

/// Проигрыватели из [`crate::utils::SOUND_PLAYERS`], найденные в `PATH`.
pub fn live_available_players() -> Vec<String> {
    let path = std::env::var("PATH").unwrap_or_default();
    crate::utils::SOUND_PLAYERS
        .iter()
        .map(|(b, _, _)| *b)
        .filter(|b| path.split(':').any(|d| !d.is_empty() && Path::new(d).join(b).is_file()))
        .map(str::to_string)
        .collect()
}

/// Запустить проигрыватель и не ждать: процесс досматривает свой поток.
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
    // Зомби не оставляем: ждём завершения в стороне.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

// ── прослушивание темы из Магазина до установки ─────────────────────────────

/// Потолок скачиваемого архива для прослушивания.
pub const PREVIEW_MAX_ARCHIVE: i64 = 64 * 1024 * 1024;
/// Потолок распакованного (защита от архива-бомбы).
pub const PREVIEW_MAX_EXTRACTED: u64 = 256 * 1024 * 1024;
/// Сколько последних прослушанных тем держать в кеше.
pub const PREVIEW_KEEP: usize = 6;
/// Таймаут скачивания, секунды.
pub const PREVIEW_TIMEOUT_SECS: u64 = 120;

/// Кеш прослушивания: `<cache>/rexon/sound-preview`.
pub fn preview_cache_root(cache_home: &str) -> String {
    format!("{}/rexon/sound-preview", cache_home.trim_end_matches('/'))
}

/// Имя подкаталога кеша для товара: только безопасные символы.
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

/// Тема из архива, готовая к прослушиванию.
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

/// Распакованный в `slot_dir/extracted` архив → темы звуков с их пятью
/// звуками. Родители тем (`Inherits`) ищутся среди тем этого же архива,
/// затем в установленных (`installed_dirs`).
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

/// Разложить уже скачанный архив `archive` в кеш `slot_dir` и найти темы.
/// Архив после распаковки удаляется; перебор по размеру — отказ с очисткой.
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

/// Оставить в кеше не больше `keep` последних товаров (по времени
/// изменения); `current` не трогать.
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

/// Скачать архив товара `content_id` по `url` в кеш, распаковать, найти
/// темы и их пять звуков. Уже распакованный ранее товар не качается
/// повторно. Ничего не устанавливается.
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
    // Ход скачивания — через `NetIo::progress`: страница показывает его,
    // пока архив идёт (без прогресса крупная тема выглядела зависшей).
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

    /// Каталоги: `T` (наследует `Base`), `Base`, `freedesktop`.
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
                // stereo раньше 5.1, хотя .oga «лучше» .ogg
                ("bell-window-system".into(), "T/stereo/bell-window-system.ogg".into()),
                // dialog-warning нет нигде — запасное событие кнопки, из freedesktop
                ("dialog-information".into(), "freedesktop/stereo/dialog-information.oga".into()),
                // точное имя в freedesktop сильнее укороченного «message» в теме
                ("message-new-instant".into(), "freedesktop/stereo/message-new-instant.oga".into()),
                // battery-caution выключен темой (.disabled) → запасное battery-low
                ("battery-low".into(), "freedesktop/stereo/battery-low.oga".into()),
                // device-added — у родителя по Inherits
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
        // Тема из Магазина не установлена — canberra её не найдёт, играем файл.
        assert_eq!(play_argv(&all, false, "X", "bell", "/x/bell.oga").unwrap(), ["pw-play", "/x/bell.oga"]);
        assert_eq!(play_argv(&[], false, "X", "bell", "/x/bell.oga"), None);
        assert_eq!(play_argv(&all, false, "X", "bell", ""), None);
    }

    /// Подставной проигрыватель: пишет свои аргументы в файл — живой звук не трогается.
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

    /// Архив как у живого товара: две темы глубоко, обои рядом. Ничего не
    /// ставится, архив после распаковки удалён, пять звуков найдены.
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
        // Чего в теме нет — из установленной freedesktop.
        assert!(got[0].sounds[1].file.ends_with("freedesktop/stereo/dialog-information.oga"));
        // Без темы звуков — честный отказ.
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
