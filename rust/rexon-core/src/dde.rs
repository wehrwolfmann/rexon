//! Deepin (DDE): что Rexon умеет в сеансе Treeland и чем отказывает вне его.
//! Здесь живёт ВСЁ знание Rexon о том, куда Deepin кладёт настройки; модули
//! тем, звуков, шрифтов и обоев берут отсюда готовые argv и тексты.
//!
//! ⭐ Rexon поддерживает Deepin ТОЛЬКО на Treeland — Wayland-композиторе
//! Deepin (решение Wehrwolfmann 26.10.2026; ставит его
//! [`crate::deepin_treeland`]). Сеанс X11 с deepin-kwin и любой другой
//! сеанс Deepin не на Treeland Rexon не настраивает: там одна честная фраза
//! «Rexon поддерживает Deepin только на Treeland» и путь перехода
//! ([`not_treeland_message`]). Сеанс Treeland опознаётся по композитору —
//! процессу на том конце сокета Wayland ([`treeland_session_live`]), а не по
//! `XDG_SESSION_TYPE=wayland`.
//!
//! Сверено с исходниками linuxdeepin и пакетами Arch (02.10.2026):
//!
//! * DConfig — ключи, которые читает сам Deepin в любом сеансе (тема звуков,
//!   нынешние темы и шрифты, док dde-shell): пишет и читает штатная утилита
//!   `dde-dconfig` (deepin-app-services), демон берёт пользователя у
//!   вызывающего, root не нужен;
//! * обои — через Центр управления ([`control_center_wallpaper_argv`]): в
//!   Treeland личные настройки идут только через него.
//!
//! ⚠️ Безопасность (openSUSE исключил Deepin в 2025 году за обход проверки
//! D-Bus-служб и polkit): Rexon в Deepin ТОЛЬКО зовёт пользовательские
//! интерфейсы самого Deepin. Своих служб, правил polkit и системных файлов
//! здесь нет и быть не должно.
//!
//! ⚠️ `--auto-start=no` у вызовов служб на шине: они заведены
//! D-Bus-активацией, и без запрета вызов из ДРУГОГО окружения (где Deepin
//! просто установлен) поднял бы в чужом сеансе службу Deepin.

use crate::i18n::tr;

/// Звуки событий DDE v23/v25 (шина сеанса, dde-daemon `soundeffect1`).
pub const SOUND_EFFECT_DEST: &str = "org.deepin.dde.SoundEffect1";
pub const SOUND_EFFECT_PATH: &str = "/org/deepin/dde/SoundEffect1";

/// Ключ DConfig: приложение, ресурс (имя схемы в `/usr/share/dsg/configs`), ключ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DConfigKey {
    pub appid: &'static str,
    pub resource: &'static str,
    pub key: &'static str,
}

const fn appearance_key(key: &'static str) -> DConfigKey {
    DConfigKey { appid: "org.deepin.dde.appearance", resource: "org.deepin.dde.appearance", key }
}

/// Схема `org.deepin.dde.appearance` (пакет deepin-appearance): нынешние
/// темы и шрифты (только чтение) и тема звуков.
pub const GTK_THEME: DConfigKey = appearance_key("Gtk_Theme");
pub const ICON_THEME: DConfigKey = appearance_key("Icon_Theme");
pub const CURSOR_THEME: DConfigKey = appearance_key("Cursor_Theme");
pub const CURSOR_SIZE: DConfigKey = appearance_key("cursorSize");
pub const FONT_STANDARD: DConfigKey = appearance_key("Font_Standard");
pub const FONT_MONOSPACE: DConfigKey = appearance_key("Font_Monospace");
pub const SOUND_THEME: DConfigKey = appearance_key("Sound_Theme");
/// Схема `org.deepin.XSettings` (пакет deepin-services): её раздаёт
/// GTK-программам служба XSettings.
pub const XS_SOUND_THEME: DConfigKey =
    DConfigKey { appid: "org.deepin.dde.daemon", resource: "org.deepin.XSettings", key: "sound-theme-name" };
/// Звуки событий, общий выключатель (dde-daemon `soundeffect1`).
pub const EVENT_SOUNDS: DConfigKey = DConfigKey {
    appid: "org.deepin.dde.daemon",
    resource: "org.deepin.dde.daemon.soundeffect",
    key: "enabled",
};

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_string()).collect()
}

