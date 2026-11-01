//! Признаки риска пакета AUR («Проверка источника»).
//!
//! Эталоны — ЗАПИСАННЫЕ живые ответы от 09.10.2026:
//!
//! * `yay-bin` — обычный надёжный пакет (370 голосов, сопровождающий с
//!   2016 года, исходник — выпуск на GitHub с контрольными суммами);
//! * `sweet-kvantum-git` — пакет, перешедший в другие руки (журнал: до 2020
//!   года — один человек, с апреля 2026 — другой), источник — ФОРК на GitHub,
//!   ноль голосов;
//! * `ixnewton/org.kde.plasma.quicklaunch` — настоящий репозиторий из атаки
//!   09.10.2026: 08.10 в него дописан `.github/workflows/security-audit.yml`
//!   (ответы GitHub записаны как есть, текст самого сценария выброшен).
//!   Тот же репозиторий записан ещё раз вечером 09.10: в 11:16 UTC автор
//!   удалил вредный файл правкой `4eb5620b`, и список правок сценариев
//!   показывает обе; каталога `.github/workflows` в ветке `main` больше
//!   нет (GitHub отвечает 404).
//!   AUR-пакет `plasma6-applet-quicklaunch` к 09.10 уже удалён, поэтому его
//!   сторона (паспорт RPC, `.SRCINFO`, журнал, пакеты сопровождающего)
//!   СМОДЕЛИРОВАНА здесь же, в тесте.
//!
//! Записанные живые ответы от 19.10.2026 — пакет `xsp` из захвата июня
//! 2026 (aur-general): учётка `fennekuiper` через три дня после регистрации
//! подобрала 13 брошенных пакетов, Arch её заблокировал, а сотрудник
//! (`tippfehlr`) вычистил пакеты. Страница пакета (последнюю версию
//! выложил сотрудник Arch), журнал git, паспорт RPC, пакеты учётки. Списки
//! сотрудников Arch с archlinux.org записаны ВЫДЕРЖКОЙ — по две карточки,
//! строки с адресами убраны. Страницы `yay-bin` и `sweet-kvantum-git` —
//! того же дня (у `yay-bin` срезаны комментарии).
//!
//! Адреса электронной почты и тела сообщений правок из ответов GitHub
//! вычищены. Сеть в проверках не участвует.

use rexon_core::aur::parse_info;
use rexon_core::aur_trust::*;
use rexon_core::net::{LogLevel, NetError, NetIo};
use serde_json::Value;
use std::collections::HashMap;

const NOW_ISO: &str = "2026-10-09T12:00:00Z";

fn now() -> i64 {
    parse_time(NOW_ISO).unwrap()
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

const INFO: &str = include_str!("golden/aur_trust_info.json");
const LOG_YAY: &str = include_str!("golden/aur_trust_log_yay-bin.html");
const LOG_SWEET: &str = include_str!("golden/aur_trust_log_sweet-kvantum-git.html");
const SRC_YAY: &str = include_str!("golden/aur_trust_srcinfo_yay-bin.txt");
const SRC_SWEET: &str = include_str!("golden/aur_trust_srcinfo_sweet-kvantum-git.txt");
const MAINT_JGUER: &str = include_str!("golden/aur_trust_maint_jguer.json");
const MAINT_GIGAS: &str = include_str!("golden/aur_trust_maint_gigas002.json");
const GH_YAY: &str = include_str!("golden/aur_trust_gh_repo_jguer_yay.json");
const GH_SWEET: &str = include_str!("golden/aur_trust_gh_repo_gigas002_sweet-kde.json");
const GH_IX: &str = include_str!("golden/aur_trust_gh_repo_ixnewton_quicklaunch.json");
const WF_YAY: &str = include_str!("golden/aur_trust_gh_workflows_jguer_yay.json");
// Живой ответ GitHub для Gigas002/Sweet-kde — пустой список (правок
// сценариев за 30 дней нет); отдельным файлом не лежит: пустые эталоны
// сторож `golden_sources_alive` не пускает.
const WF_SWEET: &str = "[]";
const WF_IX: &str = include_str!("golden/aur_trust_gh_workflows_ixnewton_quicklaunch.json");
const COMMIT_IX: &str = include_str!("golden/aur_trust_gh_commit_ixnewton_quicklaunch.json");
const IX_SHA: &str = "847cb698aecd8ec90cc84534e6aad3f1cec5aee5";
const WF_IX_REMOVED: &str = include_str!("golden/aur_trust_gh_workflows_ixnewton_quicklaunch_removed.json");
const COMMIT_IX_REMOVAL: &str = include_str!("golden/aur_trust_gh_commit_ixnewton_quicklaunch_removal.json");
const IX_REMOVAL_SHA: &str = "4eb5620b007322749b7a50c42514a710a109f03c";
const IX_REPO: &str = "ixnewton/org.kde.plasma.quicklaunch";
const PAGE_YAY: &str = include_str!("golden/aur_trust_pkg_yay-bin.html");
const PAGE_SWEET: &str = include_str!("golden/aur_trust_pkg_sweet-kvantum-git.html");
const PAGE_XSP: &str = include_str!("golden/aur_trust_pkg_xsp.html");
const INFO_XSP: &str = include_str!("golden/aur_trust_info_xsp.json");
const LOG_XSP: &str = include_str!("golden/aur_trust_log_xsp.html");
const SRC_XSP: &str = include_str!("golden/aur_trust_srcinfo_xsp.txt");
const MAINT_FENNE: &str = include_str!("golden/aur_trust_maint_fennekuiper.json");
const INFO_FENNE: &str = include_str!("golden/aur_trust_info_fennekuiper.json");
const STAFF_PM: &str = include_str!("golden/aur_trust_staff_package-maintainers.html");
const STAFF_DEV: &str = include_str!("golden/aur_trust_staff_developers.html");

/// День записи ответов про `xsp`.
fn now_xsp() -> i64 {
    parse_time("2026-10-19T12:00:00Z").unwrap()
}
/// Каталог сценариев в `main` утром 09.10, пока вредный файл был на месте
/// (СМОДЕЛИРОВАН: тогда его не записали; поля — как в ответе `contents`).
const LISTING_IX_PRESENT: &str = r#"[{"name":"security-audit.yml","path":".github/workflows/security-audit.yml","type":"file"}]"#;

// ── Смоделированная сторона AUR для пакета из атаки ─────────────────────

const QL_INFO: &str = r#"{"version":5,"type":"multiinfo","resultcount":1,"results":[{
 "Name":"plasma6-applet-quicklaunch","PackageBase":"plasma6-applet-quicklaunch",
 "Version":"1.0.4-1","Description":"Quicklaunch applet for Plasma 6",
 "URL":"https://github.com/ixnewton/org.kde.plasma.quicklaunch",
 "Maintainer":"CxOrg","Submitter":"CxOrg","NumVotes":2,"Popularity":0.31,
 "FirstSubmitted":1784000000,"LastModified":1791000000,"OutOfDate":null}]}"#;

const QL_SRCINFO: &str = "pkgbase = plasma6-applet-quicklaunch
\tpkgver = 1.0.4
\tpkgrel = 1
\turl = https://github.com/ixnewton/org.kde.plasma.quicklaunch
\tsource = quicklaunch-1.0.4.tar.gz::https://github.com/ixnewton/org.kde.plasma.quicklaunch/archive/refs/tags/v1.0.4.tar.gz
\tsha256sums = SKIP

pkgname = plasma6-applet-quicklaunch
";

