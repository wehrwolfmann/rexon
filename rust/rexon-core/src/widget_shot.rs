//! Снимок настоящих виджетов для плитки «Оформления»: стиль Qt (Breeze,
//! Fusion, Oxygen, Kvantum с его темой) и тема GTK 3.
//!
//! Окно «Стиль приложений» KDE не рисует образец само: каждая плитка — живая
//! форма из настоящих виджетов (вкладки, кнопка, флажок, переключатель,
//! список, поле, счётчик, ползунок, полоса хода, полоса прокрутки),
//! которой назначен выбранный стиль, с палитрой текущей цветовой схемы. Rexon
//! делает то же, но снимком в ОТДЕЛЬНОМ процессе — помощнике
//! [`HELPER_BINARY`]: чужой модуль стиля (или движок темы GTK) в процесс
//! Rexon не загружается, падение или зависание стоит одного снимка, процесс
//! убивается по сроку, плитка получает прежнюю нарисованную картинку.
//!
//! * Qt — платформа `offscreen`, экрана не нужно вовсе; виджеты снимаются
//!   `QWidget::grab()`.
//! * GTK 3 — `GtkOffscreenWindow`: окно никогда не выходит на экран, но
//!   соединение с дисплеем GTK 3 требует (безэкранного вывода у него нет),
//!   поэтому снимок GTK берётся только в сеансе с `WAYLAND_DISPLAY` или
//!   `DISPLAY`.
//!
//! Здесь всё, что решается: что снимать для плитки, строка запуска и
//! окружение помощника, палитра из схемы KDE, ключ кеша, разбор аргументов
//! самого помощника. Помощник (`rexon-gui`, `src/bin/theme_shot/`) только
//! исполняет.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Имя помощника. В дереве разработчика он лежит рядом с бинарём окна, в
/// пакете — в `/usr/lib/rexon` ([`PACKAGE_HELPER`]).
pub const HELPER_BINARY: &str = "rexon-theme-shot";
/// Где помощник лежит после установки пакета.
pub const PACKAGE_HELPER: &str = "/usr/lib/rexon/rexon-theme-shot";
/// Сколько ждать снимка, секунд.
pub const SHOT_TIMEOUT_SECS: u64 = 15;
/// Размер формы с виджетами в логических точках. Плитка
/// ([`crate::local_preview::PREVIEW_WIDTH`]×[`crate::local_preview::PREVIEW_HEIGHT`])
/// для настоящих виджетов мала: форма в полтора раза больше. Рисуется она в
/// [`render_scale`] раз крупнее и сжимается до кадра [`SHOT_WIDTH`]×
/// [`SHOT_HEIGHT`] с сохранением сторон.
pub const FORM_WIDTH: i64 = 450;
/// См. [`FORM_WIDTH`].
pub const FORM_HEIGHT: i64 = 255;
/// Размер готового кадра — размер картинки в подробностях темы
/// ([`crate::appearance_page::DETAIL_PREVIEW_W`]×[`crate::appearance_page::DETAIL_PREVIEW_H`],
/// ровно вдвое больше плитки). Кадр размером с плитку там растягивался
/// вдвое и выходил размытым; плитка же крупный кадр просто сжимает.
pub const SHOT_WIDTH: i32 = crate::appearance_page::DETAIL_PREVIEW_W;
/// См. [`SHOT_WIDTH`].
pub const SHOT_HEIGHT: i32 = crate::appearance_page::DETAIL_PREVIEW_H;
/// Отметка правил снимка в ключе кеша: меняется — все снимки переснимаются.
pub const SHOT_STYLE: &str = "w2";

/// Во сколько раз крупнее рисовать форму `form`, чтобы кадр `size` из неё
/// только сжимался, а не растягивался: целое, от 1 до 4. Форма 450×255 в
/// кадр 600×340 — вдвое (900×510 → 600×340).
pub fn render_scale(form: (i32, i32), size: (i32, i32)) -> i32 {
    if form.0 <= 0 || form.1 <= 0 {
        return 1;
    }
    let k = (size.0 as f64 / form.0 as f64).max(size.1 as f64 / form.1 as f64);
    (k.ceil() as i32).clamp(1, 4)
}

