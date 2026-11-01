//! Тема GTK в программах GTK 4 и libadwaita.
//!
//! libadwaita не читает имя темы (`gtk-theme-name`, ключ GNOME `gtk-theme`):
//! у неё свой вид, а чужую тему она берёт только из пользовательской таблицы
//! стилей `~/.config/gtk-4.0/gtk.css`. Так делают все, кто это умеет:
//!
//! * установщики тем (Orchis, Colloid, Fluent — `install.sh -l`,
//!   `link_libadwaita` в `core.sh`): снять `gtk-4.0/{assets,gtk.css,
//!   gtk-dark.css}` и положить на их место ссылки в `gtk-4.0` темы;
//! * nwg-look (`linkGtk4Stuff`, переключатель «export-gtk4-symlinks», по
//!   умолчанию включён) — те же три ссылки и `gtk-4.0/settings.ini`; у темы
//!   без `gtk-4.0` ничего не трогает;
//! * KDE (kde-gtk-config, `SettingsIniEditor`) пишет `settings.ini` GTK 3 и
//!   GTK 4 (`gtk-theme-name` и прочее), а свой `gtk.css` заводит только под
//!   цвета Breeze (`@import 'colors.css'`); ссылок на темы не ставит;
//! * Gradience пишет свой `gtk.css`, прежний кладёт рядом в `gtk.css.bak` и
//!   возвращает его по «Восстановить»; gnome-tweaks тему GTK 4 не трогает
//!   вовсе («Устаревшие приложения»).
//!
//! Rexon делает как установщики тем, но бережнее: ссылается на ВСЕ таблицы
//! стилей и каталоги из `gtk-4.0` темы (у adw-gtk3 `gtk.css` подключает
//! соседние `libadwaita.css`, у Materia — каталог `icons`: относительные
//! пути GTK ищет рядом со ссылкой, а не с файлом темы), чужие файлы на этих
//! местах не стирает, а убирает в `<имя>.rexon-backup` и возвращает, когда
//! выбрана Adwaita, тема без `gtk-4.0` или место освободилось. Ссылки в
//! `gtk-4.0` какой-либо темы (`…/themes/<тема>/gtk-4.0/…` — наши, Orchis,
//! nwg-look) считаются заменимыми и снимаются без копии.
//!
//! Решение — чистая функция [`steps`] над снятыми заранее [`Facts`]
//! ([`facts_live`]); шаги — файловые шаги общего исполнителя, под откатом
//! нажатия и в журнале «вернуть как было».

use crate::ext_manager::ExtStep;
use crate::i18n::{tr, trf};
use crate::install_plan::Step;

/// Ключ `settings.json`: применять ли тему GTK к программам GTK 4 и
/// libadwaita. Нет ключа — да (как у nwg-look).
pub const SETTING_KEY: &str = "gtk4_apply";

/// Хвост имени копии чужого файла: `gtk.css.rexon-backup`, следующая —
/// `gtk.css.rexon-backup-2`.
pub const BACKUP_SUFFIX: &str = ".rexon-backup";

/// Что лежит в `~/.config/gtk-4.0` под одним именем.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// Ссылка в `gtk-4.0` какой-либо темы — заменима, копия не нужна.
    ThemeLink,
    /// Ссылка в никуда (не в тему) — беречь нечего.
    Dangling,
    /// Свой файл человека (или ссылка на файл вне тем).
    File,
    /// Свой каталог человека (или ссылка на каталог вне тем).
    Dir,
}

/// Одна запись каталога `~/.config/gtk-4.0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
}

/// Версия темы для GTK 4.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ThemeGtk4 {
    /// Каталога `gtk-4.0` (или `gtk-4.0/gtk.css`) в теме нет.
    #[default]
    Missing,
    /// Adwaita — встроенный вид GTK 4 и libadwaita: ничего не подкладывать.
    Builtin,
    /// Стили упакованы в `gtk.gresource` и подключаются как
    /// `resource:///…` — GTK регистрирует ресурс, только загружая тему по
    /// имени; из `~/.config/gtk-4.0` такая таблица пуста (WhiteSur).
    Packed,
    /// Есть что подложить: каталог `gtk-4.0` темы и имена в нём —
    /// таблицы `*.css` и каталоги (`assets`, `icons`); `true` — каталог.
    Files { dir: String, names: Vec<(String, bool)> },
}

