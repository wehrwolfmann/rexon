//! «Запускать при входе» у виджета Conky: задержка, окружение, композиторы.
//!
//! ЗАЧЕМ. Виджет, запущенный в первую же секунду входа, встаёт под панель
//! или рисуется без фона: панель и обои ещё не поднялись (жалоба
//! пользователей программ-менеджеров conky, у них по умолчанию задержка
//! 20 с). Человеку с двумя рабочими столами нужен виджет только в одном
//! из них. А голые композиторы (sway, Hyprland без uwsm, river, labwc,
//! Wayfire, niri без `niri-session`) записей `~/.config/autostart` не
//! исполняют вовсе — галочка горела, а виджет при входе не появлялся.
//!
//! Как устроено:
//! * задержка — `sleep N` в самой строке запуска
//!   ([`crate::ext_manager::conky_desktop_entry_with`]): ключ
//!   `X-GNOME-Autostart-Delay` исполняют не все окружения;
//! * «только в этом окружении» — `OnlyShowIn=` с именами из
//!   `XDG_CURRENT_DESKTOP` (у композитора — его имена);
//! * композиторы — общий путь Rexon для своих программ
//!   ([`crate::compositor_autostart`]): строка запуска управляемым блоком в
//!   родном конфиге композитора и `NotShowIn=` в записи, чтобы виджет не
//!   стартовал дважды там, где композитор исполняет и то и другое. Юнит
//!   systemd с `PartOf=graphical-session.target` тут ничего не добавил бы:
//!   у голых sway и Hyprland эта цель не поднимается (её поднимают uwsm,
//!   `niri-session`, sway-systemd — а там XDG-записи и так исполняются).
//!   Строка в конфиг пишется по щелчку «Запускать при входе» и ТОЛЬКО в
//!   конфиг композитора текущего сеанса (решение Wehrwolfmann 23.10.2026):
//!   в конфигах других установленных композиторов новой строки не
//!   появляется, а в полных окружениях (KDE, MATE, Cinnamon…) — ни в одном.
//!   Строка, оставшаяся там от прежних включений, лишь подновляется вместе
//!   с записью (иначе звала бы снятую накладку). Выключение и удаление
//!   снимают строку отовсюду. Не легла — человеку сказано, что вставить
//!   руками и куда.
//!
//! Настройки записи хранятся в ней самой ([`options_of`]): переписывание
//! записи (переход на накладку, обновление старого вида) их не теряет.

use crate::compositor_autostart::{self as ca, Compositor, CurrentSession, SessionEnv};
use crate::de_apply::Ctx;
use crate::i18n::{tr, trf};

/// Ключ записи с задержкой в секундах.
pub const DELAY_KEY: &str = "X-Rexon-Conky-Delay";

/// Задержка нового автозапуска: панель и обои успевают подняться.
pub const DEFAULT_DELAY: u32 = 10;

/// Наибольшая задержка, которую можно выбрать.
pub const MAX_DELAY: u32 = 600;

/// Настройки записи автозапуска.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Options {
    /// Секунды до запуска.
    pub delay: u32,
    /// `OnlyShowIn=`; пусто — во всех окружениях.
    pub only_in: Vec<String>,
}