/// Роли палитры Qt по группам схемы KDE — то же соответствие, что у палитры
/// приложения KDE: окно — группа `Colors:Window`, поля и списки —
/// `Colors:View`, кнопки — `Colors:Button`, выделение — `Colors:Selection`,
/// подсказки — `Colors:Tooltip`. Имена ролей — имена `QPalette::ColorRole`.
pub const PALETTE_ROLES: &[(&str, &str, &str)] = &[
    ("Window", "Colors:Window", "BackgroundNormal"),
    ("WindowText", "Colors:Window", "ForegroundNormal"),
    ("Base", "Colors:View", "BackgroundNormal"),
    ("AlternateBase", "Colors:View", "BackgroundAlternate"),
    ("Text", "Colors:View", "ForegroundNormal"),
    ("Button", "Colors:Button", "BackgroundNormal"),
    ("ButtonText", "Colors:Button", "ForegroundNormal"),
    ("Highlight", "Colors:Selection", "BackgroundNormal"),
    ("HighlightedText", "Colors:Selection", "ForegroundNormal"),
    ("ToolTipBase", "Colors:Tooltip", "BackgroundNormal"),
    ("ToolTipText", "Colors:Tooltip", "ForegroundNormal"),
    ("Link", "Colors:View", "ForegroundLink"),
    ("LinkVisited", "Colors:View", "ForegroundVisited"),
    ("PlaceholderText", "Colors:View", "ForegroundInactive"),
];
/// Роли текста, которые у недоступных виджетов бледнеют к своему фону.
const DISABLED_TEXT: &[(&str, &str)] =
    &[("WindowText", "Window"), ("Text", "Base"), ("ButtonText", "Button")];
/// Префикс роли группы «недоступно» в строке `--color`.
pub const DISABLED_PREFIX: &str = "Disabled:";

/// Подписи формы по порядку: две вкладки, главная и вторая кнопка, флажок,
/// переключатель, список, текст в поле. Помощник берёт их по номеру.
pub fn sample_texts() -> Vec<String> {
    use crate::i18n::tr;
    vec![
        tr("Preview"),
        tr("Settings"),
        tr("Apply"),
        tr("Cancel"),
        tr("Check box"),
        tr("Radio button"),
        tr("Combo box"),
        tr("Text"),
    ]
}

/// Что снимать.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShotKind {
    /// Стиль Qt по ключу `QStyleFactory`; у темы Kvantum — ещё её имя и
    /// каталог (Kvantum берёт тему из своего файла настроек, переменной
    /// окружения для неё нет — помощнику подставляется свой каталог настроек).
    Qt { style: String, kvantum: Option<(String, String)> },
    /// Тема GTK 3: имя, каталог, тёмный вариант.
    Gtk { theme: String, dir: String, dark: bool },
}

/// Заказ снимка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotRequest {
    pub kind: ShotKind,
    /// Палитра Qt: `(роль, #rrggbb)`; роли группы «недоступно» — с
    /// [`DISABLED_PREFIX`]. Пусто — родная палитра стиля.
    pub palette: Vec<(String, String)>,
}

/// Цвет KConfig «r,g,b» → `#rrggbb`.
fn kconfig_hex(value: &str) -> Option<(u8, u8, u8)> {
    let parts: Vec<u8> = value.split(',').map(|p| p.trim().parse::<u8>().ok()).collect::<Option<_>>()?;
    match parts[..] {
        [r, g, b] | [r, g, b, _] => Some((r, g, b)),
        _ => None,
    }
}

fn hex((r, g, b): (u8, u8, u8)) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Палитра Qt из текста `kdeglobals` (или файла схемы `.colors`): роли
/// [`PALETTE_ROLES`] и бледный текст недоступных виджетов (середина между
/// текстом и его фоном). Схемы нет (вне KDE `kdeglobals` без цветов) —
/// пусто: стиль берёт свою родную палитру, как его увидит программа Qt без
/// платформенной темы KDE.
pub fn palette_from_kdeglobals(text: &str) -> Vec<(String, String)> {
    let ini = crate::gtk_sanitizer::read_kde_ini(Some(text));
    let mut got: HashMap<&str, (u8, u8, u8)> = HashMap::new();
    for (role, group, key) in PALETTE_ROLES {
        if let Some(c) = kconfig_hex(&crate::gtk_sanitizer::kde_value(&ini, group, key)) {
            got.insert(role, c);
        }
    }
    // Без цвета окна и его текста схемы нет: обрывки не смешиваем с чужой
    // родной палитрой стиля.
    if !got.contains_key("Window") || !got.contains_key("WindowText") {
        return Vec::new();
    }
    let mut out: Vec<(String, String)> = PALETTE_ROLES
        .iter()
        .filter_map(|(role, _, _)| got.get(role).map(|c| (role.to_string(), hex(*c))))
        .collect();
    for (text_role, back_role) in DISABLED_TEXT {
        if let (Some(t), Some(b)) = (got.get(text_role), got.get(back_role)) {
            let mid = |x: u8, y: u8| ((x as u16 + y as u16) / 2) as u8;
            out.push((format!("{DISABLED_PREFIX}{text_role}"), hex((mid(t.0, b.0), mid(t.1, b.1), mid(t.2, b.2)))));
        }
    }
    out
}

/// Есть ли у каталога темы GTK таблица стилей GTK 3 (`gtk-3.0/gtk.css` либо
/// `gtk-3.<N>/gtk.css`). Без неё GTK 3 молча нарисует Adwaita — такой снимок
/// хуже нарисованного образца.
pub fn has_gtk3(dir: &Path) -> bool {
    gtk3_css(dir, "gtk.css").is_some()
}

