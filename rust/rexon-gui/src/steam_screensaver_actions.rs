// SteamScreensaverActions — the object of the "Screensaver" sub-tab of the
// "Steam" tab: the screensavers of the Steam Big Picture idle screen as cards
// with a preview (Steam's own, the person's own and the Rexon modes) and the
// Rexon screensaver itself.
//
// Decisions (what to collect, where to put it, whose folder not to touch,
// which screensavers Steam has and what it calls them, all labels and
// options) belong to the core `rexon_core::steam_screensaver`. Here there is
// only the executor: counting what was found, page snapshots, building the
// preview page, installing, updating and removing run in worker threads (the
// pictures are scaled down, a browser takes a snapshot — seconds), and the
// markup polls `poll()` with a timer without blocking.
//
// Never: Rexon does not touch the screensaver choice in Steam
// (`screensaver_current_id`), neither here nor in the core: it is only read to
// mark the card; the user picks the screensaver in the Steam settings — on
// SteamOS too.
//
// Numbers do not travel in invokable signatures (see the warning in
// `steam_page.rs`): the settings arrive as one JSON string.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type SteamScreensaverActions = super::SteamScreensaverActionsRust;
    }

    extern "RustQt" {
        /// Options and limits: JSON `{modes, logos, intervalMin, intervalMax, howTo,
        /// restartHint, sizes…}`.
        #[qinvokable]
        fn choices(self: &SteamScreensaverActions) -> QString;

        /// State across all Steam roots: JSON `{steamFound, installed, foreign,
        /// text, options, updated, roots: [{label, installed, foreign, slides}]}`.
        /// Only reading markers — instant.
        #[qinvokable]
        fn status(self: &SteamScreensaverActions) -> QString;

        /// The screensaver cards: JSON `[{key, origin, preview, title, badge,
        /// state, current, installed, thumb, hint}]`. Lists folders and reads the
        /// Steam choice — fast; snapshots come later through `poll()`.
        #[qinvokable]
        fn catalog(self: &SteamScreensaverActions) -> QString;

        /// Take snapshots of the HTML screensavers that have none in the cache yet.
        /// The result goes into `poll()` under the key `snapshots`.
        #[qinvokable]
        #[cxx_name = "startSnapshots"]
        fn start_snapshots(self: &SteamScreensaverActions) -> QString;

        /// Count how many pictures will be collected with the settings `request`
        /// (settings JSON). The result goes into `poll()` under the key `scan`.
        #[qinvokable]
        #[cxx_name = "startScan"]
        fn start_scan(self: &SteamScreensaverActions, request: &QString) -> QString;

        /// Install or update the screensaver with the settings `request`.
        /// "" means it started, otherwise the reason.
        #[qinvokable]
        #[cxx_name = "startInstall"]
        fn start_install(self: &SteamScreensaverActions, request: &QString) -> QString;

        /// Remove the Rexon screensaver. "" means it started, otherwise the reason.
        #[qinvokable]
        #[cxx_name = "startRemove"]
        fn start_remove(self: &SteamScreensaverActions) -> QString;

        /// Show the screensaver of card `key` over the whole screen (a Rexon mode —
        /// with the settings `request`, before installing). "" means it started,
        /// otherwise the reason; the end of preparing goes into `poll()` under `show`.
        #[qinvokable]
        #[cxx_name = "startShow"]
        fn start_show(self: &SteamScreensaverActions, key: &QString, request: &QString) -> QString;

        /// Task state: JSON `{scan: {...}, job: {...}, snapshots: {...}, show: {...}}`.
        #[qinvokable]
        fn poll(self: &SteamScreensaverActions) -> QString;
    }
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use rexon_core::i18n::tr;
use rexon_core::qt_steam as qs;
use rexon_core::steam_screensaver as ss;
use serde_json::{json, Value};

use crate::steam_showcase_actions::file_url;

/// Counting what was found.
#[derive(Default, Clone)]
struct ScanJob {
    busy: bool,
    serial: u64,
    result: Option<ScanResult>,
}

#[derive(Default, Clone)]
struct ScanResult {
    art: usize,
    shots: usize,
    text: String,
    /// Pictures of the Rexon slideshow with the current settings.
    pictures: Vec<Value>,
    /// Pictures of the Steam game slideshow (art and screenshots).
    steam_pictures: Vec<Value>,
}

/// Installation or removal.
#[derive(Default, Clone)]
struct InstallJob {
    busy: bool,
    serial: u64,
    /// (did it work, a message for the user).
    result: Option<(bool, String)>,
}

/// Page snapshots: card key → picture address.
#[derive(Default, Clone)]
struct SnapshotJob {
    busy: bool,
    serial: u64,
    shots: HashMap<String, String>,
    /// Why there are no snapshots (no browser), once.
    note: String,
}

