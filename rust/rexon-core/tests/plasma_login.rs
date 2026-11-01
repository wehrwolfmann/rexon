//! Экран входа Plasma Login Manager: фон и перенос оформления через его
//! собственный помощник KAuth (`plasma_login`). Три слоя сверки:
//!
//! * байты `QDataStream` — против снятых один раз настоящими Qt 6.12 и
//!   KAuth 6.31 (раздел `qt` эталона `golden/plasma_login_cases.json`,
//!   руками, не переснимается);
//! * что уходит помощнику — подставная шина записывает действие,
//!   разобранные аргументы и ключи дескрипторов (раздел `cases`;
//!   переснять: `REXON_GOLDEN_WRITE=1 cargo test-core --test plasma_login`,
//!   затем прочитать разницу глазами);
//! * настоящий D-Bus — частная шина `dbus-daemon` и поддельная служба с
//!   тем же интерфейсом `org.kde.kf6auth`, которая только записывает
//!   пришедшее (подпись `s ay a{sv} ay a{sh}` и передача дескриптора).

use std::os::fd::OwnedFd;

use rexon_core::plasma_login::{self as plm, KauthBus, KauthCall, KauthReply, PlmFacts, PlmLook, QVariant};
use rexon_core::system_plan::{DesktopLook, SddmLook, SddmNumlock};
use serde_json::{json, Value};

const GOLDEN: &str = "tests/golden/plasma_login_cases.json";

fn golden() -> Value {
    serde_json::from_str(include_str!("golden/plasma_login_cases.json")).unwrap()
}

fn qt(name: &str) -> Vec<u8> {
    let hex = golden()["qt"][name].as_str().unwrap_or_else(|| panic!("нет qt.{name}")).to_string();
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
}

fn s(v: &str) -> QVariant {
    QVariant::String(Some(v.to_string()))
}

fn var_json(v: &QVariant) -> Value {
    match v {
        QVariant::String(s) => json!(s),
        QVariant::StringList(l) => json!(l),
        QVariant::Bool(b) => json!(b),
        QVariant::Int(i) => json!(i),
        other => json!(format!("{other:?}")),
    }
}

/// Подставная шина: записывает вызовы, отвечает заданными байтами.
struct Fake {
    calls: Vec<Value>,
    reply: Result<Vec<u8>, String>,
}

impl Fake {
    fn ok() -> Self {
        Fake { calls: Vec::new(), reply: Ok(qt("reply_success")) }
    }
}

impl KauthBus for Fake {
    fn perform(&mut self, call: KauthCall) -> Result<Vec<u8>, String> {
        let args = plm::decode_qvariant_map(&call.arguments).expect("аргументы разбираются как QVariantMap");
        let args: serde_json::Map<String, Value> = args.iter().map(|(k, v)| (k.clone(), var_json(v))).collect();
        let fds: Vec<&String> = call.fds.iter().map(|(k, _)| k).collect();
        self.calls.push(json!({"action": call.action, "arguments": args, "fds": fds}));
        self.reply.clone()
    }
}

/// Шина, которую звать нельзя (отказ до пароля).
struct Never;
impl KauthBus for Never {
    fn perform(&mut self, call: KauthCall) -> Result<Vec<u8>, String> {
        panic!("помощник позван, хотя не должен: {}", call.action);
    }
}

fn some_fd() -> Option<OwnedFd> {
    std::fs::File::open(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).ok().map(OwnedFd::from)
}

const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 0, 0x10, b'J', b'F', b'I', b'F'];

/// Машина как ВМ стенда: автовход и часы заданы, фон стандартный.
fn vm_facts() -> PlmFacts {
    PlmFacts {
        config: Some("[Autologin]\nRelogin=false\nSession=plasma\nUser=tester\n\n[Greeter]\nShowClock=false\n".into()),
        layers: vec!["[Greeter]\nWallpaperPluginId=org.kde.image\n".into()],
        home: Some("/var/lib/plasmalogin".into()),
        helper: true,
        webp: false,
        kept: None,
        image_readable: false,
    }
}

fn bg(facts: &PlmFacts, image: Option<&str>, bus: &mut dyn KauthBus) -> Result<String, String> {
    plm::background_with(facts, image, &|_| Some(JPEG.to_vec()), &|_| some_fd(), bus)
}