/// Файл `name` в подкаталоге GTK 3 темы.
fn gtk3_css(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join("gtk-3.0").join(name);
    if direct.is_file() {
        return Some(direct);
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("gtk-3."))
        .map(|e| e.path().join(name))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.pop()
}

/// Просит ли человек тёмный вариант тем GTK (`settings.ini` GTK 3,
/// `gtk-application-prefer-dark-theme`).
pub fn prefers_dark(settings_ini: &str) -> bool {
    settings_ini.lines().any(|l| {
        let mut kv = l.splitn(2, '=');
        kv.next().map(str::trim) == Some("gtk-application-prefer-dark-theme")
            && matches!(kv.next().map(|v| v.trim().to_ascii_lowercase()).as_deref(), Some("1" | "true"))
    })
}

/// Что снимать для плитки `local_id` (`local:<вид>:<имя>`). `None` — плитка
/// не стиль Qt, не тема Kvantum и не тема GTK 3 либо снимать нечем.
pub fn shot_kind(local_id: &str, install_path: &str, gtk_display: bool, gtk_dark: bool) -> Option<ShotKind> {
    let rest = local_id.strip_prefix("local:")?;
    let (kind, name) = rest.split_once(':')?;
    if name.is_empty() {
        return None;
    }
    match kind {
        "widgetstyle" => Some(ShotKind::Qt { style: name.to_string(), kvantum: None }),
        "kvantum" if !install_path.is_empty() => {
            Some(ShotKind::Qt { style: "kvantum".into(), kvantum: Some((name.to_string(), install_path.to_string())) })
        }
        "gtk" if gtk_display && has_gtk3(Path::new(install_path)) => {
            let dir = Path::new(install_path);
            let theme = dir.file_name()?.to_string_lossy().into_owned();
            let dark = gtk_dark && gtk3_css(dir, "gtk-dark.css").is_some();
            Some(ShotKind::Gtk { theme, dir: install_path.to_string(), dark })
        }
        _ => None,
    }
}

/// Отметка времени файла, секунды (нет файла — 0).
fn stamp(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

/// Что известно о снимках на этой машине — собирается один раз на пачку
/// плиток.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotSetup {
    /// Помощник ([`find_helper`]); `None` — снимать нечем.
    pub helper: Option<String>,
    /// Палитра текущей схемы KDE ([`palette_from_kdeglobals`]).
    pub palette: Vec<(String, String)>,
    /// Отметка `kdeglobals` — палитра в ключе кеша.
    pub colors_stamp: i64,
    /// Есть ли дисплей для GTK.
    pub gtk_display: bool,
    /// Тёмный вариант тем GTK.
    pub gtk_dark: bool,
}

/// Помощник: рядом с бинарём окна (дерево разработчика, `/usr/lib/rexon`
/// пакета), затем [`PACKAGE_HELPER`], затем `$PATH`.
pub fn find_helper(exe_dir: Option<&Path>, is_exec: &dyn Fn(&Path) -> bool, path_var: &str) -> Option<String> {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(d) = exe_dir {
        cands.push(d.join(HELPER_BINARY));
    }
    cands.push(PathBuf::from(PACKAGE_HELPER));
    cands.extend(path_var.split(':').filter(|d| d.starts_with('/')).map(|d| Path::new(d).join(HELPER_BINARY)));
    cands.into_iter().find(|p| is_exec(p)).map(|p| p.to_string_lossy().into_owned())
}

fn is_exec_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

