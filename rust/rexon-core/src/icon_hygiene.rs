//! Гигиена каталога значков пользователя: общий запасной набор `hicolor`.
//!
//! Дефект 09.11.2026: в `~/.local/share/icons/hicolor/index.theme` лежал
//! урезанный файл (только девять каталогов `*/apps`), записанный весенней
//! версией Rexon на Python. Пользовательский `index.theme` ПЕРЕКРЫВАЕТ
//! системный `/usr/share/icons/hicolor/index.theme` целиком: GTK и оболочка
//! GNOME после этого видят в `hicolor` только перечисленные в нём каталоги, и
//! значки из `actions/`, `status/`, `devices/`, 22×22, `*-symbolic`
//! пропадают по всему столу («значок не найден»).
//!
//! Здесь два правила:
//!
//! * [`is_reserved_icon_set`] — `hicolor` и `locolor` не темы, а общие
//!   каталоги, куда кладут значки все программы. Установщики тем (магазин,
//!   `.rxn`, «Как в дистрибутиве», импорт из Windows) такой каталог не ставят
//!   и не заменяют: тема с таким именем пропускается с заметкой.
//! * [`heal_user_hicolor`] — урезанный пользовательский `index.theme` у
//!   `hicolor` переименовывается в `index.theme.bak-rexon-<дата>` (НЕ
//!   удаляется) с записью в журнал. Критерий — по смыслу, а не по размеру
//!   ([`judge_hicolor_index`]): нет раздела `[Icon Theme]`, пустой
//!   `Directories=` либо в нём нет части каталогов системного файла. Полный
//!   файл человека (копия системного, в том числе с добавками) не трогается.

use std::path::{Path, PathBuf};

use crate::i18n::trf;

/// Общие наборы значков, которые не темы: их каталоги в профиле делят все
/// программы (спецификация freedesktop: `hicolor` — обязательный запасной
/// набор, `locolor` — его устаревший спутник для 16 цветов).
pub const RESERVED_ICON_SETS: &[&str] = &["hicolor", "locolor"];

/// Имя каталога темы — общий набор значков ([`RESERVED_ICON_SETS`]).
pub fn is_reserved_icon_set(name: &str) -> bool {
    RESERVED_ICON_SETS.contains(&name.trim_end_matches('/'))
}

/// Что не так с пользовательским `hicolor/index.theme`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HicolorVerdict {
    /// Файл полный: оставить как есть.
    Healthy,
    /// Нет раздела `[Icon Theme]` — файл не описывает набор вовсе.
    NoSection,
    /// Раздел есть, а каталогов (`Directories=`/`ScaledDirectories=`) нет.
    NoDirectories,
    /// Каталогов меньше, чем у системного файла: `missing` из `system`
    /// системных каталогов в нём нет.
    MissingSystemDirs { missing: usize, system: usize },
}

impl HicolorVerdict {
    /// Файл урезан и перекрывает системный во вред.
    pub fn is_truncated(&self) -> bool {
        *self != HicolorVerdict::Healthy
    }

    /// Причина для журнала (по-русски, журнал читает разработчик).
    pub fn reason(&self) -> String {
        match self {
            HicolorVerdict::Healthy => "файл полный".into(),
            HicolorVerdict::NoSection => "нет раздела [Icon Theme]".into(),
            HicolorVerdict::NoDirectories => "пустой Directories=".into(),
            HicolorVerdict::MissingSystemDirs { missing, system } => {
                format!("нет {missing} из {system} каталогов системного hicolor")
            }
        }
    }
}

/// Каталоги раздела `[Icon Theme]`: `Directories=` и `ScaledDirectories=`.
/// `None` — раздела нет.
fn icon_theme_dirs(text: &str) -> Option<Vec<String>> {
    let mut in_section = false;
    let mut seen_section = false;
    let mut dirs: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line == "[Icon Theme]";
            seen_section |= in_section;
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        if matches!(key.trim(), "Directories" | "ScaledDirectories") {
            for d in value.split(',').map(str::trim).filter(|d| !d.is_empty()) {
                if !dirs.iter().any(|x| x == d) {
                    dirs.push(d.to_string());
                }
            }
        }
    }
    seen_section.then_some(dirs)
}

