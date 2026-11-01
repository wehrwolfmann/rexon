//! Признаки риска пакета AUR — «Проверка источника» на карточке пакета и
//! короткий значок на плитке.
//!
//! Зачем. AUR — пакеты, которые выкладывают сами пользователи; правила Arch
//! прямо говорят, что проверять источник — забота того, кто ставит. Обычный
//! покупатель PKGBUILD не читает, поэтому Rexon сам собирает то, что можно
//! узнать без входа на сайты, и говорит человеческими словами, что насторожило.
//! Установку это НЕ запрещает — только предупреждает.
//!
//! Откуда что берётся:
//!
//! * RPC `info` (уже есть у карточки и плитки) — голоса, популярность,
//!   «устарел», нет сопровождающего;
//! * RPC `search?by=maintainer` — сколько пакетов у сопровождающего и когда
//!   появился первый из них (даты регистрации учётки RPC не отдаёт, а
//!   страница учётки закрыта для гостей); если по нему видно, что за одну
//!   неделю обновили много СТАРЫХ пакетов, — ещё один запрос `info` по
//!   ним: кто их когда-то выложил. Учётка без своих давних пакетов,
//!   разом подобравшая чужие брошенные, — так захватили пакеты в июне
//!   2026;
//! * журнал git пакета (страница `cgit … /log/`) — кто правил пакет: новое
//!   имя в последние полгода при прежних других — «пакет перешёл в другие
//!   руки»; и дата самой новой правки: если AUR пишет «обновлён» заметно
//!   ПОЗЖЕ неё, историю пакета переписали (силой это может только
//!   сотрудник Arch, вычищая вредную правку) или правку выложили с
//!   поддельной датой. Отдельного запроса не нужно — журнал уже есть;
//! * страница пакета на сайте AUR — кто выложил последнюю версию (Last
//!   Packager; RPC этого не отдаёт). Если это не сопровождающий и не его
//!   помощник, а сотрудник Arch (списки Package Maintainers и Developers
//!   на archlinux.org, держатся неделю), — так Arch вычищает взломанные
//!   пакеты. Отметку «Inactive since» и дату регистрации учётки сайт
//!   показывает только вошедшим (страница учётки отвечает гостю 401),
//!   поэтому Rexon их не видит;
//! * собственные снимки: при каждом открытии карточки запоминается
//!   сопровождающий, смена между открытиями видна сразу;
//! * `.SRCINFO` — откуда берутся исходники и сверяются ли они по
//!   контрольной сумме;
//! * API GitHub для источников на GitHub — форк ли это и не меняли ли
//!   недавно сценарии GitHub Actions (`.github/workflows`): ровно так
//!   выглядела атака 09.10.2026 — украденная учётка, во все репозитории
//!   дописан сценарий, крадущий ключи. Новый файл сценария, который в
//!   текущей версии репозитория (ветка по умолчанию) уже удалён, — жёлтый
//!   признак, а не красный: после той атаки автор удалил вредный файл, и
//!   красный ещё 30 дней был бы ложной тревогой.
//!
//! Нагрузка. Глубокая проверка идёт ТОЛЬКО при открытии карточки, ответ
//! лежит на диске сутки (`~/.cache/rexon/aur-trust.json`), обращения к
//! страницам AUR — с паузой (её даёт вызывающий), к GitHub без входа —
//! не больше `MAX_GITHUB_REPOS` репозиториев и `MAX_GITHUB_PER_REPO`
//! запросов на каждый (лимит GitHub для гостя — 60 в час). Нет сети, защита сайта от ботов,
//! лимит GitHub — проверка молча пропускается и помечается неполной.
//!
//! Разбор — чистые функции над записанными ответами (`tests/aur_trust.rs`).

use crate::aur::{AurInfo, AurPackage};
use crate::i18n::{fill, ntr, tr, trf};
use crate::net::{NetError, NetIo};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Mutex;

// ── Пороги ────────────────────────────────────────────────────────────────

/// Сколько держать глубокую проверку на диске, секунды.
pub const CACHE_TTL_SECS: i64 = 24 * 3600;
/// Неполную проверку (сеть, лимит GitHub) повторяем раньше — через час.
pub const PARTIAL_TTL_SECS: i64 = 3600;
/// Записи старше этого из файла выбрасываются (снимки сопровождающих тоже).
pub const KEEP_SECS: i64 = 365 * 24 * 3600;

/// «Недавно» для смены сопровождающего и нового имени в журнале: полгода.
/// Захват брошенного пакета и вредное обновление разнесены во времени на
/// недели-месяцы; старые передачи пакета из рук в руки шумом не считаются.
pub const RECENT_OWNER_SECS: i64 = 180 * 24 * 3600;
/// «Недавно» для сценариев GitHub: 30 дней, как просили после атаки 09.10.
pub const WORKFLOW_SECS: i64 = 30 * 24 * 3600;
/// Новый сопровождающий: все его пакеты моложе полугода.
pub const NEW_MAINTAINER_SECS: i64 = 180 * 24 * 3600;

/// Мало голосов: меньше пяти — пакет, по сути, никто, кроме автора, не
/// пробовал и не смотрел.
pub const FEW_VOTES: i64 = 5;
/// …или меньше двадцати при популярности ниже 0,1. Популярность AUR — сумма
/// голосов с затуханием 0,98 в день: 0,1 — это меньше одного свежего голоса
/// за последние ~4 месяца (0,98^114 ≈ 0,1), то есть пакетом сейчас не
/// пользуются.
pub const LOW_POP_VOTES: i64 = 20;
pub const LOW_POPULARITY: f64 = 0.1;
/// Мало пакетов у сопровождающего: два и меньше.
pub const FEW_PACKAGES: i64 = 2;

/// Сколько репозиториев GitHub проверять у одного пакета.
pub const MAX_GITHUB_REPOS: usize = 2;
/// Сколько свежих правок сценариев раскрывать подробно (новый ли файл).
pub const MAX_COMMIT_DETAILS: usize = 2;
/// Сколько всего запросов к GitHub на один репозиторий: паспорт, список
/// правок сценариев, до [`MAX_COMMIT_DETAILS`] правок подробно и, если на
/// это осталось место, список сценариев в текущей версии.
pub const MAX_GITHUB_PER_REPO: usize = 4;

/// Список сотрудников Arch держим неделю: меняется он редко, а страницы
/// большие (две по 80–230 КБ).
pub const STAFF_TTL_SECS: i64 = 7 * 24 * 3600;

/// «Обновлён» позже самой новой правки в git больше чем на неделю —
/// историю переписали. Меньше — обычное дело: правку сделали, а выложили
/// через несколько дней.
pub const HISTORY_GAP_SECS: i64 = 7 * 24 * 3600;
/// Только что обновлённый пакет так не судим: страница журнала на сайте
/// отдаётся из кеша и может отставать от выкладки.
pub const CGIT_LAG_SECS: i64 = 6 * 3600;

/// Массовая подборка: столько чужих старых пакетов, обновлённых за одну
/// неделю. Честный новичок обычно подбирает один-четыре связанных
/// пакета; при захвате июня 2026 (учётка fennekuiper) — 13 за два дня.
pub const MASS_ADOPT: usize = 8;
/// «Короткое время» для подборки.
pub const ADOPT_WINDOW_SECS: i64 = 7 * 24 * 3600;
/// Пакет «старый» (подобран, а не только что выложен): существовал хотя
/// бы месяц до обновления.
pub const ADOPT_OLD_SECS: i64 = 30 * 24 * 3600;
/// Больше паспортов одним запросом `info` не спрашиваем.
pub const MAX_INFO_NAMES: usize = 100;

/// Таймаут запросов, секунды.
pub const TIMEOUT_SECS: u64 = 25;
/// Имя файла кеша в `~/.cache/rexon/`.
pub const CACHE_FILE: &str = "aur-trust.json";
/// Версия формата файла: чужой формат читается как пустой.
pub const CACHE_VERSION: u32 = 1;

/// Цвет красного значка: тот же, что у опасных действий вкладки
/// «Расширения».
pub const DANGER_COLOR: &str = crate::qt_extensions::DANGER_COLOR;

// ── Адреса ────────────────────────────────────────────────────────────────

const AUR_WEB: &str = "https://aur.archlinux.org";
const CGIT: &str = "https://aur.archlinux.org/cgit/aur.git";

/// Списки сотрудников Arch с правом выкладывать в ЛЮБОЙ пакет AUR:
/// Package Maintainers (прежние Trusted Users) и Developers.
pub const STAFF_PAGES: [&str; 2] = [
    "https://archlinux.org/people/package-maintainers/",
    "https://archlinux.org/people/developers/",
];
/// Адрес API GitHub (сведения о репозитории верхнего проекта пакета).
const GH_API: &str = "https://api.github.com";
/// `User-Agent` запросов к API GitHub: без него оно отвечает 403.
const GH_USER_AGENT: &str = "Rexon";

/// Заголовки запросов к API GitHub (без входа: токен человека программа
/// не трогает).
pub fn github_api_headers() -> Vec<(String, String)> {
    vec![
        ("Accept".to_string(), "application/vnd.github+json".to_string()),
        ("X-GitHub-Api-Version".to_string(), "2022-11-28".to_string()),
    ]
}

