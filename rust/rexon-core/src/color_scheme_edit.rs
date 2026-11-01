//! «Изменить…» и «Создать на основе…» у цветовой схемы — как в модуле KDE
//! «Цвета».
//!
//! Что делает KDE (сверено с plasma-workspace 6.8.2, `kcms/colors`,
//! `kcm_colors.so` и `kcolorschemeeditor --help`, 31.10.2026): кнопка
//! «Изменить» плитки зовёт `KCMColors::editScheme`, а тот запускает
//! отдельную программу `kcolorschemeeditor <схема>` (служба
//! `org.kde.kcolorschemeeditor`). Своей схеме человека (лежит в
//! `~/.local/share/color-schemes`, её можно удалить) добавляется
//! `--overwrite` — в редакторе появляется «Применить», сохраняющее поверх;
//! у системной схемы есть только «Сохранить как…», то есть правка системной
//! — это и есть «создать новую на основе». Поэтому здесь:
//!
//! * «Изменить…» — `kcolorschemeeditor <схема>` (+ `--overwrite` у своей);
//! * «Создать на основе…» — `kcolorschemeeditor <схема>` без `--overwrite`;
//! * программы нет (не KDE, plasma-workspace не стоит) — свой простой
//!   редактор Rexon: фон и текст окна, области просмотра, кнопки,
//!   выделения, подсказки и заголовка окна; итог — схема в
//!   `~/.local/share/color-schemes`. Вне Plasma схемы применяет слой Rexon
//!   ([`crate::shell_palette`]), он читает те же ключи — редактор работает
//!   и там.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::{n_, tr, trf};
use crate::shell_palette::{ini_apply, ini_get, IniEdit, Rgb};

/// Программа-редактор схем KDE.
pub const KDE_EDITOR: &str = "kcolorschemeeditor";

/// Что будет делать кнопка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    /// «Изменить…»: своя схема правится поверх.
    Edit,
    /// «Создать на основе…»: всегда новая схема.
    NewFrom,
}

impl EditMode {
    pub fn from_key(key: &str) -> EditMode {
        if key == "new" {
            EditMode::NewFrom
        } else {
            EditMode::Edit
        }
    }
}

/// Своя ли схема: лежит в каталоге схем человека (её и удалить можно — как
/// `RemovableRole` у модуля KDE).
pub fn is_user_scheme(path: &str, home: &str) -> bool {
    let dir = format!("{}/.local/share/color-schemes/", home.trim_end_matches('/'));
    path.starts_with(&dir)
}

/// Команда редактора KDE для схемы `name` (имя файла без `.colors`), как у
/// `KCMColors::editScheme`. `None` — программы нет (`has_editor`), тогда
/// работает свой редактор.
pub fn kde_editor_argv(name: &str, user_scheme: bool, mode: EditMode, has_editor: bool) -> Option<Vec<String>> {
    if !has_editor || name.trim().is_empty() {
        return None;
    }
    let mut argv = vec![KDE_EDITOR.to_string(), name.to_string()];
    if mode == EditMode::Edit && user_scheme {
        argv.push("--overwrite".to_string());
    }
    Some(argv)
}

/// Схема местной плитки: (имя файла без `.colors`, путь к файлу). У плитки
/// схемы номер `local:colors:<имя>`, а `install_path` — путь без
/// расширения (`installed_scan::mk_item`).
pub fn scheme_of_item(item_id: &str, install_path: &str) -> Option<(String, String)> {
    let name = item_id.strip_prefix("local:colors:")?;
    if name.is_empty() || install_path.trim().is_empty() {
        return None;
    }
    let file = if install_path.ends_with(".colors") {
        install_path.to_string()
    } else {
        format!("{install_path}.colors")
    };
    Some((name.to_string(), file))
}

/// Что открыть по кнопке: редактор KDE или свой.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditOpen {
    /// `kde` — запущен редактор KDE (`argv`), `own` — окно Rexon.
    pub mode: String,
    pub argv: Vec<String>,
    /// Своё окно: предложенное имя, роли, файл-основа и имя файла, который
    /// можно переписать (`overwrite`, пусто — только новая схема).
    pub name: String,
    pub roles: Vec<SchemeRole>,
    pub base: String,
    pub overwrite: String,
}