/// Preparing a full-screen look.
#[derive(Default, Clone)]
struct ShowJob {
    busy: bool,
    serial: u64,
    /// (did it start, a reason when not).
    result: Option<(bool, String)>,
}

#[derive(Default)]
pub struct SteamScreensaverActionsRust {
    scan: Arc<Mutex<ScanJob>>,
    job: Arc<Mutex<InstallJob>>,
    snaps: Arc<Mutex<SnapshotJob>>,
    show: Arc<Mutex<ShowJob>>,
    /// Pages of the HTML cards from the last `catalog()`: key → `index.html`.
    pages: Mutex<HashMap<String, PathBuf>>,
}

fn qjson(v: &Value) -> QString {
    QString::from(&v.to_string())
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

fn roots() -> Vec<PathBuf> {
    ss::steam_roots(&home())
}

fn cache() -> PathBuf {
    ss::cache_dir(&home())
}

/// Settings from the markup on top of those the screensaver is already
/// built with.
fn options_of(request: &QString, base: &ss::Options) -> ss::Options {
    let v: Value = serde_json::from_str(&request.to_string()).unwrap_or(Value::Null);
    ss::Options::from_ui(&v, base)
}

/// Settings of the installed screensaver (the first one found), otherwise
/// the defaults.
fn installed_options() -> ss::Options {
    roots()
        .iter()
        .map(|r| ss::status(r))
        .find(|s| s.installed)
        .map(|s| s.options)
        .unwrap_or_default()
}

fn status_json() -> Value {
    let statuses: Vec<ss::Status> = roots().iter().map(|r| ss::status(r)).collect();
    let installed = statuses.iter().find(|s| s.installed);
    json!({
        "steamFound": !statuses.is_empty(),
        "installed": installed.is_some(),
        "foreign": statuses.iter().any(|s| s.foreign),
        "text": ss::status_text(&statuses),
        "options": installed.map(|s| s.options.clone()).unwrap_or_default().to_ui(),
        "updated": installed.map_or(0, |s| s.updated),
        "roots": statuses.iter().map(|s| json!({
            "label": ss::root_label(std::path::Path::new(&s.root)),
            "installed": s.installed,
            "foreign": s.foreign,
            "slides": s.slides,
        })).collect::<Vec<_>>(),
    })
}

/// Page language tag — the Rexon interface language.
fn page_locale() -> String {
    ss::page_locale(&rexon_core::i18n::current_language())
}

/// The root pictures are taken from: the one with the most found (usually
/// there is one); the installation collects them the same way.
fn richest_root(lang: &str) -> Option<(PathBuf, Vec<ss::GameArt>, Vec<ss::ShotFile>)> {
    roots()
        .into_iter()
        .map(|r| {
            let (g, s) = ss::collect(&r, lang);
            (r, g, s)
        })
        .max_by_key(|(_, g, s)| g.len() + s.len())
}

/// Pictures as items of the shared large view (`RexonImageViewer.qml`).
fn viewer_items(pictures: &[(String, PathBuf)]) -> Vec<Value> {
    pictures
        .iter()
        .map(|(title, path)| {
            let url = file_url(&path.to_string_lossy());
            json!({"key": path.to_string_lossy(), "title": title, "thumb": url, "source": url})
        })
        .collect()
}

/// Start the page viewer over the whole screen. The process lives on its own;
/// a thread waits for it so that it does not stay a zombie.
fn spawn_show(page: &Path) -> Result<(), String> {
    let host = crate::we_socket::display_host_binary().ok_or_else(ss::preview_host_missing)?;
    let mut child = std::process::Command::new(host)
        .args([
            rexon_core::wallpaper_engine::HOST_MODE_FLAG,
            rexon_core::wallpaper_engine::HostMode::Web.as_arg(),
            "--preview",
        ])
        .arg(page)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

impl qobject::SteamScreensaverActions {
    fn choices(&self) -> QString {
        qjson(&json!({
            "modes": ss::mode_choices(),
            "logos": ss::logo_choices(),
            "intervalMin": ss::INTERVAL_MIN,
            "intervalMax": ss::INTERVAL_MAX,
            "howTo": ss::how_to_select(),
            "restartHint": ss::restart_hint(),
            // Card and thumbnail sizes, the full-screen button — the same as in
            // the «Video splash» sub-tab.
            "thumbWidth": qs::SV_THUMB_W,
            "thumbHeight": qs::SV_THUMB_H,
            "cardWidth": qs::SV_CARD_W,
            "heroWidth": qs::SV_HERO_W,
            "heroHeight": qs::SV_HERO_H,
            "titleElideWidth": qs::SV_TITLE_ELIDE_W,
            "showText": qs::sv_test_text(),
            "logo": ss::logo_file(&cache()).map(|p| file_url(&p.to_string_lossy())).unwrap_or_default(),
        }))
    }

    fn status(&self) -> QString {
        qjson(&status_json())
    }

    fn catalog(&self) -> QString {
        let lang = qs::steam_language();
        let root = roots().into_iter().next();
        let st = root.as_deref().map(ss::status);
        let entries = ss::catalog(root.as_deref(), lang, st.as_ref());
        // How the Steam list names our screensaver — in the Steam client language.
        let names = root.as_deref().map(|r| ss::steam_names(r, lang)).unwrap_or_default();
        let our_name = ss::steam_title(ss::STEAM_ID, &names);
        let hints: HashMap<&str, String> = ss::mode_choices().into_iter().map(|c| (c.key, c.hint)).collect();
        let snap_cache = cache();
        let dark = rexon_core::ui_base::active_skin_is_dark();
        let mut pages = HashMap::new();
        let cards: Vec<Value> = entries
            .iter()
            .map(|e| {
                let mut thumb = String::new();
                if !e.page.is_empty() {
                    let page = PathBuf::from(&e.page);
                    let shot = ss::snapshot_file(&snap_cache, &page);
                    if shot.is_file() {
                        thumb = file_url(&shot.to_string_lossy());
                    }
                    pages.insert(e.key.clone(), page);
                }
                let hint = match e.origin {
                    ss::Origin::Rexon => {
                        let mode = e.key.strip_prefix("rexon:").unwrap_or("");
                        ss::rexon_card_hint(hints.get(mode).map_or("", String::as_str), &our_name, e.current, e.installed)
                    }
                    _ => ss::steam_choice_hint(&e.title, e.current),
                };
                // Badge tones of the «Video splash» cards: Steam — accent, Rexon — gold,
                // the person's own — neutral.
                let tone = match e.origin {
                    ss::Origin::Steam => qs::BADGE_ACCENT,
                    ss::Origin::Rexon => qs::BADGE_GOLD,
                    ss::Origin::User => qs::BADGE_NEUTRAL,
                };
                let (badge_bg, badge_fg) = qs::badge_colors(tone, dark);
                let mut v = serde_json::to_value(e).unwrap_or(Value::Null);
                v["thumb"] = json!(thumb);
                v["hint"] = json!(hint);
                v["badgeBackground"] = json!(badge_bg);
                v["badgeColor"] = json!(badge_fg);
                v
            })
            .collect();
        *self.rust().pages.lock().unwrap() = pages;
        qjson(&Value::Array(cards))
    }

    fn start_snapshots(&self) -> QString {
        let pages: Vec<(String, PathBuf)> =
            self.rust().pages.lock().unwrap().iter().map(|(k, p)| (k.clone(), p.clone())).collect();
        let slot = self.rust().snaps.clone();
        let serial = {
            let mut job = slot.lock().unwrap();
            if job.busy {
                return QString::default();
            }
            job.busy = true;
            job.serial += 1;
            job.serial
        };
        std::thread::spawn(move || {
            let cache = cache();
            let browser = ss::find_snapshot_browser();
            for (key, page) in pages {
                let out = ss::snapshot_file(&cache, &page);
                if !out.is_file() {
                    let Some((name, bin)) = &browser else {
                        slot.lock().unwrap().note = ss::no_snapshot_browser();
                        break;
                    };
                    if let Err(e) = ss::take_snapshot((*name, bin.as_str()), &page, &out) {
                        rexon_core::log_warn!("steam-screensaver", "snapshot of {} failed: {e}", page.display());
                        continue;
                    }
                }
                let mut job = slot.lock().unwrap();
                if job.serial == serial {
                    job.shots.insert(key, file_url(&out.to_string_lossy()));
                }
            }
            slot.lock().unwrap().busy = false;
        });
        QString::default()
    }

    fn start_scan(&self, request: &QString) -> QString {
        let opts = options_of(request, &installed_options());
        let slot = self.rust().scan.clone();
        let serial = {
            let mut job = slot.lock().unwrap();
            job.busy = true;
            job.serial += 1;
            job.result = None;
            job.serial
        };
        std::thread::spawn(move || {
            let lang = qs::steam_language();
            let mut res = ScanResult::default();
            if let Some((_, g, s)) = richest_root(lang) {
                (res.art, res.shots) = ss::planned_counts(&g, &s, &opts);
                res.pictures = viewer_items(&ss::viewer_pictures(&g, &s, &opts));
                let all = ss::Options { game_art: true, screenshots: true, ..ss::Options::default() };
                res.steam_pictures = viewer_items(&ss::viewer_pictures(&g, &s, &all));
            }
            res.text = ss::found_text(res.art, res.shots);
            let mut job = slot.lock().unwrap();
            // The user has already changed the settings — the old count is not needed.
            if job.serial == serial {
                job.busy = false;
                job.result = Some(res);
            }
        });
        QString::default()
    }

    fn start_install(&self, request: &QString) -> QString {
        let targets = roots();
        if targets.is_empty() {
            return QString::from(&ss::status_text(&[]));
        }
        let opts = options_of(request, &installed_options());
        self.run_job(move || {
            let lang = qs::steam_language();
            let locale = page_locale();
            let mut reports = Vec::new();
            let mut errors = Vec::new();
            for root in &targets {
                match ss::install(root, &opts, lang, &locale) {
                    Ok(r) => reports.push(r),
                    Err(e) => errors.push(e),
                }
            }
            (!reports.is_empty(), ss::installed_message(&reports, &errors))
        })
    }

    fn start_remove(&self) -> QString {
        let targets = roots();
        self.run_job(move || {
            let mut removed = 0;
            let mut errors = Vec::new();
            for root in &targets {
                match ss::remove(root) {
                    Ok(true) => removed += 1,
                    Ok(false) => {}
                    Err(e) => errors.push(e),
                }
            }
            (errors.is_empty(), ss::removed_message(removed, &errors))
        })
    }

    /// Start installation or removal in a worker thread; a second start while
    /// the first has not finished is refused in words.
    fn run_job(&self, work: impl FnOnce() -> (bool, String) + Send + 'static) -> QString {
        let slot = self.rust().job.clone();
        {
            let mut job = slot.lock().unwrap();
            if job.busy {
                return QString::from(&tr("Already working — please wait."));
            }
            job.busy = true;
            job.serial += 1;
            job.result = None;
        }
        std::thread::spawn(move || {
            let res = work();
            let mut job = slot.lock().unwrap();
            job.busy = false;
            job.result = Some(res);
        });
        QString::default()
    }

    fn start_show(&self, key: &QString, request: &QString) -> QString {
        let key = key.to_string();
        // A Steam or user page — shown as it is, straight from its folder (read only).
        if let Some(page) = self.rust().pages.lock().unwrap().get(&key).cloned() {
            return match spawn_show(&page) {
                Ok(()) => QString::default(),
                Err(e) => QString::from(&e),
            };
        }
        let mode = match key.as_str() {
            ss::REXON_SLIDESHOW_KEY => ss::Mode::Slideshow,
            ss::REXON_BOUNCE_KEY => ss::Mode::Bounce,
            _ => return QString::from(&ss::how_to_select()),
        };
        // A Rexon mode: the page is built into the cache with the settings on
        // screen, so the person sees it before installing.
        let opts = ss::Options { mode, ..options_of(request, &installed_options()) };
        let slot = self.rust().show.clone();
        {
            let mut job = slot.lock().unwrap();
            if job.busy {
                return QString::from(&tr("Already working — please wait."));
            }
            job.busy = true;
            job.serial += 1;
            job.result = None;
        }
        std::thread::spawn(move || {
            let lang = qs::steam_language();
            let root = roots().into_iter().max_by_key(|r| {
                let (g, s) = ss::collect(r, lang);
                g.len() + s.len()
            });
            let res = ss::build_preview(root.as_deref(), &cache(), &opts, lang, &page_locale())
                .and_then(|page| spawn_show(&page));
            let mut job = slot.lock().unwrap();
            job.busy = false;
            job.result = Some(match res {
                Ok(()) => (true, String::new()),
                Err(e) => (false, e),
            });
        });
        QString::default()
    }

    fn poll(&self) -> QString {
        let s = self.rust().scan.lock().unwrap().clone();
        let j = self.rust().job.lock().unwrap().clone();
        let n = self.rust().snaps.lock().unwrap().clone();
        let w = self.rust().show.lock().unwrap().clone();
        let scan = match &s.result {
            None => json!({"busy": s.busy, "serial": s.serial, "done": false}),
            Some(r) => json!({
                "busy": false, "serial": s.serial, "done": true,
                "art": r.art, "shots": r.shots, "text": r.text,
                "pictures": r.pictures, "steamPictures": r.steam_pictures,
            }),
        };
        let job = match &j.result {
            None => json!({"busy": j.busy, "serial": j.serial, "done": false}),
            Some((ok, message)) => json!({
                "busy": false, "serial": j.serial, "done": true, "ok": ok, "message": message,
                // The fresh state right away — the markup does not need a second call.
                "status": status_json(),
            }),
        };
        let show = match &w.result {
            None => json!({"busy": w.busy, "serial": w.serial, "done": false}),
            Some((ok, message)) => json!({"busy": false, "serial": w.serial, "done": true, "ok": ok, "message": message}),
        };
        qjson(&json!({
            "scan": scan,
            "job": job,
            "snapshots": {"busy": n.busy, "shots": n.shots, "note": n.note},
            "show": show,
        }))
    }
}
