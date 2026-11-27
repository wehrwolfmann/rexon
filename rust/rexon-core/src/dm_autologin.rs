//! Автовход и звук экрана входа при смене менеджера входа.
//!
//! ⭐ Дефект 15.11.2026: помощник `switch-dm` («Сделать основным») менял
//! экран входа, а автовход молча пропадал — у каждого менеджера входа своё
//! место этой настройки, и Rexon её не переносил. Здесь — чистые решения:
//! где какой экран входа держит автовход, что сейчас включено у прежнего,
//! что и куда записать у нового и что честно сказать человеку ДО
//! переключения, если перенести нельзя. Исполняет помощник
//! (`privilege::helper::switch_dm`) в том же плане и с тем же откатом.
//!
//! Где живёт автовход (по man-страницам и строкам самих программ на машине):
//! * SDDM — `[Autologin] User= Session= Relogin=`; файлы
//!   `/usr/lib/sddm/sddm.conf.d/*.conf`, `/etc/sddm.conf.d/*.conf`, последним
//!   `/etc/sddm.conf` (поздний перекрывает).
//! * Plasma Login Manager — те же ключи; `/usr/lib/plasmalogin/plasmalogin.conf.d`,
//!   `/etc/plasmalogin.conf.d`, последним `/etc/plasmalogin.conf` (его целиком
//!   переписывает модуль «Параметров системы» — свой файл Rexon кладёт в каталог).
//! * DDM 0.4 (Deepin) — `[Autologin] User= Session=` (без `Relogin`);
//!   `/usr/lib/ddm/ddm.conf.d`, `/etc/ddm.conf.d`, последним `/etc/ddm.conf`.
//! * GDM — `[daemon] AutomaticLoginEnable= AutomaticLogin=` в одном файле
//!   (`/etc/gdm/custom.conf`; у Debian `gdm3` — `/etc/gdm3/daemon.conf`, у
//!   Ubuntu `/etc/gdm3/custom.conf`). Стол для автовхода GDM не задаётся:
//!   он берёт последний выбранный у AccountsService.
//! * LightDM — `[Seat:*] autologin-user= autologin-session=` (старое имя
//!   группы `[SeatDefaults]`, частное `[Seat:seat0]` сильнее); последним
//!   читается `/etc/lightdm/lightdm.conf`. На Arch PAM `lightdm-autologin`
//!   пускает без пароля только членов группы `autologin`.
//! * greetd (и cosmic-greeter поверх него) — `[initial_session] user= command=`:
//!   команда запуска стола, а не имя сеанса. Читаем, но не пишем.
//!
//! Звук экрана входа: у приветствия свой пользователь и свой PipeWire. Без
//! сохранённого состояния WirePlumber выставляет железному микшеру громкость
//! по умолчанию (40 %), и после входа громкость человека падает. Обход —
//! маски пяти звуковых юнитов в `~/.config/systemd/user` пользователя
//! приветствия (ссылки на `/dev/null`). Если у прежнего экрана входа такие
//! маски есть, помощник ставит их и пользователю нового.

use crate::i18n::trf;
use crate::login_rescue::ini_value;
use crate::system_plan::{rewrite_ini_values, sddm_conf_read_order, SessionEdit, SessionKey};

/// Как экран входа хранит автовход.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// `[Autologin] User= Session=`, с `Relogin=` или без.
    Autologin { relogin: bool },
    /// `[daemon] AutomaticLoginEnable= AutomaticLogin=` (GDM).
    Gdm,
    /// `[Seat:*] autologin-user= autologin-session=` (LightDM).
    LightDm,
    /// `[initial_session] user= command=` (greetd) — только чтение.
    Greetd,
}

/// Место автовхода у одного экрана входа.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutologinSpec {
    /// Юнит без `.service` (как в `system_plan::LOGIN_MANAGER_SPECS`).
    pub unit: &'static str,
    pub layout: Layout,
    /// Каталоги `*.conf` по порядку чтения (внутри — по имени).
    pub dirs: &'static [&'static str],
    /// Файлы после каталогов. У `Autologin`/`LightDm` читаются все по
    /// порядку (поздний перекрывает); у `Gdm`/`Greetd` — первый
    /// существующий (программа читает один файл).
    pub files: &'static [&'static str],
    /// Свой файл Rexon — туда дописывается ключ, которого нет в чужих
    /// файлах `/etc`. Пусто — тот файл, что читает программа.
    pub own: &'static str,
    /// Служба PAM автовхода.
    pub pam: &'static str,
}