/// Решить, что открыть для плитки схемы. `display` — имя схемы для людей,
/// `base_text` — содержимое её файла (нужно только своему окну).
pub fn edit_open(
    item_id: &str,
    install_path: &str,
    display: &str,
    home: &str,
    mode: EditMode,
    has_kde_editor: bool,
    base_text: &str,
) -> Option<EditOpen> {
    let (name, file) = scheme_of_item(item_id, install_path)?;
    let user = is_user_scheme(&file, home);
    if let Some(argv) = kde_editor_argv(&name, user, mode, has_kde_editor) {
        return Some(EditOpen {
            mode: "kde".into(),
            argv,
            name: String::new(),
            roles: Vec::new(),
            base: file,
            overwrite: String::new(),
        });
    }
    let shown = if display.trim().is_empty() { name.as_str() } else { display.trim() };
    Some(EditOpen {
        mode: "own".into(),
        argv: Vec::new(),
        name: suggested_name(shown, mode, user),
        roles: read_roles(base_text),
        base: file,
        overwrite: if mode == EditMode::Edit && user { name } else { String::new() },
    })
}

// ============================================================================
// Свой редактор: роли
// ============================================================================

/// Роль схемы, которую правит свой редактор.
struct RoleDef {
    key: &'static str,
    label: &'static str,
    group: &'static str,
    bg_key: &'static str,
    fg_key: &'static str,
}

/// Основные роли. Заголовок окна — цвета рамки активного окна (`[WM]`),
/// остальные — «обычный» фон и текст своей группы `Colors:*`.
const ROLES: &[RoleDef] = &[
    RoleDef { key: "window", label: n_("Window"), group: "Colors:Window", bg_key: "BackgroundNormal", fg_key: "ForegroundNormal" },
    RoleDef { key: "view", label: n_("View (lists and text fields)"), group: "Colors:View", bg_key: "BackgroundNormal", fg_key: "ForegroundNormal" },
    RoleDef { key: "button", label: n_("Button"), group: "Colors:Button", bg_key: "BackgroundNormal", fg_key: "ForegroundNormal" },
    RoleDef { key: "selection", label: n_("Selection"), group: "Colors:Selection", bg_key: "BackgroundNormal", fg_key: "ForegroundNormal" },
    RoleDef { key: "tooltip", label: n_("Tooltip"), group: "Colors:Tooltip", bg_key: "BackgroundNormal", fg_key: "ForegroundNormal" },
    RoleDef { key: "titlebar", label: n_("Window title bar"), group: "WM", bg_key: "activeBackground", fg_key: "activeForeground" },
];

/// Роль с цветами для окна редактора (`#rrggbb`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemeRole {
    pub key: String,
    #[serde(default)]
    pub label: String,
    pub background: String,
    pub foreground: String,
}

/// Цвета ролей схемы `text`. Ключа нет — у окна берётся «Окно», у
/// остальных — тоже «Окно» (как KColorScheme подставляет при пропуске), в
/// крайнем случае — светлая Breeze.
pub fn read_roles(text: &str) -> Vec<SchemeRole> {
    let get = |group: &str, key: &str| ini_get(text, group, key).and_then(|v| Rgb::parse(&v));
    let win_bg = get("Colors:Window", "BackgroundNormal").unwrap_or(Rgb(239, 240, 241));
    let win_fg = get("Colors:Window", "ForegroundNormal").unwrap_or(Rgb(35, 38, 41));
    ROLES
        .iter()
        .map(|r| SchemeRole {
            key: r.key.to_string(),
            label: tr(r.label),
            background: get(r.group, r.bg_key).unwrap_or(win_bg).hex(),
            foreground: get(r.group, r.fg_key).unwrap_or(win_fg).hex(),
        })
        .collect()
}

/// Имя файла новой схемы из имени для людей — как «Сохранить как…»
/// редактора KDE: остаются буквы и цифры (пробелы и знаки уходят). Пусто —
/// `None`.
pub fn file_stem_for(name: &str) -> Option<String> {
    let stem: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
    (!stem.is_empty()).then_some(stem)
}

/// Предложенное имя новой схемы на основе `base`.
pub fn suggested_name(base: &str, mode: EditMode, user_scheme: bool) -> String {
    if mode == EditMode::Edit && user_scheme {
        base.to_string()
    } else {
        trf("%1 (copy)", &[&base])
    }
}