/// Значение для `?h=` (имя ветки в общем git AUR) — экранированное.
fn q(base: &str) -> String {
    let mut out = String::new();
    for &b in base.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Журнал правок пакета (страница cgit).
pub fn cgit_log_url(pkgbase: &str) -> String {
    format!("{CGIT}/log/?h={}", q(pkgbase))
}

/// Страница пакета на сайте AUR.
pub fn package_page_url(name: &str) -> String {
    format!("{AUR_WEB}/packages/{}", q(name))
}

/// `.SRCINFO` пакета.
pub fn srcinfo_url(pkgbase: &str) -> String {
    format!("{CGIT}/plain/.SRCINFO?h={}", q(pkgbase))
}

/// PKGBUILD пакета — открывается внутри программы.
pub fn pkgbuild_url(pkgbase: &str) -> String {
    format!("{CGIT}/plain/PKGBUILD?h={}", q(pkgbase))
}

/// Все пакеты сопровождающего (RPC).
pub fn maintainer_url(maintainer: &str) -> String {
    crate::aur::search_url(maintainer, "maintainer")
}

/// Паспорт репозитория на GitHub.
pub fn github_repo_url(repo: &str) -> String {
    format!("{GH_API}/repos/{repo}")
}

/// Правки сценариев GitHub Actions с момента `since`.
pub fn github_workflows_url(repo: &str, since: i64) -> String {
    format!(
        "{GH_API}/repos/{repo}/commits?path=.github/workflows&since={}&per_page=20",
        iso_utc(since)
    )
}

/// Одна правка целиком (какие файлы она добавила).
pub fn github_commit_url(repo: &str, sha: &str) -> String {
    format!("{GH_API}/repos/{repo}/commits/{sha}")
}

/// Сценарии в текущей версии репозитория (ветка `branch`; пусто — ветка
/// по умолчанию на стороне GitHub). 404 — каталога сценариев нет вовсе.
pub fn github_workflow_dir_url(repo: &str, branch: &str) -> String {
    let base = format!("{GH_API}/repos/{repo}/contents/.github/workflows");
    if branch.is_empty() {
        base
    } else {
        format!("{base}?ref={}", q(branch))
    }
}

// ── Время ─────────────────────────────────────────────────────────────────

/// Дней от 1970-01-01 до даты (григорианский календарь).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `2026-10-08T20:08:55Z` / `2026-06-19 21:25:48 +0000` → epoch (UTC).
/// Смещение пояса учитывается; мусор — `None`.
pub fn parse_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |a: usize, b: usize| -> Option<i64> { s.get(a..b)?.parse().ok() };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    if s.get(4..5)? != "-" || s.get(7..8)? != "-" {
        return None;
    }
    let (h, mi, sec) = if s.len() >= 19 {
        (num(11, 13)?, num(14, 16)?, num(17, 19)?)
    } else {
        (0, 0, 0)
    };
    let mut ts = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec;
    // Пояс: `Z`, `+0000`, `+02:00`.
    let tail = s.get(19..).unwrap_or("").trim();
    if let Some(sign) = tail.chars().next().filter(|c| *c == '+' || *c == '-') {
        let digits: String = tail[1..].chars().filter(char::is_ascii_digit).collect();
        if digits.len() >= 4 {
            let oh: i64 = digits[0..2].parse().unwrap_or(0);
            let om: i64 = digits[2..4].parse().unwrap_or(0);
            let off = oh * 3600 + om * 60;
            ts += if sign == '+' { -off } else { off };
        }
    }
    Some(ts)
}

/// Epoch → `2026-10-08T20:08:55Z`.
pub fn iso_utc(ts: i64) -> String {
    crate::engine_update::now_iso(ts)
}

/// Epoch → `2026-10-08` (пусто для нуля).
fn day(ts: i64) -> String {
    crate::aur::epoch_day(ts)
}

/// Сейчас, epoch.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── Журнал пакета (cgit) ─────────────────────────────────────────────────

/// Строка журнала: когда и кто.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogEntry {
    pub date: i64,
    pub author: String,
}

/// Журнал со страницы cgit. `None` — пришла не страница журнала (защита
/// сайта от ботов, ошибка): обходить её нельзя, проверка пропускается.
pub fn parse_cgit_log(html: &str) -> Option<Vec<LogEntry>> {
    if !html.contains("<table class='list nowrap'>") {
        return None;
    }
    let mut out = Vec::new();
    for row in html.split("<tr>").skip(1) {
        let row = row.split("</tr>").next().unwrap_or("");
        let Some(at) = row.find("<span title='") else { continue };
        let rest = &row[at + "<span title='".len()..];
        let Some(date) = rest.split('\'').next().and_then(parse_time) else { continue };
        let cells: Vec<&str> = row.split("<td").skip(1).collect();
        let Some(last) = cells.last() else { continue };
        let inner = last.split_once('>').map(|(_, r)| r).unwrap_or("");
        let inner = inner.split("</td>").next().unwrap_or("");
        let author = crate::aur::html_to_text(inner).trim().to_string();
        if author.is_empty() {
            continue;
        }
        out.push(LogEntry { date, author });
    }
    Some(out)
}

/// Пакет недавно стал править новый человек.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NewAuthor {
    /// Новое имя (как подписаны правки).
    pub name: String,
    /// Первая его правка.
    pub since: i64,
    /// Кто правил раньше (самый частый автор старых правок).
    pub before: String,
}

/// Новое имя в журнале: в последние полгода правки подписаны тем, кого
/// среди старых правок нет вовсе. Нет старых правок на странице (пакет
/// молодой или очень активный) — судить не о чем.
pub fn new_author(log: &[LogEntry], now: i64) -> Option<NewAuthor> {
    let border = now - RECENT_OWNER_SECS;
    let norm = |s: &str| s.trim().to_lowercase();
    let old: Vec<&LogEntry> = log.iter().filter(|e| e.date < border).collect();
    if old.is_empty() {
        return None;
    }
    let old_names: Vec<String> = old.iter().map(|e| norm(&e.author)).collect();
    // Самая ранняя свежая правка нового имени (журнал — от новых к старым).
    let newcomer = log
        .iter()
        .filter(|e| e.date >= border && !old_names.contains(&norm(&e.author)))
        .min_by_key(|e| e.date)?;
    let mut counts: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for e in &old {
        let c = counts.entry(norm(&e.author)).or_insert((0, e.author.clone()));
        c.0 += 1;
    }
    let before = counts
        .values()
        .max_by_key(|(n, _)| *n)
        .map(|(_, name)| name.clone())
        .unwrap_or_default();
    Some(NewAuthor { name: newcomer.author.clone(), since: newcomer.date, before })
}

// ── Страница пакета и сотрудники Arch ────────────────────────────────────

/// Что берём со страницы пакета (гостевой вид, имена — текстом).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackagePage {
    pub maintainer: String,
    /// Помощники сопровождающего (в скобках после его имени).
    pub comaintainers: Vec<String>,
    /// Кто выложил последнюю версию («» — неизвестно, учётка удалена).
    pub last_packager: String,
}

/// Ячейка таблицы паспорта по подписи строки, текстом в одну строку.
fn page_cell(html: &str, label: &str) -> Option<String> {
    let at = html.find(&format!("<th>{label}:</th>"))?;
    let rest = &html[at..];
    let rest = &rest[rest.find("<td")?..];
    let inner = rest.split_once('>')?.1.split("</td>").next()?;
    Some(crate::aur::html_to_text(inner).split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Разобрать страницу пакета. `None` — пришла не она (защита сайта от
/// ботов, ошибка).
pub fn parse_package_page(html: &str) -> Option<PackagePage> {
    if !html.contains("id=\"pkgdetails\"") {
        return None;
    }
    let none = |s: &str| if s == "None" { String::new() } else { s.trim().to_string() };
    let packager = page_cell(html, "Last Packager")?;
    let maint = page_cell(html, "Maintainer").unwrap_or_default();
    let (name, co) = match maint.split_once('(') {
        Some((n, rest)) => (
            n,
            rest.trim_end_matches(')')
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        ),
        None => (maint.as_str(), Vec::new()),
    };
    Some(PackagePage { maintainer: none(name), comaintainers: co, last_packager: none(&packager) })
}

/// Имена сотрудников со страницы списка archlinux.org (в нижнем регистре):
/// и имя учётки (якорь карточки), и псевдоним. `None` — не та страница.
pub fn parse_staff_page(html: &str) -> Option<Vec<String>> {
    if !html.contains("schema.org/Person") {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    for marker in ["itemprop=\"additionalName\">", "class=\"headerlink\" name=\""] {
        for part in html.split(marker).skip(1) {
            let name: String = part.chars().take_while(|c| !matches!(c, '<' | '"')).collect();
            let name = name.trim().to_lowercase();
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
        }
    }
    Some(out)
}

/// Сотрудники Arch (лежит в файле кеша неделю).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StaffList {
    pub names: Vec<String>,
    pub checked_at: i64,
}

impl StaffList {
    pub fn fresh(&self, now: i64) -> bool {
        !self.names.is_empty()
            && self.checked_at > 0
            && now >= self.checked_at
            && now - self.checked_at < STAFF_TTL_SECS
    }

    pub fn contains(&self, name: &str) -> bool {
        let n = name.trim().to_lowercase();
        self.names.iter().any(|s| *s == n)
    }
}

/// Последнюю версию выложил кто-то со стороны: не сопровождающий и не его
/// помощник. Обычный человек так не может — только сотрудник Arch или
/// прежний сопровождающий (пакет с тех пор передали).
pub fn packager_is_outsider(page: &PackagePage, maintainer: &str) -> bool {
    let norm = |s: &str| s.trim().to_lowercase();
    let p = norm(&page.last_packager);
    !p.is_empty() && p != norm(maintainer) && !page.comaintainers.iter().any(|c| norm(c) == p)
}

// ── .SRCINFO ──────────────────────────────────────────────────────────────

/// Один источник из `.SRCINFO`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Source {
    /// Адрес без `имя::`, без `git+` и без `#…`.
    pub url: String,
    /// Как записан (для показа человеку).
    pub raw: String,
    /// Система контроля версий (`git`, `hg`, …), пусто — обычный файл.
    pub vcs: String,
    /// Хвост после `#` (`branch=…`, `tag=…`, `commit=…`).
    pub fragment: String,
    /// Хоть одна контрольная сумма — не `SKIP`.
    pub verified: bool,
}

impl Source {
    /// Скачивается из сети (а не лежит рядом с PKGBUILD в git AUR).
    pub fn remote(&self) -> bool {
        !self.vcs.is_empty() || self.url.contains("://")
    }
}

/// Разобранный `.SRCINFO`: адрес проекта и источники.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SrcInfo {
    pub pkgbase: String,
    pub url: String,
    pub sources: Vec<Source>,
}

const SUM_ALGOS: &[&str] =
    &["ck", "md5", "sha1", "sha224", "sha256", "sha384", "sha512", "b2"];
const VCS: &[&str] = &["git", "hg", "svn", "bzr", "fossil"];

/// Разобрать `.SRCINFO`. `None` — это не он (защита сайта от ботов).
pub fn parse_srcinfo(text: &str) -> Option<SrcInfo> {
    if !text.trim_start().starts_with("pkgbase") {
        return None;
    }
    let mut out = SrcInfo::default();
    // Ключ → значения по порядку (только раздел pkgbase: источники и
    // суммы в разделах pkgname не задаются).
    let mut fields: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once(" = ") else { continue };
        let (k, v) = (k.trim(), v.trim());
        if k == "pkgname" {
            break;
        }
        match k {
            "pkgbase" => out.pkgbase = v.to_string(),
            "url" => out.url = v.to_string(),
            _ => fields.entry(k.to_string()).or_default().push(v.to_string()),
        }
    }
    for (key, values) in &fields {
        let Some(suffix) = key.strip_prefix("source") else { continue };
        if !(suffix.is_empty() || suffix.starts_with('_')) {
            continue;
        }
        for (i, raw) in values.iter().enumerate() {
            let verified = SUM_ALGOS.iter().any(|a| {
                fields
                    .get(&format!("{a}sums{suffix}"))
                    .and_then(|s| s.get(i))
                    .is_some_and(|s| !s.eq_ignore_ascii_case("SKIP"))
            });
            out.sources.push(split_source(raw, verified));
        }
    }
    Some(out)
}