/// Настройки, записанные в записи `text`. Задержка — из [`DELAY_KEY`], а у
/// записи без ключа — из `sleep N &&` строки запуска.
pub fn options_of(text: &str) -> Options {
    let value = |k: &str| text.lines().find_map(|l| l.strip_prefix(k)).map(str::trim);
    let delay = value(&format!("{DELAY_KEY}="))
        .and_then(|v| v.parse().ok())
        .or_else(|| {
            let exec = value("Exec=")?;
            let at = exec.find("sleep ")? + 6;
            exec[at..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
        })
        .unwrap_or(0);
    let only_in = value("OnlyShowIn=")
        .map(|v| v.split(';').filter(|s| !s.is_empty()).map(str::to_string).collect())
        .unwrap_or_default();
    Options { delay: delay.min(MAX_DELAY), only_in }
}

/// Метка блока в конфигах композиторов по части имени файла автозапуска
/// (`rexon-conky-<ключ>.desktop`). Метка идёт в комментарий и в ключ
/// Wayfire — знаки вне `[A-Za-z0-9_-]` заменяются отпечатком.
pub fn compositor_id(key: &str) -> String {
    if !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return format!("conky-{key}");
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("conky-{h:016x}")
}

/// Строка запуска для конфига композитора. Сценарий записи (`$0`, `$1`)
/// туда не годится: sway и Hyprland разворачивают `$` сами. Здесь пути
/// вписаны в сценарий в двойных кавычках; путь с `"`, `$`, `` ` `` или `\`
/// такой строки не даст — отказ композитора покажет, что вставить руками.
pub fn compositor_argv(conf: &str, launch: Option<&str>, delay: u32) -> Option<Vec<String>> {
    let bad = |s: &str| s.contains(['"', '$', '`', '\\', '\n']);
    let dir = crate::de_apply::conky_work_dir(conf);
    let file = launch.unwrap_or(conf);
    if bad(dir) || bad(file) {
        return None;
    }
    let sleep = if delay > 0 { format!("sleep {delay} && ") } else { String::new() };
    Some(vec!["sh".into(), "-c".into(), format!("{sleep}cd \"{dir}\" && exec conky -c \"{file}\"")])
}

/// Имена окружения для «только здесь»: `XDG_CURRENT_DESKTOP`, у
/// композитора без него — его имена. Пусто — сеанс не распознан.
pub fn current_names(env: &SessionEnv) -> Vec<String> {
    if !env.current_desktop.is_empty() {
        return env.current_desktop.clone();
    }
    match ca::current_session(env) {
        CurrentSession::Compositor(c) => c.desktop_names().iter().map(|s| s.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// Окружение для правки конфигов композиторов. Только для домашнего
/// каталога этого процесса — у чужого дерева (сверки) композиторов нет, и
/// конфиги не трогаются.
pub fn session_env(ctx: &Ctx) -> SessionEnv {
    let own = std::env::var("HOME").ok().is_some_and(|h| h.trim_end_matches('/') == ctx.home.trim_end_matches('/'));
    if own {
        return SessionEnv::live();
    }
    SessionEnv { home: ctx.home.clone(), config_home: ctx.config(""), ..Default::default() }
}

/// Итог сборки записи.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Built {
    /// Окончательный текст записи.
    pub desktop: String,
    /// Почему в ТЕКУЩЕМ сеансе виджет при входе не запустится (композитор,
    /// в чей конфиг строка не легла); `None` — запустится.
    pub refusal: Option<String>,
    /// Конфиги композиторов, куда легла строка.
    pub configs: Vec<String>,
}

/// Собрать запись автозапуска `key` и поставить (или снять) строки
/// композиторов. Записывать на диск — [`Built::desktop`].
pub fn build(env: &SessionEnv, key: &str, title: &str, conf: &str, launch: Option<&str>, opts: &Options) -> Built {
    let entry = crate::ext_manager::conky_desktop_entry_with(title, conf, launch, opts.delay, &opts.only_in);
    let id = compositor_id(key);
    // Строка ставится только в конфиг композитора ТЕКУЩЕГО сеанса
    // ([`ca::Scope::Current`]); в конфигах прочих подновляется лишь уже
    // стоящая там строка прежних включений, чтобы не звала старое.
    // «Только здесь»: строке не место у композиторов вне списка.
    let wanted = |c: Compositor| opts.only_in.is_empty() || c.desktop_names().iter().any(|n| opts.only_in.iter().any(|o| o == n));
    for c in ca::compositors_with_block(env, &id) {
        if !wanted(c) {
            ca::remove_from(env, c, &id);
        }
    }
    let current = match ca::current_session(env) {
        CurrentSession::Compositor(c) => Some(c),
        _ => None,
    };
    let Some(argv) = compositor_argv(conf, launch, opts.delay) else {
        ca::remove_everywhere(env, &id);
        let refusal = current.filter(|&c| wanted(c)).map(|c| {
            trf(
                // TRANSLATORS: %1 is a window manager name (Sway, Hyprland…).
                "%1 doesn't start programs from the common startup list, and the path of this widget has characters \
                 its settings file can't hold.",
                &[&c.display_name()],
            )
        });
        return Built { desktop: entry, refusal, configs: Vec::new() };
    };
    // Текущему сеансу строка не нужна — новой не ставить.
    let scope = if current.is_some_and(|c| !wanted(c)) { ca::Scope::Present } else { ca::Scope::Current };
    let report = ca::sync_command_in(env, &id, Some(&argv), scope);
    let names = report.not_show_in();
    let configs = report.outcomes.iter().filter(|o| o.result.is_ok()).map(|o| o.path.clone()).collect();
    Built { desktop: ca::with_not_show_in(&entry, &names), refusal: report.current_refusal(env), configs }
}

/// Снять строки композиторов записи `key`.
pub fn remove(env: &SessionEnv, key: &str) {
    ca::remove_everywhere(env, &compositor_id(key));
}

/// Часть имени файла по пути записи (`…/rexon-conky-<ключ>.desktop`).
pub fn key_of_path(path: &str) -> Option<&str> {
    path.rsplit('/').next()?.strip_prefix("rexon-conky-")?.strip_suffix(".desktop")
}

/// Подписи настроек автозапуска для карточки.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Texts {
    pub delay: String,
    pub delay_tooltip: String,
    pub only_here: String,
    pub only_here_tooltip: String,
}

pub fn texts(names: &[String]) -> Texts {
    let here = names.first().cloned().unwrap_or_default();
    Texts {
        delay: tr("Wait before starting, seconds"),
        delay_tooltip: tr("Gives the panel and the wallpaper time to appear, so the widget doesn't end up under the panel."),
        // TRANSLATORS: %1 is a desktop name (KDE, MATE, sway…).
        only_here: trf("Only in %1", &[&here]),
        only_here_tooltip: tr("Don't start this widget when you log in to other desktops."),
    }
}

/// Что карточка показывает под переключателем «Запускать при входе».
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct View {
    pub on: bool,
    pub delay: u32,
    pub only_here: bool,
    /// Можно ли ограничить окружением (сеанс распознан).
    pub can_limit: bool,
    pub max_delay: u32,
    pub t: Texts,
}

pub fn view(text: Option<&str>, env: &SessionEnv) -> View {
    let names = current_names(env);
    let opts = text.map(options_of).unwrap_or(Options { delay: DEFAULT_DELAY, only_in: Vec::new() });
    View {
        on: text.is_some(),
        delay: opts.delay,
        only_here: !opts.only_in.is_empty(),
        can_limit: !names.is_empty(),
        max_delay: MAX_DELAY,
        t: texts(&names),
    }
}

/// Настройки автозапуска виджета `id` для карточки.
pub fn widget_view(io: &dyn crate::animations::LookIo, ctx: &Ctx, id: &str) -> View {
    let text = io.read(&crate::de_apply::conky_autostart_path(ctx, id));
    view(text.as_deref(), &session_env(ctx))
}

/// Поменять задержку и «только здесь» у включённого автозапуска.
/// `(получилось, сообщение)`.
pub fn set_widget(io: &dyn crate::animations::LookIo, ctx: &Ctx, id: &str, delay: u32, only_here: bool) -> (bool, String) {
    let env = session_env(ctx);
    let only_in = if only_here { current_names(&env) } else { Vec::new() };
    let opts = Options { delay: delay.min(MAX_DELAY), only_in };
    crate::de_apply::set_conky_autostart_options(io, ctx, id, &opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_survive_in_the_entry() {
        let e = crate::ext_manager::conky_desktop_entry_with("w", "/h/w/c.conf", None, 25, &["MATE".into()]);
        assert_eq!(options_of(&e), Options { delay: 25, only_in: vec!["MATE".into()] });
        let plain = crate::ext_manager::conky_desktop_entry("w", "/h/w/c.conf");
        assert_eq!(options_of(&plain), Options::default());
        // Запись без ключа — задержка из строки запуска.
        assert_eq!(options_of("Exec=sh -c \"sleep 7 && cd x\" a\n").delay, 7);
    }

    #[test]
    fn compositor_ids_are_safe() {
        assert_eq!(compositor_id("Jhalak"), "conky-Jhalak");
        let odd = compositor_id("Мой виджет");
        assert!(odd.starts_with("conky-") && odd[6..].chars().all(|c| c.is_ascii_hexdigit()), "{odd}");
        assert_eq!(key_of_path("/h/.config/autostart/rexon-conky-a b.desktop"), Some("a b"));
    }

    #[test]
    fn compositor_line_has_no_dollar() {
        let a = compositor_argv("/h/w x/c.conf", Some("/s/o.rexon.conf"), 10).unwrap();
        assert_eq!(a[2], "sleep 10 && cd \"/h/w x\" && exec conky -c \"/s/o.rexon.conf\"");
        assert!(compositor_argv("/h/$w/c.conf", None, 0).is_none());
    }
}