/// Текст новой схемы: схема-основа `base` с цветами `roles` и именем
/// `name` (`[General] Name`, переводы имени основы убраны — иначе в
/// русском меню осталось бы старое имя). Пустой `base` — схема с нуля.
pub fn build_scheme(base: &str, name: &str, stem: &str, roles: &[SchemeRole]) -> String {
    // Переводы имени основы (`Name[ru]=…`) — вон.
    let cleaned: String = base
        .split_inclusive('\n')
        .filter(|l| !l.trim_start().starts_with("Name["))
        .collect();
    let mut edits = vec![
        IniEdit { group: "General".into(), key: "Name".into(), value: Some(name.to_string()) },
        IniEdit { group: "General".into(), key: "ColorScheme".into(), value: Some(stem.to_string()) },
    ];
    for role in roles {
        let Some(def) = ROLES.iter().find(|r| r.key == role.key) else { continue };
        for (key, color) in [(def.bg_key, &role.background), (def.fg_key, &role.foreground)] {
            if let Some(c) = Rgb::parse(color) {
                edits.push(IniEdit { group: def.group.into(), key: key.into(), value: Some(c.kde()) });
            }
        }
    }
    ini_apply(Some(&cleaned), &edits)
}

/// Итог сохранения.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SaveResult {
    pub ok: bool,
    pub message: String,
    /// Файл новой схемы (при успехе).
    pub path: String,
}

/// Сохранить схему своего редактора в `~/.local/share/color-schemes`.
/// `base_path` — файл схемы-основы; `overwrite` — имя файла своей схемы,
/// которую разрешено переписать («Изменить…» у своей схемы, имя то же).
pub fn save_scheme(
    home: &str,
    base_path: &str,
    name: &str,
    roles: &[SchemeRole],
    overwrite: Option<&str>,
    system_dirs: &[String],
) -> SaveResult {
    let fail = |message: String| SaveResult { ok: false, message, path: String::new() };
    let name = name.trim();
    let Some(stem) = file_stem_for(name) else {
        return fail(tr("Enter a name for the color scheme."));
    };
    let dir = PathBuf::from(home).join(".local/share/color-schemes");
    let target = dir.join(format!("{stem}.colors"));
    let taken_by_user = target.exists() && overwrite != Some(stem.as_str());
    // Системная схема с тем же файлом ушла бы в тень своей — так тоже нельзя.
    let taken_by_system = system_dirs
        .iter()
        .any(|d| Path::new(d).join(format!("{stem}.colors")).exists());
    if taken_by_user || taken_by_system {
        return fail(trf("A color scheme named “%1” already exists — choose another name.", &[&stem]));
    }
    let base = std::fs::read_to_string(base_path).unwrap_or_default();
    let text = build_scheme(&base, name, &stem, roles);
    let wrote = std::fs::create_dir_all(&dir).and_then(|_| {
        let tmp = dir.join(format!(".{stem}.colors.rexon-tmp"));
        std::fs::write(&tmp, text.as_bytes())?;
        std::fs::rename(&tmp, &target)
    });
    match wrote {
        Ok(()) => SaveResult {
            ok: true,
            message: trf("Color scheme “%1” saved.", &[&name]),
            path: target.to_string_lossy().into_owned(),
        },
        Err(e) => fail(trf("Couldn't save the color scheme: %1", &[&e])),
    }
}

/// Системные каталоги схем (`$XDG_DATA_DIRS/color-schemes`).
pub fn system_scheme_dirs(xdg_data_dirs: Option<&str>) -> Vec<String> {
    let dirs = xdg_data_dirs.filter(|s| !s.trim().is_empty()).unwrap_or("/usr/local/share:/usr/share");
    dirs.split(':')
        .filter(|d| !d.is_empty())
        .map(|d| format!("{}/color-schemes", d.trim_end_matches('/')))
        .collect()
}

// ============================================================================
// Подписи
// ============================================================================

/// Подписи кнопок и окна своего редактора.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditorTexts {
    pub edit: String,
    pub edit_tooltip: String,
    pub new_from: String,
    pub new_from_tooltip: String,
    pub title: String,
    pub name_label: String,
    pub background: String,
    pub foreground: String,
    pub save: String,
    pub cancel: String,
    pub hint: String,
}

pub fn editor_texts(has_kde_editor: bool) -> EditorTexts {
    EditorTexts {
        edit: tr("Edit…"),
        edit_tooltip: if has_kde_editor {
            tr("Open the scheme in the KDE color scheme editor")
        } else {
            tr("Change the main colors of the scheme")
        },
        new_from: tr("New Based on This…"),
        new_from_tooltip: tr("Make a new color scheme from this one; the original stays as it is"),
        title: tr("Color Scheme Editor"),
        name_label: tr("Name:"),
        background: tr("Background"),
        foreground: tr("Text"),
        save: tr("Save"),
        cancel: tr("Cancel"),
        hint: tr("The scheme is saved to your color schemes and appears in the list; apply it as usual."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_drops_spaces_and_signs() {
        assert_eq!(file_stem_for("My Dark 2!").as_deref(), Some("MyDark2"));
        assert_eq!(file_stem_for("  ").as_deref(), None);
    }
}
