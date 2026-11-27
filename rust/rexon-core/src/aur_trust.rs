//! AUR package risk signs — the "Source check" on the package card and
//! a short badge on the tile.
//!
//! Why. The AUR is packages uploaded by users themselves; the Arch rules
//! say plainly that checking the source is the job of whoever installs. A regular
//! buyer does not read PKGBUILDs, so Rexon itself collects what can be
//! learned without logging into sites, and says in human words what looked suspicious.
//! This does NOT forbid installation — it only warns.
//!
//! Where things come from:
//!
//! * the RPC `info` (the card and tile already have it) — votes, popularity,
//!   "out of date", no maintainer;
//! * the RPC `search?by=maintainer` — how many packages the maintainer has and when
//!   the first of them appeared (the RPC does not give the account registration date, and
//!   the account page is closed to guests); if it shows that many OLD packages
//!   were updated within one week — one more `info` request for
//!   them: who originally uploaded them. An account without old packages of its own
//!   that picked up other people's abandoned ones all at once — that is how packages were hijacked in June
//!   2026;
//! * the package's git log (the `cgit … /log/` page) — who edited the package: a new
//!   name in the last half year where there used to be others — "the package changed
//!   hands"; and the date of the newest edit: if the AUR says "updated" noticeably
//!   LATER than it, the package history was rewritten (only an Arch staff member
//!   can force that, when scrubbing a malicious edit) or the edit was uploaded with
//!   a forged date. No separate request is needed — the log is already there;
//! * the package page on the AUR site — who uploaded the latest version (Last
//!   Packager; the RPC does not give it). If it is neither the maintainer nor their
//!   co-maintainer but an Arch staff member (the Package Maintainers and Developers lists
//!   on archlinux.org, kept for a week) — that is how Arch scrubs hacked
//!   packages. The site shows the "Inactive since" mark and the account registration date
//!   only to logged-in users (the account page answers a guest with 401),
//!   so Rexon cannot see them;
//! * our own snapshots: every time the card is opened the maintainer is
//!   remembered, a change between openings is visible right away;
//! * `.SRCINFO` — where the sources come from and whether they are verified by
//!   a checksum;
//! * the GitHub API for sources on GitHub — whether it is a fork and whether the
//!   GitHub Actions workflows (`.github/workflows`) were changed recently: this is exactly
//!   what the attack of 09.10.2026 looked like — a stolen account, a key-stealing
//!   workflow added to every repository. A new workflow file that is already deleted in
//!   the current version of the repository (the default branch) is a yellow
//!   sign, not a red one: after that attack the author deleted the malicious file, and
//!   red would have been a false alarm for another 30 days.
//!
//! Load. The deep check runs ONLY when the card is opened, the answer
//! stays on disk for a day (`~/.cache/rexon/aur-trust.json`), requests to
//! AUR pages go with a pause (provided by the caller), to GitHub without logging in —
//! no more than `MAX_GITHUB_REPOS` repositories and `MAX_GITHUB_PER_REPO`
//! requests each (GitHub's guest limit is 60 per hour). No network, the site's bot protection,
//! the GitHub limit — the check is silently skipped and marked incomplete.
//!
//! Parsing — pure functions over recorded responses (`tests/aur_trust.rs`).

use crate::aur::{AurInfo, AurPackage};
use crate::i18n::{fill, ntr, tr, trf};
use crate::net::{NetError, NetIo};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Mutex;

// ── Thresholds ────────────────────────────────────────────────────────────

/// How long to keep a deep check on disk, seconds.
pub const CACHE_TTL_SECS: i64 = 24 * 3600;
/// An incomplete check (network, GitHub limit) is repeated sooner — after an hour.
pub const PARTIAL_TTL_SECS: i64 = 3600;
/// Records older than this are dropped from the file (maintainer snapshots too).
pub const KEEP_SECS: i64 = 365 * 24 * 3600;

/// "Recently" for a maintainer change and a new name in the log: half a year.
/// Hijacking an abandoned package and a malicious update are weeks to months
/// apart; old handovers of a package from hand to hand are not counted as noise.
pub const RECENT_OWNER_SECS: i64 = 180 * 24 * 3600;
/// "Recently" for GitHub workflows: 30 days, as requested after the 09.10 attack.
pub const WORKFLOW_SECS: i64 = 30 * 24 * 3600;
/// A new maintainer: all their packages are younger than half a year.
pub const NEW_MAINTAINER_SECS: i64 = 180 * 24 * 3600;