fn apply_save(facts: &PlmFacts, call: &Value) -> PlmFacts {
    let mut out = facts.clone();
    out.config = call["arguments"]["config"].as_str().map(str::to_string);
    out
}

fn plasma_desktop() -> DesktopLook {
    DesktopLook {
        look: SddmLook {
            cursor_theme: Some("Bibata-Modern-Ice".into()),
            cursor_size: Some(24),
            font: Some("Inter,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1".into()),
            numlock: Some(SddmNumlock::On),
            dpi: None,
        },
        ..DesktopLook::default()
    }
}

fn user_files() -> std::collections::BTreeMap<String, String> {
    [
        ("/home/u/.config/kdeglobals", "[General]\nColorScheme=BreezeDark\nfont=Inter,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\n"),
        ("/home/u/.config/kcminputrc", "[Keyboard]\nNumLock=0\n\n[Mouse]\ncursorTheme=Bibata-Modern-Ice\ncursorSize=24\n"),
        ("/home/u/.config/plasmarc", "[Theme]\nname=breeze-dark\n"),
        ("/home/u/.config/kxkbrc", "[Layout]\nLayoutList=us,ru\nUse=true\n"),
        ("/home/u/.config/fontconfig/fonts.conf", "<fontconfig/>\n"),
    ]
    .iter()
    .map(|(p, t)| (p.to_string(), t.to_string()))
    .collect()
}

fn sync_call(look: &PlmLook) -> Value {
    let files = user_files();
    let mut bus = Fake::ok();
    plm::look_with(&vm_facts(), Some(look), &|p| files.get(p).cloned(), &|p| files.get(p).map(|t| t.len() as u64), &mut bus)
        .unwrap();
    bus.calls.remove(0)
}

fn cases() -> Value {
    let mut out = serde_json::Map::new();
    // Фон: своя картинка на машине со стандартным фоном.
    let facts = vm_facts();
    let mut bus = Fake::ok();
    bg(&facts, Some("/home/u/Картинки/Закат.jpg"), &mut bus).unwrap();
    let set = bus.calls.remove(0);
    out.insert("background_set".into(), set.clone());
    // Вернуть стандартный: ключи фона уходят, чужие остаются.
    let mut bus = Fake::ok();
    bg(&apply_save(&facts, &set), None, &mut bus).unwrap();
    out.insert("background_reset".into(), bus.calls.remove(0));
    // Сейчас другой модуль обоев (цвет) и временный PreviewImage.
    let color = PlmFacts {
        config: Some(
            "[Greeter]\nWallpaperPluginId=org.kde.color\n\n[Greeter][Wallpaper][org.kde.color][General]\nColor=0,0,0\n\n\
             [Greeter][Wallpaper][org.kde.image][General]\nPreviewImage=file:///tmp/x.png\n"
                .into(),
        ),
        ..vm_facts()
    };
    let mut bus = Fake::ok();
    bg(&color, Some("/x/a.jpg"), &mut bus).unwrap();
    out.insert("background_from_color".into(), bus.calls.remove(0));
    // Перенос оформления: Plasma — файлы как у kcm.
    let files = user_files();
    let exists = |p: &str| files.contains_key(p);
    let plasma = plm::plm_look(true, "/home/u/.config", &plasma_desktop(), &exists);
    out.insert("look_plasma_items".into(), json!({"items": plasma.items, "missing": plasma.missing}));
    out.insert("look_plasma_sync".into(), sync_call(&plasma));
    // Вне Plasma: цвета Qt и fontconfig файлами, шрифт и курсор значениями.
    let other = DesktopLook {
        look: SddmLook {
            cursor_theme: Some("Adwaita".into()),
            cursor_size: Some(32),
            font: Some("Cantarell,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1".into()),
            ..SddmLook::default()
        },
        ..DesktopLook::default()
    };
    let gnome = plm::plm_look(false, "/home/u/.config", &other, &exists);
    out.insert("look_other_items".into(), json!({"items": gnome.items, "missing": gnome.missing}));
    out.insert("look_other_sync".into(), sync_call(&gnome));
    // «Сбросить».
    let mut bus = Fake::ok();
    plm::look_with(&vm_facts(), None, &|_| None, &|_| None, &mut bus).unwrap();
    out.insert("look_reset".into(), bus.calls.remove(0));
    Value::Object(out)
}

