// SteamShowcaseActions — объект раздела «Витрины профиля» вкладки «Steam».
//
// Решения (виды витрин, геометрия, проверка исходника, нарезка, сжатие,
// имена и памятка) — в ядре: `rexon_core::steam_showcase`. Здесь только
// исполнитель: окна выбора файла, рабочие потоки (нарезка и подготовка фона
// для предпросмотра) и опрос их состояния из разметки.
//
// ⛔ В Steam ничего не пишется, от имени учётной записи ничего не делается.
// Сетевых запросов два, оба без cookie и токенов: картинка фона с
// публичного CDN Steam (тем же путём, что сетка фонов вкладки) и ПУБЛИЧНАЯ
// страница профиля для шага «Проверить». Помощник загрузки только открывает
// человеку страницы Steam, кладёт текст в буфер обмена и показывает папку —
// всё, что нажимается на страницах Steam, нажимает сам человек.
//
// Числа в подписях инвокаблов не ходят (см. предупреждение в
// `steam_page.rs`): параметры приходят одной строкой JSON.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        type SteamShowcaseActions = super::SteamShowcaseActionsRust;
    }

    extern "RustQt" {
        /// Виды витрин: JSON `[{key, label, hint}]`.
        #[qinvokable]
        fn kinds(self: &SteamShowcaseActions) -> QString;

        /// Окно выбора своей картинки. Пусто — человек передумал.
        #[qinvokable]
        #[cxx_name = "pickImage"]
        fn pick_image(self: &SteamShowcaseActions) -> QString;

        /// Окно выбора своего фона (картинка или ролик).
        #[qinvokable]
        #[cxx_name = "pickBackground"]
        fn pick_background(self: &SteamShowcaseActions) -> QString;

        /// Проверка исходника: JSON `{ok, error, info, notes, url}`.
        #[qinvokable]
        fn inspect(self: &SteamShowcaseActions, path: &QString) -> QString;

        /// Расчёт макета. Вход — JSON `{kind, width, height, zoom, panX,
        /// panY}`, выход — JSON макета ядра плюс предупреждения и место
        /// витрины на фоне.
        #[qinvokable]
        fn layout(self: &SteamShowcaseActions, request: &QString) -> QString;

        /// Начать нарезку. Вход — JSON `{path, kind, zoom, panX, panY}`.
        /// «» — пошло, иначе причина.
        #[qinvokable]
        #[cxx_name = "startSlice"]
        fn start_slice(self: &SteamShowcaseActions, request: &QString) -> QString;

        /// Подготовить фон для предпросмотра: JSON `{id, url, path}` —
        /// фон Steam (по адресу) или свой файл (картинка или ролик).
        #[qinvokable]
        #[cxx_name = "startBackground"]
        fn start_background(self: &SteamShowcaseActions, request: &QString) -> QString;

        /// Состояние обеих задач: JSON `{slice: {...}, bg: {...}}`.
        #[qinvokable]
        fn poll(self: &SteamShowcaseActions) -> QString;

        /// Отменить нарезку.
        #[qinvokable]
        fn cancel(self: &SteamShowcaseActions);

        /// Открыть папку в файловом менеджере.
        #[qinvokable]
        #[cxx_name = "openFolder"]
        fn open_folder(self: &SteamShowcaseActions, path: &QString) -> bool;

        /// Куда ложатся наборы.
        #[qinvokable]
        #[cxx_name = "outputRoot"]
        fn output_root(self: &SteamShowcaseActions) -> QString;

        // ── Помощник загрузки ───────────────────────────────────────────

        /// Помощник для набора в папке `dir`: JSON `{ok, dir, name, kind,
        /// step, parts: [{file, label, size, title, uploaded, url}], steps,
        /// placed, verified, blankTitle}`. Нет набора — `{ok: false}`.
        #[qinvokable]
        fn wizard(self: &SteamShowcaseActions, dir: &QString) -> QString;

        /// Недавние наборы, к которым можно вернуться: JSON `[{dir, name,
        /// step}]`, свежие первыми.
        #[qinvokable]
        #[cxx_name = "recentSets"]
        fn recent_sets(self: &SteamShowcaseActions) -> QString;

        /// Отметить часть загруженной. Вход — JSON `{dir, file, on}`,
        /// выход — как у `wizard`.
        #[qinvokable]
        #[cxx_name = "wizardSetUploaded"]
        fn wizard_set_uploaded(self: &SteamShowcaseActions, request: &QString) -> QString;

        /// Отметить «выставлено в витрине». Вход — JSON `{dir, on}`.
        #[qinvokable]
        #[cxx_name = "wizardSetPlaced"]
        fn wizard_set_placed(self: &SteamShowcaseActions, request: &QString) -> QString;

        /// Шаг 1: открыть страницу загрузки иллюстраций (в клиенте Steam,
        /// иначе в браузере) и папку набора с выделенным первым файлом.
        /// «» — открылось, иначе что не вышло.
        #[qinvokable]
        #[cxx_name = "openUploadPage"]
        fn open_upload_page(self: &SteamShowcaseActions, dir: &QString) -> QString;

        /// Шаг 2: открыть редактор профиля на разделе витрин.
        #[qinvokable]
        #[cxx_name = "openProfileEditor"]
        fn open_profile_editor(self: &SteamShowcaseActions) -> QString;

        /// Текст в буфер обмена.
        #[qinvokable]
        #[cxx_name = "copyText"]
        fn copy_text(self: &SteamShowcaseActions, text: &QString);

        /// Шаг 3: прочитать публичную страницу профиля и сверить витрину с
        /// частями набора `dir`. Итог — в `poll()` под ключом `check`.
        #[qinvokable]
        #[cxx_name = "startCheck"]
        fn start_check(self: &SteamShowcaseActions, dir: &QString) -> QString;
    }
}

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;
use rexon_core::i18n::tr;
use rexon_core::qt_steam as qs;
use rexon_core::session_splash as ss;
use rexon_core::steam_showcase as sc;
use serde_json::{json, Value};