/// Few votes: fewer than five — essentially nobody but the author has
/// tried or looked at the package.
pub const FEW_VOTES: i64 = 5;
/// …or fewer than twenty with popularity below 0.1. AUR popularity is a sum
/// of votes decaying by 0.98 per day: 0.1 is less than one fresh vote
/// over the last ~4 months (0.98^114 ≈ 0.1), i.e. nobody uses the package
/// now.
pub const LOW_POP_VOTES: i64 = 20;
pub const LOW_POPULARITY: f64 = 0.1;
/// Few packages for the maintainer: two or fewer.
pub const FEW_PACKAGES: i64 = 2;

/// How many GitHub repositories to check per package.
pub const MAX_GITHUB_REPOS: usize = 2;
/// How many fresh workflow edits to expand in detail (whether a file is new).
pub const MAX_COMMIT_DETAILS: usize = 2;
/// How many GitHub requests in total per repository: the profile, the list of
/// workflow edits, up to [`MAX_COMMIT_DETAILS`] edits in detail and, if there is
/// room left, the list of workflows in the current version.
pub const MAX_GITHUB_PER_REPO: usize = 4;

/// We keep the Arch staff list for a week: it changes rarely, and the pages
/// are big (two of 80–230 KB).
pub const STAFF_TTL_SECS: i64 = 7 * 24 * 3600;

/// "Updated" later than the newest git edit by more than a week —
/// the history was rewritten. Less is normal: the edit was made and uploaded
/// a few days later.
pub const HISTORY_GAP_SECS: i64 = 7 * 24 * 3600;
/// A just-updated package is not judged this way: the site's log page
/// is served from a cache and may lag behind the upload.
pub const CGIT_LAG_SECS: i64 = 6 * 3600;

/// Mass adoption: this many other people's old packages updated within one
/// week. An honest newcomer usually picks up one to four related
/// packages; in the June 2026 hijack (the fennekuiper account) — 13 in two days.
pub const MASS_ADOPT: usize = 8;
/// The "short time" for an adoption.
pub const ADOPT_WINDOW_SECS: i64 = 7 * 24 * 3600;
/// A package is "old" (adopted, not just uploaded): it existed at least
/// a month before the update.
pub const ADOPT_OLD_SECS: i64 = 30 * 24 * 3600;
/// We do not ask for more profiles than this in one `info` request.
pub const MAX_INFO_NAMES: usize = 100;

/// Request timeout, seconds.
pub const TIMEOUT_SECS: u64 = 25;
/// The cache file name in `~/.cache/rexon/`.
pub const CACHE_FILE: &str = "aur-trust.json";
/// File format version: a foreign format is read as empty.
pub const CACHE_VERSION: u32 = 1;

/// The red badge color: the same as for the dangerous actions of the
/// "Extensions" tab.
pub const DANGER_COLOR: &str = crate::qt_extensions::DANGER_COLOR;

// ── Addresses ─────────────────────────────────────────────────────────────

const AUR_WEB: &str = "https://aur.archlinux.org";
const CGIT: &str = "https://aur.archlinux.org/cgit/aur.git";

/// Lists of Arch staff with the right to upload to ANY AUR package:
/// Package Maintainers (formerly Trusted Users) and Developers.
pub const STAFF_PAGES: [&str; 2] = [
    "https://archlinux.org/people/package-maintainers/",
    "https://archlinux.org/people/developers/",
];
/// GitHub API address (information about the package's upstream repository).
const GH_API: &str = "https://api.github.com";
/// `User-Agent` of GitHub API requests: without it the API answers 403.
const GH_USER_AGENT: &str = "Rexon";

/// Headers of GitHub API requests (without logging in: the program does not touch the
/// person's token).
pub fn github_api_headers() -> Vec<(String, String)> {
    vec![
        ("Accept".to_string(), "application/vnd.github+json".to_string()),
        ("X-GitHub-Api-Version".to_string(), "2022-11-28".to_string()),
    ]
}

/// The value for `?h=` (the branch name in the shared AUR git) — escaped.
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

/// The package's edit log (the cgit page).
pub fn cgit_log_url(pkgbase: &str) -> String {
    format!("{CGIT}/log/?h={}", q(pkgbase))
}

/// The package page on the AUR site.
pub fn package_page_url(name: &str) -> String {
    format!("{AUR_WEB}/packages/{}", q(name))
}

/// The package's `.SRCINFO`.
pub fn srcinfo_url(pkgbase: &str) -> String {
    format!("{CGIT}/plain/.SRCINFO?h={}", q(pkgbase))
}

/// The package's PKGBUILD — opened inside the program.
pub fn pkgbuild_url(pkgbase: &str) -> String {
    format!("{CGIT}/plain/PKGBUILD?h={}", q(pkgbase))
}