/// Общий выключатель звуков событий: свойство `Enabled` службы звуков
/// (её обработчик записи ещё и передаёт значение проигрывателю тем).
pub fn event_sounds_argv(enabled: bool) -> Vec<String> {
    s(&[
        "busctl", "--user", "--auto-start=no", "set-property", SOUND_EFFECT_DEST,
        SOUND_EFFECT_PATH, SOUND_EFFECT_DEST, "Enabled", "b",
        if enabled { "true" } else { "false" },
    ])
}

/// `dde-dconfig set` — запись ключа DConfig от имени вызывающего.
pub fn dconfig_set_argv(k: DConfigKey, value: &str) -> Vec<String> {
    s(&["dde-dconfig", "set", "-a", k.appid, "-r", k.resource, "-k", k.key, "-v", value])
}

/// `dde-dconfig get` — текущее значение. Строки утилита печатает в двойных
/// кавычках (`"deepin"`), числа и логические — как есть.
pub fn dconfig_get_argv(k: DConfigKey) -> Vec<String> {
    s(&["dde-dconfig", "get", "-a", k.appid, "-r", k.resource, "-k", k.key])
}

// ── DConfig по ключам, известным только во время работы (копия дока) ─────

/// Схема ресурса DConfig: `/usr/share/dsg/configs/<appid>/<resource>.json`.
pub fn dconfig_schema_path(appid: &str, resource: &str) -> String {
    format!("/usr/share/dsg/configs/{appid}/{resource}.json")
}

/// Годится ли имя ключа DConfig в команду (имена из чужого файла схемы).
pub fn dconfig_key_ok(key: &str) -> bool {
    !key.is_empty() && !key.starts_with('-') && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Ключи схемы, которые можно записать (`permissions: readwrite`), в
/// порядке схемы. Схема не читается — пусто.
pub fn dconfig_schema_writable_keys(schema: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(schema) else {
        return Vec::new();
    };
    let Some(contents) = v.get("contents").and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };
    contents
        .iter()
        .filter(|(k, meta)| {
            dconfig_key_ok(k) && meta.get("permissions").and_then(serde_json::Value::as_str) == Some("readwrite")
        })
        .map(|(k, _)| k.clone())
        .collect()
}

pub fn dconfig_get_key_argv(appid: &str, resource: &str, key: &str) -> Vec<String> {
    s(&["dde-dconfig", "get", "-a", appid, "-r", resource, "-k", key])
}

/// Стоит ли у ключа значение по умолчанию (ответ `true` / `false`).
pub fn dconfig_is_default_argv(appid: &str, resource: &str, key: &str) -> Vec<String> {
    s(&["dde-dconfig", "get", "-a", appid, "-r", resource, "-k", key, "-m", "isDefaultValue"])
}

pub fn dconfig_set_key_argv(appid: &str, resource: &str, key: &str, value: &str) -> Vec<String> {
    s(&["dde-dconfig", "set", "-a", appid, "-r", resource, "-k", key, "-v", value])
}

/// Вернуть ключу значение по умолчанию.
pub fn dconfig_reset_key_argv(appid: &str, resource: &str, key: &str) -> Vec<String> {
    s(&["dde-dconfig", "reset", "-a", appid, "-r", resource, "-k", key])
}

/// Ответ `dde-dconfig get` → то, что принимает `dde-dconfig set -v`.
/// Логические и дробные утилита печатает как есть, всё прочее — в двойных
/// кавычках, списки и словари — развёрнутым JSON внутри кавычек; `set`
/// разбирает JSON-список или словарь, остальное берёт строкой.
pub fn dconfig_cli_value(raw: &str) -> String {
    let t = raw.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        let inner = &t[1..t.len() - 1];
        let body = inner.trim();
        if body.starts_with('[') || body.starts_with('{') {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
                return v.to_string();
            }
        }
        return inner.to_string();
    }
    t.to_string()
}