#[test]
fn эталон_вызовов_помощника() {
    let got = cases();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    if std::env::var_os("REXON_GOLDEN_WRITE").is_some() {
        let mut all = golden();
        all["cases"] = got.clone();
        std::fs::write(&path, serde_json::to_string_pretty(&all).unwrap() + "\n").unwrap();
    }
    assert_eq!(got, golden()["cases"], "эталон {GOLDEN} разошёлся");
}

#[test]
fn байты_qdatastream_как_у_qt() {
    assert_eq!(plm::encode_qvariant_map(&[]), qt("empty_map"));
    let config = "[Greeter]\nWallpaperPluginId=org.kde.image\n\n[Greeter][Wallpaper][org.kde.image][General]\n\
                  Image=file:///var/lib/plasmalogin/wallpapers/rexon-background.jpg\n";
    // Порядок вставки обратный — QMap всё равно пишет по ключам.
    let save = vec![
        ("wallpapers".to_string(), QVariant::StringList(vec!["rexon-background.jpg".into()])),
        ("config".to_string(), s(config)),
    ];
    assert_eq!(plm::encode_qvariant_map(&save), qt("save_args"));
    let sync = vec![
        ("kdeglobals".to_string(), s("[General]\nfont=Noto Sans,10\nИмя=Фон 🎨\n")),
        ("fontconfig/fonts.conf".to_string(), QVariant::String(None)),
        ("plasmarc".to_string(), s("")),
    ];
    assert_eq!(plm::encode_qvariant_map(&sync), qt("sync_args"));
    let reset = vec![
        ("wallpapers".to_string(), QVariant::StringList(Vec::new())),
        ("config".to_string(), QVariant::String(None)),
    ];
    assert_eq!(plm::encode_qvariant_map(&reset), qt("reset_bg_args"));
    // Разбор Qt-байтов и обратная запись совпадают.
    for name in ["save_args", "sync_args", "reset_bg_args"] {
        let m = plm::decode_qvariant_map(&qt(name)).unwrap();
        assert_eq!(plm::encode_qvariant_map(&m), qt(name), "{name}");
    }
}

#[test]
fn ответы_помощника_как_у_kauth() {
    let r = |n: &str| plm::decode_reply(&qt(n)).unwrap();
    assert_eq!((r("reply_success").kind, r("reply_success").code), (plm::REPLY_SUCCESS, 0));
    assert_eq!((r("reply_helper_error").kind, r("reply_helper_error").code), (plm::REPLY_HELPER_ERROR, -1));
    assert_eq!(r("reply_denied").code, plm::ERR_AUTHORIZATION_DENIED);
    assert_eq!(r("reply_cancelled").code, plm::ERR_USER_CANCELLED);
    assert_eq!(r("reply_busy").code, plm::ERR_HELPER_BUSY);
    let d = r("reply_helper_error_desc");
    assert_eq!(d.description, "Нет");
    assert_eq!(d.data, vec![("b".to_string(), QVariant::Bool(true)), ("n".to_string(), QVariant::Int(5))]);
    for n in ["reply_success", "reply_helper_error", "reply_denied", "reply_helper_error_desc"] {
        assert_eq!(plm::encode_reply(&r(n)), qt(n), "{n}");
    }
    assert!(plm::reply_outcome(&r("reply_success")).is_ok());
    assert!(plm::reply_outcome(&r("reply_denied")).unwrap_err().contains("password"));
    assert!(plm::reply_outcome(&r("reply_helper_error_desc")).unwrap_err().ends_with("\nНет"));
    // Мусор и обрыв — ошибка разбора, а не паника.
    assert!(plm::decode_reply(&[0, 0, 0]).is_err());
    assert!(plm::decode_reply(&[0xff, 0xff, 0xff, 0xff]).is_err());
    assert!(plm::decode_qvariant_map(&[0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0x7f, 0xff, 0]).is_err());
}