/// Состояние нарезки.
#[derive(Default, Clone)]
struct SliceJob {
    busy: bool,
    progress: f32,
    /// Номер задачи — разметка отличает «новый итог» от прежнего.
    serial: u64,
    result: Option<Result<sc::SliceReport, String>>,
}

/// Состояние подготовки фона.
#[derive(Default, Clone)]
struct BgJob {
    busy: bool,
    serial: u64,
    result: Option<Result<String, String>>,
}

/// Состояние проверки публичного профиля.
#[derive(Default, Clone)]
struct CheckJob {
    busy: bool,
    serial: u64,
    result: Option<Result<sc::CheckReport, String>>,
}

#[derive(Default)]
pub struct SteamShowcaseActionsRust {
    slice: Arc<Mutex<SliceJob>>,
    bg: Arc<Mutex<BgJob>>,
    check: Arc<Mutex<CheckJob>>,
    cancel: Arc<AtomicBool>,
}

fn qjson(v: &Value) -> QString {
    QString::from(&v.to_string())
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".to_string())
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Каталог «Изображения» по XDG.
fn pictures() -> PathBuf {
    let home = home();
    let cfg = rexon_core::utils::config_home(env_opt("XDG_CONFIG_HOME").as_deref(), &home);
    let user_dirs = std::fs::read_to_string(format!("{cfg}/user-dirs.dirs")).ok();
    sc::pictures_dir(env_opt("XDG_PICTURES_DIR").as_deref(), user_dirs.as_deref(), &home)
}

/// Путь → адрес для `Image.source` (спецсимволы — процентами, как в
/// `steam_page.rs`: `#` в имени иначе обрывает путь).
fn file_url(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let mut out = String::from("file://");
    for b in path.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => out.push(*b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn num(v: &Value, key: &str, dflt: f64) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(dflt)
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn placement_of(v: &Value) -> sc::Placement {
    sc::Placement { zoom: num(v, "zoom", 1.0), pan_x: num(v, "panX", 0.5), pan_y: num(v, "panY", 0.5) }
    .normalized()
}

/// Ролик ли это (свой фон может быть видео) — по расширению.
fn is_video(path: &str) -> bool {
    let low = path.to_lowercase();
    [".webm", ".mp4", ".mkv", ".mov", ".avi"].iter().any(|e| low.ends_with(e))
}

/// Первый кадр ролика в кэш Rexon (ffmpeg). Решения о запуске — ядро
/// (`session_splash::preview_ffmpeg_argv`).
fn video_frame(video: &str) -> Result<String, String> {
    let home = home();
    let ffmpeg = rexon_core::utils::which("ffmpeg", std::env::var("PATH").ok().as_deref())
        .ok_or_else(|| tr("A video background needs ffmpeg to take a frame from it."))?;
    let dir = format!(
        "{}/steam_showcase_bg",
        qs::rexon_cache_dir(env_opt("XDG_CACHE_HOME").as_deref(), &home)
    );
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stem = Path::new(video).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let stamp = std::fs::metadata(video)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out = format!("{dir}/{}-{stamp}.png", qs::safe_name(&stem));
    if std::fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(out);
    }
    let tmp = format!("{dir}/.frame-{}.png", std::process::id());
    let argv = ss::preview_ffmpeg_argv(&ffmpeg, video, &tmp, 0.0);
    let ok = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok || std::fs::rename(&tmp, &out).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Err(tr("Could not take a frame from the video."));
    }
    Ok(out)
}

/// Фон Steam: статичная картинка в кэш миниатюр вкладки (тот же каталог и
/// то же имя файла, что у сетки фонов — уже скачанное не качается второй раз).
fn steam_background(id: &str, url: &str) -> Result<String, String> {
    if url.is_empty() {
        return Err(tr("This background has no picture to show."));
    }
    let home = home();
    let dir = qs::thumb_cache_dir(env_opt("XDG_CACHE_HOME").as_deref(), &home);
    let dest = format!("{dir}/{}", qs::thumb_cache_file_name(id, url));
    crate::steam_workers::download_cached(url, &dest, qs::BG_DOWNLOAD_TIMEOUT_S, rexon_core::net::CancelToken::new())
}

/// SteamID64 человека: тот же способ, каким его находит вкладка (корень
/// Steam → учётная запись со свежим `localconfig.vdf`).
fn steam_id64() -> Option<String> {
    let home = home();
    let mut io = rexon_core::splash_exec::LiveSplashIo::new(&home, rexon_core::installer::PYTHON_FALLBACK_EXE);
    let root = ss::Splash::new(&mut io, &home).steam_root()?;
    crate::steam_workers::detect_account(&root)
}

/// Открыть страницу Steam: сначала в клиенте (там человек уже вошёл),
/// не вышло — в браузере.
fn open_steam_page(url: &str) -> bool {
    let open = |u: &str| crate::appearance_actions::qobject::open_url_external(&QString::from(u));
    open(&sc::steam_client_url(url)) || open(url)
}

/// Показать файл в файловом менеджере ВЫДЕЛЕННЫМ — штатным вызовом
/// рабочего стола `org.freedesktop.FileManager1.ShowItems`. Его понимают
/// Dolphin, Nautilus, Nemo, Thunar, Caja; нет его — откроется просто папка.
fn show_in_file_manager(file: &Path) -> bool {
    let shown = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
        ])
        .arg(format!("array:string:{}", file_url(&file.to_string_lossy())))
        .arg("string:")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    shown
        || file
            .parent()
            .map(|dir| {
                crate::extensions_actions::qobject::open_folder_url(&QString::from(
                    &dir.to_string_lossy().into_owned(),
                ))
            })
            .unwrap_or(false)
}