impl ThemeGtk4 {
    /// Есть ли что подложить программам GTK 4.
    pub fn linkable(&self) -> bool {
        matches!(self, ThemeGtk4::Files { .. })
    }
}

/// Снятое с машины для [`steps`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// `~/.config/gtk-4.0` (с учётом `$XDG_CONFIG_HOME`).
    pub config_dir: String,
    /// Что в нём сейчас (без `settings.ini`).
    pub entries: Vec<Entry>,
    /// Версия выбранной темы для GTK 4.
    pub theme: ThemeGtk4,
    /// Переключатель «Применять к программам GTK 4/libadwaita».
    pub enabled: bool,
}

/// Имя копии `name` с номером `n` (1 — без номера).
fn backup_name(name: &str, n: usize) -> String {
    if n <= 1 { format!("{name}{BACKUP_SUFFIX}") } else { format!("{name}{BACKUP_SUFFIX}-{n}") }
}

/// Номер копии, если `entry` — копия `name`.
fn backup_number(entry: &str, name: &str) -> Option<usize> {
    let rest = entry.strip_prefix(name)?.strip_prefix(BACKUP_SUFFIX)?;
    if rest.is_empty() {
        return Some(1);
    }
    rest.strip_prefix('-')?.parse().ok().filter(|n| *n >= 2)
}

/// Копия ли это вообще (такие имена план не трогает как места).
fn is_backup(entry: &str) -> bool {
    entry.contains(BACKUP_SUFFIX)
}

/// Ведёт ли ссылка в `gtk-4.0` какой-либо темы: `…/themes/<тема>/gtk-4.0/…`
/// или `…/.themes/<тема>/gtk-4.0/…`.
pub fn is_theme_link_target(target: &str) -> bool {
    let Some(at) = target.find("/gtk-4.0/") else { return false };
    let theme_dir = std::path::Path::new(&target[..at]);
    theme_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "themes" || n == ".themes")
}

/// Шаги для выбранной темы. Пусто — трогать нечего (переключатель
/// выключен, место и так в порядке).
///
/// Порядок: снять все ссылки в темы; на каждое имя, которое несёт тема, —
/// чужое убрать в копию и поставить ссылку; место, которое освободилось и
/// темой не занято, — вернуть из последней копии.
pub fn steps(facts: &Facts) -> Vec<Step> {
    if !facts.enabled {
        return Vec::new();
    }
    let dir = facts.config_dir.trim_end_matches('/');
    let at = |name: &str| format!("{dir}/{name}");
    let wanted: &[(String, bool)] = match &facts.theme {
        ThemeGtk4::Files { names, .. } => names,
        _ => &[],
    };
    let mut out: Vec<Step> = Vec::new();
    // Ссылки прежней темы — снять (новая тема подложит свои).
    for e in facts.entries.iter().filter(|e| !is_backup(&e.name)) {
        if matches!(e.kind, EntryKind::ThemeLink) {
            out.push(Step::File(ExtStep::RemoveFile { path: at(&e.name) }));
        }
    }
    let entry = |name: &str| facts.entries.iter().find(|e| e.name == name);
    if let ThemeGtk4::Files { dir: theme_dir, names } = &facts.theme {
        if !names.is_empty() {
            out.insert(0, Step::File(ExtStep::MakeDirs { path: dir.to_string() }));
        }
        for (name, _) in names {
            match entry(name).map(|e| e.kind) {
                Some(EntryKind::File) | Some(EntryKind::Dir) => {
                    let is_dir = entry(name).is_some_and(|e| e.kind == EntryKind::Dir);
                    let free = (1..)
                        .map(|n| backup_name(name, n))
                        .find(|b| entry(b).is_none())
                        .unwrap_or_default();
                    let (src, dest) = (at(name), at(&free));
                    if is_dir {
                        out.push(Step::File(ExtStep::CopyTree { src: src.clone(), dest }));
                        out.push(Step::File(ExtStep::RemoveTree { path: src, safe: false, ignore_errors: false }));
                    } else {
                        out.push(Step::File(ExtStep::CopyFile { src: src.clone(), dest }));
                        out.push(Step::File(ExtStep::RemoveFile { path: src }));
                    }
                }
                Some(EntryKind::Dangling) => out.push(Step::File(ExtStep::RemoveFile { path: at(name) })),
                Some(EntryKind::ThemeLink) | None => {}
            }
            out.push(Step::File(ExtStep::Symlink {
                target: format!("{}/{name}", theme_dir.trim_end_matches('/')),
                link: at(name),
            }));
        }
    }
    // Освободившееся (или пустовавшее) место, которое тема не заняла, —
    // вернуть из последней копии.
    let mut restored: Vec<&str> = Vec::new();
    for e in &facts.entries {
        let Some(base) = e.name.find(BACKUP_SUFFIX).map(|i| &e.name[..i]) else { continue };
        if restored.contains(&base) || wanted.iter().any(|(n, _)| n == base) {
            continue;
        }
        if entry(base).is_some_and(|b| b.kind != EntryKind::ThemeLink) {
            continue;
        }
        let Some((latest, _)) = facts
            .entries
            .iter()
            .filter_map(|b| backup_number(&b.name, base).map(|n| (b, n)))
            .max_by_key(|(_, n)| *n)
        else {
            continue;
        };
        restored.push(base);
        let (src, dest) = (at(&latest.name), at(base));
        if latest.kind == EntryKind::Dir {
            out.push(Step::File(ExtStep::CopyTree { src: src.clone(), dest }));
            out.push(Step::File(ExtStep::RemoveTree { path: src, safe: false, ignore_errors: false }));
        } else {
            out.push(Step::File(ExtStep::CopyFile { src: src.clone(), dest }));
            out.push(Step::File(ExtStep::RemoveFile { path: src }));
        }
    }
    out
}