fn split_source(raw: &str, verified: bool) -> Source {
    let addr = raw.split_once("::").map(|(_, a)| a).unwrap_or(raw);
    let (addr, fragment) = match addr.split_once('#') {
        Some((a, f)) => (a, f.to_string()),
        None => (addr, String::new()),
    };
    let addr = addr.split_once("?signed").map(|(a, _)| a).unwrap_or(addr);
    let mut vcs = String::new();
    let mut url = addr.to_string();
    for v in VCS {
        if let Some(rest) = addr.strip_prefix(&format!("{v}+")) {
            vcs = v.to_string();
            url = rest.to_string();
        } else if addr.starts_with(&format!("{v}://")) {
            vcs = v.to_string();
        }
    }
    Source { url, raw: raw.to_string(), vcs, fragment, verified }
}

/// Имя пакета говорит, что он собирается из свежего кода (`-git` и т. п.):
/// у таких источник «ветка» — норма, а не риск.
pub fn is_vcs_package(name: &str) -> bool {
    ["-git", "-hg", "-svn", "-bzr", "-fossil", "-darcs", "-cvs", "-nightly"]
        .iter()
        .any(|s| name.ends_with(s))
}

/// Файлы из сети без контрольной суммы (у пакета не из свежего кода).
pub fn unverified_sources(info: &SrcInfo) -> Vec<String> {
    info.sources
        .iter()
        .filter(|s| s.vcs.is_empty() && s.remote() && !s.verified)
        .map(|s| s.url.clone())
        .collect()
}

/// Код из ветки, а не из закреплённой версии (у пакета не из свежего кода).
pub fn branch_sources(info: &SrcInfo) -> Vec<String> {
    if is_vcs_package(&info.pkgbase) {
        return Vec::new();
    }
    info.sources
        .iter()
        .filter(|s| !s.vcs.is_empty())
        .filter(|s| !s.fragment.starts_with("commit=") && !s.fragment.starts_with("tag="))
        .map(|s| s.raw.clone())
        .collect()
}

/// `владелец/репозиторий` на GitHub из адреса (страница, архив, выпуск,
/// raw/codeload). Регистр сохраняется, сравнение — без регистра.
pub fn github_repo_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .or_else(|| url.strip_prefix("git://"))
        .or_else(|| url.strip_prefix("ssh://git@"))?;
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let path = ["github.com/", "raw.githubusercontent.com/", "codeload.github.com/"]
        .iter()
        .find_map(|h| rest.strip_prefix(h))?;
    let mut parts = path.split(['/', '#', '?']);
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    let ok = |s: &str| {
        !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (ok(owner) && ok(repo)).then(|| format!("{owner}/{repo}"))
}

/// Репозитории GitHub, которые стоит проверить: сначала источники (из них
/// собирается пакет), потом страница проекта. Без повторов, не больше
/// [`MAX_GITHUB_REPOS`].
pub fn github_repos(src: Option<&SrcInfo>, upstream_url: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |r: Option<String>| {
        if let Some(r) = r {
            if !out.iter().any(|o| o.eq_ignore_ascii_case(&r)) && out.len() < MAX_GITHUB_REPOS {
                out.push(r);
            }
        }
    };
    if let Some(s) = src {
        for source in &s.sources {
            push(github_repo_of(&source.url));
        }
        push(github_repo_of(&s.url));
    }
    push(github_repo_of(upstream_url));
    out
}

// ── GitHub ────────────────────────────────────────────────────────────────

/// Правка сценариев GitHub Actions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkflowChange {
    pub sha: String,
    pub date: i64,
    pub author: String,
    pub message: String,
    /// Какие файлы сценариев правка ДОБАВИЛА (пусто — только меняла или не
    /// раскрывали).
    pub added: Vec<String>,
    /// Какие файлы сценариев правка УДАЛИЛА.
    pub removed: Vec<String>,
    /// Правку раскрыли (известно, какие файлы она добавила и удалила).
    pub detailed: bool,
    /// Те из `added`, которых в текущей версии репозитория уже нет
    /// (проверено). Неизвестно — сюда не попадает: осторожнее считать
    /// файл на месте.
    pub gone: Vec<String>,
}

impl WorkflowChange {
    /// Добавленные файлы, которые (насколько известно) всё ещё на месте.
    pub fn still_present(&self) -> Vec<&String> {
        self.added.iter().filter(|f| !self.gone.contains(f)).collect()
    }
}

/// Что известно про репозиторий на GitHub.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GithubFacts {
    /// `владелец/репозиторий`.
    pub repo: String,
    /// Паспорт получен.
    pub checked: bool,
    /// Репозитория нет (404).
    pub missing: bool,
    pub fork: bool,
    /// Откуда форк (`владелец/репозиторий`).
    pub parent: String,
    pub archived: bool,
    /// Ветка по умолчанию (её и считаем «текущей версией»).
    pub default_branch: String,
    /// Правки сценариев получены.
    pub workflows_checked: bool,
    /// Свежие правки сценариев не роботами, от новых к старым.
    pub workflow_changes: Vec<WorkflowChange>,
}

/// Разобрать паспорт репозитория.
pub fn parse_github_repo(repo: &str, json: &Value) -> GithubFacts {
    GithubFacts {
        repo: json
            .get("full_name")
            .and_then(Value::as_str)
            .unwrap_or(repo)
            .to_string(),
        checked: json.is_object() && json.get("full_name").is_some(),
        missing: false,
        fork: json.get("fork").and_then(Value::as_bool).unwrap_or(false),
        parent: json
            .pointer("/parent/full_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        archived: json.get("archived").and_then(Value::as_bool).unwrap_or(false),
        default_branch: json
            .get("default_branch")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        workflows_checked: false,
        workflow_changes: Vec::new(),
    }
}

/// Правка сделана роботом (обновление зависимостей и т. п.) — такие идут
/// потоком у любого живого проекта и шумом не считаются.
fn is_bot(login: &str, name: &str) -> bool {
    let l = login.to_ascii_lowercase();
    let n = name.to_ascii_lowercase();
    l.ends_with("[bot]") || n.ends_with("[bot]") || l == "dependabot" || l == "renovate-bot"
}

/// Свежие (за [`WORKFLOW_SECS`]) правки сценариев не роботами.
pub fn parse_workflow_commits(json: &Value, now: i64) -> Vec<WorkflowChange> {
    let Some(list) = json.as_array() else { return Vec::new() };
    let border = now - WORKFLOW_SECS;
    list.iter()
        .filter_map(|c| {
            let login = c.pointer("/author/login").and_then(Value::as_str).unwrap_or("");
            let name = c.pointer("/commit/author/name").and_then(Value::as_str).unwrap_or("");
            if is_bot(login, name) {
                return None;
            }
            // Дату автора подделать проще всего — берём позднюю из двух.
            let date = ["/commit/author/date", "/commit/committer/date"]
                .iter()
                .filter_map(|p| c.pointer(p).and_then(Value::as_str).and_then(parse_time))
                .max()?;
            if date < border {
                return None;
            }
            let message = c
                .pointer("/commit/message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            Some(WorkflowChange {
                sha: c.get("sha").and_then(Value::as_str).unwrap_or("").to_string(),
                date,
                author: if login.is_empty() { name.to_string() } else { login.to_string() },
                message,
                ..WorkflowChange::default()
            })
        })
        .collect()
}