/// Экраны входа, у которых Rexon знает место автовхода.
pub const AUTOLOGIN_SPECS: &[AutologinSpec] = &[
    AutologinSpec {
        unit: "plasmalogin",
        layout: Layout::Autologin { relogin: true },
        dirs: &["/usr/lib/plasmalogin/plasmalogin.conf.d", "/etc/plasmalogin.conf.d"],
        files: &["/etc/plasmalogin.conf"],
        own: "/etc/plasmalogin.conf.d/zz-rexon-autologin.conf",
        pam: "plasmalogin-autologin",
    },
    AutologinSpec {
        unit: "sddm",
        layout: Layout::Autologin { relogin: true },
        dirs: &["/usr/lib/sddm/sddm.conf.d", "/etc/sddm.conf.d"],
        files: &["/etc/sddm.conf"],
        own: "/etc/sddm.conf.d/zz-rexon-autologin.conf",
        pam: "sddm-autologin",
    },
    AutologinSpec {
        unit: "ddm",
        layout: Layout::Autologin { relogin: false },
        dirs: &["/usr/lib/ddm/ddm.conf.d", "/etc/ddm.conf.d"],
        files: &["/etc/ddm.conf"],
        own: "/etc/ddm.conf.d/zz-rexon-autologin.conf",
        pam: "ddm-autologin",
    },
    AutologinSpec {
        unit: "gdm",
        layout: Layout::Gdm,
        dirs: &[],
        files: &["/etc/gdm/custom.conf"],
        own: "",
        pam: "gdm-autologin",
    },
    AutologinSpec {
        unit: "gdm3",
        layout: Layout::Gdm,
        dirs: &[],
        files: &["/etc/gdm3/daemon.conf", "/etc/gdm3/custom.conf"],
        own: "",
        pam: "gdm-autologin",
    },
    AutologinSpec {
        unit: "lightdm",
        layout: Layout::LightDm,
        dirs: &[
            "/usr/share/lightdm/lightdm.conf.d",
            "/etc/xdg/lightdm/lightdm.conf.d",
            "/etc/lightdm/lightdm.conf.d",
        ],
        files: &["/etc/lightdm/lightdm.conf"],
        // Главный файл читается последним — ключ в нём ничто не перекроет.
        own: "",
        pam: "lightdm-autologin",
    },
    AutologinSpec {
        unit: "greetd",
        layout: Layout::Greetd,
        dirs: &[],
        files: &[crate::system_plan::GREETD_DEFAULT_CONFIG],
        own: "",
        pam: "greetd",
    },
    AutologinSpec {
        unit: "cosmic-greeter",
        layout: Layout::Greetd,
        dirs: &[],
        files: &["/etc/greetd/cosmic-greeter.toml", crate::system_plan::GREETD_DEFAULT_CONFIG],
        own: "",
        pam: "greetd",
    },
];

/// Место автовхода экрана входа; `None` — Rexon его не знает.
pub fn autologin_spec(unit: &str) -> Option<&'static AutologinSpec> {
    let unit = unit.trim();
    AUTOLOGIN_SPECS.iter().find(|s| s.unit == unit)
}

/// Включённый автовход.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Autologin {
    pub user: String,
    /// Имя файла сеанса без `.desktop`; пусто — последний выбранный.
    pub session: String,
    /// Снова войти после выхода (`Relogin=true`, на нём держится игровой режим).
    pub relogin: bool,
}

/// Автовход экрана входа сейчас.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutologinState {
    Off,
    On(Autologin),
    /// Rexon не знает, где этот экран входа держит автовход.
    Unknown,
}

type Read<'a> = &'a dyn Fn(&str) -> Option<String>;
type List<'a> = &'a dyn Fn(&str) -> Vec<String>;

/// Группы настроек по возрастанию силы.
fn groups(layout: Layout) -> &'static [&'static str] {
    match layout {
        Layout::Autologin { .. } => &["Autologin"],
        Layout::Gdm => &["daemon"],
        Layout::LightDm => &["SeatDefaults", "Seat:*", "Seat:seat0"],
        Layout::Greetd => &["initial_session"],
    }
}

/// Группа, в которую Rexon дописывает ключ.
fn write_group(layout: Layout) -> &'static str {
    match layout {
        Layout::LightDm => "Seat:*",
        other => groups(other)[0],
    }
}

/// Файлы, которые экран входа читает, в его порядке.
pub fn config_files(spec: &AutologinSpec, read: Read<'_>, list: List<'_>) -> Vec<String> {
    match spec.layout {
        Layout::Gdm | Layout::Greetd => {
            let first = spec.files.iter().find(|f| read(f).is_some()).or(spec.files.first());
            first.map(|f| vec![f.to_string()]).unwrap_or_default()
        }
        _ => {
            let mut out = Vec::new();
            for dir in spec.dirs {
                out.extend(sddm_conf_read_order(&list(dir)).iter().map(|n| format!("{dir}/{n}")));
            }
            out.extend(spec.files.iter().map(|f| f.to_string()));
            out
        }
    }
}

/// Итоговое значение ключа у экрана входа (сильная группа перекрывает слабую).
fn effective(spec: &AutologinSpec, key: &str, texts: &[String]) -> Option<String> {
    groups(spec.layout).iter().filter_map(|g| ini_value(texts, g, key)).last()
}

fn truthy(v: Option<&str>) -> bool {
    matches!(v.map(|s| s.trim().to_ascii_lowercase()).as_deref(), Some("true" | "1" | "yes"))
}

/// Имя сеанса без каталога и `.desktop`.
pub fn session_name(value: &str) -> String {
    crate::update_guard::session_file_name(value)
}