/// Значение из ответа `dde-dconfig get` или `busctl get-property`: без
/// подписи типа (`i `, `s `, `d `, `b `) и кавычек.
pub fn parse_value(raw: &str) -> String {
    let v = raw.trim();
    let v = match v.split_once(' ') {
        Some((sig, rest)) if matches!(sig, "s" | "i" | "u" | "d" | "b" | "y" | "n" | "q" | "x" | "t") => rest,
        _ => v,
    };
    v.trim().trim_matches('"').trim().to_string()
}

// ============================================================================
// Сеанс Treeland и отказ вне его
// ============================================================================
//
// В сеансе Treeland окна, курсор и обои рисует композитор Treeland, и своё
// оформление он берёт НЕ у службы оформления: служба там пропускает
// глобальную тему («в Treeland все личные настройки идут через протокол
// Центра управления»), обои `Set("background")` отдаёт в никуда, а темы
// значков, курсора, шрифты, углы и прозрачность Treeland получает от Центра
// управления по своему протоколу Wayland (`treeland_personalization_manager_v1`,
// в новых выпусках — `treeland_appearance_manager_v1`, объявлен
// привилегированным: «только для Центра управления»). Значения Treeland
// хранит в DConfig `org.deepin.dde.treeland` / `org.deepin.dde.treeland.user`
// своего процесса, а он в сеансе DDM запущен от служебного пользователя
// `dde`, — записать туда от имени человека нельзя. Поэтому здесь: обои —
// через сам Центр управления (`ShowPage("personalization/wallpaper?url=…")`),
// остальное — честный отказ ([`treeland_message`]).

/// Имя процесса композитора Treeland (`/proc/<pid>/comm`).
pub const TREELAND_PROCESS: &str = "treeland";

/// Центр управления DDE (шина сеанса).
pub const CONTROL_CENTER_DEST: &str = "org.deepin.dde.ControlCenter1";
pub const CONTROL_CENTER_PATH: &str = "/org/deepin/dde/ControlCenter1";

/// Сокет Wayland сеанса: `WAYLAND_DISPLAY` — путь или имя в
/// `XDG_RUNTIME_DIR`. `None` — сеанс не Wayland.
pub fn wayland_socket_path(wayland_display: &str, runtime_dir: &str) -> Option<String> {
    let name = wayland_display.trim();
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') {
        return Some(name.to_string());
    }
    (!runtime_dir.is_empty()).then(|| format!("{}/{name}", runtime_dir.trim_end_matches('/')))
}

/// Композитор — Treeland: так зовётся процесс на том конце сокета Wayland.
pub fn is_treeland_process(comm: Option<&str>) -> bool {
    comm.map(str::trim) == Some(TREELAND_PROCESS)
}

/// Имя процесса на том конце сокета Wayland сеанса (`SO_PEERCRED` → pid →
/// `/proc/<pid>/comm`; `comm` читается и у чужого пользователя — в сеансе
/// DDM Treeland запущен от `dde`). `None` — сокета нет или ядро не ответило.
pub fn wayland_compositor_live() -> Option<String> {
    use std::os::fd::AsRawFd;
    let env = |n: &str| std::env::var(n).unwrap_or_default();
    let path = wayland_socket_path(&env("WAYLAND_DISPLAY"), &env("XDG_RUNTIME_DIR"))?;
    let stream = std::os::unix::net::UnixStream::connect(path).ok()?;
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: буфер — живая `ucred` нужного размера, длина передана верно.
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if rc != 0 || cred.pid <= 0 {
        return None;
    }
    std::fs::read_to_string(format!("/proc/{}/comm", cred.pid)).ok().map(|c| c.trim().to_string())
}

/// Идёт ли сейчас сеанс Treeland — по композитору, а не по типу сеанса.
pub fn treeland_session_live() -> bool {
    is_treeland_process(wayland_compositor_live().as_deref())
}

/// Отказ «в Treeland это задаёт только Центр управления».
pub fn treeland_message() -> String {
    tr("In the Deepin Wayland session (Treeland) this is set only by Deepin's Control Center — Rexon can't change it there yet: use Control Center → Personalization.")
}

/// Отказ в любом сеансе Deepin не на Treeland (X11 с deepin-kwin и прочие):
/// одна фраза и путь перехода.
pub fn not_treeland_message() -> String {
    tr("Rexon supports Deepin only on Treeland. To switch, open Rexon → Desktops → Deepin → “Switch to Deepin on Treeland”, then log in with “Treeland User”.")
}