/// Файлы сценариев правки с данным состоянием (`added`, `removed`).
fn workflow_files_with(json: &Value, status: &str) -> Vec<String> {
    json.get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|f| f.get("status").and_then(Value::as_str) == Some(status))
        .filter_map(|f| f.get("filename").and_then(Value::as_str))
        .filter(|n| n.starts_with(".github/workflows/"))
        .map(str::to_string)
        .collect()
}

/// Файлы сценариев, которые правка добавила.
pub fn parse_added_workflows(json: &Value) -> Vec<String> {
    workflow_files_with(json, "added")
}

/// Файлы сценариев, которые правка удалила. Переименование удалением НЕ
/// считается: содержимое осталось, только под другим именем.
pub fn parse_removed_workflows(json: &Value) -> Vec<String> {
    workflow_files_with(json, "removed")
}

/// Список каталога `.github/workflows` (ответ `contents`) → пути файлов.
/// `None` — пришло не то (не список).
pub fn parse_workflow_listing(json: &Value) -> Option<Vec<String>> {
    let list = json.as_array()?;
    Some(
        list.iter()
            .filter_map(|e| {
                e.get("path")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        let name = e.get("name").and_then(Value::as_str)?;
                        Some(format!(".github/workflows/{name}"))
                    })
            })
            .collect(),
    )
}

/// Добавленные файлы, которые удалила более поздняя правка. Правки — от
/// новых к старым; судим только по непрерывно раскрытым сверху правкам:
/// нераскрытая правка новее добавления могла файл вернуть. Ставит `gone`
/// и возвращает, остались ли добавленные файлы, чья судьба неизвестна.
pub fn resolve_gone_by_commits(changes: &mut [WorkflowChange]) -> bool {
    let mut unknown = false;
    for i in 0..changes.len() {
        if changes[i].added.is_empty() {
            continue;
        }
        let newer = &changes[..i];
        if !changes[i].detailed || newer.iter().any(|c| !c.detailed) {
            unknown = true;
            continue;
        }
        let mut gone = Vec::new();
        for file in &changes[i].added {
            // Самое новое событие с файлом решает: удалён или возвращён.
            let last = newer
                .iter()
                .find(|c| c.added.contains(file) || c.removed.contains(file));
            if last.is_some_and(|c| c.removed.contains(file)) {
                gone.push(file.clone());
            } else {
                unknown = true;
            }
        }
        changes[i].gone = gone;
    }
    unknown
}

/// Сверить добавленные файлы со списком сценариев текущей версии: чего в
/// списке нет — удалено. `listing = None` — каталога нет вовсе (404).
pub fn resolve_gone_by_listing(changes: &mut [WorkflowChange], listing: Option<&[String]>) {
    for c in changes.iter_mut() {
        c.gone = c
            .added
            .iter()
            .filter(|f| listing.is_none_or(|l| !l.iter().any(|p| p == *f)))
            .cloned()
            .collect();
    }
}

// ── Сопровождающий ───────────────────────────────────────────────────────

/// Сколько пакетов у сопровождающего и когда появился самый ранний.
pub fn parse_maintainer_packages(json: &Value) -> Option<(i64, i64)> {
    crate::aur::check_rpc_error(json).ok()?;
    let list = json.get("results")?.as_array()?;
    let first = list
        .iter()
        .filter_map(|r| r.get("FirstSubmitted").and_then(Value::as_i64))
        .filter(|t| *t > 0)
        .min()
        .unwrap_or(0);
    Some((list.len() as i64, first))
}

/// Пакет сопровождающего: из поиска и (если спрашивали) из паспорта.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaintPkg {
    pub name: String,
    pub first_submitted: i64,
    pub last_modified: i64,
    /// Кто выложил пакет впервые: `None` — паспорт не спрашивали, «» —
    /// учётки больше нет.
    pub submitter: Option<String>,
}

/// Пакеты сопровождающего из ответа `search by=maintainer`.
pub fn parse_maintainer_list(json: &Value) -> Option<Vec<MaintPkg>> {
    crate::aur::check_rpc_error(json).ok()?;
    let list = json.get("results")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|r| {
                Some(MaintPkg {
                    name: r.get("Name")?.as_str()?.to_string(),
                    first_submitted: r.get("FirstSubmitted").and_then(Value::as_i64).unwrap_or(0),
                    last_modified: r.get("LastModified").and_then(Value::as_i64).unwrap_or(0),
                    submitter: None,
                })
            })
            .collect(),
    )
}

/// Обновлён недавно, а существовал задолго до того — подобран, а не
/// только что выложен.
fn old_and_recently_updated(p: &MaintPkg, now: i64) -> bool {
    p.last_modified >= now - RECENT_OWNER_SECS
        && p.first_submitted > 0
        && p.first_submitted < p.last_modified - ADOPT_OLD_SECS
}

/// Самая плотная неделя обновлений: (начало, сколько в ней пакетов).
fn densest_week(times: &mut [i64]) -> (i64, usize) {
    times.sort_unstable();
    let (mut best, mut start, mut j) = (0usize, 0i64, 0usize);
    for i in 0..times.len() {
        while times[i] - times[j] > ADOPT_WINDOW_SECS {
            j += 1;
        }
        if i - j + 1 > best {
            best = i - j + 1;
            start = times[j];
        }
    }
    (start, best)
}

/// Чьи паспорта спросить, чтобы проверить массовую подборку. Пусто —
/// по одному поиску видно, что подборки нет (так у почти всех). Иначе —
/// пакеты той недели и самые давние (по ним видно, есть ли у учётки
/// свои старые пакеты), не больше [`MAX_INFO_NAMES`], по алфавиту.
pub fn adoption_info_names(list: &[MaintPkg], now: i64) -> Vec<String> {
    let mut times: Vec<i64> = list
        .iter()
        .filter(|p| old_and_recently_updated(p, now))
        .map(|p| p.last_modified)
        .collect();
    let (start, count) = densest_week(&mut times);
    if count < MASS_ADOPT {
        return Vec::new();
    }
    let in_week = |p: &MaintPkg| {
        old_and_recently_updated(p, now) && p.last_modified >= start && p.last_modified - start <= ADOPT_WINDOW_SECS
    };
    let mut names: Vec<String> = list.iter().filter(|p| in_week(p)).map(|p| p.name.clone()).collect();
    let mut rest: Vec<&MaintPkg> = list.iter().filter(|p| !in_week(p)).collect();
    rest.sort_by_key(|p| p.first_submitted);
    for p in rest {
        if names.len() >= MAX_INFO_NAMES {
            break;
        }
        names.push(p.name.clone());
    }
    names.truncate(MAX_INFO_NAMES);
    names.sort();
    names
}

/// Вписать, кто выложил пакеты впервые, из ответа `info`.
pub fn fill_submitters(list: &mut [MaintPkg], info: &Value) {
    for p in list.iter_mut() {
        if let Some(i) = crate::aur::parse_info(info, &p.name) {
            p.submitter = Some(i.submitter);
        }
    }
}

/// Новая учётка массово подобрала чужие старые пакеты.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MassAdoption {
    /// Сколько чужих старых пакетов обновлено за ту неделю.
    pub count: i64,
    /// Начало недели (первое обновление).
    pub since: i64,
}

/// Массовая подборка: не меньше [`MASS_ADOPT`] пакетов, выложенных
/// КОГДА-ТО другими (паспорт получен), обновлены за одну неделю в
/// последние полгода, и у учётки нет своих пакетов, выложенных больше
/// чем за полгода до этой недели (дату регистрации AUR гостю не
/// показывает — это её замена).
pub fn mass_adoption(maintainer: &str, list: &[MaintPkg], now: i64) -> Option<MassAdoption> {
    let me = maintainer.trim().to_lowercase();
    let adopted = |p: &&MaintPkg| {
        p.submitter.as_deref().is_some_and(|s| s.trim().to_lowercase() != me) && old_and_recently_updated(p, now)
    };
    let mut times: Vec<i64> = list.iter().filter(adopted).map(|p| p.last_modified).collect();
    let (since, count) = densest_week(&mut times);
    if count < MASS_ADOPT {
        return None;
    }
    let veteran = list.iter().any(|p| {
        p.submitter.as_deref().is_some_and(|s| s.trim().to_lowercase() == me)
            && p.first_submitted > 0
            && p.first_submitted < since - NEW_MAINTAINER_SECS
    });
    (!veteran).then_some(MassAdoption { count: count as i64, since })
}

/// Чьи паспорта спросить, чтобы узнать возраст учётки по СВОИМ пакетам.
/// Пусто — по одному поиску всё ясно (так у почти всех): либо у учётки
/// есть пакет, не обновлявшийся больше полугода (она точно не новая —
/// свой он или подобранный, она держит его с тех пор), либо все её пакеты
/// выложены недавно (новая при любом раскладе). Иначе — старые пакеты,
/// которые могли быть подобраны, от самых давних, не больше
/// [`MAX_INFO_NAMES`], по алфавиту.
pub fn account_age_info_names(list: &[MaintPkg], now: i64) -> Vec<String> {
    let border = now - NEW_MAINTAINER_SECS;
    if list.iter().any(|p| p.last_modified > 0 && p.last_modified < border) {
        return Vec::new();
    }
    let mut old: Vec<&MaintPkg> = list.iter().filter(|p| p.first_submitted > 0 && p.first_submitted < border).collect();
    old.sort_by_key(|p| p.first_submitted);
    let mut names: Vec<String> = old.iter().take(MAX_INFO_NAMES).map(|p| p.name.clone()).collect();
    names.sort();
    names
}