/// Автовход, который экран входа выполнит при таких настройках.
pub fn read_autologin(unit: &str, read: Read<'_>, list: List<'_>) -> AutologinState {
    let Some(spec) = autologin_spec(unit) else {
        return AutologinState::Unknown;
    };
    let files = config_files(spec, read, list);
    let texts: Vec<String> = files.iter().filter_map(|p| read(p)).collect();
    let get = |k: &str| effective(spec, k, &texts).map(|v| v.trim().to_string()).unwrap_or_default();
    let (user, session, relogin) = match spec.layout {
        Layout::Autologin { relogin } => (get("User"), session_name(&get("Session")), relogin && truthy(Some(&get("Relogin")))),
        Layout::Gdm => {
            let user = if truthy(Some(&get("AutomaticLoginEnable"))) { get("AutomaticLogin") } else { String::new() };
            (user, String::new(), false)
        }
        Layout::LightDm => (get("autologin-user"), session_name(&get("autologin-session")), false),
        Layout::Greetd => {
            let text = texts.first().map(String::as_str).unwrap_or_default();
            let user = crate::system_plan::toml_string_value(text, "initial_session", "user").unwrap_or_default();
            (user.trim().to_string(), String::new(), false)
        }
    };
    if user.is_empty() {
        AutologinState::Off
    } else {
        AutologinState::On(Autologin { user, session, relogin })
    }
}

/// Значение флага: пусто и `false` — одно и то же; не флаг — `None`.
fn flag(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "" | "false" | "0" | "no" => Some(false),
        "true" | "1" | "yes" => Some(true),
        _ => None,
    }
}

/// Одинаковы ли значения для экрана входа: ключа нет = пусто, флаги без
/// учёта регистра (`True`/`true`), имена — строго.
fn same(a: Option<&str>, b: &str) -> bool {
    let a = a.unwrap_or("").trim();
    let b = b.trim();
    a == b || matches!((flag(a), flag(b)), (Some(x), Some(y)) if x == y)
}

/// Ключи и значения, которые надо получить у целевого экрана входа.
fn desired(layout: Layout, want: Option<&Autologin>) -> Vec<(&'static str, String)> {
    match (layout, want) {
        (Layout::Autologin { relogin }, Some(a)) => {
            let mut v = vec![
                ("User", a.user.clone()),
                ("Session", if a.session.is_empty() { String::new() } else { format!("{}.desktop", a.session) }),
            ];
            if relogin {
                v.push(("Relogin", (if a.relogin { "true" } else { "false" }).to_string()));
            }
            v
        }
        (Layout::Autologin { .. }, None) => vec![("User", String::new())],
        (Layout::Gdm, Some(a)) => vec![("AutomaticLoginEnable", "True".into()), ("AutomaticLogin", a.user.clone())],
        (Layout::Gdm, None) => vec![("AutomaticLoginEnable", "False".into())],
        (Layout::LightDm, Some(a)) => {
            vec![("autologin-user", a.user.clone()), ("autologin-session", a.session.clone())]
        }
        (Layout::LightDm, None) => vec![("autologin-user", String::new())],
        (Layout::Greetd, _) => Vec::new(),
    }
}

/// Правки файлов, после которых у экрана входа `spec` итоговые значения
/// ключей равны `want`. Сначала меняются уже существующие строки в файлах
/// `/etc` (прочие строки байт в байт), недостающие ключи дописываются в
/// свой файл. `None` — итог всё равно другой (значение задаёт файл пакета
/// поверх `/etc`) — тогда не трогать ничего.
fn plan_keys(spec: &AutologinSpec, want: &[(&'static str, String)], read: Read<'_>, list: List<'_>) -> Option<Vec<SessionEdit>> {
    let files = config_files(spec, read, list);
    let mut edits: Vec<SessionEdit> = Vec::new();
    let current = |edits: &[SessionEdit], p: &str| edits.iter().find(|e| e.path == p).map(|e| e.text.clone()).or_else(|| read(p));
    let put = |edits: &mut Vec<SessionEdit>, path: &str, text: String| match edits.iter_mut().find(|e| e.path == path) {
        Some(e) => e.text = text,
        None => edits.push(SessionEdit { path: path.to_string(), text }),
    };
    for (key, value) in want {
        let keys: Vec<SessionKey> = groups(spec.layout).iter().map(|g| SessionKey { group: g, key: *key }).collect();
        for path in files.iter().filter(|p| p.starts_with("/etc/")) {
            let Some(text) = current(&edits, path) else { continue };
            if let Some(new) = rewrite_ini_values(&text, &keys, &|v| !same(Some(v), value), &|_| value.clone()) {
                put(&mut edits, path, new);
            }
        }
    }
    let own = if spec.own.is_empty() { files.last().cloned().unwrap_or_default() } else { spec.own.to_string() };
    if !own.starts_with("/etc/") {
        return None;
    }
    let all_files = |edits: &[SessionEdit]| {
        let mut f = files.clone();
        for e in edits {
            if !f.contains(&e.path) {
                // Свой файл каталога — на своё место по имени.
                let pos = f.iter().position(|x| x.as_str() > e.path.as_str() && x.rsplit_once('/').map(|p| p.0) == e.path.rsplit_once('/').map(|p| p.0));
                match pos {
                    Some(i) => f.insert(i, e.path.clone()),
                    None => {
                        // Каталога в списке ещё нет — перед файлами после каталогов.
                        let i = f.iter().position(|x| spec.files.contains(&x.as_str())).unwrap_or(f.len());
                        f.insert(i, e.path.clone());
                    }
                }
            }
        }
        f
    };
    let value_now = |edits: &[SessionEdit], key: &str| {
        let texts: Vec<String> = all_files(edits).iter().filter_map(|p| current(edits, p)).collect();
        effective(spec, key, &texts)
    };
    for (key, value) in want {
        if same(value_now(&edits, key).as_deref(), value) {
            continue;
        }
        let base = current(&edits, &own);
        let group = format!("[{}]", write_group(spec.layout));
        let text = crate::defaults::set_ini_value(base.as_deref(), &group, key, value);
        put(&mut edits, &own, text);
    }
    let ok = want.iter().all(|(k, v)| same(value_now(&edits, k).as_deref(), v));
    ok.then_some(edits)
}

/// Наблюдения о целевом экране входа, нужные для плана.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetFacts {
    /// Текст PAM-службы автовхода целевого экрана входа.
    pub pam_autologin: Option<String>,
    /// Члены группы (из `getent group`), если PAM требует группу; `None` —
    /// группы нет.
    pub group_members: Option<Vec<String>>,
    /// Пользователь приветствия целевого экрана входа (`LOGIN_MANAGER_SPECS`).
    pub greeter_user: Option<String>,
}

/// Группа, без которой PAM автовхода спросит пароль
/// (`pam_succeed_if.so … user ingroup <группа>` в строке `auth`).
pub fn pam_autologin_group(pam: &str) -> Option<String> {
    pam.lines().map(str::trim).filter(|l| !l.starts_with('#') && l.starts_with("auth")).find_map(|l| {
        if !l.contains("pam_succeed_if") {
            return None;
        }
        let words: Vec<&str> = l.split_whitespace().collect();
        let i = words.iter().position(|w| *w == "ingroup")?;
        let g = words.get(i + 1)?;
        // Несколько групп через «:» — бессмысленно для автовхода; одна.
        (!g.contains(':')).then(|| g.to_string())
    })
}

/// Что сказать человеку о переносе.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// Автовход пользователя переносится.
    Moved { user: String },
    /// Сеанс не задан — откроется последний выбранный.
    LastSession,
    /// GDM не задаёт стол автовхода.
    GdmSession,
    /// Новый не умеет снова входить после выхода.
    ReloginLost,
    /// Пользователь добавляется в группу для PAM.
    Group { user: String, group: String },
    /// Автовход, настроенный ранее у целевого, выключается.
    TurnedOff { user: String },
    /// Перенести нельзя: у целевого автовход — команда запуска (greetd).
    LostCommand { user: String },
    /// Перенести нельзя: Rexon не знает, где целевой держит автовход.
    LostUnknown { user: String },
    /// Перенести нельзя: значение задаёт файл пакета.
    LostElsewhere { user: String },
    /// Пользователь автовхода — сам пользователь приветствия.
    LostGreeterUser { user: String },
}