/// Светлый/тёмный режим программ libadwaita Rexon ставит по теме GTK
/// там, где тема пишется в схему GNOME ([`crate::themes::COLOR_SCHEME_KEY`]).
pub fn mode_follows(de: &str) -> bool {
    crate::themes::resolve_schemas(de)
        .role("gtk")
        .is_some_and(|(s, _)| s == crate::themes::COLOR_SCHEME_KEY.0)
}

/// Дописать к шагам темы GTK `base` то, что нужно GTK 4: `gtk-theme-name`
/// в `gtk-4.0/settings.ini` (как KDE; если план окружения его ещё не
/// пишет) и ссылки [`steps`]. У Deepin план — один стоп, к нему ничего.
pub fn extend_press(base: &mut Vec<Step>, theme: &str, de: &str, facts: &Facts) {
    use crate::themes::ApplyStep;
    if de == "deepin" {
        return;
    }
    let ini = format!("{}/settings.ini", facts.config_dir.trim_end_matches('/'));
    let has_ini = base.iter().any(|s| {
        matches!(s, Step::Apply(ApplyStep::GtkIni { path, key, .. }) if *path == ini && key == "gtk-theme-name")
    });
    if !has_ini {
        base.push(Step::Apply(ApplyStep::GtkIni {
            path: ini,
            key: "gtk-theme-name".into(),
            value: theme.to_string(),
        }));
    }
    base.extend(steps(facts));
}

/// Встроенная ли это тема GTK 4 (Adwaita и её тёмный вариант).
pub fn is_builtin(theme: &str) -> bool {
    theme.trim().to_lowercase().starts_with("adwaita")
}

/// Версия темы для GTK 4 по каталогу темы `theme_dir` (`None` — каталога
/// темы не нашли). `list` — имена в `gtk-4.0` с признаком каталога,
/// `read` — текст файла.
pub fn theme_gtk4(
    theme: &str,
    theme_dir: Option<&str>,
    list: &dyn Fn(&str) -> Option<Vec<(String, bool)>>,
    read: &dyn Fn(&str) -> Option<String>,
) -> ThemeGtk4 {
    if is_builtin(theme) {
        return ThemeGtk4::Builtin;
    }
    let Some(theme_dir) = theme_dir else { return ThemeGtk4::Missing };
    let dir = format!("{}/gtk-4.0", theme_dir.trim_end_matches('/'));
    let Some(all) = list(&dir) else { return ThemeGtk4::Missing };
    if !all.iter().any(|(n, d)| n == "gtk.css" && !d) {
        return ThemeGtk4::Missing;
    }
    let packed = all.iter().any(|(n, _)| n == "gtk.gresource")
        && read(&format!("{dir}/gtk.css")).is_some_and(|css| css.contains("resource:///"));
    if packed {
        return ThemeGtk4::Packed;
    }
    let mut names: Vec<(String, bool)> = all
        .into_iter()
        .filter(|(n, d)| !n.starts_with('.') && (*d || n.ends_with(".css")))
        .collect();
    names.sort();
    ThemeGtk4::Files { dir, names }
}