/// Почему Rexon этого в Deepin не меняет: в Treeland — его задаёт только
/// Центр управления, вне Treeland — [`not_treeland_message`].
pub fn refusal(treeland: bool) -> String {
    if treeland {
        treeland_message()
    } else {
        not_treeland_message()
    }
}

/// [`refusal`] для сеанса, который идёт сейчас.
pub fn refusal_live() -> String {
    refusal(treeland_session_live())
}

/// Обои через Центр управления: команда страницы «Обои» с адресом
/// картинки. С командой Центр управления окна не открывает — ставит обои
/// своим путём Treeland (на экран, выбранный у него сейчас) и отвечает,
/// найдя страницу. `lock` — экран блокировки вместо рабочего стола.
///
/// ⚠️ Без `--auto-start=no`: Центр управления заводится D-Bus-активацией по
/// первому вызову, а зовётся это только в сеансе Treeland. Адрес — с
/// percent-кодированием: команду Центр управления режет по `&` и `=`.
pub fn control_center_wallpaper_argv(path: &str, lock: bool) -> Vec<String> {
    let url = crate::appearance_installer::path_as_uri(path);
    let kind = if lock { "lock" } else { "desktop" };
    let page = format!("personalization/wallpaper?url={url}&type={kind}");
    s(&[
        "busctl", "--user", "--timeout=60", "call", CONTROL_CENTER_DEST, CONTROL_CENTER_PATH,
        CONTROL_CENTER_DEST, "ShowPage", "s", &page,
    ])
}

/// Итог передачи обоев Центру управления: прочитать их обратно у Treeland
/// нельзя, поэтому — что сделано, без «✅ поставлены».
pub fn wallpaper_handed_message() -> String {
    tr("Wallpaper handed to Deepin's Control Center — it sets it on the screen selected there.")
}

/// Центр управления не принял обои.
pub fn control_center_failed_message() -> String {
    tr("Deepin's Control Center didn't take the wallpaper — set it in Control Center → Personalization → Wallpaper.")
}

// ============================================================================
// Раздел «Окна Deepin» вкладки «Настройки»
// ============================================================================

/// Снимок раздела «Окна Deepin»: углы, прозрачность и акцент окон Treeland
/// берёт только от Центра управления, поэтому раздел — объяснение, где их
/// менять (в Treeland) или что Rexon поддерживает Deepin только на Treeland.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LookState {
    /// Раздел показывается только в сеансе Deepin.
    pub available: bool,
    pub note: String,
}