/// Решение о переносе автовхода.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    /// Что записать (пусто — ничего).
    pub edits: Vec<SessionEdit>,
    /// Пользователя добавить в эту группу: `(пользователь, группа, группы ещё нет)`.
    pub group: Option<(String, String, bool)>,
    pub notes: Vec<Note>,
}

impl Transfer {
    fn nothing() -> Self {
        Transfer { edits: Vec::new(), group: None, notes: Vec::new() }
    }
    /// Записывать нечего и говорить нечего.
    pub fn is_nothing(&self) -> bool {
        self.edits.is_empty() && self.group.is_none() && self.notes.is_empty()
    }
    /// Автовход пропадёт.
    pub fn is_lost(&self) -> bool {
        self.notes.iter().any(|n| {
            matches!(n, Note::LostCommand { .. } | Note::LostUnknown { .. } | Note::LostElsewhere { .. } | Note::LostGreeterUser { .. })
        })
    }
}

/// Что сделать с автовходом при смене экрана входа `from` → `to`.
///
/// Перенос — «как у прежнего»: был автовход — тот же пользователь, тот же
/// сеанс, тот же `Relogin` у нового; не было — у нового он выключается
/// (иначе после смены экрана входа компьютер вдруг начнёт входить сам).
/// Прежний экран входа не трогается: вернёт его откат — вернётся и его
/// автовход.
pub fn plan_transfer(from: &str, to: &str, read: Read<'_>, list: List<'_>, facts: &TargetFacts) -> Transfer {
    let source = read_autologin(from, read, list);
    let Some(spec) = autologin_spec(to) else {
        return match source {
            AutologinState::On(a) => Transfer { notes: vec![Note::LostUnknown { user: a.user }], ..Transfer::nothing() },
            _ => Transfer::nothing(),
        };
    };
    let target = read_autologin(to, read, list);
    let want = match source {
        AutologinState::Unknown => return Transfer::nothing(),
        AutologinState::Off => match target {
            AutologinState::On(t) if spec.layout != Layout::Greetd => {
                let Some(edits) = plan_keys(spec, &desired(spec.layout, None), read, list) else {
                    return Transfer::nothing();
                };
                return Transfer { edits, group: None, notes: vec![Note::TurnedOff { user: t.user }] };
            }
            _ => return Transfer::nothing(),
        },
        AutologinState::On(a) => a,
    };
    if spec.layout == Layout::Greetd {
        return Transfer { notes: vec![Note::LostCommand { user: want.user }], ..Transfer::nothing() };
    }
    if facts.greeter_user.as_deref() == Some(want.user.as_str()) {
        return Transfer { notes: vec![Note::LostGreeterUser { user: want.user }], ..Transfer::nothing() };
    }
    let Some(edits) = plan_keys(spec, &desired(spec.layout, Some(&want)), read, list) else {
        return Transfer { notes: vec![Note::LostElsewhere { user: want.user }], ..Transfer::nothing() };
    };
    let group = facts.pam_autologin.as_deref().and_then(pam_autologin_group).and_then(|g| match &facts.group_members {
        Some(m) if m.iter().any(|u| *u == want.user) => None,
        Some(_) => Some((want.user.clone(), g, false)),
        None => Some((want.user.clone(), g, true)),
    });
    // Всё уже так — у нового тот же автовход.
    if edits.is_empty() && group.is_none() {
        return Transfer::nothing();
    }
    let mut notes = vec![Note::Moved { user: want.user.clone() }];
    if spec.layout == Layout::Gdm {
        if !want.session.is_empty() {
            notes.push(Note::GdmSession);
        }
    } else if want.session.is_empty() {
        notes.push(Note::LastSession);
    }
    if want.relogin && !matches!(spec.layout, Layout::Autologin { relogin: true }) {
        notes.push(Note::ReloginLost);
    }
    if let Some((u, g, _)) = &group {
        notes.push(Note::Group { user: u.clone(), group: g.clone() });
    }
    Transfer { edits, group, notes }
}