const QL_MAINT: &str = r#"{"version":5,"type":"search","resultcount":3,"results":[
 {"Name":"plasma6-applet-quicklaunch","FirstSubmitted":1784000000},
 {"Name":"plasma6-applet-a","FirstSubmitted":1785000000},
 {"Name":"plasma6-applet-b","FirstSubmitted":1786000000}]}"#;

/// Страница пакета (СМОДЕЛИРОВАНА, разметка — как у живой): выкладывал
/// сам сопровождающий.
const QL_PAGE: &str = "<div id=\"pkgdetails\" class=\"box\"><table id=\"pkginfo\">
<tr class=\"pkgmaint\"><th>Maintainer:</th><td>
    CxOrg
</td></tr>
<tr><th>Last Packager:</th><td>
    CxOrg
</td></tr></table></div>";

const QL_LOG: &str = "<table class='list nowrap'>
<tr><td><span title='2026-10-08 21:00:00 +0000'>2026-10-08</span></td><td><a href='#'>1.0.4</a></td><td>CxOrg</td></tr>
<tr><td><span title='2026-07-14 10:00:00 +0000'>2026-07-14</span></td><td><a href='#'>initial</a></td><td>CxOrg</td></tr>
</table>";

// ── Подставная сеть ──────────────────────────────────────────────────────

#[derive(Default)]
struct FakeNet {
    replies: HashMap<String, Result<String, NetError>>,
    asked: Vec<String>,
}

impl FakeNet {
    fn ok(mut self, url: &str, body: &str) -> Self {
        self.replies.insert(url.to_string(), Ok(body.to_string()));
        self
    }
    fn fail(mut self, url: &str, e: NetError) -> Self {
        self.replies.insert(url.to_string(), Err(e));
        self
    }
}

fn offline() -> NetError {
    NetError::Transport { detail: "<urlopen error offline>".into() }
}

impl NetIo for FakeNet {
    fn get(&mut self, url: &str, _: &str, _: u64) -> Result<Vec<u8>, NetError> {
        self.asked.push(url.to_string());
        match self.replies.get(url) {
            Some(Ok(b)) => Ok(b.as_bytes().to_vec()),
            Some(Err(e)) => Err(e.clone()),
            None => Err(offline()),
        }
    }
    fn open(&mut self, _: &str, _: &str, _: u64) -> Result<i64, NetError> {
        Err(offline())
    }
    fn read(&mut self, _: usize) -> Result<Vec<u8>, NetError> {
        Ok(Vec::new())
    }
    fn close(&mut self) {}
    fn makedirs(&mut self, _: &str) {}
    fn create(&mut self, _: &str) -> Result<(), NetError> {
        Ok(())
    }
    fn write(&mut self, _: &[u8]) -> Result<(), NetError> {
        Ok(())
    }
    fn close_file(&mut self) {}
    fn exists(&mut self, _: &str) -> bool {
        false
    }
    fn remove(&mut self, _: &str) {}
    fn progress(&mut self, _: i64, _: i64) {}
    fn log(&mut self, _: LogLevel, _: &str) {}
    fn sleep(&mut self, _: u64) {}
    fn content_type(&mut self) -> String {
        String::new()
    }
}

fn yay_nets() -> (FakeNet, FakeNet) {
    let aur = FakeNet::default()
        .ok(&maintainer_url("jguer"), MAINT_JGUER)
        .ok(&cgit_log_url("yay-bin"), LOG_YAY)
        .ok(&package_page_url("yay-bin"), PAGE_YAY)
        .ok(&srcinfo_url("yay-bin"), SRC_YAY);
    let gh = FakeNet::default()
        .ok(&github_repo_url("Jguer/yay"), GH_YAY)
        .ok(&github_workflows_url("Jguer/yay", now() - WORKFLOW_SECS), WF_YAY);
    (aur, gh)
}

fn sweet_nets() -> (FakeNet, FakeNet) {
    let aur = FakeNet::default()
        .ok(&maintainer_url("Gigas002"), MAINT_GIGAS)
        .ok(&cgit_log_url("sweet-kvantum-git"), LOG_SWEET)
        .ok(&package_page_url("sweet-kvantum-git"), PAGE_SWEET)
        .ok(&srcinfo_url("sweet-kvantum-git"), SRC_SWEET);
    let gh = FakeNet::default()
        .ok(&github_repo_url("Gigas002/Sweet-kde"), GH_SWEET)
        .ok(&github_workflows_url("Gigas002/Sweet-kde", now() - WORKFLOW_SECS), WF_SWEET);
    (aur, gh)
}

fn ql_nets() -> (FakeNet, FakeNet) {
    let aur = FakeNet::default()
        .ok(&maintainer_url("CxOrg"), QL_MAINT)
        .ok(&cgit_log_url("plasma6-applet-quicklaunch"), QL_LOG)
        .ok(&package_page_url("plasma6-applet-quicklaunch"), QL_PAGE)
        .ok(&srcinfo_url("plasma6-applet-quicklaunch"), QL_SRCINFO);
    let repo = "ixnewton/org.kde.plasma.quicklaunch";
    let gh = FakeNet::default()
        .ok(&github_repo_url(repo), GH_IX)
        .ok(&github_workflows_url(repo, now() - WORKFLOW_SECS), WF_IX)
        .ok(&github_commit_url(repo, IX_SHA), COMMIT_IX)
        .ok(&github_workflow_dir_url(repo, "main"), LISTING_IX_PRESENT);
    (aur, gh)
}

/// Тот же пакет вечером 09.10: вредный файл уже удалён.
fn ql_removed_nets() -> (FakeNet, FakeNet) {
    let (aur, _) = ql_nets();
    let gh = FakeNet::default()
        .ok(&github_repo_url(IX_REPO), GH_IX)
        .ok(&github_workflows_url(IX_REPO, now() - WORKFLOW_SECS), WF_IX_REMOVED)
        .ok(&github_commit_url(IX_REPO, IX_REMOVAL_SHA), COMMIT_IX_REMOVAL)
        .ok(&github_commit_url(IX_REPO, IX_SHA), COMMIT_IX);
    (aur, gh)
}

/// Утренний список правок (одна — добавление) и заданный ответ на запрос
/// каталога сценариев.
fn ql_listing_nets(listing: Result<&str, NetError>) -> (FakeNet, FakeNet) {
    let (aur, _) = ql_nets();
    let gh = FakeNet::default()
        .ok(&github_repo_url(IX_REPO), GH_IX)
        .ok(&github_workflows_url(IX_REPO, now() - WORKFLOW_SECS), WF_IX)
        .ok(&github_commit_url(IX_REPO, IX_SHA), COMMIT_IX);
    let url = github_workflow_dir_url(IX_REPO, "main");
    let gh = match listing {
        Ok(body) => gh.ok(&url, body),
        Err(e) => gh.fail(&url, e),
    };
    (aur, gh)
}

fn ql_signals(facts: TrustFacts) -> Vec<Signal> {
    let info = parse_info(&json(QL_INFO), "plasma6-applet-quicklaunch").unwrap();
    let entry = CacheEntry { facts: Some(facts), maintainers: Vec::new() };
    signals(&RpcView::from(&info), Some(&entry), now())
}

fn collect(info_json: &str, name: &str, nets: (FakeNet, FakeNet)) -> (TrustFacts, FakeNet, FakeNet, usize) {
    let info = parse_info(&json(info_json), name).unwrap();
    let (mut aur, mut gh) = nets;
    let mut pauses = 0usize;
    let mut pause = || pauses += 1;
    let facts = {
        let mut f = Fetchers { aur: &mut aur, github: &mut gh, pause: &mut pause };
        collect_facts(&mut f, &info, now())
    };
    (facts, aur, gh, pauses)
}