/// All of the maintainer's packages (RPC).
pub fn maintainer_url(maintainer: &str) -> String {
    crate::aur::search_url(maintainer, "maintainer")
}

/// The repository profile on GitHub.
pub fn github_repo_url(repo: &str) -> String {
    format!("{GH_API}/repos/{repo}")
}

/// GitHub Actions workflow edits since `since`.
pub fn github_workflows_url(repo: &str, since: i64) -> String {
    format!(
        "{GH_API}/repos/{repo}/commits?path=.github/workflows&since={}&per_page=20",
        iso_utc(since)
    )
}

/// One whole edit (which files it added).
pub fn github_commit_url(repo: &str, sha: &str) -> String {
    format!("{GH_API}/repos/{repo}/commits/{sha}")
}

/// Workflows in the current version of the repository (branch `branch`; empty — the
/// default branch on GitHub's side). 404 — there is no workflows directory at all.
pub fn github_workflow_dir_url(repo: &str, branch: &str) -> String {
    let base = format!("{GH_API}/repos/{repo}/contents/.github/workflows");
    if branch.is_empty() {
        base
    } else {
        format!("{base}?ref={}", q(branch))
    }
}

// ── Time ──────────────────────────────────────────────────────────────────

/// Days from 1970-01-01 to a date (Gregorian calendar).
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
/// The time zone offset is taken into account; garbage — `None`.
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
    // Zone: `Z`, `+0000`, `+02:00`.
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

/// Epoch → `2026-10-08` (empty for zero).
fn day(ts: i64) -> String {
    crate::aur::epoch_day(ts)
}

/// Now, epoch.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ── Package log (cgit) ───────────────────────────────────────────────────

/// A log line: when and who.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogEntry {
    pub date: i64,
    pub author: String,
}

/// The log from a cgit page. `None` — something other than a log page arrived (the site's
/// bot protection, an error): it must not be bypassed, the check is skipped.
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

/// A new person recently started editing the package.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NewAuthor {
    /// The new name (as the edits are signed).
    pub name: String,
    /// Their first edit.
    pub since: i64,
    /// Who edited before (the most frequent author of old edits).
    pub before: String,
}

/// A new name in the log: in the last half year edits are signed by someone who
/// does not appear among the old edits at all. No old edits on the page (the package
/// is young or very active) — nothing to judge.
pub fn new_author(log: &[LogEntry], now: i64) -> Option<NewAuthor> {
    let border = now - RECENT_OWNER_SECS;
    let norm = |s: &str| s.trim().to_lowercase();
    let old: Vec<&LogEntry> = log.iter().filter(|e| e.date < border).collect();
    if old.is_empty() {
        return None;
    }
    let old_names: Vec<String> = old.iter().map(|e| norm(&e.author)).collect();
    // The earliest fresh edit by the new name (the log goes from new to old).
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

// ── The package page and Arch staff ──────────────────────────────────────

/// What we take from the package page (the guest view, names as text).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackagePage {
    pub maintainer: String,
    /// The maintainer's co-maintainers (in parentheses after their name).
    pub comaintainers: Vec<String>,
    /// Who uploaded the latest version ("" — unknown, the account was deleted).
    pub last_packager: String,
}