// ============================================================================
// Подписи
// ============================================================================

/// Переключатель под плиткой темы GTK: `checked` — что будет при нажатии
/// «Применить», `enabled` — есть ли что переносить, `note` — пояснение
/// (у недоступного переключателя — видимой строкой, у доступного —
/// подсказкой).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Toggle {
    pub visible: bool,
    pub enabled: bool,
    pub checked: bool,
    pub text: String,
    pub note: String,
}

/// Переключатель для темы `theme` с версией `gtk4`; `on` — значение
/// настройки [`SETTING_KEY`]; `mode_follows` — светлый/тёмный режим
/// программ libadwaita Rexon ставит по теме (окружения со схемой GNOME).
pub fn toggle(theme: &str, gtk4: &ThemeGtk4, on: bool, mode_follows: bool) -> Toggle {
    let mode = if mode_follows {
        tr(" Only light or dark mode follows the theme.")
    } else {
        String::new()
    };
    let note = match gtk4 {
        ThemeGtk4::Files { .. } => tr(
            "The theme has its own GTK 4 version: Rexon links its styles into \
             ~/.config/gtk-4.0 (gtk.css, gtk-dark.css, assets), as the theme's own \
             installer does. Your own files there are kept as *.rexon-backup and come \
             back when you choose Adwaita.",
        ),
        ThemeGtk4::Builtin => tr(
            "Adwaita is the built-in look of GTK 4 and libadwaita apps: links to other \
             themes in ~/.config/gtk-4.0 are removed and your own files come back.",
        ),
        ThemeGtk4::Packed => {
            trf(
                "The GTK 4 version of “%1” is packed into gtk.gresource and works only \
                 when GTK loads the theme by name, so GTK 4 and libadwaita apps keep \
                 their own look.",
                &[&theme],
            ) + &mode
        }
        ThemeGtk4::Missing => {
            trf(
                "“%1” has no GTK 4 version, so GTK 4 and libadwaita apps keep their \
                 own look.",
                &[&theme],
            ) + &mode
        }
    };
    let enabled = gtk4.linkable();
    Toggle {
        visible: true,
        enabled,
        checked: enabled && on,
        text: tr("Apply to GTK 4/libadwaita apps"),
        note,
    }
}

// ============================================================================
// Живая машина
// ============================================================================

/// Каталог темы GTK `theme` — первый найденный в путях поиска тем.
pub fn theme_dir_live(theme: &str, home: &str) -> Option<String> {
    let data = crate::utils::data_home(std::env::var("XDG_DATA_HOME").ok().as_deref(), home);
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    crate::themes::gtk_theme_search_roots(home, &data, &dirs)
        .into_iter()
        .map(|root| format!("{root}/{theme}"))
        .find(|d| std::path::Path::new(d).is_dir())
}

fn list_live(dir: &str) -> Option<Vec<(String, bool)>> {
    let rd = std::fs::read_dir(dir).ok()?;
    Some(
        rd.flatten()
            .filter_map(|e| {
                let name = e.file_name().to_str()?.to_string();
                Some((name, e.path().is_dir()))
            })
            .collect(),
    )
}

/// Версия темы для GTK 4 на этой машине.
pub fn theme_gtk4_live(theme: &str, home: &str) -> ThemeGtk4 {
    let dir = theme_dir_live(theme, home);
    theme_gtk4(theme, dir.as_deref(), &list_live, &|p| std::fs::read_to_string(p).ok())
}

/// Что лежит в каталоге `dir` (`~/.config/gtk-4.0`), кроме `settings.ini`.
pub fn entries_live(dir: &str) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            if name == "settings.ini" {
                return None;
            }
            let path = e.path();
            let link = std::fs::symlink_metadata(&path).ok()?.file_type().is_symlink();
            let kind = if link {
                let target = std::fs::read_link(&path).ok()?.to_string_lossy().into_owned();
                if is_theme_link_target(&target) {
                    EntryKind::ThemeLink
                } else if !path.exists() {
                    EntryKind::Dangling
                } else if path.is_dir() {
                    EntryKind::Dir
                } else {
                    EntryKind::File
                }
            } else if path.is_dir() {
                EntryKind::Dir
            } else {
                EntryKind::File
            };
            Some(Entry { name, kind })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Значение переключателя из `settings.json` (нет ключа — да).