/// Помощник набора в папке `dir` — для разметки.
fn wizard_json(dir: &Path) -> Value {
    let Some(state) = sc::load_wizard_state(dir) else {
        return json!({"ok": false});
    };
    let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let parts: Vec<Value> = state
        .parts
        .iter()
        .map(|p| {
            let role = match p.role.as_str() {
                "side" => sc::PartRole::Side,
                "featured" => sc::PartRole::Featured,
                _ => sc::PartRole::Main,
            };
            json!({
                "file": p.file_name,
                "label": role.label(),
                "size": format!("{}×{} px", p.width, p.height),
                "title": sc::upload_title(&name, role),
                "uploaded": p.uploaded,
                "url": file_url(&dir.join(&p.file_name).to_string_lossy()),
            })
        })
        .collect();
    json!({
        "ok": true,
        "dir": dir.to_string_lossy(),
        "name": name,
        "kind": state.kind,
        "kindLabel": state.showcase_kind().label(),
        "step": state.current_step(),
        "parts": parts,
        "steps": sc::wizard_steps(&state),
        "placed": state.placed,
        "verified": state.verified,
        "blankTitle": sc::BLANK_TITLE,
    })
}

impl qobject::SteamShowcaseActions {
    fn wizard(&self, dir: &QString) -> QString {
        qjson(&wizard_json(Path::new(&dir.to_string())))
    }