/// Снимок раздела. `de` — окружение, как его зовёт ядро; `treeland` — идёт
/// ли сеанс Treeland ([`treeland_session_live`]).
pub fn read_state(de: &str, treeland: bool) -> LookState {
    LookState { available: de == "deepin", note: refusal(treeland) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bus_call_refuses_autostart() {
        let argv = event_sounds_argv(true);
        assert_eq!(argv[0], "busctl");
        assert_eq!(argv[1], "--user", "только шина сеанса: {argv:?}");
        assert_eq!(argv[2], "--auto-start=no", "{argv:?}");
    }

    #[test]
    fn values_parse_from_both_tools() {
        assert_eq!(parse_value("\"deepin\"\n"), "deepin");
        assert_eq!(parse_value("s \"Noto Sans\""), "Noto Sans");
        assert_eq!(parse_value("i 12\n"), "12");
    }

    /// Ответ `dde-dconfig get` → значение для `set -v`: кавычки снимаются,
    /// развёрнутый JSON списков и словарей сжимается, логические — как есть.
    #[test]
    fn dconfig_cli_values_round_trip() {
        assert_eq!(dconfig_cli_value("\"bottom\"\n"), "bottom");
        assert_eq!(dconfig_cli_value("\"48\"\n"), "48");
        assert_eq!(dconfig_cli_value("true\n"), "true");
        assert_eq!(dconfig_cli_value("\"[\n    \"a\",\n    \"b c\"\n]\n\"\n"), "[\"a\",\"b c\"]");
        assert_eq!(dconfig_cli_value("\"{\n}\n\""), "{}");
        assert_eq!(dconfig_cli_value("\"\""), "");
        let schema = r#"{"contents": {"b": {"permissions": "readwrite"}, "a": {"permissions": "readonly"}, "-x": {"permissions": "readwrite"}, "c": {"permissions": "readwrite"}}}"#;
        assert_eq!(dconfig_schema_writable_keys(schema), ["b", "c"]);
        assert!(dconfig_schema_writable_keys("not json").is_empty());
    }

    /// Сеанс Treeland — по процессу композитора на том конце сокета
    /// Wayland, а не по типу сеанса.
    #[test]
    fn treeland_is_recognised_by_its_compositor() {
        assert_eq!(wayland_socket_path("wayland-0", "/run/user/1000/").as_deref(), Some("/run/user/1000/wayland-0"));
        assert_eq!(wayland_socket_path("/tmp/w", "").as_deref(), Some("/tmp/w"));
        assert_eq!(wayland_socket_path("", "/run/user/1000"), None, "сеанс X11");
        assert_eq!(wayland_socket_path("wayland-0", ""), None);
        assert!(is_treeland_process(Some("treeland\n")));
        for other in [Some("kwin_wayland"), Some("deepin-kwin_wayland"), Some("treeland-sd"), None] {
            assert!(!is_treeland_process(other), "{other:?}");
        }
    }

    /// Отказ: в Treeland — «только Центр управления», в любом другом сеансе
    /// Deepin — «Rexon поддерживает Deepin только на Treeland» с путём перехода.
    #[test]
    fn refusal_depends_on_the_session() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        assert_eq!(refusal(true), treeland_message());
        let other = refusal(false);
        assert!(other.starts_with("Rexon supports Deepin only on Treeland"), "{other}");
        assert!(other.contains("Switch to Deepin on Treeland"), "{other}");
    }

    /// Темы GTK, значков и курсора Deepin Rexon не пишет ни в одном сеансе:
    /// план — один стоп до записи (причина — [`refusal`] по сеансу), ключи
    /// GNOME не трогаются.
    #[test]
    fn deepin_theme_plans_stop_before_writing() {
        use crate::themes::{self, ApplyStep};
        let gtk = themes::apply_gtk_plan_xdg("Materia", "deepin", "/h", None);
        assert_eq!(gtk.steps, vec![ApplyStep::DdeTreelandGate]);
        let icons = themes::apply_icon_plan_xdg("Papirus", "deepin", "/h", None, true).unwrap();
        assert_eq!(icons.steps, vec![ApplyStep::DdeTreelandGate]);
        let cur = themes::apply_cursor_plan_sized("Bibata", Some(32), "deepin", "/h", None, true).unwrap();
        assert_eq!(cur.steps[0], ApplyStep::DdeTreelandGate);
        assert!(!format!("{:?}", cur.steps).contains("org.gnome.desktop.interface"));
        let q = themes::cursor_queries("deepin", "/h", None, false);
        assert!(matches!(&q[0], themes::CursorQuery::Cmd(c) if c.argv == dconfig_get_argv(CURSOR_THEME)));
    }

    /// Звуки Deepin — в его ключи DConfig (их читает сам Deepin в любом
    /// сеансе); шрифты — только чтение.
    #[test]
    fn deepin_sounds_use_native_keys_and_fonts_are_read_only() {
        use crate::qt_settings as qs;
        let plan = qs::sound_plan("deepin", qs::SoundSetting::Theme, "freedesktop", "", "", true);
        assert_eq!(plan.write[0], dconfig_set_argv(SOUND_THEME, "freedesktop"));
        assert!(plan.write.contains(&dconfig_set_argv(XS_SOUND_THEME, "freedesktop")));
        assert!(plan.write.iter().any(|a| a.contains(&"org.gnome.desktop.sound".to_string())));
        assert_eq!(plan.verify, Some(dconfig_get_argv(SOUND_THEME)));
        let ev = qs::sound_plan("deepin", qs::SoundSetting::EventSounds, "false", "", "", false);
        assert_eq!(ev.write[0], event_sounds_argv(false));
        assert_eq!(ev.verify, Some(dconfig_get_argv(EVENT_SOUNDS)));
        for target in ["system", "mono"] {
            let fp = qs::font_plan("deepin", target);
            assert_eq!(qs::font_store_write_argv(&fp.primary, "Noto Sans", 11, ""), None, "{target}");
        }
        assert!(!qs::font_target_available("deepin", "title"), "шрифта заголовков у Deepin нет");
    }

    /// Обои: в Treeland их принимает Центр управления командой своей
    /// страницы (сообщение честно говорит «переданы»); вне Treeland — отказ,
    /// без сторонних рисовальщиков.
    #[test]
    fn deepin_wallpaper_goes_to_control_center_only_in_treeland() {
        use crate::wallpaper_engine::{static_plan, TREELAND_SESSION};
        let steps = static_plan("deepin", TREELAND_SESSION, "/h/a&b=c.png", &[], "");
        assert_eq!(steps[0].argv, control_center_wallpaper_argv("/h/a&b=c.png", false));
        let page = steps[0].argv.last().unwrap();
        assert_eq!(page, "personalization/wallpaper?url=file:///h/a%26b%3Dc.png&type=desktop");
        assert!(!steps[0].argv.contains(&"--auto-start=no".to_string()), "Центр управления заводится по вызову");
        assert!(!steps.iter().any(|s| s.argv.first().map(String::as_str) == Some("swaybg")));
        for session in ["x11", "wayland", ""] {
            let other = static_plan("deepin", session, "/h/a.png", &["feh".into(), "swaybg".into()], "");
            assert!(other.iter().all(|s| s.argv.is_empty()), "{session}: {other:?}");
        }
    }

    /// Раздел «Окна Deepin»: только объяснение; вне Deepin скрыт.
    #[test]
    fn look_section_explains_instead_of_writing() {
        assert_eq!(read_state("deepin", true), LookState { available: true, note: treeland_message() });
        assert_eq!(read_state("deepin", false).note, not_treeland_message());
        assert!(!read_state("kde", true).available);
    }

    /// Опознание: v23/v25 (`DDE`), deepin 20 (`Deepin`), Wayland-сеанс
    /// Treeland и имена сеансов — всё это Deepin; во всех местах Rexon,
    /// которые перечисляют окружения, он есть.
    #[test]
    fn deepin_is_recognised_everywhere() {
        use crate::system::detect_current_de;
        for (xdg, session) in [("DDE", "deepin"), ("Deepin", ""), ("Treeland", "treeland"), ("", "deepin"), ("", "treeland-user")] {
            assert_eq!(detect_current_de(xdg, session, false).as_deref(), Some("deepin"), "{xdg:?} {session:?}");
        }
        assert_eq!(crate::de_customization::de_from_id("deepin"), Some(crate::de_customization::De::Deepin));
        assert!(crate::system::DE_META.iter().any(|m| m.id == "deepin"));
        assert!(crate::system::DE_DATABASE.iter().any(|(id, _)| *id == "deepin"));
        assert_eq!(crate::system::de_installed_binaries("deepin"), &["dde-session"]);
        assert_eq!(crate::appdefaults::resolve_mimeapps_filename("deepin"), "dde-mimeapps.list");
        assert!(!crate::themes::is_generic_wm("deepin"));
    }

    /// Терминал Deepin — его собственная схема (`app-id` без `.desktop`).
    #[test]
    fn deepin_terminal_goes_to_its_schema() {
        use crate::defaults as d;
        assert_eq!(d::terminal_home("DDE"), d::TerminalHome::Deepin);
        assert_eq!(d::terminal_exec_schema("DDE"), Some(d::DEEPIN_TERMINAL_SCHEMA));
        let steps = d::terminal_home_steps(d::TerminalHome::Deepin, "deepin-terminal.desktop", "/h/.config");
        let argvs: Vec<Vec<String>> = steps
            .into_iter()
            .filter_map(|s| match s {
                d::TermStep::Run { argv } => Some(argv),
                _ => None,
            })
            .collect();
        assert_eq!(argvs[0], s(&["gsettings", "set", d::DEEPIN_TERMINAL_SCHEMA, "app-id", "deepin-terminal"]));
        assert!(argvs.contains(&s(&["gsettings", "set", d::DEEPIN_TERMINAL_SCHEMA, "exec", "deepin-terminal"])));
        assert_eq!(
            d::get_current_terminal(None, "DDE", None, Some("'deepin-terminal'\n")).as_deref(),
            Some("deepin-terminal.desktop")
        );
    }
}