#[test]
fn отказы_до_пароля_и_ошибки_шины() {
    // Не картинка — помощник не зовётся.
    let e = plm::background_with(&vm_facts(), Some("/x/a.txt"), &|_| Some(b"hello".to_vec()), &|_| some_fd(), &mut Never)
        .unwrap_err();
    assert!(e.contains("PNG and JPEG"));
    // Помощника нет / нет учётной записи — честная причина, без вызова.
    let no_helper = PlmFacts { helper: false, ..vm_facts() };
    assert!(bg(&no_helper, None, &mut Never).unwrap_err().contains("helper isn't installed"));
    let no_user = PlmFacts { home: None, ..vm_facts() };
    assert!(plm::look_with(&no_user, None, &|_| None, &|_| None, &mut Never).unwrap_err().contains("plasmalogin"));
    // Пароль не введён.
    let mut bus = Fake { calls: Vec::new(), reply: Ok(qt("reply_cancelled")) };
    assert!(bg(&vm_facts(), None, &mut bus).unwrap_err().contains("password"));
    // Шина недоступна и непонятный ответ.
    let mut bus = Fake { calls: Vec::new(), reply: Err("org.freedesktop.DBus.Error.ServiceUnknown".into()) };
    assert!(bg(&vm_facts(), None, &mut bus).unwrap_err().contains("ServiceUnknown"));
    let mut bus = Fake { calls: Vec::new(), reply: Ok(vec![1, 2, 3]) };
    assert!(bg(&vm_facts(), None, &mut bus).unwrap_err().contains("doesn't understand"));
}

#[test]
fn вид_раздела() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let look = PlmLook { plasma: true, items: vec!["A".into()], ..PlmLook::default() };
    let std_view = plm::greeter_view(&vm_facts(), &look, "/usr/share/wallpapers/Next/x.png", "");
    assert!(std_view.visible && std_view.can_change);
    assert_eq!(std_view.background.image, "/usr/share/wallpapers/Next/x.png");
    assert!(!std_view.background.can_reset);
    let mut own = vm_facts();
    own.config = Some(plm::background_config(&own, Some("rexon-background.png")));
    // Своя картинка: дом `plasmalogin` закрыт — превью из копии Rexon; копии
    // нет — честная подпись без превью.
    let own_view = plm::greeter_view(&own, &look, "/std.png", "");
    assert_eq!(own_view.background.image, "");
    assert!(own_view.background.state.contains("no preview"), "{}", own_view.background.state);
    assert!(own_view.background.can_reset);
    let kept = PlmFacts { kept: Some("/home/u/.local/share/rexon/login-background/plm.png".into()), ..own.clone() };
    let kept_view = plm::greeter_view(&kept, &look, "/std.png", "");
    assert_eq!(kept_view.background.image, "/home/u/.local/share/rexon/login-background/plm.png");
    assert_eq!(kept_view.background.state, "Background: your picture");
    // Фон поставлен не Rexon: читается — показывается, нет — без превью.
    let mut other = vm_facts();
    other.config = Some("[Greeter][Wallpaper][org.kde.image][General]\nImage=file:///var/lib/plasmalogin/wallpapers/mine.jpg\n".into());
    let other_view = plm::greeter_view(&other, &look, "/std.png", "");
    assert_eq!(other_view.background.image, "");
    assert!(other_view.background.state.contains("set outside Rexon"), "{}", other_view.background.state);
    let readable = PlmFacts { image_readable: true, ..other };
    assert_eq!(plm::greeter_view(&readable, &look, "", "").background.image, "/var/lib/plasmalogin/wallpapers/mine.jpg");
    assert!(plm::is_own_background("rexon-background.webp") && !plm::is_own_background("mine.jpg"));
    let blocked = plm::greeter_view(&PlmFacts { helper: false, ..vm_facts() }, &look, "", "");
    assert!(!blocked.can_change && !blocked.blocked.is_empty());
    let r = KauthReply { data: Vec::new(), code: 0, kind: plm::REPLY_SUCCESS, description: String::new() };
    assert_eq!(plm::decode_reply(&plm::encode_reply(&r)).unwrap(), r);
}