/// A profile table cell by the row label, as single-line text.
fn page_cell(html: &str, label: &str) -> Option<String> {
    let at = html.find(&format!("<th>{label}:</th>"))?;
    let rest = &html[at..];
    let rest = &rest[rest.find("<td")?..];
    let inner = rest.split_once('>')?.1.split("</td>").next()?;
    Some(crate::aur::html_to_text(inner).split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Parse the package page. `None` — something else arrived (the site's bot
/// protection, an error).
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

/// Staff names from an archlinux.org list page (in lowercase):
/// both the account name (the card anchor) and the alias. `None` — the wrong page.
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

/// Arch staff (kept in the cache file for a week).
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

/// The latest version was uploaded by an outsider: neither the maintainer nor their
/// co-maintainer. A regular person cannot do that — only an Arch staff member or
/// a previous maintainer (the package has been handed over since).
pub fn packager_is_outsider(page: &PackagePage, maintainer: &str) -> bool {
    let norm = |s: &str| s.trim().to_lowercase();
    let p = norm(&page.last_packager);
    !p.is_empty() && p != norm(maintainer) && !page.comaintainers.iter().any(|c| norm(c) == p)
}

// ── .SRCINFO ──────────────────────────────────────────────────────────────

/// One source from `.SRCINFO`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Source {
    /// The address without `name::`, without `git+` and without `#…`.
    pub url: String,
    /// As written (for showing to the person).
    pub raw: String,
    /// Version control system (`git`, `hg`, …), empty — a plain file.
    pub vcs: String,
    /// The tail after `#` (`branch=…`, `tag=…`, `commit=…`).
    pub fragment: String,
    /// At least one checksum is not `SKIP`.
    pub verified: bool,
}

impl Source {
    /// Downloaded from the network (rather than sitting next to the PKGBUILD in the AUR git).
    pub fn remote(&self) -> bool {
        !self.vcs.is_empty() || self.url.contains("://")
    }
}

/// A parsed `.SRCINFO`: the project address and the sources.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SrcInfo {
    pub pkgbase: String,
    pub url: String,
    pub sources: Vec<Source>,
}

const SUM_ALGOS: &[&str] =
    &["ck", "md5", "sha1", "sha224", "sha256", "sha384", "sha512", "b2"];
const VCS: &[&str] = &["git", "hg", "svn", "bzr", "fossil"];

/// Parse `.SRCINFO`. `None` — it is not one (the site's bot protection).
pub fn parse_srcinfo(text: &str) -> Option<SrcInfo> {
    if !text.trim_start().starts_with("pkgbase") {
        return None;
    }
    let mut out = SrcInfo::default();
    // Key → values in order (only the pkgbase section: sources and
    // sums are not set in pkgname sections).
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

/// The package name says it is built from fresh code (`-git` and the like):
/// for such packages a "branch" source is the norm, not a risk.
pub fn is_vcs_package(name: &str) -> bool {
    ["-git", "-hg", "-svn", "-bzr", "-fossil", "-darcs", "-cvs", "-nightly"]
        .iter()
        .any(|s| name.ends_with(s))
}

/// Files from the network without a checksum (for a package not built from fresh code).
pub fn unverified_sources(info: &SrcInfo) -> Vec<String> {
    info.sources
        .iter()
        .filter(|s| s.vcs.is_empty() && s.remote() && !s.verified)
        .map(|s| s.url.clone())
        .collect()
}

/// Code from a branch rather than a pinned version (for a package not built from fresh code).
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

/// `owner/repository` on GitHub from an address (a page, an archive, a release,
/// raw/codeload). Case is preserved, comparison is case-insensitive.
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

/// GitHub repositories worth checking: first the sources (the package is built
/// from them), then the project page. Without repeats, no more than
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

/// A GitHub Actions workflow edit.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkflowChange {
    pub sha: String,
    pub date: i64,
    pub author: String,
    pub message: String,
    /// Which workflow files the edit ADDED (empty — it only changed them or was not
    /// expanded).
    pub added: Vec<String>,
    /// Which workflow files the edit DELETED.
    pub removed: Vec<String>,
    /// The edit was expanded (it is known which files it added and deleted).
    pub detailed: bool,
    /// Those of `added` that are no longer in the current version of the repository
    /// (checked). Unknown ones do not go here: it is safer to assume
    /// the file is in place.
    pub gone: Vec<String>,
}

impl WorkflowChange {
    /// Added files that (as far as is known) are still in place.
    pub fn still_present(&self) -> Vec<&String> {
        self.added.iter().filter(|f| !self.gone.contains(f)).collect()
    }
}

/// What is known about a repository on GitHub.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GithubFacts {
    /// `owner/repository`.
    pub repo: String,
    /// The profile was received.
    pub checked: bool,
    /// The repository does not exist (404).
    pub missing: bool,
    pub fork: bool,
    /// Where the fork comes from (`owner/repository`).
    pub parent: String,
    pub archived: bool,
    /// The default branch (this is what we consider the "current version").
    pub default_branch: String,
    /// The workflow edits were received.
    pub workflows_checked: bool,
    /// Fresh workflow edits not by bots, from new to old.
    pub workflow_changes: Vec<WorkflowChange>,
}

/// Parse the repository profile.
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

/// The edit was made by a bot (dependency updates and the like) — these come in
/// a stream in any living project and are not counted as noise.
fn is_bot(login: &str, name: &str) -> bool {
    let l = login.to_ascii_lowercase();
    let n = name.to_ascii_lowercase();
    l.ends_with("[bot]") || n.ends_with("[bot]") || l == "dependabot" || l == "renovate-bot"
}

/// Fresh (within [`WORKFLOW_SECS`]) workflow edits not by bots.
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
            // The author date is the easiest to forge — we take the later of the two.
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

/// Workflow files of an edit with the given status (`added`, `removed`).
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