fn keys(s: &[Signal]) -> Vec<&'static str> {
    s.iter().map(|x| x.key).collect()
}

// ── Время ────────────────────────────────────────────────────────────────

#[test]
fn time_parsing_handles_both_forms_and_zones() {
    assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(
        parse_time("2026-10-08T20:08:55Z"),
        parse_time("2026-10-08 22:08:55 +0200")
    );
    assert_eq!(parse_time("2026-06-19 21:25:48 +0000"), Some(1_781_904_348));
    assert_eq!(iso_utc(parse_time("2026-10-08T20:08:55Z").unwrap()), "2026-10-08T20:08:55Z");
    assert_eq!(parse_time("вчера"), None);
    assert_eq!(parse_time("2024-02-29"), Some(1_709_164_800));
}

// ── Журнал cgit ──────────────────────────────────────────────────────────

#[test]
fn cgit_log_yay_same_people_no_newcomer() {
    let log = parse_cgit_log(LOG_YAY).unwrap();
    assert_eq!(log.len(), 50);
    assert_eq!(log[0].author, "Jo Garnier");
    assert_eq!(log[0].date, parse_time("2026-06-19 21:25:48 +0000").unwrap());
    assert_eq!(new_author(&log, now()), None);
}

#[test]
fn cgit_log_sweet_changed_hands() {
    let log = parse_cgit_log(LOG_SWEET).unwrap();
    assert_eq!(log.len(), 8);
    let a = new_author(&log, now()).expect("новое имя в журнале");
    assert_eq!(a.name, "gigas002");
    assert_eq!(a.before, "Duong Do Minh Chau");
    assert_eq!(rexon_core::aur::epoch_day(a.since), "2026-04-19");
    // Полгода спустя та же передача уже не «недавно».
    assert_eq!(new_author(&log, now() + RECENT_OWNER_SECS), None);
}

#[test]
fn cgit_log_blocked_page_is_none() {
    assert_eq!(parse_cgit_log("<html><title>Making sure you're not a bot!</title></html>"), None);
    // Пакет из одной свежей правки — судить не о чем.
    let log = parse_cgit_log(QL_LOG).unwrap();
    assert_eq!(log.len(), 2);
    assert_eq!(new_author(&log, now()), None);
}

// ── .SRCINFO ─────────────────────────────────────────────────────────────

#[test]
fn srcinfo_yay_release_files_with_checksums() {
    let s = parse_srcinfo(SRC_YAY).unwrap();
    assert_eq!(s.pkgbase, "yay-bin");
    assert_eq!(s.url, "https://github.com/Jguer/yay");
    assert_eq!(s.sources.len(), 3);
    assert!(s.sources.iter().all(|x| x.verified && x.vcs.is_empty() && x.remote()));
    assert!(unverified_sources(&s).is_empty());
    assert!(branch_sources(&s).is_empty());
    assert_eq!(github_repos(Some(&s), "https://github.com/Jguer/yay"), vec!["Jguer/yay"]);
}

#[test]
fn srcinfo_sweet_git_branch_is_normal_for_git_package() {
    let s = parse_srcinfo(SRC_SWEET).unwrap();
    assert_eq!(s.sources.len(), 1);
    let src = &s.sources[0];
    assert_eq!(src.vcs, "git");
    assert_eq!(src.url, "https://github.com/Gigas002/Sweet-kde.git");
    assert_eq!(src.fragment, "branch=plasma-6-migration");
    assert!(!src.verified);
    // `-git`: свежий код — его суть, а суммы у git-источника быть не может.
    assert!(unverified_sources(&s).is_empty());
    assert!(branch_sources(&s).is_empty());
    assert_eq!(github_repos(Some(&s), ""), vec!["Gigas002/Sweet-kde"]);
}

#[test]
fn srcinfo_unpinned_sources_of_release_package() {
    let s = parse_srcinfo(QL_SRCINFO).unwrap();
    assert_eq!(
        unverified_sources(&s),
        vec!["https://github.com/ixnewton/org.kde.plasma.quicklaunch/archive/refs/tags/v1.0.4.tar.gz"]
    );
    let branchy = parse_srcinfo(
        "pkgbase = foo\n\tsource = git+https://example.org/foo.git#branch=main\n\tsource = git+https://example.org/bar.git#commit=abc\n\tsource = local.patch\n\tsha256sums = SKIP\n\tsha256sums = SKIP\n\tsha256sums = SKIP\n",
    )
    .unwrap();
    assert_eq!(branch_sources(&branchy), vec!["git+https://example.org/foo.git#branch=main"]);
    // Файл рядом с PKGBUILD лежит в git самого AUR — без суммы не беда.
    assert!(unverified_sources(&branchy).is_empty());
    // Сумма по архитектуре засчитывается своей архитектуре.
    let arch = parse_srcinfo(
        "pkgbase = bar\n\tsource_x86_64 = https://e.org/a.tgz\n\tsha256sums_x86_64 = 00ff\n\tsource_aarch64 = https://e.org/b.tgz\n\tsha256sums_aarch64 = SKIP\n\tb2sums_aarch64 = SKIP\n",
    )
    .unwrap();
    assert_eq!(unverified_sources(&arch), vec!["https://e.org/b.tgz"]);
    assert_eq!(parse_srcinfo("<!doctype html><html>"), None);
}

#[test]
fn github_repo_from_various_addresses() {
    assert_eq!(github_repo_of("https://github.com/Jguer/yay").as_deref(), Some("Jguer/yay"));
    assert_eq!(
        github_repo_of("https://github.com/a/b/releases/download/v1/x.tar.gz").as_deref(),
        Some("a/b")
    );
    assert_eq!(github_repo_of("https://github.com/a/b.git").as_deref(), Some("a/b"));
    assert_eq!(github_repo_of("https://raw.githubusercontent.com/a/b/main/x").as_deref(), Some("a/b"));
    assert_eq!(github_repo_of("https://codeload.github.com/a/b/tar.gz/v1").as_deref(), Some("a/b"));
    assert_eq!(github_repo_of("https://gitlab.com/a/b"), None);
    assert_eq!(github_repo_of("https://github.com/a"), None);
    // Повтор без регистра и лимит.
    let s = parse_srcinfo(
        "pkgbase = x\n\tsource = https://github.com/A/B/archive/1.tar.gz\n\tsource = https://github.com/c/d/archive/1.tar.gz\n\tsource = https://github.com/e/f/archive/1.tar.gz\n",
    )
    .unwrap();
    assert_eq!(github_repos(Some(&s), "https://github.com/a/b"), vec!["A/B", "c/d"]);
}

// ── GitHub ───────────────────────────────────────────────────────────────

#[test]
fn github_repo_fork_and_original() {
    let yay = parse_github_repo("Jguer/yay", &json(GH_YAY));
    assert!(yay.checked && !yay.fork && yay.parent.is_empty());
    let sweet = parse_github_repo("Gigas002/Sweet-kde", &json(GH_SWEET));
    assert!(sweet.fork);
    assert_eq!(sweet.parent, "EliverLara/Sweet-kde");
}