/// Паспорта для обоих признаков учётки — одним запросом: сначала нужные
/// массовой подборке, потом возрасту, не больше [`MAX_INFO_NAMES`].
pub fn trust_info_names(list: &[MaintPkg], now: i64) -> Vec<String> {
    let mut names = adoption_info_names(list, now);
    for n in account_age_info_names(list, now) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    names.truncate(MAX_INFO_NAMES);
    names.sort();
    names
}

/// С каких пор учётка в AUR: самый ранний пакет, который она выложила
/// сама. Подобранный чужой пакет говорит не о дате его выкладки, а только
/// о последнем обновлении (раньше него учётка его не держала наверняка).
/// Пакет без паспорта — по дате выкладки, как раньше.
pub fn account_since(maintainer: &str, list: &[MaintPkg]) -> Option<i64> {
    let me = maintainer.trim().to_lowercase();
    list.iter()
        .map(|p| match p.submitter.as_deref() {
            Some(s) if s.trim().to_lowercase() != me => p.last_modified,
            _ => p.first_submitted,
        })
        .filter(|t| *t > 0)
        .min()
}

// ── Итог глубокой проверки ───────────────────────────────────────────────

/// Всё, что узнала глубокая проверка (лежит на диске сутки).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrustFacts {
    pub name: String,
    pub checked_at: i64,
    /// Часть проверок не удалась (сеть, защита сайта, лимит GitHub).
    pub partial: bool,
    pub maintainer: String,
    /// Пакетов у сопровождающего (`None` — не узнали).
    pub maintainer_packages: Option<i64>,
    /// С каких пор учётка в AUR (epoch): самый ранний СВОЙ пакет, см.
    /// [`account_since`]; без паспортов — самый ранний из всех.
    pub maintainer_since: Option<i64>,
    /// Учётка сопровождающего массово подобрала чужие старые пакеты.
    pub mass_adoption: Option<MassAdoption>,
    pub log_checked: bool,
    pub new_author: Option<NewAuthor>,
    /// Дата самой новой правки в журнале git (дата коммитера).
    pub tip_date: Option<i64>,
    /// «Обновлён» по RPC на момент проверки.
    pub last_modified: i64,
    /// Страница пакета прочитана.
    pub page_checked: bool,
    /// Кто выложил последнюю версию.
    pub last_packager: String,
    /// …и это сотрудник Arch, а не сопровождающий и не его помощник.
    pub staff_packager: bool,
    pub srcinfo_checked: bool,
    pub unverified: Vec<String>,
    pub branch_sources: Vec<String>,
    pub github: Vec<GithubFacts>,
}

impl TrustFacts {
    /// Свежа ли проверка.
    pub fn fresh(&self, now: i64) -> bool {
        let ttl = if self.partial { PARTIAL_TTL_SECS } else { CACHE_TTL_SECS };
        self.checked_at > 0 && now >= self.checked_at && now - self.checked_at < ttl
    }
}

/// Кто сопровождал пакет при открытии карточки.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaintainerSeen {
    pub name: String,
    pub first_seen: i64,
    pub last_seen: i64,
}

/// Запись файла кеша о пакете.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheEntry {
    pub facts: Option<TrustFacts>,
    /// История сопровождающих по нашим снимкам, от старых к новым.
    pub maintainers: Vec<MaintainerSeen>,
}

/// Файл кеша целиком.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrustCache {
    pub version: u32,
    pub packages: BTreeMap<String, CacheEntry>,
    /// Сотрудники Arch — общий для всех пакетов список.
    pub staff: StaffList,
}

/// Смена сопровождающего, замеченная нашими снимками.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaintainerChange {
    pub from: String,
    pub to: String,
    pub at: i64,
}

impl CacheEntry {
    /// Запомнить, кто сопровождает пакет сейчас («» — никто).
    pub fn record_maintainer(&mut self, name: &str, now: i64) {
        match self.maintainers.last_mut() {
            Some(last) if last.name == name => last.last_seen = now,
            _ => self.maintainers.push(MaintainerSeen {
                name: name.to_string(),
                first_seen: now,
                last_seen: now,
            }),
        }
        let extra = self.maintainers.len().saturating_sub(5);
        self.maintainers.drain(..extra);
    }

    /// Сопровождающий сменился между открытиями карточки за последние
    /// полгода. Пакет, брошенный прежним («» → имя), — тоже смена.
    pub fn maintainer_change(&self, now: i64) -> Option<MaintainerChange> {
        let n = self.maintainers.len();
        if n < 2 {
            return None;
        }
        let (prev, last) = (&self.maintainers[n - 2], &self.maintainers[n - 1]);
        if last.name.is_empty() || now - last.first_seen > RECENT_OWNER_SECS {
            return None;
        }
        Some(MaintainerChange { from: prev.name.clone(), to: last.name.clone(), at: last.first_seen })
    }
}

impl TrustCache {
    /// Выбросить устаревшие записи.
    pub fn prune(&mut self, now: i64) {
        self.packages.retain(|_, e| {
            let fact = e.facts.as_ref().map(|f| f.checked_at).unwrap_or(0);
            let seen = e.maintainers.last().map(|m| m.last_seen).unwrap_or(0);
            now - fact.max(seen) < KEEP_SECS
        });
    }
}

/// Путь файла кеша.
pub fn cache_path(xdg_cache_home: Option<&str>, home: &str) -> String {
    format!("{}/rexon/{CACHE_FILE}", crate::utils::cache_home(xdg_cache_home, home))
}

/// Путь файла кеша этого процесса.
pub fn live_cache_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    cache_path(std::env::var("XDG_CACHE_HOME").ok().as_deref(), &home)
}

/// Прочитать кеш; нет файла, битый, чужая версия — пустой.
pub fn load_cache(path: &str) -> TrustCache {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<TrustCache>(&t).ok())
        .filter(|c| c.version == CACHE_VERSION)
        .unwrap_or_else(|| TrustCache { version: CACHE_VERSION, ..TrustCache::default() })
}

/// Записать кеш (через временный файл).
pub fn save_cache(path: &str, cache: &TrustCache) -> Result<(), String> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string(cache).map_err(|e| e.to_string())?;
    crate::utils::atomic_write_str_err(path, &text)
}

/// Кеш в памяти процесса: файл читается один раз, плитки спрашивают
/// отсюда без диска.
static MEMORY: Mutex<Option<(String, TrustCache)>> = Mutex::new(None);

/// Запись о пакете из кеша (память, при первом обращении — файл).
pub fn cached_entry(path: &str, name: &str) -> Option<CacheEntry> {
    let mut guard = MEMORY.lock().ok()?;
    if guard.as_ref().map(|(p, _)| p != path).unwrap_or(true) {
        *guard = Some((path.to_string(), load_cache(path)));
    }
    guard.as_ref()?.1.packages.get(name).cloned()
}

/// Сотрудники Arch из кеша (память, при первом обращении — файл).
pub fn cached_staff(path: &str) -> StaffList {
    let Ok(mut guard) = MEMORY.lock() else { return StaffList::default() };
    if guard.as_ref().map(|(p, _)| p != path).unwrap_or(true) {
        *guard = Some((path.to_string(), load_cache(path)));
    }
    guard.as_ref().map(|(_, c)| c.staff.clone()).unwrap_or_default()
}

/// Поправить файл кеша целиком: в памяти и на диске.
fn with_cache<R>(path: &str, now: i64, f: impl FnOnce(&mut TrustCache) -> R) -> Option<R> {
    let mut guard = MEMORY.lock().ok()?;
    if guard.as_ref().map(|(p, _)| p != path).unwrap_or(true) {
        *guard = Some((path.to_string(), load_cache(path)));
    }
    let (_, cache) = guard.as_mut().expect("кеш только что загружен");
    cache.version = CACHE_VERSION;
    let out = f(cache);
    cache.prune(now);
    if let Err(e) = save_cache(path, cache) {
        crate::log_warn!("aur", "кеш проверки источника не записан: {e}");
    }
    Some(out)
}

/// Обновить запись о пакете в памяти и на диске.
pub fn update_entry(path: &str, name: &str, now: i64, f: impl FnOnce(&mut CacheEntry)) -> CacheEntry {
    let mut f = Some(f);
    let out = with_cache(path, now, |cache| {
        let entry = cache.packages.entry(name.to_string()).or_default();
        if let Some(f) = f.take() {
            f(entry);
        }
        entry.clone()
    });
    out.unwrap_or_else(|| {
        let mut e = CacheEntry::default();
        if let Some(f) = f.take() {
            f(&mut e);
        }
        e
    })
}

// ── Сеть ──────────────────────────────────────────────────────────────────

/// Чем ходить за данными: AUR (RPC и страницы), GitHub (API с его
/// заголовками) и пауза перед обращением к страницам AUR.
pub struct Fetchers<'a> {
    pub aur: &'a mut dyn NetIo,
    pub github: &'a mut dyn NetIo,
    pub pause: &'a mut dyn FnMut(),
}