/// Workflow files the edit added.
pub fn parse_added_workflows(json: &Value) -> Vec<String> {
    workflow_files_with(json, "added")
}

/// Workflow files the edit deleted. A rename does NOT count as a deletion:
/// the content stayed, just under another name.
pub fn parse_removed_workflows(json: &Value) -> Vec<String> {
    workflow_files_with(json, "removed")
}

/// The listing of the `.github/workflows` directory (the `contents` answer) → file paths.
/// `None` — something else arrived (not a list).
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

/// Added files that a later edit deleted. Edits go from
/// new to old; we judge only by edits expanded continuously from the top:
/// an unexpanded edit newer than the addition could have brought the file back. Sets `gone`
/// and returns whether there are added files left whose fate is unknown.
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
            // The newest event with the file decides: deleted or brought back.
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

/// Check the added files against the workflow list of the current version: what is not in
/// the list is deleted. `listing = None` — there is no directory at all (404).
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

// ── Maintainer ───────────────────────────────────────────────────────────

/// How many packages the maintainer has and when the earliest appeared.
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

/// A maintainer's package: from the search and (if asked) from the profile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaintPkg {
    pub name: String,
    pub first_submitted: i64,
    pub last_modified: i64,
    /// Who first uploaded the package: `None` — the profile was not asked for, "" —
    /// the account no longer exists.
    pub submitter: Option<String>,
}

/// The maintainer's packages from the `search by=maintainer` answer.
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

/// Updated recently, but existed long before that — adopted, not
/// just uploaded.
fn old_and_recently_updated(p: &MaintPkg, now: i64) -> bool {
    p.last_modified >= now - RECENT_OWNER_SECS
        && p.first_submitted > 0
        && p.first_submitted < p.last_modified - ADOPT_OLD_SECS
}

/// The densest week of updates: (start, how many packages in it).
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

/// Whose profiles to ask for in order to check a mass adoption. Empty —
/// the search alone shows there is no adoption (true for almost everyone). Otherwise —
/// the packages of that week and the oldest ones (they show whether the account has
/// old packages of its own), no more than [`MAX_INFO_NAMES`], alphabetically.
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

/// Fill in who first uploaded the packages, from the `info` answer.
pub fn fill_submitters(list: &mut [MaintPkg], info: &Value) {
    for p in list.iter_mut() {
        if let Some(i) = crate::aur::parse_info(info, &p.name) {
            p.submitter = Some(i.submitter);
        }
    }
}

/// A new account mass-adopted other people's old packages.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MassAdoption {
    /// How many other people's old packages were updated during that week.
    pub count: i64,
    /// Start of the week (the first update).
    pub since: i64,
}

/// Mass adoption: at least [`MASS_ADOPT`] packages uploaded
/// at SOME POINT by others (the profile was received) were updated within one week in
/// the last half year, and the account has no packages of its own uploaded more
/// than half a year before that week (the AUR does not show the registration date to a guest —
/// this is its substitute).
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

/// Whose profiles to ask for in order to learn the account's age from its OWN packages.
/// Empty — the search alone makes everything clear (true for almost everyone): either the account
/// has a package not updated for more than half a year (it is definitely not new —
/// its own or adopted, it has held it since then), or all its packages
/// were uploaded recently (new either way). Otherwise — old packages
/// that could have been adopted, from the oldest, no more than
/// [`MAX_INFO_NAMES`], alphabetically.
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

/// Profiles for both account signs in one request: first those needed by
/// the mass adoption check, then by the age check, no more than [`MAX_INFO_NAMES`].
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

/// Since when the account has been in the AUR: the earliest package it uploaded
/// itself. An adopted package of someone else's says nothing about its upload date, only
/// about the last update (the account surely did not hold it before that).
/// A package without a profile — by the upload date, as before.
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

// ── Result of the deep check ─────────────────────────────────────────────

/// Everything the deep check learned (stays on disk for a day).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrustFacts {
    pub name: String,
    pub checked_at: i64,
    /// Some of the checks failed (network, site protection, GitHub limit).
    pub partial: bool,
    pub maintainer: String,
    /// Packages of the maintainer (`None` — not found out).
    pub maintainer_packages: Option<i64>,
    /// Since when the account has been in the AUR (epoch): the earliest package of its OWN, see
    /// [`account_since`]; without profiles — the earliest of all.
    pub maintainer_since: Option<i64>,
    /// The maintainer's account mass-adopted other people's old packages.
    pub mass_adoption: Option<MassAdoption>,
    pub log_checked: bool,
    pub new_author: Option<NewAuthor>,
    /// Date of the newest edit in the git log (the committer date).
    pub tip_date: Option<i64>,
    /// "Updated" per the RPC at the time of the check.
    pub last_modified: i64,
    /// The package page was read.
    pub page_checked: bool,
    /// Who uploaded the latest version.
    pub last_packager: String,
    /// …and it is an Arch staff member, not the maintainer or their co-maintainer.
    pub staff_packager: bool,
    pub srcinfo_checked: bool,
    pub unverified: Vec<String>,
    pub branch_sources: Vec<String>,
    pub github: Vec<GithubFacts>,
}