impl ShotSetup {
    /// Живая машина: помощник, `kdeglobals` и `gtk-3.0/settings.ini` из
    /// `$XDG_CONFIG_HOME` (иначе `~/.config`), дисплей сеанса.
    pub fn live(home: &str) -> Self {
        let exe_dir = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
        let helper = find_helper(exe_dir.as_deref(), &is_exec_file, &std::env::var("PATH").unwrap_or_default());
        let cfg = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|d| d.starts_with('/'))
            .unwrap_or_else(|| format!("{}/.config", home.trim_end_matches('/')));
        let kdeglobals = Path::new(&cfg).join("kdeglobals");
        let palette = std::fs::read_to_string(&kdeglobals).map(|t| palette_from_kdeglobals(&t)).unwrap_or_default();
        let set = |k: &str| std::env::var(k).is_ok_and(|v| !v.trim().is_empty());
        let gtk_display = set("WAYLAND_DISPLAY") || set("DISPLAY");
        let gtk_dark = std::fs::read_to_string(Path::new(&cfg).join("gtk-3.0/settings.ini"))
            .map(|t| prefers_dark(&t))
            .unwrap_or(false);
        ShotSetup { helper, palette, colors_stamp: stamp(&kdeglobals), gtk_display, gtk_dark }
    }

    /// Что снимать для плитки (`None` — не тот вид либо снимать нечем).
    pub fn request(&self, local_id: &str, install_path: &str) -> Option<ShotRequest> {
        self.helper.as_ref()?;
        let kind = shot_kind(local_id, install_path, self.gtk_display, self.gtk_dark)?;
        let palette = match kind {
            ShotKind::Qt { .. } => self.palette.clone(),
            ShotKind::Gtk { .. } => Vec::new(),
        };
        Some(ShotRequest { kind, palette })
    }

    /// Добавка к ключу кеша картинки плитки: кадр зависит от того, есть ли
    /// чем снимать, от палитры схемы (у стилей Qt), от файлов настроек темы
    /// Kvantum и таблиц стилей GTK (отметка каталога темы меняется лишь при
    /// добавлении и удалении файлов) и от тёмного варианта GTK. Пусто —
    /// плитка снимком не делается.
    pub fn cache_tag(&self, local_id: &str, install_path: &str) -> String {
        let Some(kind) = shot_kind(local_id, install_path, self.gtk_display, self.gtk_dark) else {
            return String::new();
        };
        if self.helper.is_none() {
            return "noshot".into();
        }
        let size = format!("{FORM_WIDTH}x{FORM_HEIGHT}");
        match kind {
            ShotKind::Qt { kvantum: None, .. } => format!("{SHOT_STYLE}\u{0}{size}\u{0}{}", self.colors_stamp),
            ShotKind::Qt { kvantum: Some((name, dir)), .. } => {
                let cfg = stamp(&Path::new(&dir).join(format!("{name}.kvconfig")));
                let svg = stamp(&Path::new(&dir).join(format!("{name}.svg")));
                format!("{SHOT_STYLE}\u{0}{size}\u{0}{}\u{0}{cfg}\u{0}{svg}", self.colors_stamp)
            }
            ShotKind::Gtk { dir, dark, .. } => {
                let css = gtk3_css(Path::new(&dir), "gtk.css").map_or(0, |p| stamp(&p));
                format!("{SHOT_STYLE}\u{0}{size}\u{0}{css}\u{0}{dark}")
            }
        }
    }
}

// ── строка запуска помощника ─────────────────────────────────────────────

/// Режим помощника: Qt.
pub const MODE_QT: &str = "qt";
/// Режим помощника: GTK.
pub const MODE_GTK: &str = "gtk";

/// argv помощника: режим, затем `--out`, `--size`, `--form`, у Qt `--style`
/// и `--color роль=#цвет`, и подписи `--text` по порядку [`sample_texts`].
pub fn helper_argv(helper: &str, req: &ShotRequest, texts: &[String], out: &str) -> Vec<String> {
    let mut argv = vec![helper.to_string()];
    match &req.kind {
        ShotKind::Qt { style, .. } => {
            argv.extend([MODE_QT.to_string(), "--style".into(), style.clone()]);
        }
        ShotKind::Gtk { .. } => argv.push(MODE_GTK.into()),
    }
    argv.extend([
        "--out".into(),
        out.to_string(),
        "--size".into(),
        format!("{SHOT_WIDTH}x{SHOT_HEIGHT}"),
        "--form".into(),
        format!("{FORM_WIDTH}x{FORM_HEIGHT}"),
    ]);
    for (role, color) in &req.palette {
        argv.extend(["--color".into(), format!("{role}={color}")]);
    }
    for t in texts {
        argv.extend(["--text".into(), t.clone()]);
    }
    argv
}

/// Окружение помощника: что задать и что убрать. `work` — свой временный
/// каталог снимка (там лежат подставные настройки Kvantum и каталог тем
/// GTK).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HelperEnv {
    pub set: Vec<(String, String)>,
    pub remove: Vec<String>,
    /// Символические ссылки, которые надо сделать до запуска: (ссылка, цель).
    pub links: Vec<(PathBuf, PathBuf)>,
    /// Файлы, которые надо записать до запуска: (путь, текст).
    pub files: Vec<(PathBuf, String)>,
}