/// Текст для окна подтверждения «Сделать основным» — ДО пароля. `None` —
/// с автовходом ничего не меняется.
pub fn transfer_note(to_label: &str, t: &Transfer) -> Option<String> {
    let mut parts = Vec::new();
    for n in &t.notes {
        parts.push(match n {
            // TRANSLATORS: %1 is a user name, %2 a login screen name.
            Note::Moved { user } => trf("Automatic sign-in for %1 moves to %2 as well.", &[user, &to_label]),
            // TRANSLATORS: %1 is a login screen name.
            Note::LastSession => trf("%1 will open the desktop chosen at the last sign-in.", &[&to_label]),
            Note::GdmSession => trf(
                // TRANSLATORS: %1 is a login screen name (GDM).
                "%1 can't be told which desktop to open on automatic sign-in: it opens the one chosen at the last sign-in through %1.",
                &[&to_label],
            ),
            Note::ReloginLost => trf(
                // TRANSLATORS: %1 is a login screen name. Game mode is Rexon's "like Steam Machine" mode.
                "%1 can't sign you back in by itself after you sign out, and game mode relies on that.",
                &[&to_label],
            ),
            Note::Group { user, group } => trf(
                // TRANSLATORS: %1 is a login screen name, %2 a user name, %3 a system group name.
                "To let %1 sign in without a password, Rexon adds %2 to the group %3.",
                &[&to_label, user, group],
            ),
            Note::TurnedOff { user } => trf(
                // TRANSLATORS: %1 is a login screen name, %2 a user name.
                "%1 has automatic sign-in for %2 turned on. You don't sign in automatically now, so Rexon turns it off there too.",
                &[&to_label, user],
            ),
            Note::LostCommand { user } => trf(
                // TRANSLATORS: %1 is a user name, %2 a login screen name.
                "Automatic sign-in for %1 will be lost: %2 sets it as the desktop's start command in its own settings, and Rexon doesn't write that. Turn it on in the settings of %2 after the restart.",
                &[user, &to_label],
            ),
            Note::LostUnknown { user } => trf(
                // TRANSLATORS: %1 is a user name, %2 a login screen name.
                "Automatic sign-in for %1 will be lost: Rexon doesn't know where %2 keeps this setting.",
                &[user, &to_label],
            ),
            Note::LostElsewhere { user } => trf(
                // TRANSLATORS: %1 is a user name, %2 a login screen name.
                "Automatic sign-in for %1 will be lost: %2 takes this setting from a file of its package, which Rexon doesn't change.",
                &[user, &to_label],
            ),
            Note::LostGreeterUser { user } => trf(
                // TRANSLATORS: %1 is a user name, %2 a login screen name.
                "Automatic sign-in for %1 will be lost: %2 runs its own screen as this user.",
                &[user, &to_label],
            ),
        });
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// После переключения: автовход у нового такой, как задумано?
pub fn transfer_result_note(to_unit: &str, to_label: &str, want: &AutologinState, read: Read<'_>, list: List<'_>) -> Option<String> {
    let AutologinState::On(a) = want else { return None };
    match read_autologin(to_unit, read, list) {
        AutologinState::On(now) if now.user == a.user => {
            // TRANSLATORS: %1 is a user name, %2 a login screen name.
            Some(trf("Automatic sign-in for %1 moved to %2.", &[&a.user, &to_label]))
        }
        // TRANSLATORS: %1 is a login screen name.
        _ => Some(trf("Automatic sign-in wasn't moved to %1 — turn it on in the settings of %1.", &[&to_label])),
    }
}

// ── Звук экрана входа ────────────────────────────────────────────────────

/// Звуковые юниты пользователя приветствия, которые гасит обход «громкость
/// падает до 40 % после входа».
pub const GREETER_AUDIO_UNITS: &[&str] =
    &["pipewire.service", "pipewire.socket", "pipewire-pulse.service", "pipewire-pulse.socket", "wireplumber.service"];

/// Каталог пользовательских юнитов в доме пользователя приветствия.
pub fn greeter_user_unit_dir(home: &str) -> String {
    format!("{}/.config/systemd/user", home.trim_end_matches('/'))
}

/// Маскирован ли юнит: ссылка на `/dev/null` или пустой файл (так systemd
/// и считает: systemd.unit(5)).
pub fn unit_masked(link_target: Option<&str>, text: Option<&str>) -> bool {
    link_target == Some("/dev/null") || text.is_some_and(|t| t.is_empty())
}

/// Юниты, которые надо замаскировать у нового: у прежнего звук погашен
/// (хоть один юнит), а у нового этого юнита ещё нет вовсе (свою настройку
/// пользователя приветствия не перетираем).
pub fn audio_masks_to_add(old_masked: &[&str], new_present: &dyn Fn(&str) -> bool) -> Vec<&'static str> {
    if old_masked.is_empty() {
        return Vec::new();
    }
    GREETER_AUDIO_UNITS.iter().copied().filter(|u| !new_present(u)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn fs(files: &[(&str, &str)]) -> BTreeMap<String, String> {
        files.iter().map(|(p, t)| (p.to_string(), t.to_string())).collect()
    }

    fn io(m: &BTreeMap<String, String>) -> (impl Fn(&str) -> Option<String> + '_, impl Fn(&str) -> Vec<String> + '_) {
        let read = move |p: &str| m.get(p).cloned();
        let list = move |d: &str| {
            let pre = format!("{d}/");
            m.keys().filter_map(|k| k.strip_prefix(&pre)).filter(|r| !r.contains('/')).map(str::to_string).collect()
        };
        (read, list)
    }

    fn on(user: &str, session: &str, relogin: bool) -> AutologinState {
        AutologinState::On(Autologin { user: user.into(), session: session.into(), relogin })
    }

    /// Применить правки поверх «диска».
    fn apply(m: &BTreeMap<String, String>, t: &Transfer) -> BTreeMap<String, String> {
        let mut out = m.clone();
        for e in &t.edits {
            out.insert(e.path.clone(), e.text.clone());
        }
        out
    }

    #[test]
    fn читает_автовход_каждого_экрана_входа() {
        let m = fs(&[
            ("/usr/lib/sddm/sddm.conf.d/default.conf", "[Autologin]\nRelogin=false\nSession=\nUser=\n"),
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nRelogin=true\nSession=plasma\nUser=alice\n"),
            ("/etc/sddm.conf", "[Autologin]\nSession=gamescope-session-steam.desktop\n"),
            ("/etc/plasmalogin.conf", "[Autologin]\nSession=plasma\n"),
            ("/etc/ddm.conf.d/a.conf", "[Autologin]\nUser=bob\nSession=/usr/share/wayland-sessions/treeland.desktop\n"),
            ("/etc/gdm/custom.conf", "[daemon]\nAutomaticLoginEnable=True\nAutomaticLogin=carol\n"),
            ("/etc/gdm3/custom.conf", "[daemon]\nAutomaticLoginEnable=false\nAutomaticLogin=dave\n"),
            ("/usr/share/lightdm/lightdm.conf.d/50-x.conf", "[SeatDefaults]\nautologin-user=eve\n"),
            ("/etc/lightdm/lightdm.conf", "[Seat:*]\n#autologin-user=\nautologin-session=xfce\n"),
            ("/etc/greetd/config.toml", "[default_session]\ncommand = \"tuigreet\"\n[initial_session]\nuser = \"frank\"\ncommand = \"sway\"\n"),
        ]);
        let (read, list) = io(&m);
        assert_eq!(read_autologin("sddm", &read, &list), on("alice", "gamescope-session-steam", true));
        assert_eq!(read_autologin("plasmalogin", &read, &list), AutologinState::Off);
        assert_eq!(read_autologin("ddm", &read, &list), on("bob", "treeland", false));
        assert_eq!(read_autologin("gdm", &read, &list), on("carol", "", false));
        assert_eq!(read_autologin("gdm3", &read, &list), AutologinState::Off, "AutomaticLoginEnable=false");
        assert_eq!(read_autologin("lightdm", &read, &list), on("eve", "xfce", false));
        assert_eq!(read_autologin("greetd", &read, &list), on("frank", "", false));
        assert_eq!(read_autologin("ly", &read, &list), AutologinState::Unknown);
    }

    #[test]
    fn lightdm_частная_группа_места_сильнее() {
        let m = fs(&[(
            "/etc/lightdm/lightdm.conf",
            "[Seat:seat0]\nautologin-user=\n[Seat:*]\nautologin-user=eve\n",
        )]);
        let (read, list) = io(&m);
        assert_eq!(read_autologin("lightdm", &read, &list), AutologinState::Off);
    }

    /// Машина как у Wehrwolfmann: Plasma Login с автовходом → SDDM.
    #[test]
    fn plasma_login_в_sddm_переносит_пользователя_сеанс_и_relogin() {
        let m = fs(&[
            ("/etc/plasmalogin.conf", "[Autologin]\nRelogin=true\nSession=plasma\nUser=alice\n"),
            ("/usr/lib/sddm/sddm.conf.d/default.conf", "[Autologin]\nRelogin=false\nSession=\nUser=\n"),
            ("/etc/sddm.conf", "[Autologin]\nSession=plasmax11\n"),
            ("/etc/sddm.conf.d/kde_settings.conf", "[Theme]\nCurrent=breeze\n"),
        ]);
        let (read, list) = io(&m);
        let t = plan_transfer("plasmalogin", "sddm", &read, &list, &TargetFacts::default());
        assert!(!t.is_lost());
        assert_eq!(t.notes, vec![Note::Moved { user: "alice".into() }]);
        // Чужая строка Session= в /etc/sddm.conf переписана на месте, недостающее — в свой файл.
        let sddm_conf = t.edits.iter().find(|e| e.path == "/etc/sddm.conf").expect("sddm.conf");
        assert_eq!(sddm_conf.text, "[Autologin]\nSession=plasma.desktop\n");
        let own = t.edits.iter().find(|e| e.path == "/etc/sddm.conf.d/zz-rexon-autologin.conf").expect("свой файл");
        assert!(own.text.contains("User=alice") && own.text.contains("Relogin=true"), "{}", own.text);
        assert!(t.edits.iter().all(|e| e.path.starts_with("/etc/")), "файлы пакета не правятся");
        // Итог читается так же, как читает SDDM.
        let after = apply(&m, &t);
        let (read2, list2) = io(&after);
        assert_eq!(read_autologin("sddm", &read2, &list2), on("alice", "plasma", true));
        // Повторный план — делать нечего.
        assert!(plan_transfer("plasmalogin", "sddm", &read2, &list2, &TargetFacts::default()).is_nothing());
    }

    #[test]
    fn в_gdm_пишется_custom_conf_и_честно_про_стол() {
        let m = fs(&[
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nSession=plasma\nUser=alice\n"),
            ("/etc/gdm/custom.conf", "# GDM configuration storage\n\n[daemon]\n\n[security]\n"),
        ]);
        let (read, list) = io(&m);
        let t = plan_transfer("sddm", "gdm", &read, &list, &TargetFacts::default());
        assert_eq!(t.notes, vec![Note::Moved { user: "alice".into() }, Note::GdmSession]);
        assert_eq!(t.edits.len(), 1);
        assert_eq!(t.edits[0].path, "/etc/gdm/custom.conf");
        let after = apply(&m, &t);
        let text = &after["/etc/gdm/custom.conf"];
        assert!(text.starts_with("# GDM configuration storage\n"), "комментарий цел: {text}");
        assert!(text.contains("[security]"));
        let (read2, list2) = io(&after);
        assert_eq!(read_autologin("gdm", &read2, &list2), on("alice", "", false));
        let note = transfer_note("GDM", &t).unwrap();
        assert!(note.contains("alice") && note.contains("GDM"), "{note}");
    }

    #[test]
    fn в_lightdm_пишется_главный_файл_и_группа_autologin() {
        let m = fs(&[
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nRelogin=true\nSession=plasma\nUser=alice\n"),
            ("/etc/lightdm/lightdm.conf", "[LightDM]\n[Seat:*]\n#autologin-user=\n[XDMCPServer]\n"),
        ]);
        let (read, list) = io(&m);
        let pam = "auth        required    pam_env.so\nauth        [success=1 default=ignore]  pam_succeed_if.so user ingroup autologin\n";
        let facts = TargetFacts { pam_autologin: Some(pam.into()), group_members: None, greeter_user: Some("lightdm".into()) };
        let t = plan_transfer("sddm", "lightdm", &read, &list, &facts);
        assert_eq!(t.group, Some(("alice".into(), "autologin".into(), true)), "группы нет — создать");
        assert!(t.notes.contains(&Note::ReloginLost));
        assert!(t.notes.contains(&Note::Group { user: "alice".into(), group: "autologin".into() }));
        let after = apply(&m, &t);
        let text = &after["/etc/lightdm/lightdm.conf"];
        assert!(text.contains("#autologin-user=\n"), "закомментированная строка не тронута: {text}");
        let seat = text.find("[Seat:*]").unwrap();
        let xdmcp = text.find("[XDMCPServer]").unwrap();
        let user = text.find("autologin-user=alice").unwrap();
        assert!(seat < user && user < xdmcp, "ключ в своей группе: {text}");
        let (read2, list2) = io(&after);
        assert_eq!(read_autologin("lightdm", &read2, &list2), on("alice", "plasma", false));
        // Уже в группе — группу не трогать.
        let member = TargetFacts { group_members: Some(vec!["bob".into(), "alice".into()]), ..facts };
        assert_eq!(plan_transfer("sddm", "lightdm", &read, &list, &member).group, None);
    }

    #[test]
    fn в_ddm_свой_файл_в_каталоге_и_без_relogin() {
        let m = fs(&[("/etc/plasmalogin.conf", "[Autologin]\nRelogin=true\nSession=treeland-user.desktop\nUser=alice\n")]);
        let (read, list) = io(&m);
        let t = plan_transfer("plasmalogin", "ddm", &read, &list, &TargetFacts::default());
        assert_eq!(t.edits.len(), 1);
        assert_eq!(t.edits[0].path, "/etc/ddm.conf.d/zz-rexon-autologin.conf");
        assert!(!t.edits[0].text.contains("Relogin"), "у DDM нет Relogin: {}", t.edits[0].text);
        assert!(t.notes.contains(&Note::ReloginLost));
        let after = apply(&m, &t);
        let (read2, list2) = io(&after);
        assert_eq!(read_autologin("ddm", &read2, &list2), on("alice", "treeland-user", false));
    }

    #[test]
    fn без_сеанса_откроется_последний_выбранный() {
        let m = fs(&[("/etc/gdm/custom.conf", "[daemon]\nAutomaticLoginEnable=true\nAutomaticLogin=carol\n")]);
        let (read, list) = io(&m);
        let t = plan_transfer("gdm", "plasmalogin", &read, &list, &TargetFacts::default());
        assert_eq!(t.notes, vec![Note::Moved { user: "carol".into() }, Note::LastSession]);
        let after = apply(&m, &t);
        let (read2, list2) = io(&after);
        assert_eq!(read_autologin("plasmalogin", &read2, &list2), on("carol", "", false));
    }

    #[test]
    fn автовхода_не_было_у_нового_он_выключается() {
        let m = fs(&[
            ("/etc/plasmalogin.conf", "[Autologin]\nSession=plasma\n"),
            ("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nSession=plasma\nUser=bob\n"),
        ]);
        let (read, list) = io(&m);
        let t = plan_transfer("plasmalogin", "sddm", &read, &list, &TargetFacts::default());
        assert_eq!(t.notes, vec![Note::TurnedOff { user: "bob".into() }]);
        let after = apply(&m, &t);
        assert_eq!(after["/etc/sddm.conf.d/kde_settings.conf"], "[Autologin]\nSession=plasma\nUser=\n");
        let (read2, list2) = io(&after);
        assert_eq!(read_autologin("sddm", &read2, &list2), AutologinState::Off);
        // Нигде нет — ничего.
        let empty = fs(&[]);
        let (r, l) = io(&empty);
        assert!(plan_transfer("plasmalogin", "sddm", &r, &l, &TargetFacts::default()).is_nothing());
    }

    #[test]
    fn нельзя_перенести_честно_говорится() {
        let m = fs(&[
            ("/etc/plasmalogin.conf", "[Autologin]\nSession=plasma\nUser=alice\n"),
            // Файл пакета, читаемый ПОСЛЕ /etc у LightDM, — частная группа места.
            ("/usr/share/lightdm/lightdm.conf.d/99-x.conf", "[Seat:seat0]\nautologin-user=\n"),
        ]);
        let (read, list) = io(&m);
        let greetd = plan_transfer("plasmalogin", "greetd", &read, &list, &TargetFacts::default());
        assert!(greetd.is_lost() && greetd.edits.is_empty());
        assert!(transfer_note("greetd", &greetd).unwrap().contains("alice"));
        let unknown = plan_transfer("plasmalogin", "ly", &read, &list, &TargetFacts::default());
        assert_eq!(unknown.notes, vec![Note::LostUnknown { user: "alice".into() }]);
        let me = TargetFacts { greeter_user: Some("alice".into()), ..TargetFacts::default() };
        assert!(plan_transfer("plasmalogin", "sddm", &read, &list, &me).is_lost());
        // [Seat:seat0] из каталога пакета перекрывает [Seat:*] главного файла.
        let elsewhere = plan_transfer("plasmalogin", "lightdm", &read, &list, &TargetFacts::default());
        assert_eq!(elsewhere.notes, vec![Note::LostElsewhere { user: "alice".into() }]);
        assert!(elsewhere.edits.is_empty(), "при «нельзя» ничего не пишется");
    }

    #[test]
    fn неизвестный_прежний_ничего_не_меняет() {
        let m = fs(&[("/etc/sddm.conf.d/kde_settings.conf", "[Autologin]\nUser=bob\n")]);
        let (read, list) = io(&m);
        assert!(plan_transfer("ly", "sddm", &read, &list, &TargetFacts::default()).is_nothing());
    }

    #[test]
    fn pam_группа_автовхода() {
        let arch = "#%PAM-1.0\nauth        required    pam_env.so\nauth        [success=1 default=ignore]  pam_succeed_if.so user ingroup autologin\nauth        required    pam_unix.so\n";
        assert_eq!(pam_autologin_group(arch).as_deref(), Some("autologin"));
        assert_eq!(pam_autologin_group("auth required pam_permit.so\n"), None);
        assert_eq!(pam_autologin_group("# auth sufficient pam_succeed_if.so user ingroup x\n"), None);
        assert_eq!(pam_autologin_group("account required pam_succeed_if.so user ingroup x\n"), None);
    }

    #[test]
    fn маски_звука_переносятся_только_если_были() {
        assert!(audio_masks_to_add(&[], &|_| false).is_empty());
        let all = audio_masks_to_add(&["wireplumber.service"], &|_| false);
        assert_eq!(all, GREETER_AUDIO_UNITS.to_vec());
        let some = audio_masks_to_add(&["pipewire.service"], &|u| u == "pipewire.socket");
        assert!(!some.contains(&"pipewire.socket") && some.len() == GREETER_AUDIO_UNITS.len() - 1);
        assert!(unit_masked(Some("/dev/null"), None));
        assert!(unit_masked(None, Some("")));
        assert!(!unit_masked(Some("/usr/lib/systemd/user/pipewire.service"), Some("[Unit]\n")));
        assert_eq!(greeter_user_unit_dir("/var/lib/sddm/"), "/var/lib/sddm/.config/systemd/user");
    }

    #[test]
    fn каждый_экран_входа_из_таблицы_с_автовходом_пишется_в_etc() {
        for s in AUTOLOGIN_SPECS {
            if s.layout == Layout::Greetd {
                continue;
            }
            let own = if s.own.is_empty() { *s.files.last().unwrap() } else { s.own };
            assert!(own.starts_with("/etc/"), "{}: {own}", s.unit);
            assert!(crate::system_plan::login_manager_spec(s.unit).is_some(), "{} нет в LOGIN_MANAGER_SPECS", s.unit);
        }
    }
}