impl TrustFacts {
    /// Whether the check is fresh.
    pub fn fresh(&self, now: i64) -> bool {
        let ttl = if self.partial { PARTIAL_TTL_SECS } else { CACHE_TTL_SECS };
        self.checked_at > 0 && now >= self.checked_at && now - self.checked_at < ttl
    }
}

/// Who maintained the package when the card was opened.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaintainerSeen {
    pub name: String,
    pub first_seen: i64,
    pub last_seen: i64,
}

/// A cache file record about a package.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheEntry {
    pub facts: Option<TrustFacts>,
    /// The maintainer history from our snapshots, from old to new.
    pub maintainers: Vec<MaintainerSeen>,
}

/// The whole cache file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrustCache {
    pub version: u32,
    pub packages: BTreeMap<String, CacheEntry>,
    /// Arch staff — a list shared by all packages.
    pub staff: StaffList,
}

/// A maintainer change noticed by our snapshots.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaintainerChange {
    pub from: String,
    pub to: String,
    pub at: i64,
}

impl CacheEntry {
    /// Remember who maintains the package now ("" — nobody).
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

    /// The maintainer changed between card openings within the last
    /// half year. A package abandoned by the previous one ("" → name) is also a change.
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
    /// Drop outdated records.
    pub fn prune(&mut self, now: i64) {
        self.packages.retain(|_, e| {
            let fact = e.facts.as_ref().map(|f| f.checked_at).unwrap_or(0);
            let seen = e.maintainers.last().map(|m| m.last_seen).unwrap_or(0);
            now - fact.max(seen) < KEEP_SECS
        });
    }
}

/// The cache file path.
pub fn cache_path(xdg_cache_home: Option<&str>, home: &str) -> String {
    format!("{}/rexon/{CACHE_FILE}", crate::utils::cache_home(xdg_cache_home, home))
}

/// The cache file path of this process.
pub fn live_cache_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    cache_path(std::env::var("XDG_CACHE_HOME").ok().as_deref(), &home)
}

/// Read the cache; no file, broken, a foreign version — empty.
pub fn load_cache(path: &str) -> TrustCache {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<TrustCache>(&t).ok())
        .filter(|c| c.version == CACHE_VERSION)
        .unwrap_or_else(|| TrustCache { version: CACHE_VERSION, ..TrustCache::default() })
}

/// Write the cache (via a temporary file).
pub fn save_cache(path: &str, cache: &TrustCache) -> Result<(), String> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string(cache).map_err(|e| e.to_string())?;
    crate::utils::atomic_write_str_err(path, &text)
}

/// The in-process memory cache: the file is read once, tiles ask
/// from here without the disk.
static MEMORY: Mutex<Option<(String, TrustCache)>> = Mutex::new(None);

/// A package record from the cache (memory, on first access — the file).
pub fn cached_entry(path: &str, name: &str) -> Option<CacheEntry> {
    let mut guard = MEMORY.lock().ok()?;
    if guard.as_ref().map(|(p, _)| p != path).unwrap_or(true) {
        *guard = Some((path.to_string(), load_cache(path)));
    }
    guard.as_ref()?.1.packages.get(name).cloned()
}

/// Arch staff from the cache (memory, on first access — the file).
pub fn cached_staff(path: &str) -> StaffList {
    let Ok(mut guard) = MEMORY.lock() else { return StaffList::default() };
    if guard.as_ref().map(|(p, _)| p != path).unwrap_or(true) {
        *guard = Some((path.to_string(), load_cache(path)));
    }
    guard.as_ref().map(|(_, c)| c.staff.clone()).unwrap_or_default()
}

/// Amend the whole cache file: in memory and on disk.
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

/// Update a package record in memory and on disk.
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

// ── Network ───────────────────────────────────────────────────────────────

/// How to fetch data: the AUR (RPC and pages), GitHub (the API with its
/// headers) and a pause before requesting AUR pages.
pub struct Fetchers<'a> {
    pub aur: &'a mut dyn NetIo,
    pub github: &'a mut dyn NetIo,
    pub pause: &'a mut dyn FnMut(),
}