/// Окружение помощника для заказа `req` с временным каталогом `work`.
pub fn helper_env(req: &ShotRequest, work: &Path) -> HelperEnv {
    let mut env = HelperEnv::default();
    let set = |env: &mut HelperEnv, k: &str, v: &str| env.set.push((k.to_string(), v.to_string()));
    match &req.kind {
        ShotKind::Qt { kvantum, .. } => {
            // Без экрана и в масштабе 1: иначе кадр выходит вдвое больше.
            set(&mut env, "QT_QPA_PLATFORM", "offscreen");
            set(&mut env, "QT_SCALE_FACTOR", "1");
            // Платформенная тема (KDE, qt6ct) и стиль процесса подменили бы
            // палитру и шрифт формы: образец — стиль с палитрой схемы и
            // больше ничего.
            for k in ["QT_QPA_PLATFORMTHEME", "QT_STYLE_OVERRIDE", "QT_SCREEN_SCALE_FACTORS", "QT_AUTO_SCREEN_SCALE_FACTOR"] {
                env.remove.push(k.to_string());
            }
            if let Some((name, dir)) = kvantum {
                // Тему Kvantum берёт из `$XDG_CONFIG_HOME/Kvantum/
                // kvantum.kvconfig` (`[General] theme=`), а саму тему сперва
                // ищет в `$XDG_CONFIG_HOME/Kvantum/<имя>/`: свой каталог
                // настроек и ссылка на каталог темы дают ровно эту тему,
                // где бы она ни лежала, а настройки человека не трогаются.
                let cfg = work.join("config");
                env.files.push((cfg.join("Kvantum/kvantum.kvconfig"), format!("[General]\ntheme={name}\n")));
                env.links.push((cfg.join("Kvantum").join(name), PathBuf::from(dir)));
                set(&mut env, "XDG_CONFIG_HOME", &cfg.to_string_lossy());
            }
        }
        ShotKind::Gtk { theme, dir, dark } => {
            // GTK ищет тему по имени сперва в `$XDG_DATA_HOME/themes`: ссылка
            // там даёт ровно этот каталог, даже если одноимённая тема лежит и
            // в другом месте.
            let data = work.join("data");
            env.links.push((data.join("themes").join(theme), PathBuf::from(dir)));
            set(&mut env, "XDG_DATA_HOME", &data.to_string_lossy());
            let value = if *dark { format!("{theme}:dark") } else { theme.clone() };
            set(&mut env, "GTK_THEME", &value);
            set(&mut env, "GDK_SCALE", "1");
            set(&mut env, "GDK_DPI_SCALE", "1");
            // Без доступности и сторонних модулей: снимку они не нужны, а
            // модуль меню приложений тянет шину и замедляет запуск.
            set(&mut env, "NO_AT_BRIDGE", "1");
            set(&mut env, "GTK_A11Y", "none");
            env.remove.push("GTK_MODULES".into());
        }
    }
    env
}

/// Снять форму в `out` (PNG [`SHOT_WIDTH`]×[`SHOT_HEIGHT`]). `work_root` — каталог, где
/// заводится временный каталог снимка (рядом с кешем картинок). Процесс, не
/// уложившийся в [`SHOT_TIMEOUT_SECS`], убивается. `false` — снимка нет
/// (стиль или тема не загрузились, помощник упал, кадр не того размера);
/// недоделанный файл стирается.
pub fn take_shot(helper: &str, work_root: &str, req: &ShotRequest, out: &str) -> bool {
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let work = Path::new(work_root).join(format!(
        "wshot-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&work);
    if std::fs::create_dir_all(&work).is_err() {
        return false;
    }
    let env = helper_env(req, &work);
    let prepared = env.files.iter().all(|(p, text)| {
        p.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(p, text).is_ok()
    }) && env.links.iter().all(|(link, target)| {
        link.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok())
            && std::os::unix::fs::symlink(target, link).is_ok()
    });
    let _ = std::fs::remove_file(out);
    let ok = prepared && {
        let argv = helper_argv(helper, req, &sample_texts(), out);
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        for (k, v) in &env.set {
            cmd.env(k, v);
        }
        for k in &env.remove {
            cmd.env_remove(k);
        }
        match cmd.spawn() {
            Err(_) => false,
            Ok(mut child) => {
                let deadline = Instant::now() + Duration::from_secs(SHOT_TIMEOUT_SECS);
                loop {
                    match child.try_wait() {
                        Ok(Some(status)) => break status.success(),
                        Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
                        _ => {
                            let _ = child.kill();
                            let _ = child.wait();
                            break false;
                        }
                    }
                }
            }
        }
    };
    // Ссылки — только ссылки: remove_dir_all их не разыменовывает.
    let _ = std::fs::remove_dir_all(&work);
    let want = (SHOT_WIDTH as f64, SHOT_HEIGHT as f64);
    let good = ok && crate::local_preview::image_size(Path::new(out)) == Some(want);
    if !good {
        let _ = std::fs::remove_file(out);
    }
    good
}

// ── разбор аргументов помощника ──────────────────────────────────────────

/// Разобранная строка помощника.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperArgs {
    /// [`MODE_QT`] или [`MODE_GTK`].
    pub mode: String,
    /// Ключ стиля Qt (пусто у GTK).
    pub style: String,
    pub out: String,
    /// Размер готового кадра.
    pub size: (i32, i32),
    /// Размер формы в логических точках; рисуется она в [`render_scale`]
    /// раз крупнее.
    pub form: (i32, i32),
    pub palette: Vec<(String, String)>,
    /// Подписи по порядку [`sample_texts`]; недостающие — английские.
    pub texts: Vec<String>,
}