/// «Показывать часы» — как kcm_plasmalogin: `[Greeter] ShowClock`, по
/// умолчанию да; совпадение с умолчанием снимает ключ (KConfigSkeleton).
/// Тем же `save`, что фон, а `save` помощника стирает `~/wallpapers` —
/// своя картинка пересылается заново; не открылась — отказ до пароля.
#[test]
fn часы_как_у_kcm() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let facts = vm_facts();
    assert!(!plm::show_clock(&facts), "в ВМ часы выключены");
    let on = plm::clock_config(&facts, true);
    assert!(!on.contains("ShowClock") && on.contains("User=tester"), "{on}");
    let bare = PlmFacts { config: Some("[Autologin]\nUser=tester\n".into()), layers: Vec::new(), ..vm_facts() };
    assert!(plm::show_clock(&bare));
    assert!(plm::clock_config(&bare, false).contains("[Greeter]\nShowClock=false"));
    let layered = PlmFacts { config: None, layers: vec!["[Greeter]\nShowClock=false\n".into()], ..vm_facts() };
    assert!(plm::clock_config(&layered, true).contains("ShowClock=true"), "умолчание слоя — нет, «да» пишется");
    // Без своей картинки — только текст настроек.
    let mut bus = Fake::ok();
    assert!(plm::clock_with(&facts, true, &|_| None, &mut bus).unwrap().contains("show the clock"));
    let call = bus.calls.remove(0);
    assert_eq!(call["action"], plm::ACTION_SAVE);
    assert!(call["arguments"].get("wallpapers").is_none(), "{call}");
    // Своя картинка пересылается тем же именем.
    let mut bus = Fake::ok();
    bg(&facts, Some("/home/u/Картинки/Закат.jpg"), &mut bus).unwrap();
    let own = apply_save(&facts, &bus.calls.remove(0));
    let mut bus = Fake::ok();
    plm::clock_with(&own, false, &|p| (p == "/var/lib/plasmalogin/wallpapers/rexon-background.jpg").then(some_fd).flatten(), &mut bus)
        .unwrap();
    let call = bus.calls.remove(0);
    assert_eq!(call["arguments"]["wallpapers"], json!(["rexon-background.jpg"]));
    assert_eq!(call["fds"], json!(["_fd_rexon-background.jpg"]));
    let config = call["arguments"]["config"].as_str().unwrap();
    assert!(config.contains("ShowClock=false") && config.contains("Image=file:///var/lib/plasmalogin/wallpapers/rexon-background.jpg"), "{config}");
    // Не открылась — отказ, помощник не зван (фон молча не пропадёт).
    assert!(plm::clock_with(&own, false, &|_| None, &mut Never).unwrap_err().contains("background"));
    // Вид: галочка всегда есть, состояние — из настроек.
    let look = PlmLook { plasma: true, ..PlmLook::default() };
    let v = plm::greeter_view(&facts, &look, "", "");
    assert!(v.clock.supported && !v.clock.checked);
    assert!(v.preview.tooltip.contains("mypassword"));
}

/// WebP — только при модуле WebP у Qt 6 (приветствие PLM на Qt 6).
#[test]
fn webp_у_plm() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    let webp: &[u8] = b"RIFF\x10\0\0\0WEBPVP8 ";
    let e = plm::background_with(&vm_facts(), Some("/x.webp"), &|_| Some(webp.to_vec()), &|_| some_fd(), &mut Never)
        .unwrap_err();
    assert!(e.contains("qt6-imageformats"), "{e}");
    let with = PlmFacts { webp: true, ..vm_facts() };
    let mut bus = Fake::ok();
    plm::background_with(&with, Some("/x.webp"), &|_| Some(webp.to_vec()), &|_| some_fd(), &mut bus).unwrap();
    assert_eq!(bus.calls[0]["arguments"]["wallpapers"], json!(["rexon-background.webp"]));
    let look = PlmLook::default();
    assert!(plm::greeter_view(&with, &look, "", "").background.filter.contains("*.webp"));
}

/// Превью — `plasma-login-greeter --test` из каталога libexec (6.8.2).
#[test]
fn превью_plm() {
    rexon_core::i18n::set_language(Some(rexon_core::i18n::SOURCE_LANGUAGE));
    assert_eq!(
        plm::preview_argv(&|p| p == "/usr/libexec/plasma-login-greeter").unwrap(),
        ["/usr/libexec/plasma-login-greeter", "--test"]
    );
    assert_eq!(plm::preview_argv(&|p| p.starts_with("/usr/")).unwrap()[0], "/usr/lib/plasma-login-greeter");
    assert!(plm::preview_argv(&|_| false).unwrap_err().contains("isn't installed"));
}