fn get_text(net: &mut dyn NetIo, url: &str, ua: &str) -> Result<String, NetError> {
    net.get(url, ua, TIMEOUT_SECS).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// The same for the AUR: a single dropped connection is retried (the site sometimes does not answer
/// on the first try). GitHub is not retried — each attempt eats into the limit.
fn get_aur(net: &mut dyn NetIo, url: &str) -> Result<String, NetError> {
    let what = format!("AUR {url}");
    crate::net::with_retry(net, crate::net::Retry::IDEMPOTENT, &what, |io| {
        io.get(url, crate::aur::USER_AGENT, TIMEOUT_SECS)
    })
    .map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// The GitHub limit is exhausted (or access is denied) — we do not go there anymore.
fn github_blocked(e: &NetError) -> bool {
    matches!(e, NetError::Status { code: 403 | 429, .. })
}

/// The Arch staff list from archlinux.org. `None` — at least one of the pages
/// did not arrive: an incomplete list is worse than none (we would miss someone).
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

/// A deep check of a package over the network without the Arch staff list cache.
pub fn collect_facts(f: &mut Fetchers, info: &AurInfo, now: i64) -> TrustFacts {
    collect_facts_with(f, info, now, &mut StaffList::default())
}

/// A deep check of a package over the network. Each part that fails
/// is simply skipped (and the check is marked incomplete). `staff` — the
/// Arch staff list: a stale one is refreshed only if it was needed.
pub fn collect_facts_with(f: &mut Fetchers, info: &AurInfo, now: i64, staff: &mut StaffList) -> TrustFacts {
    let base = if info.package_base.is_empty() { &info.name } else { &info.package_base };
    let mut facts = TrustFacts {
        name: info.name.clone(),
        checked_at: now,
        maintainer: info.maintainer.clone(),
        last_modified: info.last_modified,
        ..TrustFacts::default()
    };

    // Maintainer: how many packages and since when.
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
        // Package profiles (who uploaded them) are asked for only if
        // the search shows a week when many old packages were updated, or
        // the account's age is in doubt: old packages could have been adopted.
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

    // The edit log.
    (f.pause)();
    match get_aur(f.aur, &cgit_log_url(base)).ok().as_deref().and_then(parse_cgit_log) {
        Some(log) => {
            facts.log_checked = true;
            facts.new_author = new_author(&log, now);
            facts.tip_date = log.iter().map(|e| e.date).max();
        }
        None => facts.partial = true,
    }

    // Who uploaded the latest version (the package page). The Arch staff list
    // is needed only if it is neither the maintainer nor their co-maintainer.
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

    // Sources.
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
        // The profile and the list of workflow edits.
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
                    // The edit was not expanded — the check is incomplete (repeated in an hour).
                    blocked = github_blocked(&e);
                    facts.partial = true;
                }
            }
        }
        // Is the new workflow file still in place? First by the already expanded edits
        // (free), then, if there is room left, by the directory listing in
        // the current version. Not found out — the file counts as in place (red).
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

/// A check with the cache: a fresh record from disk or collecting over the network; the
/// maintainer snapshot is always written. Returns the whole record.
pub fn check_cached(path: &str, f: &mut Fetchers, info: &AurInfo, now: i64) -> CacheEntry {
    let cached = cached_entry(path, &info.name);
    let fresh = cached
        .as_ref()
        .and_then(|e| e.facts.as_ref())
        // A new upload — the check from disk is no good.
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

/// The PKGBUILD text. An error — in words for the person.
pub fn fetch_pkgbuild(net: &mut dyn NetIo, pkgbase: &str) -> Result<String, String> {
    let text = get_aur(net, &pkgbuild_url(pkgbase))
        .map_err(|e| trf("Couldn't load PKGBUILD: %1", &[&e]))?;
    let head = text.trim_start().get(..200).unwrap_or(text.trim_start()).to_ascii_lowercase();
    if head.starts_with("<!doctype") || head.starts_with("<html") {
        return Err(tr("AUR didn't let us read the PKGBUILD here (bot protection) — it is on the website."));
    }
    Ok(text)
}

// ── Signs ─────────────────────────────────────────────────────────────────

/// How serious a sign is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Yellow: worth a closer look.
    Warn,
    /// Red: looks like attacks look.
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

/// One risk sign: what is wrong and why it matters.
#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    /// A machine key (for checks and ordering).
    pub key: &'static str,
    pub level: Level,
    pub text: String,
    pub why: String,
}

