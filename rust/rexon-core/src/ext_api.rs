//! Публичный API расширений — чистая часть порта `rexon/extensions/api.py`.
//!
//! `ExtensionAPI` в Python — тонкая обёртка над главным окном (Qt) и
//! подпроцессами (`gsettings`/`xfconf-query`/`notify-send`/`fc-cache`,
//! `shutil.copytree`), поэтому целиком в ядро без Qt не переносится.
//! Портирована ЧИСТАЯ, детерминированная часть: построение команды применения
//! GTK-/иконочной темы по текущему DE (`apply_gtk_theme`/`apply_icon_theme`) —
//! маппинг DE → бэкенд настроек (gsettings-схема или xfconf-свойство). Само
//! исполнение команды (`safe_run`) и возврат `bool` по коду выхода остаются за
//! GUI-слоем; здесь — только «какую команду запустить».
//!
//! Сверено байт-в-байт с Python: аргументы команд сняты монипатчем `safe_run`
//! на реальном `ExtensionAPI` (golden-тест `ext_api_parity.rs`).

/// Что применяем: GTK-тему или тему иконок. Отличие только в ключе gsettings
/// (`gtk-theme`/`icon-theme`) и в имени xfconf-свойства.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeKind {
    /// GTK-тема (`apply_gtk_theme`).
    Gtk,
    /// Тема иконок (`apply_icon_theme`).
    Icon,
}

impl ThemeKind {
    /// Ключ gsettings для gnome/cinnamon/mate.
    fn gsettings_key(self) -> &'static str {
        match self {
            ThemeKind::Gtk => "gtk-theme",
            ThemeKind::Icon => "icon-theme",
        }
    }

    /// Имя xfconf-свойства канала `xsettings` для XFCE.
    fn xfconf_property(self) -> &'static str {
        match self {
            ThemeKind::Gtk => "/Net/ThemeName",
            ThemeKind::Icon => "/Net/IconThemeName",
        }
    }
}

/// gsettings-схема интерфейса для DE (`schema_map` в Python). `None` — DE не
/// использует gsettings-путь.
fn gsettings_schema(de: &str) -> Option<&'static str> {
    match de {
        // Pantheon оформляется теми же ключами GNOME.
        "gnome" | "pantheon" => Some("org.gnome.desktop.interface"),
        "cinnamon" => Some("org.cinnamon.desktop.interface"),
        "mate" => Some("org.mate.interface"),
        _ => None,
    }
}

/// Команда применения темы (`kind`) с именем `name` для текущего DE `de`.
///
/// Порт ветвления `apply_gtk_theme`/`apply_icon_theme`: gnome/cinnamon/mate →
/// `gsettings set <schema> <key> <name>`; xfce → `xfconf-query -c xsettings -p
/// <prop> -s <name>`. Для остальных DE (kde/budgie/…) команды нет → `None`
/// (в Python — `return False`, тема через этот путь не применяется).
pub fn theme_apply_command(de: &str, kind: ThemeKind, name: &str) -> Option<Vec<String>> {
    if let Some(schema) = gsettings_schema(de) {
        return Some(vec![
            "gsettings".to_string(),
            "set".to_string(),
            schema.to_string(),
            kind.gsettings_key().to_string(),
            name.to_string(),
        ]);
    }
    if de == "xfce" {
        return Some(vec![
            "xfconf-query".to_string(),
            "-c".to_string(),
            "xsettings".to_string(),
            "-p".to_string(),
            kind.xfconf_property().to_string(),
            "-s".to_string(),
            name.to_string(),
        ]);
    }
    None
}

/// Полный путь цели копирования каталога оформления — чистая часть
/// `copy_theme_files`/`copy_icon_files`: `os.path.join(base_dir,
/// os.path.basename(src_dir.rstrip("/")))`.
///
/// Каталог-приёмник (`~/.themes`, `/usr/share/themes`, каталог иконок)
/// выбирает шим: он раскрывается через `utils.expand_user`, то есть зависит
/// от `XDG_DATA_HOME`, и вычислять его тут — значит потерять эту поправку.
///
/// `None` — имя пустое (`src_dir` вида `"/"`, `"///"` или пустая строка).
/// Оригинал в этом случае получал `os.path.join(base, "")` = САМ каталог-
/// приёмник и шёл его сносить (`_safe_rmtree(~/.themes)`) — снос всех тем
/// пользователя из-за мусорного аргумента. Порт такой вход отвергает
/// (правило «защита от дурака»); шим на `None` возвращает `False`.
pub fn copy_target(base_dir: &str, src_dir: &str) -> Option<String> {
    let trimmed = src_dir.trim_end_matches('/');
    let name = trimmed.rsplit('/').next().unwrap_or("");
    if name.is_empty() {
        return None;
    }
    Some(if base_dir.is_empty() || base_dir.ends_with('/') {
        format!("{base_dir}{name}")
    } else {
        format!("{base_dir}/{name}")
    })
}