/// Приговор пользовательскому `hicolor/index.theme` (`user`) против
/// системного (`system`; `None` — системного не нашлось, тогда судят только
/// раздел и пустоту списка).
pub fn judge_hicolor_index(user: &str, system: Option<&str>) -> HicolorVerdict {
    let Some(dirs) = icon_theme_dirs(user) else { return HicolorVerdict::NoSection };
    if dirs.is_empty() {
        return HicolorVerdict::NoDirectories;
    }
    let Some(sys_dirs) = system.and_then(icon_theme_dirs).filter(|d| !d.is_empty()) else {
        return HicolorVerdict::Healthy;
    };
    let missing = sys_dirs.iter().filter(|d| !dirs.contains(d)).count();
    if missing > 0 {
        return HicolorVerdict::MissingSystemDirs { missing, system: sys_dirs.len() };
    }
    HicolorVerdict::Healthy
}

/// Переименованный урезанный файл.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealedIndex {
    pub path: PathBuf,
    pub backup: PathBuf,
    pub verdict: HicolorVerdict,
}

/// Системный `hicolor/index.theme`: первый с каталогами в `data_dirs`.
fn system_hicolor_index(data_dirs: &[PathBuf], data_home: &Path) -> Option<String> {
    data_dirs
        .iter()
        .filter(|d| d.as_path() != data_home)
        .filter_map(|d| std::fs::read(d.join("icons/hicolor/index.theme")).ok())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .find(|t| icon_theme_dirs(t).is_some_and(|d| !d.is_empty()))
}

/// Свободное имя копии: `index.theme.bak-rexon-<дата>`, занято — `-2`, `-3`…
fn free_backup_name(dir: &Path, date: &str) -> PathBuf {
    let base = format!("index.theme.bak-rexon-{date}");
    let mut candidate = dir.join(&base);
    let mut n = 2;
    while std::fs::symlink_metadata(&candidate).is_ok() {
        candidate = dir.join(format!("{base}-{n}"));
        n += 1;
    }
    candidate
}

/// Лечение `<data_home>/icons/hicolor/index.theme`: урезанный файл
/// переименовывается в `index.theme.bak-rexon-<date>`. `None` — файла нет,
/// он полный или переименовать не вышло (тогда строка в журнале).
pub fn heal_user_hicolor(data_home: &Path, data_dirs: &[PathBuf], date: &str) -> Option<HealedIndex> {
    let dir = data_home.join("icons/hicolor");
    let path = dir.join("index.theme");
    // Ссылка тоже судится по содержимому, но переименовывается сама ссылка.
    std::fs::symlink_metadata(&path).ok()?;
    let user = std::fs::read(&path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    let system = system_hicolor_index(data_dirs, data_home);
    let verdict = judge_hicolor_index(&user, system.as_deref());
    if !verdict.is_truncated() {
        return None;
    }
    let backup = free_backup_name(&dir, date);
    if let Err(e) = std::fs::rename(&path, &backup) {
        crate::log_warn!("icons", "урезанный {} ({}) не переименован: {e}", path.display(), verdict.reason());
        return None;
    }
    crate::log_warn!(
        "icons",
        "урезанный {} ({}) перекрывал системный hicolor — переименован в {}",
        path.display(),
        verdict.reason(),
        backup.display()
    );
    Some(HealedIndex { path, backup, verdict })
}

/// Строка человеку о вылеченном файле.
pub fn healed_message(h: &HealedIndex) -> String {
    trf(
        "Icon protection: a truncated %1 was hiding system icons — renamed to %2",
        &[&h.path.display(), &h.backup.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()],
    )
}

/// `$XDG_DATA_HOME` (или `~/.local/share`) и `$XDG_DATA_DIRS` (или
/// `/usr/local/share:/usr/share`) живого сеанса.
fn live_dirs() -> (PathBuf, Vec<PathBuf>) {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let data_home = env("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env("HOME").unwrap_or_default()).join(".local/share")
    });
    let data_dirs = env("XDG_DATA_DIRS")
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into())
        .split(':')
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .collect();
    (data_home, data_dirs)
}