    fn recent_sets(&self) -> QString {
        let root = sc::output_root(&pictures());
        let mut sets: Vec<(std::time::SystemTime, Value)> = std::fs::read_dir(&root)
            .map(|d| {
                d.flatten()
                    .filter_map(|e| {
                        let dir = e.path();
                        let state = sc::load_wizard_state(&dir)?;
                        let when = std::fs::metadata(dir.join(sc::WIZARD_STATE_FILE)).ok()?.modified().ok()?;
                        Some((
                            when,
                            json!({
                                "dir": dir.to_string_lossy(),
                                "name": e.file_name().to_string_lossy(),
                                "step": state.current_step(),
                            }),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        sets.sort_by(|a, b| b.0.cmp(&a.0));
        qjson(&Value::Array(sets.into_iter().take(5).map(|(_, v)| v).collect()))
    }

    fn wizard_set_uploaded(&self, request: &QString) -> QString {
        let v: Value = serde_json::from_str(&request.to_string()).unwrap_or(Value::Null);
        let dir = PathBuf::from(text(&v, "dir"));
        if let Some(mut state) = sc::load_wizard_state(&dir) {
            state.set_uploaded(text(&v, "file"), v.get("on").and_then(Value::as_bool).unwrap_or(false));
            let _ = sc::save_wizard_state(&dir, &state);
        }
        qjson(&wizard_json(&dir))
    }

    fn wizard_set_placed(&self, request: &QString) -> QString {
        let v: Value = serde_json::from_str(&request.to_string()).unwrap_or(Value::Null);
        let dir = PathBuf::from(text(&v, "dir"));
        if let Some(mut state) = sc::load_wizard_state(&dir) {
            state.set_placed(v.get("on").and_then(Value::as_bool).unwrap_or(false));
            let _ = sc::save_wizard_state(&dir, &state);
        }
        qjson(&wizard_json(&dir))
    }

    fn open_upload_page(&self, dir: &QString) -> QString {
        let dir = PathBuf::from(dir.to_string());
        let page = open_steam_page(sc::UPLOAD_PAGE_URL);
        // Выделяем первую ещё не загруженную часть — её и выбирать на странице.
        let first = sc::load_wizard_state(&dir).and_then(|st| {
            st.parts.iter().find(|p| !p.uploaded).or(st.parts.first()).map(|p| dir.join(&p.file_name))
        });
        let folder = match first {
            Some(file) => show_in_file_manager(&file),
            None => crate::extensions_actions::qobject::open_folder_url(&QString::from(
                &dir.to_string_lossy().into_owned(),
            )),
        };
        QString::from(&match (page, folder) {
            (true, true) => String::new(),
            (false, _) => tr("Could not open the Steam page. Open it by hand: Profile → Artwork → Upload."),
            (true, false) => tr("The Steam page is open, but the folder with the parts could not be shown."),
        })
    }

    fn open_profile_editor(&self) -> QString {
        let url = sc::edit_showcases_url(&steam_id64().unwrap_or_default());
        QString::from(&if open_steam_page(&url) {
            String::new()
        } else {
            tr("Could not open the Steam page. Open it by hand: your profile → Edit Profile → Featured Showcase.")
        })
    }

    fn copy_text(&self, text: &QString) {
        crate::we_action::qobject::rexon_we_copy_to_clipboard(text);
    }

    fn start_check(&self, dir: &QString) -> QString {
        let dir = PathBuf::from(dir.to_string());
        let Some(state) = sc::load_wizard_state(&dir) else {
            return QString::from(&tr("Save the parts first."));
        };
        let slot = self.rust().check.clone();
        {
            let mut job = slot.lock().unwrap();
            if job.busy {
                return QString::from(&tr("Already checking — please wait."));
            }
            job.busy = true;
            job.serial += 1;
            job.result = None;
        }
        // Поток ходит в сеть, поэтому заводится через общую дверь выхода:
        // сток видит его и рвёт запрос, а не оставляет выход процесса
        // наперегонки с незаконченным соединением.
        let cancel = rexon_core::net::CancelToken::new();
        {
            let cancel = cancel.clone();
            rexon_core::exit_guard::register_stopper(move || cancel.cancel());
        }
        let busy = slot.clone();
        let started = rexon_core::exit_guard::spawn(move || {
            use rexon_core::net::NetIo;
            let res = (|| {
                let sid = steam_id64()
                    .ok_or_else(|| tr("Steam account not found on this computer — there is no profile to check."))?;
                let mut net = rexon_core::net::UreqNet::new().with_cancel(cancel);
                let body = net
                    .get(&sc::profile_page_url(&sid), qs::USER_AGENT, qs::INVENTORY_TIMEOUT_S)
                    .map_err(|e| e.human())?;
                let page = sc::parse_profile_page(&String::from_utf8_lossy(&body));
                Ok(sc::check_profile(&state, &page))
            })();
            // Итог проверки запоминается рядом с частями: сошлось — шаг закрыт.
            if let Ok(report) = &res {
                if let Some(mut fresh) = sc::load_wizard_state(&dir) {
                    if fresh.placed || report.ok {
                        fresh.verified = report.ok;
                        if report.ok {
                            fresh.placed = true;
                        }
                        let _ = sc::save_wizard_state(&dir, &fresh);
                    }
                }
            }
            let mut job = slot.lock().unwrap();
            job.busy = false;
            job.result = Some(res);
        });
        if started.is_none() {
            // Программа уже закрывается — проверять некому и незачем.
            busy.lock().unwrap().busy = false;
        }
        QString::default()
    }

    fn kinds(&self) -> QString {
        let list: Vec<Value> = sc::KINDS
            .iter()
            .map(|k| json!({"key": k.key(), "label": k.label(), "hint": k.hint()}))
            .collect();
        qjson(&Value::Array(list))
    }

    fn pick_image(&self) -> QString {
        crate::extensions_actions::qobject::pick_file(
            &QString::from(&tr("Choose a picture for the showcase")),
            &QString::from(&format!("{} (*.png *.jpg *.jpeg *.gif *.PNG *.JPG *.JPEG *.GIF)", tr("Pictures"))),
            &QString::from(&pictures().to_string_lossy().into_owned()),
        )
    }

    fn pick_background(&self) -> QString {
        crate::extensions_actions::qobject::pick_file(
            &QString::from(&tr("Choose a profile background")),
            &QString::from(&format!(
                "{} (*.png *.jpg *.jpeg *.webp *.gif *.webm *.mp4 *.mkv *.mov)",
                tr("Pictures and videos")
            )),
            &QString::from(&pictures().to_string_lossy().into_owned()),
        )
    }

    fn inspect(&self, path: &QString) -> QString {
        let path = path.to_string();
        match sc::inspect(Path::new(&path)) {
            Ok(info) => qjson(&json!({
                "ok": true,
                "info": info,
                "notes": sc::source_notes(&info),
                "url": file_url(&path),
                "setName": sc::set_name_from_source(Path::new(&path)),
            })),
            Err(e) => qjson(&json!({"ok": false, "error": e})),
        }
    }

    fn layout(&self, request: &QString) -> QString {
        let v: Value = serde_json::from_str(&request.to_string()).unwrap_or(Value::Null);
        let kind = sc::ShowcaseKind::from_key(text(&v, "kind")).unwrap_or(sc::ShowcaseKind::Artwork);
        let w = num(&v, "width", 0.0).max(0.0) as u32;
        let h = num(&v, "height", 0.0).max(0.0) as u32;
        match sc::layout(kind, w, h, placement_of(&v)) {
            Some(l) => qjson(&json!({
                "ok": true,
                "layout": l,
                "warnings": sc::layout_warnings(&l),
                "showcaseX": sc::SHOWCASE_X,
                "showcaseY": sc::FIRST_SHOWCASE_Y,
                "profileWidth": sc::PROFILE_BG_WIDTH,
                "columnX": sc::PROFILE_COLUMN_X,
                "columnWidth": sc::PROFILE_COLUMN_WIDTH,
                "viewX": sc::PREVIEW_VIEW_X,
                "viewWidth": sc::PREVIEW_VIEW_WIDTH,
                "look": sc::PREVIEW_LOOK,
                "minLevel": sc::MIN_STEAM_LEVEL,
            })),
            None => qjson(&json!({"ok": false, "error": tr("The picture is too tall for a showcase.")})),
        }
    }

    fn start_slice(&self, request: &QString) -> QString {
        let v: Value = serde_json::from_str(&request.to_string()).unwrap_or(Value::Null);
        let path = text(&v, "path").to_string();
        if path.is_empty() {
            return QString::from(&tr("Choose a picture first."));
        }
        let kind = sc::ShowcaseKind::from_key(text(&v, "kind")).unwrap_or(sc::ShowcaseKind::Artwork);
        let placement = placement_of(&v);
        let slot = self.rust().slice.clone();
        {
            let mut job = slot.lock().unwrap();
            if job.busy {
                return QString::from(&tr("Already saving — please wait."));
            }
            job.busy = true;
            job.progress = 0.0;
            job.serial += 1;
            job.result = None;
        }
        let cancel = self.rust().cancel.clone();
        cancel.store(false, Ordering::SeqCst);
        std::thread::spawn(move || {
            let root = sc::output_root(&pictures());
            let name = sc::set_name_from_source(Path::new(&path));
            let req = sc::SliceRequest {
                source: PathBuf::from(&path),
                kind,
                placement,
                out_dir: sc::unique_set_dir(&root, &name),
            };
            let progress_slot = slot.clone();
            let progress = move |p: f32| {
                if let Ok(mut j) = progress_slot.lock() {
                    j.progress = p;
                }
            };
            let ctl = sc::SliceControl { progress: &progress, cancel: &cancel };
            let res = sc::slice(&req, &ctl);
            // Помощник загрузки начинается с этого набора: его состояние
            // ложится рядом с частями.
            if let Ok(report) = &res {
                let _ = sc::save_wizard_state(
                    Path::new(&report.dir),
                    &sc::WizardState::for_files(kind, &report.files),
                );
            }
            let mut job = slot.lock().unwrap();
            job.busy = false;
            job.result = Some(res);
        });
        QString::default()
    }

    fn start_background(&self, request: &QString) -> QString {
        let v: Value = serde_json::from_str(&request.to_string()).unwrap_or(Value::Null);
        let id = text(&v, "id").to_string();
        let url = text(&v, "url").to_string();
        let path = text(&v, "path").to_string();
        let slot = self.rust().bg.clone();
        let serial = {
            let mut job = slot.lock().unwrap();
            job.busy = true;
            job.serial += 1;
            job.result = None;
            job.serial
        };
        std::thread::spawn(move || {
            let res = if !path.is_empty() {
                if is_video(&path) {
                    video_frame(&path)
                } else {
                    Ok(path)
                }
            } else {
                steam_background(&id, &url)
            };
            let mut job = slot.lock().unwrap();
            // Человек успел выбрать другой фон — старый итог не нужен.
            if job.serial == serial {
                job.busy = false;
                job.result = Some(res);
            }
        });
        QString::default()
    }

    fn poll(&self) -> QString {
        let s = self.rust().slice.lock().unwrap().clone();
        let b = self.rust().bg.lock().unwrap().clone();
        let slice = match &s.result {
            None => json!({"busy": s.busy, "progress": s.progress, "serial": s.serial, "done": false}),
            Some(Ok(r)) => json!({
                "busy": false, "progress": 1.0, "serial": s.serial, "done": true, "ok": true,
                "report": r,
                "files": r.files.iter().map(|f| json!({
                    "name": f.file_name, "label": f.label,
                    "size": format!("{}×{} px", f.width, f.height),
                    "weight": sc::human_size(f.bytes),
                    "frames": f.frames, "url": file_url(&f.path),
                })).collect::<Vec<_>>(),
            }),
            Some(Err(e)) => json!({"busy": false, "serial": s.serial, "done": true, "ok": false, "error": e}),
        };
        let bg = match &b.result {
            None => json!({"busy": b.busy, "serial": b.serial, "done": false}),
            Some(Ok(p)) => json!({"busy": false, "serial": b.serial, "done": true, "ok": true, "url": file_url(p)}),
            Some(Err(e)) => json!({"busy": false, "serial": b.serial, "done": true, "ok": false, "error": e}),
        };
        let c = self.rust().check.lock().unwrap().clone();
        let check = match &c.result {
            None => json!({"busy": c.busy, "serial": c.serial, "done": false}),
            Some(Ok(r)) => json!({"busy": false, "serial": c.serial, "done": true, "ok": true, "report": r}),
            Some(Err(e)) => json!({"busy": false, "serial": c.serial, "done": true, "ok": false, "error": e}),
        };
        qjson(&json!({"slice": slice, "bg": bg, "check": check}))
    }

    fn cancel(&self) {
        self.rust().cancel.store(true, Ordering::SeqCst);
    }

    fn open_folder(&self, path: &QString) -> bool {
        crate::extensions_actions::qobject::open_folder_url(path)
    }

    fn output_root(&self) -> QString {
        QString::from(&sc::output_root(&pictures()).to_string_lossy().into_owned())
    }
}