fn parse_size(v: &str) -> Option<(i32, i32)> {
    let (w, h) = v.split_once('x')?;
    let (w, h) = (w.parse::<i32>().ok()?, h.parse::<i32>().ok()?);
    ((16..=4096).contains(&w) && (16..=4096).contains(&h)).then_some((w, h))
}

/// Разбор аргументов помощника (без имени программы). `Err` — текст для
/// журнала.
pub fn parse_helper_args(args: &[String]) -> Result<HelperArgs, String> {
    let mut it = args.iter();
    let mode = it.next().cloned().unwrap_or_default();
    if mode != MODE_QT && mode != MODE_GTK {
        return Err(format!("режим «{mode}» неизвестен: нужен {MODE_QT} или {MODE_GTK}"));
    }
    let mut out = HelperArgs {
        mode,
        style: String::new(),
        out: String::new(),
        size: (SHOT_WIDTH, SHOT_HEIGHT),
        form: (FORM_WIDTH as i32, FORM_HEIGHT as i32),
        palette: Vec::new(),
        texts: Vec::new(),
    };
    while let Some(key) = it.next() {
        let value = it.next().ok_or_else(|| format!("у «{key}» нет значения"))?;
        match key.as_str() {
            "--style" => out.style = value.clone(),
            "--out" => out.out = value.clone(),
            "--size" => out.size = parse_size(value).ok_or_else(|| format!("размер «{value}»"))?,
            "--form" => out.form = parse_size(value).ok_or_else(|| format!("размер формы «{value}»"))?,
            "--color" => {
                let (role, color) = value.split_once('=').ok_or_else(|| format!("цвет «{value}»"))?;
                out.palette.push((role.to_string(), color.to_string()));
            }
            "--text" => out.texts.push(value.clone()),
            _ => return Err(format!("ключ «{key}» неизвестен")),
        }
    }
    if out.out.is_empty() {
        return Err("не сказано, куда сохранить (--out)".into());
    }
    if out.mode == MODE_QT && out.style.is_empty() {
        return Err("не назван стиль (--style)".into());
    }
    const ENGLISH: [&str; 8] = ["Preview", "Settings", "Apply", "Cancel", "Check box", "Radio button", "Combo box", "Text"];
    for t in ENGLISH.iter().skip(out.texts.len()) {
        out.texts.push(t.to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rexon-wshot-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const BREEZE_DARK: &str = "[Colors:Window]\nBackgroundNormal=32,35,38\nForegroundNormal=252,252,252\n\
        [Colors:View]\nBackgroundNormal=20,22,24\nBackgroundAlternate=29,31,34\nForegroundNormal=252,252,252\n\
        ForegroundLink=29,153,243\n[Colors:Button]\nBackgroundNormal=41,44,48\nForegroundNormal=252,252,252\n\
        [Colors:Selection]\nBackgroundNormal=61,174,233\nForegroundNormal=255,255,255\n";

    /// Палитра из схемы KDE: роли по группам, недоступный текст — середина
    /// между текстом и фоном; схемы нет — родная палитра стиля (пусто).
    #[test]
    fn palette_follows_kde_scheme() {
        let p = palette_from_kdeglobals(BREEZE_DARK);
        let get = |r: &str| p.iter().find(|(k, _)| k == r).map(|(_, v)| v.as_str());
        assert_eq!(get("Window"), Some("#202326"));
        assert_eq!(get("Base"), Some("#141618"));
        assert_eq!(get("Highlight"), Some("#3daee9"));
        assert_eq!(get("Link"), Some("#1d99f3"));
        assert_eq!(get("ToolTipBase"), None, "в схеме нет — роль не задаётся");
        // (252+32)/2 = 142 = 0x8e, (252+35)/2 = 143, (252+38)/2 = 145.
        assert_eq!(get("Disabled:WindowText"), Some("#8e8f91"));
        assert!(palette_from_kdeglobals("[General]\nwidgetStyle=Fusion\n").is_empty());
        assert!(palette_from_kdeglobals("[Colors:Window]\nBackgroundNormal=1,2,3\n").is_empty(), "без текста окна схемы нет");
        assert!(palette_from_kdeglobals("[Colors:Window]\nBackgroundNormal=1,2,300\nForegroundNormal=0,0,0\n").is_empty());
    }

    /// Что снимается: стиль Qt, тема Kvantum, тема GTK 3 при дисплее; тема
    /// GTK только второй версии и прочие виды — нет.
    #[test]
    fn kinds_that_are_shot() {
        let root = tmp("kinds");
        let gtk3 = root.join("Nord");
        std::fs::create_dir_all(gtk3.join("gtk-3.0")).unwrap();
        std::fs::write(gtk3.join("gtk-3.0/gtk.css"), "").unwrap();
        let gtk2 = root.join("Old");
        std::fs::create_dir_all(gtk2.join("gtk-2.0")).unwrap();
        let new_dir = root.join("Neo");
        std::fs::create_dir_all(new_dir.join("gtk-3.20")).unwrap();
        std::fs::write(new_dir.join("gtk-3.20/gtk.css"), "").unwrap();
        std::fs::write(new_dir.join("gtk-3.20/gtk-dark.css"), "").unwrap();
        let s = |p: &Path| p.to_string_lossy().into_owned();

        assert_eq!(
            shot_kind("local:widgetstyle:Breeze", "", false, false),
            Some(ShotKind::Qt { style: "Breeze".into(), kvantum: None })
        );
        assert_eq!(
            shot_kind("local:kvantum:KvArc", "/usr/share/Kvantum/KvArc", false, false),
            Some(ShotKind::Qt { style: "kvantum".into(), kvantum: Some(("KvArc".into(), "/usr/share/Kvantum/KvArc".into())) })
        );
        assert_eq!(
            shot_kind("local:gtk:Nord", &s(&gtk3), true, true),
            Some(ShotKind::Gtk { theme: "Nord".into(), dir: s(&gtk3), dark: false }),
            "тёмного файла нет — тёмный вариант не просится"
        );
        assert_eq!(
            shot_kind("local:gtk:Neo", &s(&new_dir), true, true),
            Some(ShotKind::Gtk { theme: "Neo".into(), dir: s(&new_dir), dark: true })
        );
        assert_eq!(shot_kind("local:gtk:Nord", &s(&gtk3), false, false), None, "без дисплея GTK не снимается");
        assert_eq!(shot_kind("local:gtk:Old", &s(&gtk2), true, false), None);
        assert_eq!(shot_kind("local:colors:BreezeDark", "/x", true, false), None);
        assert_eq!(shot_kind("local:widgetstyle:", "", true, false), None);
        assert_eq!(shot_kind("store:123", "", true, false), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dark_preference_from_settings_ini() {
        assert!(prefers_dark("[Settings]\ngtk-application-prefer-dark-theme=1\n"));
        assert!(prefers_dark("[Settings]\ngtk-application-prefer-dark-theme = true\n"));
        assert!(!prefers_dark("[Settings]\ngtk-application-prefer-dark-theme=0\n"));
        assert!(!prefers_dark("[Settings]\ngtk-theme-name=Adwaita\n"));
    }

    /// Помощник: рядом с бинарём окна, затем путь пакета, затем PATH.
    #[test]
    fn helper_lookup_order() {
        let all = |_: &Path| true;
        assert_eq!(
            find_helper(Some(Path::new("/tree/rust/target/release")), &all, "/usr/bin").as_deref(),
            Some("/tree/rust/target/release/rexon-theme-shot")
        );
        let pkg = |p: &Path| p == Path::new(PACKAGE_HELPER);
        assert_eq!(find_helper(Some(Path::new("/x")), &pkg, "/usr/bin").as_deref(), Some(PACKAGE_HELPER));
        let path = |p: &Path| p == Path::new("/opt/r/bin/rexon-theme-shot");
        assert_eq!(find_helper(None, &path, "relative:/opt/r/bin").as_deref(), Some("/opt/r/bin/rexon-theme-shot"));
        assert_eq!(find_helper(None, &|_: &Path| false, "/usr/bin"), None);
    }

    /// Строка запуска и её разбор помощником сходятся: что ядро написало, то
    /// помощник и прочтёт.
    #[test]
    fn argv_round_trip() {
        let req = ShotRequest {
            kind: ShotKind::Qt { style: "Fusion".into(), kvantum: None },
            palette: vec![("Window".into(), "#202326".into()), ("Disabled:Text".into(), "#808080".into())],
        };
        let texts: Vec<String> = ["A", "B"].iter().map(|s| s.to_string()).collect();
        let argv = helper_argv("/h/rexon-theme-shot", &req, &texts, "/c/x.png");
        assert_eq!(argv[0], "/h/rexon-theme-shot");
        let got = parse_helper_args(&argv[1..]).unwrap();
        assert_eq!(got.mode, MODE_QT);
        assert_eq!(got.style, "Fusion");
        assert_eq!(got.out, "/c/x.png");
        assert_eq!(got.size, (SHOT_WIDTH, SHOT_HEIGHT));
        assert_eq!(got.size, (600, 340), "кадр — размер картинки в подробностях");
        assert_eq!(got.form, (FORM_WIDTH as i32, FORM_HEIGHT as i32));
        assert_eq!(got.palette, req.palette);
        assert_eq!(got.texts.len(), 8, "недостающие подписи — английские");
        assert_eq!(&got.texts[..3], ["A", "B", "Apply"]);

        let gtk = ShotRequest { kind: ShotKind::Gtk { theme: "T".into(), dir: "/t".into(), dark: false }, palette: vec![] };
        let got = parse_helper_args(&helper_argv("h", &gtk, &[], "/o.png")[1..]).unwrap();
        assert_eq!(got.mode, MODE_GTK);
        assert!(got.style.is_empty());

        assert!(parse_helper_args(&["svg".to_string()]).is_err());
        assert!(parse_helper_args(&["qt".into(), "--out".into(), "/o".into()]).is_err(), "без стиля");
        assert!(parse_helper_args(&["gtk".into(), "--out".into()]).is_err(), "ключ без значения");
        assert!(parse_helper_args(&["gtk".into(), "--out".into(), "/o".into(), "--size".into(), "9x9".into()]).is_err());
    }

    /// Форма рисуется крупнее ровно настолько, чтобы кадр из неё только
    /// сжимался.
    #[test]
    fn render_scale_never_upscales() {
        assert_eq!(render_scale((FORM_WIDTH as i32, FORM_HEIGHT as i32), (SHOT_WIDTH, SHOT_HEIGHT)), 2);
        assert_eq!(render_scale((450, 255), (300, 170)), 1);
        assert_eq!(render_scale((100, 100), (1000, 1000)), 4, "не больше четырёх");
        assert_eq!(render_scale((0, 10), (300, 170)), 1);
    }

    /// Окружение: Qt — без экрана и без платформенной темы, у Kvantum — свой
    /// каталог настроек с выбранной темой и ссылкой на её каталог; GTK — тема
    /// по имени через свой каталог тем.
    #[test]
    fn helper_environment() {
        let work = Path::new("/c/wshot-1");
        let qt = helper_env(&ShotRequest { kind: ShotKind::Qt { style: "Breeze".into(), kvantum: None }, palette: vec![] }, work);
        assert!(qt.set.contains(&("QT_QPA_PLATFORM".into(), "offscreen".into())));
        assert!(qt.remove.contains(&"QT_QPA_PLATFORMTHEME".to_string()));
        assert!(qt.links.is_empty() && qt.files.is_empty());

        let kv = helper_env(
            &ShotRequest {
                kind: ShotKind::Qt { style: "kvantum".into(), kvantum: Some(("KvArc".into(), "/home/u/.config/Kvantum/KvArc".into())) },
                palette: vec![],
            },
            work,
        );
        assert!(kv.set.contains(&("XDG_CONFIG_HOME".into(), "/c/wshot-1/config".into())));
        assert_eq!(kv.files, vec![(PathBuf::from("/c/wshot-1/config/Kvantum/kvantum.kvconfig"), "[General]\ntheme=KvArc\n".into())]);
        assert_eq!(
            kv.links,
            vec![(PathBuf::from("/c/wshot-1/config/Kvantum/KvArc"), PathBuf::from("/home/u/.config/Kvantum/KvArc"))]
        );

        let gtk = helper_env(
            &ShotRequest { kind: ShotKind::Gtk { theme: "Nord".into(), dir: "/usr/share/themes/Nord".into(), dark: true }, palette: vec![] },
            work,
        );
        assert!(gtk.set.contains(&("GTK_THEME".into(), "Nord:dark".into())));
        assert!(gtk.set.contains(&("XDG_DATA_HOME".into(), "/c/wshot-1/data".into())));
        assert_eq!(gtk.links, vec![(PathBuf::from("/c/wshot-1/data/themes/Nord"), PathBuf::from("/usr/share/themes/Nord"))]);
        assert!(!gtk.set.iter().any(|(k, _)| k == "QT_QPA_PLATFORM"));
    }

    /// Ключ кеша: пусто у плиток не снимком; без помощника — свой ключ (чтобы
    /// после установки помощника кадр пересняли); меняется вместе с палитрой.
    #[test]
    fn cache_tag_tracks_inputs() {
        let mut setup = ShotSetup {
            helper: Some("/h".into()),
            palette: vec![],
            colors_stamp: 5,
            gtk_display: true,
            gtk_dark: false,
        };
        assert_eq!(setup.cache_tag("local:colors:X", "/x"), "");
        let a = setup.cache_tag("local:widgetstyle:Fusion", "");
        setup.colors_stamp = 6;
        assert_ne!(a, setup.cache_tag("local:widgetstyle:Fusion", ""));
        assert!(setup.request("local:widgetstyle:Fusion", "").is_some());
        setup.helper = None;
        assert_eq!(setup.cache_tag("local:widgetstyle:Fusion", ""), "noshot");
        assert!(setup.request("local:widgetstyle:Fusion", "").is_none());
    }

    /// Помощника нет — снимка нет, файл не остаётся, временный каталог убран.
    #[test]
    fn missing_helper_leaves_nothing() {
        let root = tmp("take");
        let out = root.join("x.png");
        std::fs::write(&out, b"old").unwrap();
        let req = ShotRequest { kind: ShotKind::Qt { style: "Fusion".into(), kvantum: None }, palette: vec![] };
        assert!(!take_shot("/нет/такого/помощника", &root.to_string_lossy(), &req, &out.to_string_lossy()));
        assert!(!out.exists());
        let left: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().collect();
        assert!(left.is_empty(), "временный каталог убран: {left:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