/// Настоящий D-Bus: частная шина и поддельная служба помощника.
mod bus {
    use super::*;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Read};
    use std::sync::{Arc, Mutex};
    use zbus::zvariant;

    type Seen = Arc<Mutex<Vec<(String, Vec<u8>, usize, Vec<u8>, Vec<(String, Vec<u8>)>)>>>;

    struct FakeHelper {
        seen: Seen,
        reply: Vec<u8>,
    }

    #[zbus::interface(name = "org.kde.kf6auth")]
    impl FakeHelper {
        #[zbus(name = "performAction")]
        fn perform_action(
            &self,
            action: String,
            caller_id: Vec<u8>,
            details: HashMap<String, zvariant::OwnedValue>,
            arguments: Vec<u8>,
            fd_arguments: HashMap<String, zvariant::OwnedFd>,
        ) -> Vec<u8> {
            let fds = fd_arguments
                .into_iter()
                .map(|(k, fd)| {
                    let mut f = std::fs::File::from(OwnedFd::from(fd));
                    let mut b = Vec::new();
                    f.read_to_end(&mut b).unwrap();
                    (k, b)
                })
                .collect();
            self.seen.lock().unwrap().push((action, caller_id, details.len(), arguments, fds));
            self.reply.clone()
        }
    }

    #[test]
    #[cfg_attr(feature = "isolated-checks", ignore = "нужен dbus-daemon машины")]
    fn вызов_по_настоящей_шине() {
        let mut daemon = std::process::Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("dbus-daemon");
        let mut addr = String::new();
        BufReader::new(daemon.stdout.as_mut().unwrap()).read_line(&mut addr).unwrap();
        let addr = addr.trim().to_string();
        let seen: Seen = Arc::default();
        let _server = zbus::blocking::connection::Builder::address(addr.as_str())
            .unwrap()
            .name(plm::HELPER_ID)
            .unwrap()
            .serve_at("/", FakeHelper { seen: seen.clone(), reply: qt("reply_success") })
            .unwrap()
            .build()
            .unwrap();
        let picture = std::env::temp_dir().join(format!("rexon-plm-bus-{}.jpg", std::process::id()));
        std::fs::write(&picture, JPEG).unwrap();
        let path = picture.to_string_lossy().into_owned();
        let mut client = plm::SystemKauth { address: Some(addr.clone()) };
        let done = plm::background_with(
            &vm_facts(),
            Some(&path),
            &|p| std::fs::read(p).ok(),
            &|p| std::fs::File::open(p).ok().map(OwnedFd::from),
            &mut client,
        );
        let mut client = plm::SystemKauth { address: Some(addr) };
        let reset = plm::look_with(&vm_facts(), None, &|_| None, &|_| None, &mut client);
        let _ = std::fs::remove_file(&picture);
        let _ = daemon.kill();
        let _ = daemon.wait();
        done.unwrap();
        reset.unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        let (action, caller, details, args, fds) = &seen[0];
        assert_eq!(action, plm::ACTION_SAVE);
        assert!(caller.starts_with(b":"), "callerID — имя соединения");
        assert_eq!(*details, 0);
        let args = plm::decode_qvariant_map(args).unwrap();
        assert!(args.iter().any(|(k, v)| k == "wallpapers" && *v == QVariant::StringList(vec!["rexon-background.jpg".into()])));
        assert_eq!(fds, &vec![("_fd_rexon-background.jpg".to_string(), JPEG.to_vec())]);
        assert_eq!(seen[1].0, plm::ACTION_RESET);
        assert_eq!(seen[1].3, qt("empty_map"));
    }
}

/// Живой вызов НАСТОЯЩЕГО помощника PLM — только на стенде (ВМ), никогда на
/// рабочей машине: `REXON_PLM_LIVE=bg:/путь.jpg | bg-reset | sync:kde |
/// reset cargo test --test plasma_login -- --ignored живой_вызов`.
#[test]
#[ignore = "пишет в систему через помощник PLM — только в ВМ стенда"]
fn живой_вызов_помощника_в_вм() {
    let step = std::env::var("REXON_PLM_LIVE").expect("REXON_PLM_LIVE");
    let res = if let Some(p) = step.strip_prefix("bg:") {
        plm::background_live(Some(p))
    } else if step == "bg-reset" {
        plm::background_live(None)
    } else if let Some(de) = step.strip_prefix("sync:") {
        plm::look_live(Some(de))
    } else {
        plm::look_live(None)
    };
    println!("ИТОГ {step}: {res:?}");
    res.unwrap();
}