pub fn enabled_live() -> bool {
    crate::settings::load_settings(&crate::settings::settings_file())
        .get(SETTING_KEY)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true)
}

/// Записать переключатель в `settings.json`.
pub fn set_enabled_live(on: bool) -> std::io::Result<()> {
    crate::settings::SettingsManager::new()?.set(SETTING_KEY, serde_json::Value::from(on))
}

/// Снять [`Facts`] для темы `theme`.
pub fn facts_live(theme: &str, home: &str, xdg_config_home: Option<&str>) -> Facts {
    let config_dir = format!("{}/gtk-4.0", crate::utils::config_home(xdg_config_home, home));
    Facts {
        entries: entries_live(&config_dir),
        theme: theme_gtk4_live(theme, home),
        enabled: enabled_live(),
        config_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = "/h/.config/gtk-4.0";

    fn e(name: &str, kind: EntryKind) -> Entry {
        Entry { name: name.into(), kind }
    }

    fn files(names: &[(&str, bool)]) -> ThemeGtk4 {
        ThemeGtk4::Files {
            dir: "/usr/share/themes/Orchis/gtk-4.0".into(),
            names: names.iter().map(|(n, d)| (n.to_string(), *d)).collect(),
        }
    }

    fn facts(entries: Vec<Entry>, theme: ThemeGtk4) -> Facts {
        Facts { config_dir: DIR.into(), entries, theme, enabled: true }
    }

    fn file(s: ExtStep) -> Step {
        Step::File(s)
    }

    fn link(name: &str) -> Step {
        file(ExtStep::Symlink {
            target: format!("/usr/share/themes/Orchis/gtk-4.0/{name}"),
            link: format!("{DIR}/{name}"),
        })
    }

    fn rm(name: &str) -> Step {
        file(ExtStep::RemoveFile { path: format!("{DIR}/{name}") })
    }

    #[test]
    fn links_every_style_and_directory_of_the_theme() {
        let got = steps(&facts(vec![], files(&[("assets", true), ("gtk-dark.css", false), ("gtk.css", false)])));
        assert_eq!(
            got,
            vec![
                file(ExtStep::MakeDirs { path: DIR.into() }),
                link("assets"),
                link("gtk-dark.css"),
                link("gtk.css"),
            ]
        );
    }

    #[test]
    fn own_file_goes_to_backup_and_theme_link_is_replaced_without_one() {
        let got = steps(&facts(
            vec![e("gtk.css", EntryKind::File), e("assets", EntryKind::ThemeLink), e("colors.css", EntryKind::File)],
            files(&[("assets", true), ("gtk.css", false)]),
        ));
        assert_eq!(
            got,
            vec![
                file(ExtStep::MakeDirs { path: DIR.into() }),
                rm("assets"),
                link("assets"),
                file(ExtStep::CopyFile { src: format!("{DIR}/gtk.css"), dest: format!("{DIR}/gtk.css.rexon-backup") }),
                rm("gtk.css"),
                link("gtk.css"),
            ],
            "colors.css темой не занят — его не трогаем"
        );
    }

    #[test]
    fn second_backup_gets_a_number_and_own_directory_is_copied_as_tree() {
        let got = steps(&facts(
            vec![
                e("assets", EntryKind::Dir),
                e("assets.rexon-backup", EntryKind::Dir),
                e("gtk.css", EntryKind::ThemeLink),
            ],
            files(&[("assets", true), ("gtk.css", false)]),
        ));
        assert_eq!(
            got,
            vec![
                file(ExtStep::MakeDirs { path: DIR.into() }),
                rm("gtk.css"),
                file(ExtStep::CopyTree { src: format!("{DIR}/assets"), dest: format!("{DIR}/assets.rexon-backup-2") }),
                file(ExtStep::RemoveTree { path: format!("{DIR}/assets"), safe: false, ignore_errors: false }),
                link("assets"),
                link("gtk.css"),
            ]
        );
    }

    #[test]
    fn adwaita_removes_theme_links_and_brings_back_latest_backup() {
        let got = steps(&facts(
            vec![
                e("assets", EntryKind::ThemeLink),
                e("gtk.css", EntryKind::ThemeLink),
                e("gtk.css.rexon-backup", EntryKind::File),
                e("gtk.css.rexon-backup-2", EntryKind::File),
            ],
            ThemeGtk4::Builtin,
        ));
        assert_eq!(
            got,
            vec![
                rm("assets"),
                rm("gtk.css"),
                file(ExtStep::CopyFile {
                    src: format!("{DIR}/gtk.css.rexon-backup-2"),
                    dest: format!("{DIR}/gtk.css"),
                }),
                rm("gtk.css.rexon-backup-2"),
            ]
        );
    }

    #[test]
    fn freed_place_the_new_theme_does_not_take_gets_its_backup_back() {
        // Прежняя тема занимала gtk-dark.css, новая (Materia-dark) его не
        // несёт — свой файл человека возвращается.
        let got = steps(&facts(
            vec![
                e("gtk-dark.css", EntryKind::ThemeLink),
                e("gtk-dark.css.rexon-backup", EntryKind::File),
                e("gtk.css", EntryKind::ThemeLink),
            ],
            files(&[("gtk.css", false)]),
        ));
        assert_eq!(
            got,
            vec![
                file(ExtStep::MakeDirs { path: DIR.into() }),
                rm("gtk-dark.css"),
                rm("gtk.css"),
                link("gtk.css"),
                file(ExtStep::CopyFile {
                    src: format!("{DIR}/gtk-dark.css.rexon-backup"),
                    dest: format!("{DIR}/gtk-dark.css"),
                }),
                rm("gtk-dark.css.rexon-backup"),
            ]
        );
    }

    #[test]
    fn own_files_without_backup_stay_and_switch_off_touches_nothing() {
        let own = vec![e("gtk.css", EntryKind::File), e("assets", EntryKind::ThemeLink)];
        // Тема без gtk-4.0: ссылку прежней темы снять, свой gtk.css — оставить.
        assert_eq!(steps(&facts(own.clone(), ThemeGtk4::Missing)), vec![rm("assets")]);
        let mut off = facts(own, files(&[("gtk.css", false)]));
        off.enabled = false;
        assert!(steps(&off).is_empty());
    }

    #[test]
    fn theme_link_targets() {
        assert!(is_theme_link_target("/usr/share/themes/Orchis/gtk-4.0/gtk.css"));
        assert!(is_theme_link_target("/home/u/.themes/Fluent-Dark/gtk-4.0/assets"));
        assert!(is_theme_link_target("/home/u/.local/share/themes/Colloid/gtk-4.0/gtk-dark.css"));
        assert!(!is_theme_link_target("/home/u/css/gtk-4.0/gtk.css"));
        assert!(!is_theme_link_target("/home/u/my.css"));
    }

    #[test]
    fn theme_version_kinds() {
        let list = |entries: Vec<(&'static str, bool)>| {
            move |_: &str| Some(entries.iter().map(|(n, d)| (n.to_string(), *d)).collect::<Vec<_>>())
        };
        let none = |_: &str| None::<Vec<(String, bool)>>;
        let read = |_: &str| Some("@import url(\"resource:///org/gnome/theme/gtk.css\");".to_string());
        let plain = |_: &str| Some("window {}".to_string());
        assert_eq!(theme_gtk4("Adwaita-dark", Some("/t"), &none, &plain), ThemeGtk4::Builtin);
        assert_eq!(theme_gtk4("Arc", Some("/t/Arc"), &none, &plain), ThemeGtk4::Missing);
        assert_eq!(theme_gtk4("Arc", None, &none, &plain), ThemeGtk4::Missing);
        assert_eq!(
            theme_gtk4("WhiteSur", Some("/t/W"), &list(vec![("gtk.css", false), ("gtk.gresource", false)]), &read),
            ThemeGtk4::Packed
        );
        assert_eq!(
            theme_gtk4(
                "adw-gtk3",
                Some("/t/adw-gtk3"),
                &list(vec![
                    ("gtk.css", false),
                    ("libadwaita.css", false),
                    ("assets", true),
                    ("thumbnail.png", false),
                    ("settings.ini", false),
                ]),
                &plain,
            ),
            ThemeGtk4::Files {
                dir: "/t/adw-gtk3/gtk-4.0".into(),
                names: vec![("assets".into(), true), ("gtk.css".into(), false), ("libadwaita.css".into(), false)],
            }
        );
    }

    #[test]
    fn toggle_is_honest_about_themes_without_gtk4() {
        let t = toggle("Arc", &ThemeGtk4::Missing, true, true);
        assert!(!t.enabled && !t.checked && t.visible);
        assert!(t.note.contains("Arc") && t.note.contains("light or dark"));
        assert!(!toggle("Arc", &ThemeGtk4::Missing, true, false).note.contains("light or dark"));
        let t = toggle("Orchis", &files(&[("gtk.css", false)]), true, false);
        assert!(t.enabled && t.checked);
        assert!(!toggle("Orchis", &files(&[("gtk.css", false)]), false, false).checked);
    }

    #[test]
    fn press_gets_gtk4_settings_ini_once_and_deepin_nothing() {
        use crate::themes::ApplyStep;
        let f = facts(vec![], files(&[("gtk.css", false)]));
        let ini = |v: &str| {
            Step::Apply(ApplyStep::GtkIni {
                path: format!("{DIR}/settings.ini"),
                key: "gtk-theme-name".into(),
                value: v.into(),
            })
        };
        let mut base = Vec::new();
        extend_press(&mut base, "Orchis", "gnome", &f);
        assert_eq!(base, vec![ini("Orchis"), file(ExtStep::MakeDirs { path: DIR.into() }), link("gtk.css")]);
        let mut base = vec![ini("Orchis")];
        extend_press(&mut base, "Orchis", "kde", &f);
        assert_eq!(base.iter().filter(|s| **s == ini("Orchis")).count(), 1);
        let mut base = Vec::new();
        extend_press(&mut base, "Orchis", "deepin", &f);
        assert!(base.is_empty());
    }

    /// Прогон общим исполнителем на настоящем каталоге: свой gtk.css
    /// уходит в копию и возвращается, когда выбрана Adwaita.
    #[test]
    fn real_run_keeps_own_file_and_brings_it_back() {
        use crate::install_plan::{exec, no_sync_env, Ctx};
        use std::fs;
        let root = std::env::temp_dir().join(format!("rexon-gtk4-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let theme = root.join("share/themes/Orchis");
        fs::create_dir_all(theme.join("gtk-4.0/assets")).unwrap();
        fs::write(theme.join("gtk-4.0/gtk.css"), "orchis").unwrap();
        fs::write(theme.join("gtk-4.0/gtk-dark.css"), "orchis-dark").unwrap();
        let cfg = root.join("config/gtk-4.0");
        fs::create_dir_all(&cfg).unwrap();
        fs::write(cfg.join("gtk.css"), "mine").unwrap();
        fs::write(cfg.join("settings.ini"), "[Settings]\n").unwrap();
        let cfg_s = cfg.to_string_lossy().into_owned();
        let read = |p: &str| fs::read_to_string(p).ok();
        let run_with = |gtk4: ThemeGtk4| {
            let f = Facts { config_dir: cfg_s.clone(), entries: entries_live(&cfg_s), theme: gtk4, enabled: true };
            let no_run = |_: &[String], _: Option<u64>| crate::utils::RunReport::default();
            let ctx = Ctx {
                install: true,
                steamos: false,
                privileged: None,
                run: &no_run,
                have: &|_| false,
                spawn: &|_| {},
                wayland: false,
                sync_env: &no_sync_env,
            };
            exec(&steps(&f), &mut Vec::new(), &ctx).expect("план исполнился");
        };
        run_with(theme_gtk4("Orchis", Some(&theme.to_string_lossy()), &list_live, &read));
        assert_eq!(fs::read_to_string(cfg.join("gtk.css")).unwrap(), "orchis");
        assert!(fs::symlink_metadata(cfg.join("gtk.css")).unwrap().file_type().is_symlink());
        assert!(cfg.join("assets").is_dir());
        assert_eq!(fs::read_to_string(cfg.join("gtk.css.rexon-backup")).unwrap(), "mine");
        assert!(entries_live(&cfg_s).iter().any(|e| e.name == "gtk.css" && e.kind == EntryKind::ThemeLink));
        // Повторное применение той же темы копий не плодит.
        run_with(theme_gtk4("Orchis", Some(&theme.to_string_lossy()), &list_live, &read));
        assert!(!cfg.join("gtk.css.rexon-backup-2").exists());
        run_with(ThemeGtk4::Builtin);
        let left: Vec<String> = entries_live(&cfg_s).into_iter().map(|e| e.name).collect();
        assert_eq!(left, vec!["gtk.css".to_string()]);
        assert_eq!(fs::read_to_string(cfg.join("gtk.css")).unwrap(), "mine");
        assert!(cfg.join("settings.ini").is_file());
        let _ = fs::remove_dir_all(&root);
    }
}