/// Сегодняшняя дата `ГГГГММДД` по местным часам.
fn today() -> String {
    match crate::qt_app::Timestamp::now_local() {
        Some(t) => format!("{:04}{:02}{:02}", t.year, t.month, t.day),
        None => "undated".into(),
    }
}

/// Лечение в живом профиле (старт окна, после установки значков или
/// курсоров). `Some` — строка человеку: файл переименован.
///
/// Проверки модулей ядра живой профиль не трогают: они судят песочницы через
/// [`heal_user_hicolor`]. Интеграционные прогоны установки подменяют `HOME` и
/// `XDG_*` сами.
pub fn heal_live() -> Option<String> {
    if cfg!(test) {
        return None;
    }
    let (data_home, data_dirs) = live_dirs();
    heal_user_hicolor(&data_home, &data_dirs, &today()).map(|h| healed_message(&h))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYSTEM: &str = "[Icon Theme]\nName=Hicolor\nHidden=true\nDirectories=16x16/actions,16x16/apps,22x22/apps,48x48/apps,scalable/apps\n\
                          ScaledDirectories=16x16@2/apps\n\n[16x16/actions]\nSize=16\n";
    /// Как у дефекта 09.11: только `*/apps`.
    const TRUNCATED: &str = "[Icon Theme]\nName=Hicolor\nDirectories=16x16/apps,48x48/apps,scalable/apps\n";

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rexon_icon_hygiene_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// Песочница: свой `XDG_DATA_HOME` и один системный каталог данных.
    fn sandbox(tag: &str, user: Option<&str>) -> (PathBuf, PathBuf, PathBuf) {
        let root = scratch(tag);
        let home = root.join("data-home");
        let sys = root.join("usr-share");
        put(&sys.join("icons/hicolor/index.theme"), SYSTEM);
        if let Some(u) = user {
            put(&home.join("icons/hicolor/index.theme"), u);
        }
        std::fs::create_dir_all(home.join("icons/hicolor/48x48/apps")).unwrap();
        (root, home, sys)
    }

    #[test]
    fn reserved_names_are_hicolor_and_locolor() {
        assert!(is_reserved_icon_set("hicolor"));
        assert!(is_reserved_icon_set("locolor"));
        assert!(is_reserved_icon_set("hicolor/"));
        assert!(!is_reserved_icon_set("Papirus"));
        assert!(!is_reserved_icon_set("hicolor-imported"));
        assert!(!is_reserved_icon_set("default"), "default — место курсора по умолчанию, не запрещено");
    }

    #[test]
    fn verdicts_by_meaning_not_size() {
        assert_eq!(judge_hicolor_index("", Some(SYSTEM)), HicolorVerdict::NoSection);
        assert_eq!(judge_hicolor_index("[Other]\nDirectories=48x48/apps\n", Some(SYSTEM)), HicolorVerdict::NoSection);
        assert_eq!(judge_hicolor_index("[Icon Theme]\nName=Hicolor\nDirectories=\n", None), HicolorVerdict::NoDirectories);
        assert_eq!(
            judge_hicolor_index(TRUNCATED, Some(SYSTEM)),
            HicolorVerdict::MissingSystemDirs { missing: 3, system: 6 }
        );
        // Копия системного — полная; с добавками человека — тоже.
        assert_eq!(judge_hicolor_index(SYSTEM, Some(SYSTEM)), HicolorVerdict::Healthy);
        let extra = SYSTEM.replace("scalable/apps", "scalable/apps,64x64/apps,");
        assert_eq!(judge_hicolor_index(&extra, Some(SYSTEM)), HicolorVerdict::Healthy);
        // Огромный файл без нужных каталогов всё равно урезан.
        let long = format!("[Icon Theme]\nComment={}\nDirectories=48x48/apps\n", "x".repeat(100_000));
        assert!(judge_hicolor_index(&long, Some(SYSTEM)).is_truncated());
        // Системного нет — судим только раздел и пустоту.
        assert_eq!(judge_hicolor_index(TRUNCATED, None), HicolorVerdict::Healthy);
    }

    #[test]
    fn truncated_index_is_renamed_not_deleted() {
        let (root, home, sys) = sandbox("trunc", Some(TRUNCATED));
        let healed = heal_user_hicolor(&home, &[sys.clone()], "20261114").expect("вылечен");
        let dir = home.join("icons/hicolor");
        assert!(!dir.join("index.theme").exists());
        assert_eq!(healed.backup, dir.join("index.theme.bak-rexon-20261114"));
        assert_eq!(std::fs::read_to_string(&healed.backup).unwrap(), TRUNCATED);
        assert!(dir.join("48x48/apps").is_dir(), "значки программ на месте");
        assert_eq!(std::fs::read_to_string(sys.join("icons/hicolor/index.theme")).unwrap(), SYSTEM);
        // Второй прогон — файла уже нет, делать нечего.
        assert_eq!(heal_user_hicolor(&home, &[sys], "20261114"), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn backup_name_does_not_overwrite_previous_backup() {
        let (root, home, sys) = sandbox("twice", Some(TRUNCATED));
        let dir = home.join("icons/hicolor");
        put(&dir.join("index.theme.bak-rexon-20261114"), "прежняя копия");
        let healed = heal_user_hicolor(&home, &[sys], "20261114").expect("вылечен");
        assert_eq!(healed.backup, dir.join("index.theme.bak-rexon-20261114-2"));
        assert_eq!(std::fs::read_to_string(dir.join("index.theme.bak-rexon-20261114")).unwrap(), "прежняя копия");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn full_user_file_and_absent_file_stay_untouched() {
        let full = format!("{SYSTEM}\n# моя правка\n");
        let (root, home, sys) = sandbox("full", Some(&full));
        assert_eq!(heal_user_hicolor(&home, &[sys.clone()], "20261114"), None);
        assert_eq!(std::fs::read_to_string(home.join("icons/hicolor/index.theme")).unwrap(), full);
        let _ = std::fs::remove_dir_all(root);

        let (root, home, sys) = sandbox("absent", None);
        assert_eq!(heal_user_hicolor(&home, &[sys], "20261114"), None);
        assert!(!home.join("icons/hicolor/index.theme").exists(), "файл не создаётся");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn data_home_listed_in_data_dirs_is_not_its_own_reference() {
        // XDG_DATA_DIRS иногда содержит и сам XDG_DATA_HOME: урезанный файл
        // не должен служить эталоном самому себе.
        let (root, home, _sys) = sandbox("self", Some(TRUNCATED));
        assert_eq!(heal_user_hicolor(&home, &[home.clone()], "20261114"), None, "эталона нет — судим по разделу");
        let empty = "[Icon Theme]\nName=Hicolor\nDirectories=\n";
        put(&home.join("icons/hicolor/index.theme"), empty);
        assert!(heal_user_hicolor(&home, &[home.clone()], "20261114").is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn healed_message_names_file_and_backup() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let h = HealedIndex {
            path: PathBuf::from("/h/.local/share/icons/hicolor/index.theme"),
            backup: PathBuf::from("/h/.local/share/icons/hicolor/index.theme.bak-rexon-20261114"),
            verdict: HicolorVerdict::NoSection,
        };
        assert_eq!(
            healed_message(&h),
            "Icon protection: a truncated /h/.local/share/icons/hicolor/index.theme was hiding system icons — renamed to index.theme.bak-rexon-20261114"
        );
    }
}