#[test]
fn workflow_commits_skip_bots_and_old() {
    // yay: свежая правка сценариев — от робота обновления зависимостей,
    // остальные старше 30 дней.
    assert!(parse_workflow_commits(&json(WF_YAY), now()).is_empty());
    // Те же ответы месяцем раньше: правки человека видны, робот — нет.
    let earlier = parse_time("2026-08-20T00:00:00Z").unwrap();
    let seen = parse_workflow_commits(&json(WF_YAY), earlier);
    assert!(!seen.is_empty());
    assert!(seen.iter().all(|c| !c.author.ends_with("[bot]")));
    let ix = parse_workflow_commits(&json(WF_IX), now());
    assert_eq!(ix.len(), 1);
    assert_eq!(ix[0].author, "ixnewton");
    assert_eq!(ix[0].message, "Add security audit workflow");
    assert_eq!(parse_added_workflows(&json(COMMIT_IX)), vec![".github/workflows/security-audit.yml"]);
}

#[test]
fn maintainer_packages_count_and_first() {
    assert_eq!(parse_maintainer_packages(&json(MAINT_JGUER)).map(|p| p.0), Some(6));
    let (n, first) = parse_maintainer_packages(&json(MAINT_GIGAS)).unwrap();
    assert_eq!(n, 33);
    assert_eq!(first, 1_363_097_489);
    assert_eq!(parse_maintainer_packages(&json(r#"{"type":"error","error":"x"}"#)), None);
}

// ── Целиком: три пакета ──────────────────────────────────────────────────

#[test]
fn reliable_package_has_no_signs() {
    let (facts, aur, gh, pauses) = collect(INFO, "yay-bin", yay_nets());
    assert!(!facts.partial, "все ответы были: {facts:?}");
    assert!(facts.log_checked && facts.srcinfo_checked);
    assert_eq!(facts.maintainer_packages, Some(6));
    assert_eq!(facts.github.len(), 1);
    assert!(facts.github[0].workflows_checked);
    // К страницам AUR — с паузой перед каждой.
    assert_eq!(pauses, 3);
    assert_eq!(aur.asked.len(), 4);
    // Выкладывал сам сопровождающий — список сотрудников Arch не нужен.
    assert!(facts.page_checked && !facts.staff_packager);
    assert_eq!(facts.last_packager, "jguer");
    assert!(aur.asked.iter().all(|u| !u.contains("archlinux.org/people")));
    assert_eq!(gh.asked.len(), 2);
    let info = parse_info(&json(INFO), "yay-bin").unwrap();
    let entry = CacheEntry { facts: Some(facts), maintainers: Vec::new() };
    let s = signals(&RpcView::from(&info), Some(&entry), now());
    assert!(s.is_empty(), "{:?}", keys(&s));
    assert_eq!(level(&s), None);
    assert_eq!(badge_text(&s), "");
    assert!(status_text(entry.facts.as_ref(), &s).starts_with("✅"));
}

#[test]
fn fork_that_changed_hands_is_yellow() {
    let (facts, _, _, _) = collect(INFO, "sweet-kvantum-git", sweet_nets());
    assert!(!facts.partial);
    let info = parse_info(&json(INFO), "sweet-kvantum-git").unwrap();
    let entry = CacheEntry { facts: Some(facts), maintainers: Vec::new() };
    let s = signals(&RpcView::from(&info), Some(&entry), now());
    assert_eq!(keys(&s), vec!["new_author", "fork", "few_votes"]);
    assert_eq!(level(&s), Some(Level::Warn));
    assert!(s.iter().all(|x| !x.why.is_empty()));
    assert!(badge_text(&s).starts_with("⚠️ 3"));
    assert_eq!(tile_mark(&s), "⚠️ ");
}

#[test]
fn attack_pattern_is_red() {
    let (facts, _, gh, _) = collect(QL_INFO, "plasma6-applet-quicklaunch", ql_nets());
    assert!(!facts.partial, "{facts:?}");
    assert_eq!(gh.asked.len(), 4, "паспорт, правки сценариев, одна правка подробно, каталог сценариев");
    assert_eq!(gh.asked[3], "https://api.github.com/repos/ixnewton/org.kde.plasma.quicklaunch/contents/.github/workflows?ref=main");
    assert!(facts.github[0].workflow_changes[0].gone.is_empty());
    assert_eq!(facts.github[0].workflow_changes[0].added, vec![".github/workflows/security-audit.yml"]);
    let info = parse_info(&json(QL_INFO), "plasma6-applet-quicklaunch").unwrap();
    let entry = CacheEntry { facts: Some(facts), maintainers: Vec::new() };
    let s = signals(&RpcView::from(&info), Some(&entry), now());
    assert_eq!(s[0].key, "workflow_added");
    assert_eq!(s[0].level, Level::Danger);
    assert!(s[0].text.contains("security-audit.yml"));
    assert!(keys(&s).contains(&"new_maintainer"));
    assert!(keys(&s).contains(&"few_votes"));
    assert!(keys(&s).contains(&"unverified"));
    assert_eq!(level(&s), Some(Level::Danger));
    assert!(badge_text(&s).starts_with("⛔"));
}

#[test]
fn added_then_removed_workflow_is_yellow() {
    let (facts, _, gh, _) = collect(QL_INFO, "plasma6-applet-quicklaunch", ql_removed_nets());
    assert!(!facts.partial, "{facts:?}");
    // Удаление видно по раскрытой правке — запрос каталога не нужен.
    assert_eq!(gh.asked.len(), 4, "{:?}", gh.asked);
    assert!(gh.asked.iter().all(|u| !u.contains("/contents/")));
    let changes = &facts.github[0].workflow_changes;
    assert_eq!(changes[0].removed, vec![".github/workflows/security-audit.yml"]);
    assert_eq!(changes[1].gone, vec![".github/workflows/security-audit.yml"]);
    let s = ql_signals(facts);
    let wf = s.iter().find(|x| x.key == "workflow_added_removed").unwrap();
    assert_eq!(wf.level, Level::Warn);
    assert!(wf.text.contains("security-audit.yml") && wf.text.contains("2026-10-08"), "{}", wf.text);
    assert!(!wf.why.is_empty());
    assert!(!keys(&s).contains(&"workflow_added") && !keys(&s).contains(&"workflow_changed"));
    // Новый сопровождающий есть, но сочетание до красного не поднимает.
    assert!(keys(&s).contains(&"new_maintainer"));
    assert_eq!(level(&s), Some(Level::Warn));
    assert!(badge_text(&s).starts_with("⚠️"));
}

#[test]
fn workflow_gone_from_current_version_is_yellow() {
    // Правка удаления не попала в выборку, но каталога сценариев в `main`
    // уже нет (живой ответ вечера 09.10 — 404).
    let nf = NetError::Status { code: 404, reason: "Not Found".into() };
    let (facts, _, gh, _) = collect(QL_INFO, "plasma6-applet-quicklaunch", ql_listing_nets(Err(nf)));
    assert!(!facts.partial);
    assert_eq!(gh.asked.len(), 4);
    let s = ql_signals(facts);
    assert!(keys(&s).contains(&"workflow_added_removed"));
    assert_eq!(level(&s), Some(Level::Warn));
    // Каталог есть, но файла в нём нет — тоже удалён.
    let other = r#"[{"name":"build.yml","path":".github/workflows/build.yml","type":"file"}]"#;
    let (facts, _, _, _) = collect(QL_INFO, "plasma6-applet-quicklaunch", ql_listing_nets(Ok(other)));
    assert_eq!(level(&ql_signals(facts)), Some(Level::Warn));
}

#[test]
fn unverified_presence_stays_red() {
    let limit = NetError::Status { code: 403, reason: "rate limit exceeded".into() };
    for e in [offline(), limit] {
        let (facts, _, _, _) = collect(QL_INFO, "plasma6-applet-quicklaunch", ql_listing_nets(Err(e)));
        assert!(facts.partial);
        assert!(facts.github[0].workflow_changes[0].gone.is_empty());
        let s = ql_signals(facts);
        assert_eq!(s[0].key, "workflow_added");
        assert_eq!(level(&s), Some(Level::Danger));
    }
    // Лимит на раскрытии правки удаления: о файле ничего не известно.
    let limit = NetError::Status { code: 429, reason: "Too Many Requests".into() };
    let (aur, _) = ql_nets();
    let gh = FakeNet::default()
        .ok(&github_repo_url(IX_REPO), GH_IX)
        .ok(&github_workflows_url(IX_REPO, now() - WORKFLOW_SECS), WF_IX_REMOVED)
        .fail(&github_commit_url(IX_REPO, IX_REMOVAL_SHA), limit);
    let (facts, _, gh, _) = collect(QL_INFO, "plasma6-applet-quicklaunch", (aur, gh));
    assert_eq!(gh.asked.len(), 3, "после 429 к GitHub больше не ходим");
    assert!(facts.partial);
    assert!(!keys(&ql_signals(facts)).contains(&"workflow_added_removed"));
}

#[test]
fn removal_by_commits_needs_unbroken_newer_history() {
    let wf = ".github/workflows/x.yml".to_string();
    let add = WorkflowChange { added: vec![wf.clone()], detailed: true, ..Default::default() };
    let rm = WorkflowChange { removed: vec![wf.clone()], detailed: true, ..Default::default() };
    // Удалили, потом вернули — на месте.
    let mut v = vec![add.clone(), rm.clone(), add.clone()];
    assert!(resolve_gone_by_commits(&mut v));
    assert!(v[0].gone.is_empty() && v[2].gone.is_empty());
    // Новее добавления есть нераскрытая правка — неизвестно.
    let mut v = vec![WorkflowChange::default(), rm.clone(), add.clone()];
    assert!(resolve_gone_by_commits(&mut v));
    assert!(v[2].gone.is_empty());
    // Добавили и удалили — удалён.
    let mut v = vec![rm, add];
    assert!(!resolve_gone_by_commits(&mut v));
    assert_eq!(v[1].gone, vec![wf]);
}

#[test]
fn changed_workflow_turns_red_with_suspicious_owner() {
    let facts = TrustFacts {
        maintainer_packages: Some(1),
        github: vec![GithubFacts {
            repo: "a/b".into(),
            checked: true,
            fork: true,
            workflows_checked: true,
            workflow_changes: vec![WorkflowChange { date: now() - 3600, author: "x".into(), ..Default::default() }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let entry = CacheEntry { facts: Some(facts), maintainers: Vec::new() };
    let rpc = RpcView { maintainer: "m".into(), votes: 100, popularity: 3.0, out_of_date: 0 };
    let s = signals(&rpc, Some(&entry), now());
    let wf = s.iter().find(|x| x.key == "workflow_changed").unwrap();
    assert_eq!(wf.level, Level::Danger);
}

// ── Пакет, вычищенный сотрудником Arch ───────────────────────────────────

/// Паспорта всех 13 пакетов учётки — тем запросом, каким его делает проверка.
fn fenne_info_url() -> String {
    let list = parse_maintainer_list(&json(MAINT_FENNE)).unwrap();
    let names = trust_info_names(&list, now_xsp());
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    rexon_core::aur::info_url(&refs)
}

fn xsp_nets() -> FakeNet {
    FakeNet::default()
        .ok(&fenne_info_url(), INFO_FENNE)
        .ok(&maintainer_url("fennekuiper"), MAINT_FENNE)
        .ok(&cgit_log_url("xsp"), LOG_XSP)
        .ok(&package_page_url("xsp"), PAGE_XSP)
        .ok(&srcinfo_url("xsp"), SRC_XSP)
        .ok(STAFF_PAGES[0], STAFF_PM)
        .ok(STAFF_PAGES[1], STAFF_DEV)
}

fn collect_xsp(aur: FakeNet, staff: &mut StaffList) -> (TrustFacts, FakeNet) {
    let info = parse_info(&json(INFO_XSP), "xsp").unwrap();
    let (mut aur, mut gh) = (aur, FakeNet::default());
    let mut pause = || {};
    let facts = {
        let mut f = Fetchers { aur: &mut aur, github: &mut gh, pause: &mut pause };
        collect_facts_with(&mut f, &info, now_xsp(), staff)
    };
    (facts, aur)
}

#[test]
fn package_page_last_packager() {
    let xsp = parse_package_page(PAGE_XSP).unwrap();
    assert_eq!(xsp.maintainer, "fennekuiper");
    assert!(xsp.comaintainers.is_empty());
    assert_eq!(xsp.last_packager, "tippfehlr");
    assert!(packager_is_outsider(&xsp, "fennekuiper"));
    let yay = parse_package_page(PAGE_YAY).unwrap();
    assert_eq!((yay.maintainer.as_str(), yay.last_packager.as_str()), ("jguer", "jguer"));
    assert!(!packager_is_outsider(&yay, "jguer"));
    // Помощник сопровождающего — свой человек; удалённая учётка — «None».
    let co = parse_package_page(
        "<div id=\"pkgdetails\"><tr class=\"pkgmaint\"><th>Maintainer:</th><td>\n  alice\n  (bob, Carol)\n</td></tr><tr><th>Last Packager:</th><td>\n carol \n</td></tr>",
    )
    .unwrap();
    assert_eq!(co.comaintainers, vec!["bob", "Carol"]);
    assert!(!packager_is_outsider(&co, "alice"));
    let gone = parse_package_page("<div id=\"pkgdetails\"><th>Maintainer:</th><td>None</td><th>Last Packager:</th><td>None</td>").unwrap();
    assert_eq!(gone.last_packager, "");
    assert!(!packager_is_outsider(&gone, ""));
    assert_eq!(parse_package_page("<html><title>Making sure you're not a bot!</title></html>"), None);
    assert_eq!(package_page_url("a+b"), "https://aur.archlinux.org/packages/a%2Bb");
}

#[test]
fn staff_pages_list_usernames() {
    let pm = parse_staff_page(STAFF_PM).unwrap();
    assert_eq!(pm, vec!["alex19ep", "tippfehlr"]);
    let dev = parse_staff_page(STAFF_DEV).unwrap();
    assert!(dev.contains(&"allan".to_string()), "{dev:?}");
    assert_eq!(parse_staff_page("<html>Anubis</html>"), None);
    let list = StaffList { names: pm, checked_at: now_xsp() };
    assert!(list.contains("Tippfehlr") && !list.contains("fennekuiper"));
    assert!(list.fresh(now_xsp()) && !list.fresh(now_xsp() + STAFF_TTL_SECS));
}

#[test]
fn cleaned_by_staff_is_a_sign() {
    let mut staff = StaffList::default();
    let (facts, aur) = collect_xsp(xsp_nets(), &mut staff);
    assert!(facts.page_checked && facts.staff_packager, "{facts:?}");
    assert_eq!(facts.last_packager, "tippfehlr");
    assert!(staff.fresh(now_xsp()) && staff.contains("tippfehlr"));
    assert_eq!(aur.asked.iter().filter(|u| u.contains("archlinux.org/people")).count(), 2);
    // Сам по себе (история не переписана) — жёлтый.
    let only_staff = TrustFacts { tip_date: None, ..facts.clone() };
    let s = deep_signals(&only_staff, None, now_xsp());
    let sig = s.iter().find(|x| x.key == "staff_packager").expect("признак вычистки");
    assert_eq!(sig.level, Level::Warn);
    assert!(sig.text.contains("tippfehlr") && !sig.why.is_empty());
    // Свежий список сотрудников второй раз не качается.
    let (again, aur) = collect_xsp(xsp_nets(), &mut staff);
    assert!(again.staff_packager);
    assert!(aur.asked.iter().all(|u| !u.contains("archlinux.org/people")));
}

#[test]
fn staff_list_unavailable_is_partial_not_a_sign() {
    let aur = xsp_nets().fail(STAFF_PAGES[1], offline());
    let (facts, _) = collect_xsp(aur, &mut StaffList::default());
    assert!(facts.partial && facts.page_checked && !facts.staff_packager);
    // Прежний сопровождающий (не сотрудник) — не признак.
    let page = PAGE_XSP.replace("tippfehlr", "oldowner");
    let aur = xsp_nets().ok(&package_page_url("xsp"), &page);
    let (facts, _) = collect_xsp(aur, &mut StaffList::default());
    assert!(facts.page_checked && !facts.staff_packager);
    assert!(!keys(&deep_signals(&facts, None, now_xsp())).contains(&"staff_packager"));
}

#[test]
fn staff_list_lives_in_disk_cache() {
    let dir = std::env::temp_dir().join(format!("rexon-aur-trust-staff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join(CACHE_FILE).to_string_lossy().into_owned();
    let info = parse_info(&json(INFO_XSP), "xsp").unwrap();
    let mut pause = || {};
    let mut aur = xsp_nets();
    let mut gh = FakeNet::default();
    {
        let mut f = Fetchers { aur: &mut aur, github: &mut gh, pause: &mut pause };
        check_cached(&path, &mut f, &info, now_xsp());
    }
    assert!(load_cache(&path).staff.contains("tippfehlr"));
    assert!(cached_staff(&path).fresh(now_xsp()));
    let _ = std::fs::remove_dir_all(&dir);
}

// ── История пакета переписана ────────────────────────────────────────────

#[test]
fn xsp_history_rewritten_and_cleaned_is_red() {
    let (facts, _) = collect_xsp(xsp_nets(), &mut StaffList::default());
    // Верхушка журнала — правка 01.07.2024, а AUR пишет «обновлён 12.06.2026».
    assert_eq!(facts.tip_date, parse_time("2024-07-01 20:30:55 +0200"));
    assert_eq!(facts.last_modified, 1_781_270_776);
    assert!(history_rewritten(&facts));
    let s = deep_signals(&facts, None, now_xsp());
    let k = keys(&s);
    // Сотрудник Arch + переписанная история — одно красное «взломан и вычищен».
    assert!(k.contains(&"compromised_cleaned"), "{k:?}");
    assert!(!k.contains(&"staff_packager") && !k.contains(&"history_rewritten"), "{k:?}");
    let sig = s.iter().find(|x| x.key == "compromised_cleaned").unwrap();
    assert_eq!(sig.level, Level::Danger);
    assert!(sig.text.contains("tippfehlr") && sig.text.contains("2026-06-12"), "{}", sig.text);
}

#[test]
fn history_gap_alone_is_yellow() {
    let lm = now() - 30 * 86_400;
    let base = TrustFacts {
        checked_at: now(),
        log_checked: true,
        last_modified: lm,
        tip_date: Some(lm - HISTORY_GAP_SECS - 1),
        ..Default::default()
    };
    let s = deep_signals(&base, None, now());
    assert_eq!(keys(&s), vec!["history_rewritten"]);
    assert_eq!(s[0].level, Level::Warn);
    assert!(!s[0].why.is_empty());
    // Выложили через несколько дней после правки — обычное дело.
    let late_push = TrustFacts { tip_date: Some(lm - 3 * 86_400), ..base.clone() };
    assert!(!history_rewritten(&late_push));
    // Только что обновлён: страница журнала может отставать (кеш сайта).
    let fresh = TrustFacts { checked_at: lm + CGIT_LAG_SECS - 1, ..base.clone() };
    assert!(!history_rewritten(&fresh));
    // Журнал не прочитан или пуст — судить не о чем.
    assert!(!history_rewritten(&TrustFacts { tip_date: None, ..base.clone() }));
    assert!(!history_rewritten(&TrustFacts { last_modified: 0, ..base }));
}

#[test]
fn yay_history_is_consistent() {
    let (facts, _, _, _) = collect(INFO, "yay-bin", yay_nets());
    assert!(facts.tip_date.is_some());
    assert!(!history_rewritten(&facts), "{facts:?}");
}

#[test]
fn new_upload_refreshes_cached_check() {
    let dir = std::env::temp_dir().join(format!("rexon-aur-trust-lm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join(CACHE_FILE).to_string_lossy().into_owned();
    let mut info = parse_info(&json(INFO), "yay-bin").unwrap();
    let mut pause = || {};
    let (mut aur, mut gh) = yay_nets();
    {
        let mut f = Fetchers { aur: &mut aur, github: &mut gh, pause: &mut pause };
        check_cached(&path, &mut f, &info, now());
    }
    // Вышла новая версия: проверка с диска уже не годится.
    info.last_modified += 3600;
    let (mut aur2, mut gh2) = yay_nets();
    let e = {
        let mut f = Fetchers { aur: &mut aur2, github: &mut gh2, pause: &mut pause };
        check_cached(&path, &mut f, &info, now() + 7200)
    };
    assert!(!aur2.asked.is_empty());
    assert_eq!(e.facts.unwrap().last_modified, info.last_modified);
    let _ = std::fs::remove_dir_all(&dir);
}

// ── Новая учётка массово подобрала старые пакеты ─────────────────────────

fn pkg(name: &str, submitter: Option<&str>, first: i64, last: i64) -> MaintPkg {
    MaintPkg {
        name: name.into(),
        first_submitted: first,
        last_modified: last,
        submitter: submitter.map(str::to_string),
    }
}

#[test]
fn fennekuiper_mass_adoption() {
    let list = parse_maintainer_list(&json(MAINT_FENNE)).unwrap();
    assert_eq!(list.len(), 13);
    // По одному поиску видно 13 старых пакетов, обновлённых за одну неделю.
    let names = adoption_info_names(&list, now_xsp());
    assert_eq!(names.len(), 13);
    let mut list = list;
    fill_submitters(&mut list, &json(INFO_FENNE));
    assert!(list.iter().all(|p| p.submitter.is_some()));
    let m = mass_adoption("fennekuiper", &list, now_xsp()).expect("массовое усыновление");
    assert_eq!(m.count, 13);
    assert_eq!(rexon_core::aur::epoch_day(m.since), "2026-06-12");
}

#[test]
fn xsp_all_three_signs_red() {
    let (facts, aur) = collect_xsp(xsp_nets(), &mut StaffList::default());
    assert!(aur.asked.contains(&fenne_info_url()));
    assert_eq!(facts.mass_adoption.as_ref().map(|m| m.count), Some(13));
    let info = parse_info(&json(INFO_XSP), "xsp").unwrap();
    let entry = CacheEntry { facts: Some(facts), maintainers: Vec::new() };
    let s = signals(&RpcView::from(&info), Some(&entry), now_xsp());
    let k = keys(&s);
    assert!(k.contains(&"compromised_cleaned") && k.contains(&"mass_adoption"), "{k:?}");
    let m = s.iter().find(|x| x.key == "mass_adoption").unwrap();
    // Вместе с вычисткой — красный.
    assert_eq!(m.level, Level::Danger);
    assert!(m.text.contains("fennekuiper") && m.text.contains("13"), "{}", m.text);
    assert!(!m.why.is_empty());
}

#[test]
fn mass_adoption_thresholds() {
    let now = now_xsp();
    let day = 86_400;
    let old = now - 3000 * day;
    let burst = now - 30 * day;
    let adopted = |n: usize| -> Vec<MaintPkg> {
        (0..n).map(|i| pkg(&format!("p{i}"), Some("someone"), old, burst + i as i64 * 3600)).collect()
    };
    // Порог: MASS_ADOPT пакетов за неделю.
    assert!(mass_adoption("m", &adopted(MASS_ADOPT), now).is_some());
    assert!(mass_adoption("m", &adopted(MASS_ADOPT - 1), now).is_none());
    // Растянуто на месяцы — не «за короткое время».
    let spread: Vec<MaintPkg> =
        (0..12).map(|i| pkg(&format!("s{i}"), Some("x"), old, burst - i as i64 * 10 * day)).collect();
    assert!(mass_adoption("m", &spread, now).is_none());
    // У учётки есть свой пакет, выложенный задолго до того, — учётка не новая.
    let mut veteran = adopted(10);
    veteran.push(pkg("own", Some("M"), old, old));
    assert!(mass_adoption("m", &veteran, now).is_none());
    // Свои пакеты, выложенные недавно, — не помеха признаку.
    let mut fresh = adopted(10);
    fresh.push(pkg("own", Some("m"), burst - 5 * day, burst));
    assert!(mass_adoption("m", &fresh, now).is_some());
    // Свои пакеты и неизвестные (паспорт не получен) — не «подобранные».
    let own: Vec<MaintPkg> = (0..10).map(|i| pkg(&format!("o{i}"), Some("m"), old, burst)).collect();
    assert!(mass_adoption("m", &own, now).is_none());
    let unknown: Vec<MaintPkg> = (0..10).map(|i| pkg(&format!("u{i}"), None, old, burst)).collect();
    assert!(mass_adoption("m", &unknown, now).is_none());
    // Давно — уже не признак.
    assert!(mass_adoption("m", &adopted(10), now + RECENT_OWNER_SECS).is_none());
    // Сам по себе — жёлтый.
    let facts = TrustFacts {
        maintainer: "m".into(),
        mass_adoption: mass_adoption("m", &adopted(10), now),
        ..Default::default()
    };
    let s = deep_signals(&facts, None, now);
    assert_eq!(keys(&s), vec!["mass_adoption"]);
    assert_eq!(s[0].level, Level::Warn);
}

#[test]
fn ordinary_maintainers_need_no_extra_request() {
    // jguer и Gigas002: подборки нет — паспортов пакетов не просим.
    for m in [MAINT_JGUER, MAINT_GIGAS] {
        let list = parse_maintainer_list(&json(m)).unwrap();
        assert!(adoption_info_names(&list, now()).is_empty());
    }
}

// ── Возраст учётки — по своим пакетам, а не по подобранным ──────────────

#[test]
fn account_age_needs_info_only_when_in_doubt() {
    // jguer и Gigas002: есть пакеты, которые не обновлялись больше полугода,
    // — учётка точно не новая, паспортов не просим.
    for m in [MAINT_JGUER, MAINT_GIGAS] {
        let list = parse_maintainer_list(&json(m)).unwrap();
        assert!(account_age_info_names(&list, now()).is_empty());
        assert!(trust_info_names(&list, now()).is_empty());
    }
    // fennekuiper: все 13 пакетов старые, но обновлены только что — сомнение.
    let list = parse_maintainer_list(&json(MAINT_FENNE)).unwrap();
    assert_eq!(account_age_info_names(&list, now_xsp()).len(), 13);
    // Оба признака спрашивают одним запросом.
    assert_eq!(trust_info_names(&list, now_xsp()), adoption_info_names(&list, now_xsp()));
}

#[test]
fn fennekuiper_is_new_by_own_packages() {
    let mut list = parse_maintainer_list(&json(MAINT_FENNE)).unwrap();
    // Без паспортов — как раньше, по самому старому пакету (2008).
    assert_eq!(account_since("fennekuiper", &list).map(rexon_core::aur::epoch_day).as_deref(), Some("2008-02-01"));
    fill_submitters(&mut list, &json(INFO_FENNE));
    // Своих нет — учётка проявилась, когда обновила подобранные.
    assert_eq!(account_since("fennekuiper", &list).map(rexon_core::aur::epoch_day).as_deref(), Some("2026-06-12"));
    // Целиком: один запрос паспортов, признак «новый сопровождающий».
    let (facts, aur) = collect_xsp(xsp_nets(), &mut StaffList::default());
    assert_eq!(aur.asked.iter().filter(|u| u.contains("/info?")).count(), 1, "{:?}", aur.asked);
    assert_eq!(facts.maintainer_since.map(rexon_core::aur::epoch_day).as_deref(), Some("2026-06-12"));
    let s = deep_signals(&facts, None, now_xsp());
    let m = s.iter().find(|x| x.key == "new_maintainer").expect("новый сопровождающий");
    assert!(m.text.contains("13") && m.text.contains("2026-06-12"), "{}", m.text);
}

#[test]
fn honest_adopter_with_old_own_packages_is_not_new() {
    let now = now_xsp();
    let day = 86_400;
    // Свой пакет выложен три года назад, подобранные — десять лет назад;
    // всё обновлено в последние недели (поиск один не решает).
    let mut list = vec![
        pkg("own", None, now - 3 * 365 * day, now - 10 * day),
        pkg("a1", None, now - 3650 * day, now - 20 * day),
        pkg("a2", None, now - 3650 * day, now - 40 * day),
        pkg("a3", None, now - 3000 * day, now - 60 * day),
    ];
    let names = account_age_info_names(&list, now);
    assert_eq!(names, vec!["a1", "a2", "a3", "own"]);
    for p in list.iter_mut() {
        p.submitter = Some(if p.name == "own" { "Honest".into() } else { "someone".into() });
    }
    let since = account_since("honest", &list).unwrap();
    assert_eq!(since, now - 3 * 365 * day);
    let facts = TrustFacts {
        maintainer: "honest".into(),
        maintainer_packages: Some(list.len() as i64),
        maintainer_since: Some(since),
        ..Default::default()
    };
    assert!(!keys(&deep_signals(&facts, None, now)).contains(&"new_maintainer"));
    // Паспорта не получены — по дате выкладки, как раньше: не объявлять
    // учётку новой без оснований.
    for p in list.iter_mut() {
        p.submitter = None;
    }
    assert_eq!(account_since("honest", &list), Some(now - 3650 * day));
    // Все пакеты свежие — новая без всякого запроса.
    let fresh = vec![pkg("n", None, now - 30 * day, now - 5 * day)];
    assert!(account_age_info_names(&fresh, now).is_empty());
    assert_eq!(account_since("m", &fresh), Some(now - 30 * day));
}

// ── Сбои: молча пропустить ───────────────────────────────────────────────

#[test]
fn offline_check_is_partial_and_silent() {
    let (facts, _, _, _) = collect(INFO, "yay-bin", (FakeNet::default(), FakeNet::default()));
    assert!(facts.partial);
    assert!(!facts.log_checked && !facts.srcinfo_checked);
    assert_eq!(facts.maintainer_packages, None);
    // Без источников остаётся страница проекта — но и её GitHub не дал.
    assert!(facts.github.is_empty());
    let info = parse_info(&json(INFO), "yay-bin").unwrap();
    let entry = CacheEntry { facts: Some(facts.clone()), maintainers: Vec::new() };
    assert!(signals(&RpcView::from(&info), Some(&entry), now()).is_empty());
    assert!(facts.fresh(now()) && !facts.fresh(now() + PARTIAL_TTL_SECS));
}

#[test]
fn github_limit_stops_further_github_calls() {
    let limit = NetError::Status { code: 403, reason: "rate limit exceeded".into() };
    let (aur, _) = yay_nets();
    let gh = FakeNet::default().fail(&github_repo_url("Jguer/yay"), limit);
    let (facts, _, gh, _) = collect(INFO, "yay-bin", (aur, gh));
    assert!(facts.partial);
    assert_eq!(gh.asked.len(), 1);
    assert!(facts.github.is_empty());
}

#[test]
fn missing_repo_is_a_sign() {
    let (aur, _) = yay_nets();
    let gh = FakeNet::default()
        .fail(&github_repo_url("Jguer/yay"), NetError::Status { code: 404, reason: "Not Found".into() });
    let (facts, _, _, _) = collect(INFO, "yay-bin", (aur, gh));
    assert!(facts.github[0].missing);
    let s = deep_signals(&facts, None, now());
    assert_eq!(keys(&s), vec!["repo_missing"]);
}

// ── Плитка: только RPC ───────────────────────────────────────────────────

#[test]
fn rpc_thresholds() {
    let v = |votes, popularity, maintainer: &str, ood| RpcView {
        maintainer: maintainer.into(),
        votes,
        popularity,
        out_of_date: ood,
    };
    assert_eq!(keys(&rpc_signals(&v(4, 9.0, "m", 0))), vec!["few_votes"]);
    assert_eq!(keys(&rpc_signals(&v(10, 0.05, "m", 0))), vec!["few_votes"]);
    assert!(rpc_signals(&v(10, 0.5, "m", 0)).is_empty());
    assert!(rpc_signals(&v(25, 0.0, "m", 0)).is_empty());
    assert_eq!(keys(&rpc_signals(&v(50, 1.0, "", 0))), vec!["orphaned"]);
    assert_eq!(keys(&rpc_signals(&v(50, 1.0, "m", 1))), vec!["out_of_date"]);
    let tip = tile_tooltip(&rpc_signals(&v(0, 0.0, "", 0)), false);
    assert_eq!(tip.lines().count(), 4, "{tip}");
}

// ── Снимки сопровождающего и кеш на диске ────────────────────────────────

#[test]
fn maintainer_snapshots_notice_change() {
    let mut e = CacheEntry::default();
    let t0 = now() - 10 * 86_400;
    e.record_maintainer("alice", t0);
    e.record_maintainer("alice", t0 + 100);
    assert_eq!(e.maintainers.len(), 1);
    assert_eq!(e.maintainer_change(now()), None);
    e.record_maintainer("mallory", now());
    let c = e.maintainer_change(now()).unwrap();
    assert_eq!((c.from.as_str(), c.to.as_str()), ("alice", "mallory"));
    let s = signals(&RpcView { maintainer: "mallory".into(), votes: 99, popularity: 5.0, out_of_date: 0 }, Some(&e), now());
    assert_eq!(keys(&s), vec!["maintainer_changed"]);
    assert_eq!(s[0].level, Level::Danger);
    // Брошен — не смена; подобран — смена.
    let mut o = CacheEntry::default();
    o.record_maintainer("bob", t0);
    o.record_maintainer("", t0 + 1);
    assert_eq!(o.maintainer_change(now()), None);
    o.record_maintainer("eve", now());
    assert_eq!(o.maintainer_change(now()).unwrap().from, "");
    // Через полгода — уже не «недавно».
    assert_eq!(e.maintainer_change(now() + RECENT_OWNER_SECS + 1), None);
}

#[test]
fn disk_cache_serves_second_open_without_network() {
    let dir = std::env::temp_dir().join(format!("rexon-aur-trust-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("rexon").join(CACHE_FILE).to_string_lossy().into_owned();
    let info = parse_info(&json(INFO), "yay-bin").unwrap();

    let (mut aur, mut gh) = yay_nets();
    let mut pause = || {};
    let first = {
        let mut f = Fetchers { aur: &mut aur, github: &mut gh, pause: &mut pause };
        check_cached(&path, &mut f, &info, now())
    };
    assert!(first.facts.is_some());
    assert_eq!(first.maintainers.len(), 1);
    assert!(std::path::Path::new(&path).exists());
    let on_disk = load_cache(&path);
    assert_eq!(on_disk.version, CACHE_VERSION);
    assert!(on_disk.packages.contains_key("yay-bin"));

    let (mut aur2, mut gh2) = (FakeNet::default(), FakeNet::default());
    let second = {
        let mut f = Fetchers { aur: &mut aur2, github: &mut gh2, pause: &mut pause };
        check_cached(&path, &mut f, &info, now() + 3600)
    };
    assert!(aur2.asked.is_empty() && gh2.asked.is_empty(), "сутки не прошли — сеть не нужна");
    assert_eq!(second.facts, first.facts);

    // Через сутки — снова в сеть.
    let (mut aur3, mut gh3) = yay_nets();
    {
        let mut f = Fetchers { aur: &mut aur3, github: &mut gh3, pause: &mut pause };
        check_cached(&path, &mut f, &info, now() + CACHE_TTL_SECS + 1);
    }
    assert!(!aur3.asked.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pkgbuild_fetch_and_bot_page() {
    let mut net = FakeNet::default().ok(&pkgbuild_url("yay-bin"), "pkgname=yay-bin\n");
    assert_eq!(fetch_pkgbuild(&mut net, "yay-bin").unwrap(), "pkgname=yay-bin\n");
    let mut blocked = FakeNet::default().ok(&pkgbuild_url("x"), "<!doctype html><html>Anubis</html>");
    assert!(fetch_pkgbuild(&mut blocked, "x").is_err());
    assert!(fetch_pkgbuild(&mut FakeNet::default(), "x").is_err());
    assert_eq!(pkgbuild_url("a+b"), "https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h=a%2Bb");
}

#[test]
fn desktop_notice_lists_only_risky() {
    let rows = vec![
        ("ok-pkg".to_string(), Vec::new()),
        ("brisk-menu".to_string(), rpc_signals(&RpcView { maintainer: String::new(), votes: 50, popularity: 1.0, out_of_date: 0 })),
    ];
    let n = desktop_notice(&rows);
    assert!(n.contains("brisk-menu") && !n.contains("ok-pkg"));
    assert_eq!(desktop_notice(&rows[..1]), "");
}

#[test]
fn desktop_rpc_views_one_request_then_memory() {
    let names = vec!["yay-bin".to_string(), "sweet-kvantum-git".to_string()];
    let url = rexon_core::aur::info_url(&["yay-bin", "sweet-kvantum-git"]);
    let mut net = FakeNet::default().ok(&url, INFO);
    let v = rpc_views(Some(&mut net), &names, now());
    assert_eq!(v.len(), 2);
    assert_eq!(net.asked.len(), 1, "один запрос на весь набор стола");
    let mut again = FakeNet::default();
    assert_eq!(rpc_views(Some(&mut again), &names, now() + 60).len(), 2);
    assert!(again.asked.is_empty(), "15 минут — из памяти");
    // Без сети годится и старый ответ.
    assert_eq!(rpc_views(None, &names, now() + 10 * 3600).len(), 2);
    let path = std::env::temp_dir()
        .join(format!("rexon-aur-trust-none-{}/x.json", std::process::id()))
        .to_string_lossy()
        .into_owned();
    let n = desktop_notice_for(&names, &v, &path, now());
    assert!(n.contains("sweet-kvantum-git") && !n.contains("yay-bin"), "{n}");
}