fn get_text(net: &mut dyn NetIo, url: &str, ua: &str) -> Result<String, NetError> {
    net.get(url, ua, TIMEOUT_SECS).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// То же для AUR: одиночный обрыв повторяется (сайт временами не отвечает
/// с первого раза). GitHub не повторяем — каждая попытка съедает лимит.
fn get_aur(net: &mut dyn NetIo, url: &str) -> Result<String, NetError> {
    let what = format!("AUR {url}");
    crate::net::with_retry(net, crate::net::Retry::IDEMPOTENT, &what, |io| {
        io.get(url, crate::aur::USER_AGENT, TIMEOUT_SECS)
    })
    .map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Лимит GitHub исчерпан (или доступ закрыт) — дальше к нему не ходим.
fn github_blocked(e: &NetError) -> bool {
    matches!(e, NetError::Status { code: 403 | 429, .. })
}

/// Список сотрудников Arch с archlinux.org. `None` — хоть одна из страниц
/// не пришла: неполный список хуже никакого (кого-то не узнаем).
pub fn fetch_staff(net: &mut dyn NetIo, now: i64) -> Option<StaffList> {
    let mut names: Vec<String> = Vec::new();
    for url in STAFF_PAGES {
        for n in get_aur(net, url).ok().as_deref().and_then(parse_staff_page)? {
            if !names.contains(&n) {
                names.push(n);
            }
        }
    }
    (!names.is_empty()).then_some(StaffList { names, checked_at: now })
}

/// Глубокая проверка пакета по сети без кеша списка сотрудников Arch.
pub fn collect_facts(f: &mut Fetchers, info: &AurInfo, now: i64) -> TrustFacts {
    collect_facts_with(f, info, now, &mut StaffList::default())
}

/// Глубокая проверка пакета по сети. Каждая часть, которая не удалась,
/// просто пропускается (и проверка помечается неполной). `staff` — список
/// сотрудников Arch: несвежий обновляется, только если он понадобился.
pub fn collect_facts_with(f: &mut Fetchers, info: &AurInfo, now: i64, staff: &mut StaffList) -> TrustFacts {
    let base = if info.package_base.is_empty() { &info.name } else { &info.package_base };
    let mut facts = TrustFacts {
        name: info.name.clone(),
        checked_at: now,
        maintainer: info.maintainer.clone(),
        last_modified: info.last_modified,
        ..TrustFacts::default()
    };

    // Сопровождающий: сколько пакетов и с каких пор.
    if !info.maintainer.is_empty() {
        let answer = get_aur(f.aur, &maintainer_url(&info.maintainer))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        match answer.as_ref().and_then(parse_maintainer_packages) {
            Some((count, first)) => {
                facts.maintainer_packages = Some(count);
                facts.maintainer_since = (first > 0).then_some(first);
            }
            None => facts.partial = true,
        }
        // Паспорта пакетов (кто их выложил) спрашиваем, только если по
        // поиску видна неделя, когда обновили много старых пакетов, или
        // возраст учётки под сомнением: старые пакеты могли быть подобраны.
        if let Some(mut list) = answer.as_ref().and_then(parse_maintainer_list) {
            let names = trust_info_names(&list, now);
            if !names.is_empty() {
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                match get_aur(f.aur, &crate::aur::info_url(&refs))
                    .ok()
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                    .filter(|v| crate::aur::check_rpc_error(v).is_ok())
                {
                    Some(v) => {
                        fill_submitters(&mut list, &v);
                        facts.mass_adoption = mass_adoption(&info.maintainer, &list, now);
                        facts.maintainer_since = account_since(&info.maintainer, &list);
                    }
                    None => facts.partial = true,
                }
            }
        }
    }

    // Журнал правок.
    (f.pause)();
    match get_aur(f.aur, &cgit_log_url(base)).ok().as_deref().and_then(parse_cgit_log) {
        Some(log) => {
            facts.log_checked = true;
            facts.new_author = new_author(&log, now);
            facts.tip_date = log.iter().map(|e| e.date).max();
        }
        None => facts.partial = true,
    }

    // Кто выложил последнюю версию (страница пакета). Список сотрудников
    // Arch нужен, только если это не сопровождающий и не его помощник.
    (f.pause)();
    match get_aur(f.aur, &package_page_url(&info.name)).ok().as_deref().and_then(parse_package_page) {
        Some(page) => {
            facts.page_checked = true;
            facts.last_packager = page.last_packager.clone();
            if packager_is_outsider(&page, &info.maintainer) {
                if !staff.fresh(now) {
                    if let Some(s) = fetch_staff(f.aur, now) {
                        *staff = s;
                    }
                }
                if staff.fresh(now) {
                    facts.staff_packager = staff.contains(&page.last_packager);
                } else {
                    facts.partial = true;
                }
            }
        }
        None => facts.partial = true,
    }

    // Источники.
    (f.pause)();
    let src = get_aur(f.aur, &srcinfo_url(base)).ok().as_deref().and_then(parse_srcinfo);
    match &src {
        Some(s) => {
            facts.srcinfo_checked = true;
            facts.unverified = unverified_sources(s);
            facts.branch_sources = branch_sources(s);
        }
        None => facts.partial = true,
    }

    // GitHub.
    let gh_ua = GH_USER_AGENT;
    let mut blocked = false;
    for repo in github_repos(src.as_ref(), &info.url) {
        if blocked {
            facts.partial = true;
            break;
        }
        let mut gh = match get_text(f.github, &github_repo_url(&repo), gh_ua) {
            Ok(t) => match serde_json::from_str::<Value>(&t) {
                Ok(v) => parse_github_repo(&repo, &v),
                Err(_) => {
                    facts.partial = true;
                    continue;
                }
            },
            Err(NetError::Status { code: 404, .. }) => {
                facts.github.push(GithubFacts {
                    repo: repo.clone(),
                    checked: true,
                    missing: true,
                    ..GithubFacts::default()
                });
                continue;
            }
            Err(e) => {
                blocked = github_blocked(&e);
                facts.partial = true;
                continue;
            }
        };
        // Паспорт и список правок сценариев.
        let mut used = 2usize;
        match get_text(f.github, &github_workflows_url(&gh.repo, now - WORKFLOW_SECS), gh_ua) {
            Ok(t) => {
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    gh.workflows_checked = v.is_array();
                    gh.workflow_changes = parse_workflow_commits(&v, now);
                }
            }
            Err(e) => {
                blocked = github_blocked(&e);
                facts.partial = true;
            }
        }
        for change in gh.workflow_changes.iter_mut().take(MAX_COMMIT_DETAILS) {
            if blocked || used >= MAX_GITHUB_PER_REPO {
                break;
            }
            used += 1;
            match get_text(f.github, &github_commit_url(&gh.repo, &change.sha), gh_ua) {
                Ok(t) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&t) {
                        change.added = parse_added_workflows(&v);
                        change.removed = parse_removed_workflows(&v);
                        change.detailed = v.get("files").is_some_and(Value::is_array);
                    }
                }
                Err(e) => {
                    // Правка не раскрыта — проверка неполная (повтор через час).
                    blocked = github_blocked(&e);
                    facts.partial = true;
                }
            }
        }
        // Новый файл сценария ещё на месте? Сначала по уже раскрытым правкам
        // (бесплатно), потом, если осталось место, — списком каталога в
        // текущей версии. Не узнали — файл считается на месте (красный).
        let unknown = resolve_gone_by_commits(&mut gh.workflow_changes);
        if unknown && !blocked && used < MAX_GITHUB_PER_REPO {
            match get_text(f.github, &github_workflow_dir_url(&gh.repo, &gh.default_branch), gh_ua) {
                Ok(t) => {
                    let listing = serde_json::from_str::<Value>(&t)
                        .ok()
                        .and_then(|v| parse_workflow_listing(&v));
                    match listing {
                        Some(l) => resolve_gone_by_listing(&mut gh.workflow_changes, Some(&l)),
                        None => facts.partial = true,
                    }
                }
                Err(NetError::Status { code: 404, .. }) => {
                    resolve_gone_by_listing(&mut gh.workflow_changes, None);
                }
                Err(e) => {
                    blocked = github_blocked(&e);
                    facts.partial = true;
                }
            }
        }
        facts.github.push(gh);
    }
    facts
}

/// Проверка с кешем: свежая запись с диска либо сбор по сети; снимок
/// сопровождающего пишется всегда. Возвращает запись целиком.
pub fn check_cached(path: &str, f: &mut Fetchers, info: &AurInfo, now: i64) -> CacheEntry {
    let cached = cached_entry(path, &info.name);
    let fresh = cached
        .as_ref()
        .and_then(|e| e.facts.as_ref())
        // Новая выкладка — проверка с диска не годится.
        .filter(|fa| {
            fa.fresh(now) && fa.maintainer == info.maintainer && fa.last_modified == info.last_modified
        })
        .cloned();
    let mut staff = cached_staff(path);
    let facts = fresh.unwrap_or_else(|| collect_facts_with(f, info, now, &mut staff));
    let update = |e: &mut CacheEntry| {
        e.record_maintainer(&info.maintainer, now);
        e.facts = Some(facts.clone());
    };
    let out = with_cache(path, now, |cache| {
        if staff.fresh(now) {
            cache.staff = staff.clone();
        }
        let entry = cache.packages.entry(info.name.clone()).or_default();
        update(entry);
        entry.clone()
    });
    out.unwrap_or_else(|| {
        let mut e = CacheEntry::default();
        update(&mut e);
        e
    })
}

/// Текст PKGBUILD. Ошибка — словами для человека.
pub fn fetch_pkgbuild(net: &mut dyn NetIo, pkgbase: &str) -> Result<String, String> {
    let text = get_aur(net, &pkgbuild_url(pkgbase))
        .map_err(|e| trf("Couldn't load PKGBUILD: %1", &[&e]))?;
    let head = text.trim_start().get(..200).unwrap_or(text.trim_start()).to_ascii_lowercase();
    if head.starts_with("<!doctype") || head.starts_with("<html") {
        return Err(tr("AUR didn't let us read the PKGBUILD here (bot protection) — it is on the website."));
    }
    Ok(text)
}

// ── Признаки ──────────────────────────────────────────────────────────────