/// What is known about a package without a deep check — the tile and the RPC profile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RpcView {
    pub maintainer: String,
    pub votes: i64,
    pub popularity: f64,
    /// Epoch of the "out of date" flag; 1 — flagged, date unknown; 0 — not flagged.
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

/// Signs from the RPC data (what is also visible on the tile).
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
                &[&crate::i18n::format_decimal(v.popularity, 2)],
            ),
            why: tr("Votes show how many people tried the package. With few of them, maybe nobody besides the author has looked at what it does."),
        });
    }
    out
}

/// The AUR says "updated" noticeably later than the newest edit in the package's git.
pub fn history_rewritten(facts: &TrustFacts) -> bool {
    let Some(tip) = facts.tip_date else { return false };
    let lm = facts.last_modified;
    lm > 0 && lm - tip > HISTORY_GAP_SECS && facts.checked_at - lm >= CGIT_LAG_SECS
}

/// Deep check signs.
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
        // An Arch staff member uploaded a version, and the history was rewritten: this is how Arch
        // scrubs a hacked package.
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
            // All recently added files are already deleted: yellow, and a combination
            // with a fork or a new person does NOT raise it to red.
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

/// All signs: deep ones (if any) and RPC ones, red first.
///
/// A fresh GitHub workflow edit together with a fork, a new person or
/// a new maintainer is already red: this is what a hijack looks like. An added
/// and already deleted workflow (`workflow_added_removed`) is not raised like that.
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
    // Mass adoption together with a scrub, a rewritten history or
    // a change of hands is already red: this is what a hijack looks like.
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

/// The overall level: the most serious of the signs.
pub fn level(signals: &[Signal]) -> Option<Level> {
    signals.iter().map(|s| s.level).max()
}

/// The badge next to "Install": "⚠ 3 risk signs".
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

/// A short list for the badge tooltip.
pub fn badge_tooltip(signals: &[Signal]) -> String {
    let mut lines: Vec<String> = signals.iter().map(|s| format!("• {}", s.text)).collect();
    if !lines.is_empty() {
        lines.push(String::new());
        lines.push(tr("Installation isn't blocked: this is a hint. Details are in “Source check” below."));
    }
    lines.join("\n")
}

/// The badge before the source line on the tile (empty — no signs).
pub fn tile_mark(signals: &[Signal]) -> &'static str {
    match level(signals) {
        Some(Level::Danger) => "⛔ ",
        Some(Level::Warn) => "⚠️ ",
        None => "",
    }
}

/// The tile badge tooltip.
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

/// The card section title.
pub fn section_title() -> String {
    tr("Source check")
}

/// The section caption: what this is and what Rexon does NOT do.
pub fn section_note() -> String {
    tr("AUR packages are published by users. Arch's rule: checking the source is up to whoever installs. Rexon doesn't block installation — it shows what it noticed.")
}

/// While the check is running.
pub fn status_checking() -> String {
    tr("⏳ Checking the source…")
}

/// The check result in words.
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

/// The "Show PKGBUILD" button.
pub fn pkgbuild_btn() -> String {
    tr("📄 Show PKGBUILD")
}

/// Its tooltip.
pub fn pkgbuild_tooltip() -> String {
    tr("The build script of the package: what will be downloaded and run")
}

/// The title of the PKGBUILD window.
pub fn pkgbuild_title(name: &str) -> String {
    trf("PKGBUILD — %1", &[&name])
}

/// While the PKGBUILD is on its way.
pub fn pkgbuild_loading() -> String {
    tr("Loading PKGBUILD…")
}

/// A warning line for "Desktops": which AUR packages in the desktop's
/// set have risk signs. Empty — none.
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

// ── "Desktops": AUR packages in the desktop's set ────────────────────────
//
// A desktop has its own short list of AUR packages (without a card and without
// a PKGBUILD), so here there is only what ONE RPC `info` request gives for
// the whole list, plus the deep check from disk if the package card was already
// opened in "Appearance". The RPC answer is kept in memory for 15 minutes.

static RPC_MEMO: Mutex<BTreeMap<String, (i64, RpcView)>> = Mutex::new(BTreeMap::new());

/// RPC data for a list of packages: from memory if fresh (15 minutes), otherwise
/// with one `info` request. Without network (`None`) — whatever is in memory.
pub fn rpc_views(net: Option<&mut dyn NetIo>, names: &[String], now: i64) -> BTreeMap<String, RpcView> {
    // Without network any answer from memory will do: an outdated one is better than nothing.
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

/// The desktop warning about AUR packages (see [`desktop_notice`]).
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