/// Насколько серьёзен признак.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Жёлтый: стоит присмотреться.
    Warn,
    /// Красный: похоже на то, как выглядят атаки.
    Danger,
}

impl Level {
    pub fn key(self) -> &'static str {
        match self {
            Level::Warn => "warn",
            Level::Danger => "danger",
        }
    }
}

/// Один признак риска: что не так и почему это важно.
#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    /// Машинный ключ (для проверок и порядка).
    pub key: &'static str,
    pub level: Level,
    pub text: String,
    pub why: String,
}

/// Что известно о пакете без глубокой проверки — плитка и паспорт RPC.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RpcView {
    pub maintainer: String,
    pub votes: i64,
    pub popularity: f64,
    /// Epoch пометки «устарел»; 1 — помечен, дата неизвестна; 0 — нет.
    pub out_of_date: i64,
}

impl From<&AurInfo> for RpcView {
    fn from(i: &AurInfo) -> Self {
        RpcView {
            maintainer: i.maintainer.clone(),
            votes: i.votes,
            popularity: i.popularity,
            out_of_date: i.out_of_date,
        }
    }
}

impl From<&AurPackage> for RpcView {
    fn from(p: &AurPackage) -> Self {
        RpcView {
            maintainer: p.maintainer.clone(),
            votes: p.votes,
            popularity: p.popularity,
            out_of_date: i64::from(p.out_of_date),
        }
    }
}

fn why_owner() -> String {
    tr("Whoever maintains a package decides what gets built on your computer. Attackers take over abandoned packages and then publish a harmful update.")
}

fn why_workflows() -> String {
    tr("This is how the attack of 9 October 2026 looked: a stolen GitHub account got a new automation script added to all its projects, and AUR packages were built from them. Such changes are usually harmless, but check who made them and why.")
}

/// Признаки по данным RPC (то, что видно и на плитке).
pub fn rpc_signals(v: &RpcView) -> Vec<Signal> {
    let mut out = Vec::new();
    if v.maintainer.is_empty() {
        out.push(Signal {
            key: "orphaned",
            level: Level::Warn,
            text: tr("Nobody maintains the package (orphaned)"),
            why: tr("Updates and fixes don't come, and anyone can take the package over and change it."),
        });
    }
    if v.out_of_date > 0 {
        let text = if v.out_of_date > 1 {
            trf("Flagged out-of-date since %1", &[&day(v.out_of_date)])
        } else {
            tr("Flagged out-of-date")
        };
        out.push(Signal {
            key: "out_of_date",
            level: Level::Warn,
            text,
            why: tr("Users reported that a newer version exists. The package may fail to build or carry known bugs."),
        });
    }
    if v.votes < FEW_VOTES || (v.votes < LOW_POP_VOTES && v.popularity < LOW_POPULARITY) {
        out.push(Signal {
            key: "few_votes",
            level: Level::Warn,
            text: fill(
                &ntr(
                    "Few people use it: %n vote, popularity %1",
                    "Few people use it: %n votes, popularity %1",
                    v.votes.max(0) as u64,
                ),
                &[&format!("{:.2}", v.popularity)],
            ),
            why: tr("Votes show how many people tried the package. With few of them, maybe nobody besides the author has looked at what it does."),
        });
    }
    out
}

/// AUR пишет «обновлён» заметно позже самой новой правки в git пакета.
pub fn history_rewritten(facts: &TrustFacts) -> bool {
    let Some(tip) = facts.tip_date else { return false };
    let lm = facts.last_modified;
    lm > 0 && lm - tip > HISTORY_GAP_SECS && facts.checked_at - lm >= CGIT_LAG_SECS
}

/// Признаки глубокой проверки.
pub fn deep_signals(facts: &TrustFacts, change: Option<&MaintainerChange>, now: i64) -> Vec<Signal> {
    let mut out = Vec::new();
    if let Some(c) = change {
        let text = if c.from.is_empty() {
            trf("An orphaned package was taken over by %1 (noticed %2)", &[&c.to, &day(c.at)])
        } else {
            trf("The maintainer changed: was %1, now %2 (noticed %3)", &[&c.from, &c.to, &day(c.at)])
        };
        out.push(Signal { key: "maintainer_changed", level: Level::Danger, text, why: why_owner() });
    }
    if let Some(a) = &facts.new_author {
        out.push(Signal {
            key: "new_author",
            level: Level::Warn,
            text: trf(
                "Since %1 the package is edited by someone new (%2); before that it was %3",
                &[&day(a.since), &a.name, &a.before],
            ),
            why: why_owner(),
        });
    }
    if let Some(count) = facts.maintainer_packages {
        let new = facts
            .maintainer_since
            .is_some_and(|t| now - t < NEW_MAINTAINER_SECS);
        if new || count <= FEW_PACKAGES {
            let text = match (new, facts.maintainer_since) {
                (true, Some(t)) => fill(
                    &ntr(
                        "New maintainer: %n package in AUR, the first appeared %1",
                        "New maintainer: %n packages in AUR, the first appeared %1",
                        count.max(0) as u64,
                    ),
                    &[&day(t)],
                ),
                _ => ntr(
                    "The maintainer has only %n package in AUR",
                    "The maintainer has only %n packages in AUR",
                    count.max(0) as u64,
                ),
            };
            out.push(Signal {
                key: "new_maintainer",
                level: Level::Warn,
                text,
                why: tr("An experienced maintainer with many packages is hard to fake; fresh accounts are what attackers use."),
            });
        }
    }
    let rewritten = history_rewritten(facts);
    if facts.staff_packager && rewritten {
        // Сотрудник Arch выложил версию, и история переписана: так Arch
        // вычищает взломанный пакет.
        out.push(Signal {
            key: "compromised_cleaned",
            level: Level::Danger,
            text: trf(
                "The package was apparently broken into and cleaned up: Arch staff member %1 rewrote its history on %2",
                &[&facts.last_packager, &day(facts.last_modified)],
            ),
            why: tr("Only Arch staff can rewrite the history of an AUR package — this is how a harmful change is removed. A package that was broken into once stays questionable after the cleanup: check who maintains it now and read the comments on the package page before installing."),
        });
    } else if rewritten {
        out.push(Signal {
            key: "history_rewritten",
            level: Level::Warn,
            text: trf(
                "AUR says the package was updated on %1, but the newest change in its history is from %2",
                &[&day(facts.last_modified), &day(facts.tip_date.unwrap_or_default())],
            ),
            why: tr("This is how it looks when the history of a package was rewritten (only Arch staff can do this, when removing a harmful change) or a change was uploaded with a fake date. More rarely, a change was simply uploaded days after it was made."),
        });
    } else if facts.staff_packager {
        out.push(Signal {
            key: "staff_packager",
            level: Level::Warn,
            text: trf(
                "The latest version was uploaded by Arch staff member %1, not by the maintainer",
                &[&facts.last_packager],
            ),
            why: tr("Arch staff upload into someone else's package mostly to clean it up after a break-in: the harmful change is removed, but the package may still belong to the account that made it. It can also be harmless (staff fixed the package or handed it over) — read the latest comments on the package page."),
        });
    }
    if let Some(m) = &facts.mass_adoption {
        out.push(Signal {
            key: "mass_adoption",
            level: Level::Warn,
            text: fill(
                &ntr(
                    "Account %1 has no old packages of its own, but in one week (from %2) updated %n old package taken over from others",
                    "Account %1 has no old packages of its own, but in one week (from %2) updated %n old packages taken over from others",
                    m.count.max(0) as u64,
                ),
                &[&facts.maintainer, &day(m.since)],
            ),
            why: tr("This is how packages get hijacked: a fresh account takes over many abandoned packages nobody watches at once and publishes a harmful update in them — a dozen AUR packages were taken over this way in June 2026. AUR doesn't show guests when an account was registered, so Rexon judges by the account having no old packages of its own."),
        });
    }
    for gh in &facts.github {
        if gh.missing {
            out.push(Signal {
                key: "repo_missing",
                level: Level::Warn,
                text: trf("The source repository %1 no longer exists on GitHub", &[&gh.repo]),
                why: tr("The package can't be built as it is, or the code has moved — check where it comes from now."),
            });
            continue;
        }
        if gh.fork {
            let text = if gh.parent.is_empty() {
                trf("The source %1 is a fork on GitHub, not the original project", &[&gh.repo])
            } else {
                trf("The source %1 is a fork of %2 on GitHub, not the original project", &[&gh.repo, &gh.parent])
            };
            out.push(Signal {
                key: "fork",
                level: Level::Warn,
                text,
                why: tr("A fork is someone's copy of a project. It may be fine, but you trust that person, not the authors of the original."),
            });
        }
        let short = |f: &String| f.rsplit('/').next().unwrap_or(f).to_string();
        let present = gh.workflow_changes.iter().find(|c| !c.still_present().is_empty());
        let removed = gh.workflow_changes.iter().find(|c| !c.added.is_empty());
        if let Some(c) = present {
            let files: Vec<String> = c.still_present().into_iter().map(short).collect();
            out.push(Signal {
                key: "workflow_added",
                level: Level::Danger,
                text: trf(
                    "A new GitHub automation script (%1) was added to %2 on %3 by %4",
                    &[&files.join(", "), &gh.repo, &day(c.date), &c.author],
                ),
                why: why_workflows(),
            });
        } else if let Some(c) = removed {
            // Все недавно добавленные файлы уже удалены: жёлтый, и сочетание
            // с форком или новым человеком его НЕ поднимает до красного.
            let files: Vec<String> = c.added.iter().map(short).collect();
            out.push(Signal {
                key: "workflow_added_removed",
                level: Level::Warn,
                text: trf(
                    "A GitHub automation script (%1) added to %2 on %3 by %4 has already been removed",
                    &[&files.join(", "), &gh.repo, &day(c.date), &c.author],
                ),
                why: tr("The file is gone now, but since it appeared, someone was able to change the repository: it was compromised, or someone experimented in it. Look at the latest changes of the source before installing."),
            });
        } else if let Some(c) = gh.workflow_changes.first() {
            out.push(Signal {
                key: "workflow_changed",
                level: Level::Warn,
                text: trf(
                    "GitHub automation scripts in %1 were changed recently (%2, by %3)",
                    &[&gh.repo, &day(c.date), &c.author],
                ),
                why: why_workflows(),
            });
        }
    }
    if !facts.unverified.is_empty() {
        out.push(Signal {
            key: "unverified",
            level: Level::Warn,
            text: trf("A downloaded file isn't checked against a checksum: %1", &[&facts.unverified.join(", ")]),
            why: tr("Without a checksum, whatever the server returns on the day gets built — including a replaced file."),
        });
    }
    if !facts.branch_sources.is_empty() {
        out.push(Signal {
            key: "branch",
            level: Level::Warn,
            text: trf("Built from a branch, not from a fixed version: %1", &[&facts.branch_sources.join(", ")]),
            why: tr("The code is taken as it is at the moment of building: a change upstream gets in without the package being updated."),
        });
    }
    out
}

/// Все признаки: глубокие (если были) и по RPC, красные первыми.
///
/// Свежая правка сценариев GitHub вместе с форком, новым человеком или
/// новым сопровождающим — уже красный: так и выглядит захват. Добавленный
/// и уже удалённый сценарий (`workflow_added_removed`) так не поднимается.
pub fn signals(rpc: &RpcView, entry: Option<&CacheEntry>, now: i64) -> Vec<Signal> {
    let mut out = Vec::new();
    if let Some(e) = entry {
        let change = e.maintainer_change(now);
        if let Some(f) = &e.facts {
            out.extend(deep_signals(f, change.as_ref(), now));
        } else if let Some(c) = change {
            out.extend(deep_signals(&TrustFacts::default(), Some(&c), now));
        }
    }
    out.extend(rpc_signals(rpc));
    let suspicious_owner = out.iter().any(|s| {
        matches!(s.key, "fork" | "new_author" | "new_maintainer" | "maintainer_changed" | "mass_adoption")
    });
    if suspicious_owner {
        for s in out.iter_mut().filter(|s| s.key == "workflow_changed") {
            s.level = Level::Danger;
        }
    }
    // Массовая подборка вместе с вычисткой, переписанной историей или
    // сменой рук — уже красный: так и выглядит захват.
    let tampered = out.iter().any(|s| {
        matches!(
            s.key,
            "compromised_cleaned" | "history_rewritten" | "staff_packager" | "maintainer_changed" | "new_author"
        )
    });
    if tampered {
        for s in out.iter_mut().filter(|s| s.key == "mass_adoption") {
            s.level = Level::Danger;
        }
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.level));
    out
}

/// Общий уровень: самый серьёзный из признаков.
pub fn level(signals: &[Signal]) -> Option<Level> {
    signals.iter().map(|s| s.level).max()
}

/// Значок рядом с «Установить»: «⚠ 3 признака риска».
pub fn badge_text(signals: &[Signal]) -> String {
    let glyph = match level(signals) {
        Some(Level::Danger) => "⛔",
        Some(Level::Warn) => "⚠️",
        None => return String::new(),
    };
    format!(
        "{glyph} {}",
        ntr("%n sign of risk", "%n signs of risk", signals.len() as u64)
    )
}

/// Краткий список для подсказки значка.
pub fn badge_tooltip(signals: &[Signal]) -> String {
    let mut lines: Vec<String> = signals.iter().map(|s| format!("• {}", s.text)).collect();
    if !lines.is_empty() {
        lines.push(String::new());
        lines.push(tr("Installation isn't blocked: this is a hint. Details are in “Source check” below."));
    }
    lines.join("\n")
}

/// Значок перед строкой источника на плитке (пусто — признаков нет).
pub fn tile_mark(signals: &[Signal]) -> &'static str {
    match level(signals) {
        Some(Level::Danger) => "⛔ ",
        Some(Level::Warn) => "⚠️ ",
        None => "",
    }
}

/// Подсказка значка на плитке.
pub fn tile_tooltip(signals: &[Signal], deep_known: bool) -> String {
    if signals.is_empty() {
        return String::new();
    }
    let mut lines = vec![tr("Signs of risk:")];
    lines.extend(signals.iter().map(|s| format!("• {}", s.text)));
    if !deep_known {
        lines.push(tr("Open the package card to check the source."));
    }
    lines.join("\n")
}

/// Заголовок раздела карточки.
pub fn section_title() -> String {
    tr("Source check")
}

/// Подпись раздела: что это и чего Rexon НЕ делает.
pub fn section_note() -> String {
    tr("AUR packages are published by users. Arch's rule: checking the source is up to whoever installs. Rexon doesn't block installation — it shows what it noticed.")
}

/// Пока идёт проверка.
pub fn status_checking() -> String {
    tr("⏳ Checking the source…")
}

/// Итог проверки словами.
pub fn status_text(facts: Option<&TrustFacts>, signals: &[Signal]) -> String {
    let Some(f) = facts else {
        return tr("Couldn't check the source (no connection) — only what AUR reports is shown.");
    };
    let mut line = if signals.is_empty() {
        trf("✅ No signs of risk found (checked %1).", &[&day(f.checked_at)])
    } else {
        trf("Checked %1.", &[&day(f.checked_at)])
    };
    if f.partial {
        line.push(' ');
        line.push_str(&tr("Some checks were skipped (no connection, site protection or the GitHub limit) — try again later."));
    }
    line
}

/// Кнопка «Показать PKGBUILD».
pub fn pkgbuild_btn() -> String {
    tr("📄 Show PKGBUILD")
}

/// Подсказка к ней.
pub fn pkgbuild_tooltip() -> String {
    tr("The build script of the package: what will be downloaded and run")
}

/// Заголовок окна с PKGBUILD.
pub fn pkgbuild_title(name: &str) -> String {
    trf("PKGBUILD — %1", &[&name])
}

/// Пока PKGBUILD едет.
pub fn pkgbuild_loading() -> String {
    tr("Loading PKGBUILD…")
}

/// Строка-предупреждение для «Рабочих столов»: у каких пакетов из AUR в
/// наборе стола есть признаки риска. Пусто — нет.
pub fn desktop_notice(rows: &[(String, Vec<Signal>)]) -> String {
    let parts: Vec<String> = rows
        .iter()
        .filter(|(_, s)| !s.is_empty())
        .map(|(name, s)| {
            let what: Vec<&str> = s.iter().map(|x| x.text.as_str()).collect();
            format!("{name} — {}", what.join("; "))
        })
        .collect();
    if parts.is_empty() {
        return String::new();
    }
    format!(
        "{}\n{}",
        tr("⚠️ Signs of risk in packages from AUR (installation isn't blocked):"),
        parts.join("\n")
    )
}

// ── «Рабочие столы»: пакеты из AUR в наборе стола ────────────────────────
//
// У стола свой короткий список пакетов из AUR (без карточки и без
// PKGBUILD), поэтому здесь только то, что даёт ОДИН запрос RPC `info` на
// весь список, плюс глубокая проверка с диска, если карточку пакета уже
// открывали в «Оформлении». Ответ RPC держится в памяти 15 минут.

static RPC_MEMO: Mutex<BTreeMap<String, (i64, RpcView)>> = Mutex::new(BTreeMap::new());

/// Данные RPC по списку пакетов: из памяти, если свежие (15 минут), иначе
/// одним запросом `info`. Без сети (`None`) — всё, что есть в памяти.
pub fn rpc_views(net: Option<&mut dyn NetIo>, names: &[String], now: i64) -> BTreeMap<String, RpcView> {
    // Без сети годится любой ответ из памяти: устаревший лучше, чем ничего.
    let online = net.is_some();
    let fresh = |at: i64| !online || now - at < crate::aur::DETAIL_CACHE_SECS as i64;
    let mut out = BTreeMap::new();
    let mut missing: Vec<&str> = Vec::new();
    if let Ok(memo) = RPC_MEMO.lock() {
        for n in names {
            match memo.get(n) {
                Some((at, v)) if fresh(*at) => {
                    out.insert(n.clone(), v.clone());
                }
                _ => missing.push(n),
            }
        }
    }
    let Some(net) = net else { return out };
    if missing.is_empty() {
        return out;
    }
    let Ok(text) = get_aur(net, &crate::aur::info_url(&missing)) else { return out };
    let Ok(data) = serde_json::from_str::<Value>(&text) else { return out };
    let mut memo = RPC_MEMO.lock().ok();
    for name in missing {
        if let Some(info) = crate::aur::parse_info(&data, name) {
            let v = RpcView::from(&info);
            if let Some(m) = memo.as_mut() {
                m.insert(name.to_string(), (now, v.clone()));
            }
            out.insert(name.to_string(), v);
        }
    }
    out
}

/// Предупреждение стола по пакетам из AUR (см. [`desktop_notice`]).
pub fn desktop_notice_for(names: &[String], views: &BTreeMap<String, RpcView>, path: &str, now: i64) -> String {
    let rows: Vec<(String, Vec<Signal>)> = names
        .iter()
        .filter_map(|n| {
            let v = views.get(n)?;
            let entry = cached_entry(path, n);
            Some((n.clone(), signals(v, entry.as_ref(), now)))
        })
        .collect();
    desktop_notice(&rows)
}
