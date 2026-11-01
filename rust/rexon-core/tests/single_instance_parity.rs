//! Один Rexon на сессию: замок, слушатель «подними окно» и стук в него.
//!
//! Сверять построчно не с чем: в оригинальном `rexon/app.py` замка никогда
//! не было — механизм родился из двух ЖИВЫХ дефектов (пять значков в лотке
//! 02.09.2026 и «программа работает, двери в неё нет» 06.09.2026), и оба
//! разобраны в шапке `rexon_core::single_instance`. Поэтому здесь проверяется
//! не совпадение с Python, а САМО ПОВЕДЕНИЕ, ради которого механизм есть:
//! замок держит межпроцессно, слушатель отвечает по факту, стук приносит
//! честный ответ, а бинд не ворует чужой живой сокет.
//!
//! ⚠️⚠️ ГДЕ ЭТИ СВЕРКИ РАБОТАЮТ. Только в СВОЁМ подставном каталоге
//! (`/tmp/rxsi-<PID>-<n>`, см. `sandbox`), никогда в `$XDG_RUNTIME_DIR`
//! живого сеанса. Причина записана
//! в проекте кровью: тестовый демон уже забирал и сносил ОБЩИЕ сокеты в
//! `/run/user/$UID` — приватной шины мало, потому что адрес общий на весь
//! сеанс. Настоящие `rexon.lock` и `rexon-activate.sock` владельца эти
//! сверки не открывают ни на чтение, ни на запись.
//!
//! ⚠️ СТОРОЖ. Каждая сверка с живым сокетом обёрнута в сторожа: любое
//! ожидание здесь конечно, и зависший `accept`/`connect` обязан УБИТЬ
//! прогон с внятной строкой, а не висеть до общего таймаута. Сторож
//! проверен на укус (см. `watchdog_bites`).

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

mod i18n_pin;

use rexon_core::single_instance as si;

// ============================================================================
// Свой каталог и сторож
// ============================================================================

/// Подставной runtime-каталог: живой `$XDG_RUNTIME_DIR` эти сверки не трогают
/// вовсе. Уходит вместе с сверкой (`Drop`, в том числе при панике).
struct Sandbox(PathBuf);

impl std::ops::Deref for Sandbox {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Самый длинный адрес, который примет `bind`: `sockaddr_un.sun_path` — 108
/// байт вместе с завершающим нулём.
const SUN_PATH_MAX: usize = si::SUN_PATH_MAX;

/// ⚠️⚠️ ПОЧЕМУ НЕ `CARGO_TARGET_TMPDIR`. Раньше каталог лежал под `target/`,
/// и адрес сокета собирался из четырёх переменных частей: корень дерева,
/// имя сверки, PID и номер вызова. В основном дереве корень
/// `…/Загрузки/Rexon/rexon_v5/rust/target/tmp` — 63 байта (кириллица по 2),
/// и `si-clear-dead-<PID>-<n>/rexon-activate.sock` переходил за 107 байт,
/// как только PID становился семизначным (`pid_max` = 4 194 304, к вечеру
/// PID перевалил за миллион), а номер вызова — двузначным (сверки идут
/// потоками, номер зависит от порядка). Замер 15.09: 9 красных из 10,
/// путь ровно 108 байт, `InvalidInput: path must be shorter than SUN_LEN`.
/// Утром при шестизначном PID та же сверка была зелёной, а в копии дерева
/// с более длинным корнем краснела `rust_holder_hands_over_the_lock` —
/// сверка краснела на длине пути, а не на поведении.
///
/// Поэтому корень — системный временный каталог (`/tmp`), имя без имени
/// сверки, и длина проверяется здесь же, до первого сокета.
///
/// ⚠️ Длина меряется ОСНОВНЫМ адресом (`preferred_socket_path`): с 15.09
/// `socket_path` при длинном каталоге сам уходит в запасной `/tmp/rexon-<uid>`,
/// а сверкам ниже сокет нужен именно в подставном каталоге.
fn sandbox(_name: &str) -> Sandbox {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let leaf = format!("rxsi-{}-{n}", std::process::id());
    let mut root = std::env::temp_dir();
    let fits = |root: &std::path::Path| {
        si::preferred_socket_path(root.join(&leaf).to_str().unwrap_or_default()).len()
            <= SUN_PATH_MAX
    };
    // Нестандартный длинный `TMPDIR` — не повод краснеть: `/tmp` есть всегда.
    if !fits(&root) {
        root = PathBuf::from("/tmp");
    }
    let dir = root.join(&leaf);
    let sock = si::preferred_socket_path(dir.to_str().expect("путь стенда в UTF-8"));
    assert_eq!(sock, si::socket_path(dir.to_str().unwrap()), "стенд: адрес обязан быть основным");
    assert!(
        sock.len() <= SUN_PATH_MAX,
        "стенд: адрес {sock} — {} байт, в sun_path влезает {SUN_PATH_MAX}",
        sock.len()
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("подставной runtime-каталог");
    Sandbox(dir)
}

/// Выполнить `body` под сторожем: не уложился в `limit` — прогон падает с
/// названной причиной, а не висит.
///
/// ⚠️ Сторож обязан именно УБИВАТЬ процесс. Мягкий `panic!` из чужого
/// потока libtest не заметит: он ждёт поток сверки, а тот стоит в `accept`.
fn under_watchdog<T, F>(what: &str, limit: Duration, body: F) -> T
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let out = body();
        let _ = tx.send(());
        out
    });
    if rx.recv_timeout(limit).is_err() {
        eprintln!("сторож: «{what}» не уложилась в {limit:?} — прогон убит");
        std::process::exit(97);
    }
    worker.join().expect("поток сверки")
}

// ============================================================================
// Адреса
// ============================================================================

#[test]
fn addresses_live_next_to_each_other() {
    assert_eq!(si::lock_path("/run/user/1000"), "/run/user/1000/rexon.lock");
    assert_eq!(si::lock_path("/tmp"), "/tmp/rexon.lock");
    assert_eq!(
        si::socket_path("/run/user/1000"),
        "/run/user/1000/rexon-activate.sock"
    );
    // Замок и сокет — в ОДНОМ каталоге: сокет обязан умирать вместе с
    // сеансом, как и замок.
    for dir in ["/run/user/1000", "/tmp"] {
        let lock = si::lock_path(dir);
        let sock = si::socket_path(dir);
        assert_eq!(
            std::path::Path::new(&lock).parent(),
            std::path::Path::new(&sock).parent()
        );
    }
}

/// Каталог ровно такой длины, чтобы основной адрес вышел `total` байт.
fn dir_for_address_len(total: usize) -> String {
    let tail = "/rexon-activate.sock".len();
    let mut dir = String::from("/run/user/1000/");
    while dir.len() < total - tail {
        dir.push('d');
    }
    assert_eq!(si::preferred_socket_path(&dir).len(), total);
    dir
}

#[test]
fn address_moves_to_a_short_fallback_only_past_the_limit() {
    let uid = 1000;
    // 107 байт — ещё помещается: адрес рядом с замком, как всегда.
    let d107 = dir_for_address_len(107);
    assert_eq!(si::socket_path_for(&d107, uid), si::preferred_socket_path(&d107));
    assert_eq!(si::socket_address_note(&d107), None);

    // 108 и 200 — уже нет: запасной адрес в личном каталоге пользователя.
    for total in [108, 200] {
        let dir = dir_for_address_len(total);
        let sock = si::socket_path_for(&dir, uid);
        assert!(
            sock.starts_with("/tmp/rexon-1000/activate-") && sock.ends_with(".sock"),
            "{total} байт: {sock}"
        );
        assert!(sock.len() <= SUN_PATH_MAX, "запасной адрес сам обязан помещаться: {sock}");
        // Одна функция — один ответ: слушатель и стук приходят к одному адресу.
        assert_eq!(sock, si::socket_path_for(&dir, uid));
        assert_eq!(sock, si::fallback_socket_path(&dir, uid));
        let note = si::socket_address_note(&dir).expect("запасной адрес обязан попасть в журнал");
        assert!(
            note.contains(&format!("{total} байт")) && note.contains("107"),
            "журнал обязан назвать длину и предел: {note}"
        );
    }

    // Разные сеансы одного пользователя — разные адреса; хвостовая черта — тот же.
    let a = dir_for_address_len(150);
    let b = format!("{}x", &a[..a.len() - 1]);
    assert_ne!(si::socket_path_for(&a, uid), si::socket_path_for(&b, uid));
    assert_eq!(si::socket_path_for(&a, uid), si::fallback_socket_path(&format!("{a}/"), uid));
    // Разные пользователи — разные каталоги.
    assert_ne!(si::socket_path_for(&a, 1000), si::socket_path_for(&a, 1001));
    // Хеш закреплён: стук и слушатель бывают разными бинарями.
    assert_eq!(
        si::fallback_socket_path("/x", 7),
        format!("/tmp/rexon-7/activate-{:016x}.sock", 0x07d6_4e07_b49c_aeb2_u64)
    );
}

#[test]
fn runtime_dir_falls_back_to_tmp() {
    // Порт `os.environ.get("XDG_RUNTIME_DIR") or "/tmp"`: пустая строка
    // ложна в Python, и здесь обязана вести себя так же.
    assert_eq!(si::runtime_dir(Some("/run/user/1000")), "/run/user/1000");
    assert_eq!(si::runtime_dir(Some("")), "/tmp");
    assert_eq!(si::runtime_dir(None), "/tmp");
}

// ============================================================================
// Протокол
// ============================================================================

#[test]
fn wire_words_survive_trailing_newline() {
    assert!(si::is_show_request(b"show\n"));
    assert!(si::is_show_request(b"show"));
    assert!(si::is_show_request(b" show \r\n"));
    assert!(!si::is_show_request(b""));
    assert!(!si::is_show_request(b"hide\n"));

    assert_eq!(si::reply_for(true), si::REPLY_RAISED);
    assert_eq!(si::reply_for(false), si::REPLY_FAILED);

    // Соврать «поднял» нельзя: всё, кроме «ok», — отказ.
    assert!(si::knock_accepted(b"ok\n"));
    assert!(si::knock_accepted(b"ok"));
    assert!(!si::knock_accepted(b"no\n"));
    assert!(!si::knock_accepted(b""));
    assert!(!si::knock_accepted(b"okay\n"));
}

// ============================================================================
// Замок
// ============================================================================

/// Замок на процесс один (`LOCK_FD`), а сверки libtest идут потоками ОДНОГО
/// процесса: удачный захват в одной сверке сменил бы замок другой. Сверки,
/// берущие замок, идут по очереди.
static LOCK_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn process_knows_it_holds_the_lock() {
    let _turn = LOCK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = sandbox("holds");
    let lock = si::lock_path(dir.to_str().unwrap());

    assert!(!si::holds_lock(&lock), "замок ещё не брали");
    assert!(si::acquire_lock(&lock));
    // Второй вход в том же процессе обязан узнать свой замок, а не
    // упереться в него как второй экземпляр.
    assert!(si::holds_lock(&lock), "взятый замок обязан числиться нашим");
    assert!(!si::acquire_lock(&lock), "повторный flock блокирует сам себя");
    assert!(si::holds_lock(&lock), "неудачный повтор замок не отнимает");
    assert!(!si::holds_lock(&format!("{lock}.other")), "чужой путь — не наш");
}

#[test]
fn lock_holds_across_processes() {
    let _turn = LOCK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = sandbox("lock");
    let lock = si::lock_path(dir.to_str().unwrap());

    // Первый захват в этом процессе — успешен.
    assert!(si::acquire_lock(&lock), "первый запуск обязан взять замок");
    // Повторный захват ТОГО ЖЕ пути: оба дескриптора смотрят в один inode,
    // второй flock блокирует сам себя — как если бы это были два процесса.
    assert!(!si::acquire_lock(&lock), "второй запуск не должен пройти");

    // Межпроцессная проверка настоящим процессом: пока держим мы, чужой
    // взять не может.
    let child = std::process::Command::new("/usr/bin/flock")
        .args(["-n", &lock, "true"])
        .status()
        .expect("flock(1) есть в util-linux");
    assert!(
        !child.success(),
        "чужой процесс обязан увидеть замок занятым"
    );

    // Стухшего замка не бывает: файл от прошлой сессии остаётся, но замок
    // на нём свободен — ядро сняло его вместе с умершим держателем.
    let stale = dir.join("stale.lock");
    let stale = stale.to_str().unwrap();
    let held = std::process::Command::new("/usr/bin/flock")
        .args(["-n", stale, "true"])
        .status()
        .expect("flock(1)");
    assert!(held.success(), "свободный файл обязан пускать");
    assert!(
        si::acquire_lock(stale),
        "после смерти держателя замок обязан быть свободен"
    );
}

#[test]
fn show_word_carries_a_section() {
    let r = |b: &[u8]| si::parse_show_request(b);
    let bare = Some(si::ShowRequest { section: None });
    let sec = |s: &str, p: &str| {
        Some(si::ShowRequest {
            section: Some((s.to_string(), p.to_string())),
        })
    };
    // Голое `show` — прежний протокол, работает как раньше.
    assert_eq!(r(b"show\n"), bare);
    assert_eq!(r(b"show settings\n"), sec("settings", ""));
    assert_eq!(r(b"show desktops compare\n"), sec("desktops", "compare"));
    // Подраздел, которого у раздела не бывает, отбрасывается; раздел — нет.
    assert_eq!(r(b"show settings compare\n"), sec("settings", ""));
    // Мусорный раздел или лишние слова: окно поднять, никуда не переключать.
    assert_eq!(r(b"show ../etc\n"), bare);
    assert_eq!(r(b"show zzz\n"), bare);
    assert_eq!(r(b"show desktops compare extra\n"), bare);
    // Не «show» вовсе — окно не трогать.
    assert_eq!(r(b"showsettings\n"), None);
    assert_eq!(r(b"hide settings\n"), None);
    assert_eq!(r(b"\xff\xfe\n"), None);
    assert!(si::is_show_request(b"show desktops compare\n"));

    // Слово строится обратно тем же разбором и влезает в буфер слушателя.
    for target in [None, Some(("settings", "")), Some(("desktops", "compare"))] {
        let word = si::show_request(target);
        assert!(word.len() < 128, "слово обязано влезть в буфер слушателя");
        assert_eq!(
            si::parse_show_request(&word).expect("своё слово — просьба").section,
            target.map(|(s, p)| (s.to_string(), p.to_string()))
        );
    }
    // Непроверенная цель в провод не уходит.
    assert_eq!(si::show_request(Some(("zzz", ""))), si::SHOW_REQUEST);
}

#[test]
fn knock_target_follows_the_start_section() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let t = |s: &str, p: &str| Some((s.to_string(), p.to_string()));
    assert_eq!(si::knock_target(&a(&["--page=backups"])), t("settings", ""));
    assert_eq!(si::knock_target(&a(&["--page=install"])), t("desktops", ""));
    assert_eq!(si::knock_target(&a(&["--page=compare"])), t("desktops", "compare"));
    assert_eq!(si::knock_target(&a(&["--section=appearance"])), t("appearance", ""));
    assert_eq!(si::knock_target(&a(&["--section=zzz"])), None);
    assert_eq!(si::knock_target(&a(&["--page=zzz"])), None);
    assert_eq!(si::knock_target(&a(&[])), None);
}

#[test]
fn knock_brings_the_section_to_the_listener() {
    let dir = sandbox("sect");
    let sock = si::socket_path(dir.to_str().unwrap());
    let (tx, got) = mpsc::channel();
    si::ActivationListener::spawn_to(&sock, move |section| {
        let _ = tx.send(section);
        true
    })
    .expect("слушатель");

    let probe = sock.clone();
    let ok = under_watchdog("стук с разделом", Duration::from_secs(10), move || {
        si::activate_running_instance_to(&probe, Some(("desktops", "compare")))
    });
    assert!(ok);
    assert_eq!(
        got.recv_timeout(Duration::from_secs(5)).expect("слушатель спросил"),
        Some(("desktops".to_string(), "compare".to_string()))
    );

    // Голое `show` доходит без раздела, как раньше.
    let probe = sock.clone();
    let ok = under_watchdog("голый стук", Duration::from_secs(10), move || {
        si::activate_running_instance(&probe)
    });
    assert!(ok);
    assert_eq!(got.recv_timeout(Duration::from_secs(5)).expect("спросил"), None);
}

#[test]
fn listener_without_sections_still_raises_on_a_sectioned_knock() {
    // Слушатель, поставленный через `spawn` (без раздела — так пока стоит
    // Rust-вход), обязан поднимать окно и на новое слово с разделом.
    let dir = sandbox("oldsect");
    let (sock, asked) = listener_saying(&dir, true);
    let ok = under_watchdog("стук с разделом в старого", Duration::from_secs(10), move || {
        si::activate_running_instance_to(&sock, Some(("settings", "")))
    });
    assert!(ok);
    assert!(asked.recv_timeout(Duration::from_secs(5)).is_ok());
}

#[test]
fn classic_start_asks_for_handover() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        si::on_busy_lock(&a(&["--classic"])),
        si::OnBusyLock::RequestHandover
    );
    // Скрытый запуск окна не просил вовсе — и классическим его не сделать.
    assert_eq!(
        si::on_busy_lock(&a(&["--tray", "--classic"])),
        si::OnBusyLock::ExitQuietly
    );
    assert_eq!(si::on_busy_lock(&a(&["--classical"])), si::OnBusyLock::Knock);
}

#[test]
fn handover_word_and_who_answers_it() {
    assert_eq!(si::parse_request(b"handover\n"), Some(si::Request::Handover));
    assert_eq!(si::parse_request(b"handover now\n"), None);
    assert_eq!(
        si::parse_request(b"show\n"),
        Some(si::Request::Show(si::ShowRequest { section: None }))
    );
    // Держатель — классическое окно: передавать нечего, поднять себя.
    assert_eq!(
        si::listener_action(si::Holder::Python, b"handover\n"),
        Some(si::ListenerAction::Raise(None))
    );
    assert_eq!(
        si::listener_action(si::Holder::Rust, b"handover\n"),
        Some(si::ListenerAction::HandOver)
    );
    assert_eq!(
        si::listener_action(si::Holder::Rust, b"show settings\n"),
        Some(si::ListenerAction::Raise(Some((
            "settings".to_string(),
            String::new()
        ))))
    );
    assert_eq!(si::listener_action(si::Holder::Rust, b"hide\n"), None);
}

#[test]
fn listener_without_handover_refuses_it() {
    // Слушатель через `spawn` (так пока стоит Rust-вход) хозяина не отдаёт:
    // отказ, окно не трогает, классический запуск честно получает «не вышло».
    let dir = sandbox("nohand");
    let lock = si::lock_path(dir.to_str().unwrap());
    let (sock, asked) = listener_saying(&dir, true);
    let out = under_watchdog("handover старому слушателю", Duration::from_secs(10), move || {
        si::request_handover(&sock, &lock, Duration::from_millis(300))
    });
    assert_eq!(out, si::HandoverOutcome::Failed);
    assert!(asked.recv_timeout(Duration::from_millis(300)).is_err(), "окно трогать не смели");
}

#[test]
fn python_holder_raises_instead_of_handing_over() {
    let dir = sandbox("pyhold");
    let lock = si::lock_path(dir.to_str().unwrap());
    let sock = si::socket_path(dir.to_str().unwrap());
    let (tx, asked) = mpsc::channel();
    si::ActivationListener::spawn_for(&sock, si::Holder::Python, move |action| {
        let _ = tx.send(action);
        true
    })
    .expect("слушатель");
    let out = under_watchdog("handover классическому держателю", Duration::from_secs(10), move || {
        si::request_handover(&sock, &lock, Duration::from_millis(300))
    });
    assert_eq!(out, si::HandoverOutcome::Raised);
    assert_eq!(
        asked.recv_timeout(Duration::from_secs(5)).expect("спросил"),
        si::ListenerAction::Raise(None)
    );
}

/// Настоящая передача замка между ДВУМЯ процессами: дочерний прогон этого же
/// файла держит замок слушателем Rust-держателя и на «handover» уходит, а
/// этот процесс ждёт замок функцией ядра и берёт его.
#[test]
fn rust_holder_hands_over_the_lock() {
    if let Ok(dir) = std::env::var("REXON_SI_HANDOVER_HOLDER") {
        // Роль держателя.
        let lock = si::lock_path(&dir);
        let sock = si::socket_path(&dir);
        assert!(si::acquire_lock(&lock), "держатель обязан взять замок");
        si::ActivationListener::spawn_for(&sock, si::Holder::Rust, |action| {
            if action == si::ListenerAction::HandOver {
                // Уход запланирован, а не немедленный: «bye» уходит первым.
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(200));
                    std::process::exit(0);
                });
            }
            true
        })
        .expect("слушатель держателя");
        std::thread::sleep(Duration::from_secs(20));
        eprintln!("держатель так и не получил просьбы уступить замок");
        std::process::exit(3);
    }

    let _turn = LOCK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = sandbox("handover");
    let d = dir.to_str().unwrap().to_string();
    let lock = si::lock_path(&d);
    let sock = si::socket_path(&d);
    let mut child = std::process::Command::new(std::env::current_exe().expect("свой бинарь"))
        .args(["rust_holder_hands_over_the_lock", "--exact", "--nocapture"])
        .env("REXON_SI_HANDOVER_HOLDER", &d)
        .spawn()
        .expect("дочерний держатель");

    let probe = sock.clone();
    under_watchdog("держатель встал", Duration::from_secs(10), move || {
        while !si::socket_alive(&probe) {
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    assert!(!si::acquire_lock(&lock), "замок обязан держать дочерний процесс");

    let (s, l) = (sock.clone(), lock.clone());
    let out = under_watchdog("передача хозяина", Duration::from_secs(15), move || {
        si::request_handover(&s, &l, si::HANDOVER_WAIT)
    });
    assert_eq!(out, si::HandoverOutcome::Acquired, "замок обязан перейти к нам");
    assert!(si::holds_lock(&lock), "взятый замок обязан числиться нашим");
    let status = child.wait().expect("держатель завершился");
    assert!(status.success(), "держатель обязан уйти штатно: {status:?}");
}

#[test]
fn hidden_start_never_knocks_on_a_busy_lock() {
    // Скрытый запуск (автозапуск при входе) окна не просил: стук вытащил
    // бы уже открытое окно на экран.
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        si::on_busy_lock(&args(&["--tray"])),
        si::OnBusyLock::ExitQuietly
    );
    assert_eq!(
        si::on_busy_lock(&args(&["--page=install", "--tray"])),
        si::OnBusyLock::ExitQuietly
    );
    // Обычный второй запуск (ярлык) — стучится и поднимает окно.
    assert_eq!(si::on_busy_lock(&args(&[])), si::OnBusyLock::Knock);
    assert_eq!(
        si::on_busy_lock(&args(&["--page=backups"])),
        si::OnBusyLock::Knock
    );
    assert_eq!(si::on_busy_lock(&args(&["--traylike"])), si::OnBusyLock::Knock);
}

// ============================================================================
// Слушатель и стук
// ============================================================================

/// Поднять слушателя, отвечающего заданным исходом, и вернуть его адрес.
/// Оставить по адресу МЁРТВЫЙ сокет — файл есть, слушателя нет, — как после
/// убитого процесса.
///
/// ⚠️ Мало привязать слушатель и закрыть его. Соседние сверки в этом же
/// процессе запускают дочерние процессы (`flock`, свой бинарь), и дочерний
/// процесс между `fork` и `exec` держит копии ВСЕХ дескрипторов родителя —
/// в том числе только что закрытого слушателя. Пока такая копия жива, сокет
/// честно принимает подключения, и `socket_alive` правильно отвечает «жив».
/// Сверка тогда краснела бы изредка (замер: 3 из 60) не на поведении, а на
/// гонке с соседним тестом. Поэтому ждём, пока адрес действительно перестанет
/// принимать подключения. У убитого процесса такой копии нет, продукт это не
/// затрагивает.
fn leave_dead_socket(path: &str) {
    std::os::unix::net::UnixListener::bind(path)
        .map(drop)
        .unwrap_or_else(|e| panic!("привязка мёртвого сокета {path} ({} байт): {e}", path.len()));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while std::os::unix::net::UnixStream::connect(path).is_ok() {
        assert!(
            std::time::Instant::now() < deadline,
            "закрытый слушатель на {path} принимает подключения дольше 5 с"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn listener_saying(dir: &PathBuf, raised: bool) -> (String, mpsc::Receiver<bool>) {
    let sock = si::socket_path(dir.to_str().unwrap());
    let (tx, rx) = mpsc::channel();
    si::ActivationListener::spawn(&sock, move || {
        let _ = tx.send(true);
        raised
    })
    .unwrap_or_else(|e| {
        panic!("слушатель обязан встать в подставном каталоге: {sock} ({} байт): {e}", sock.len())
    });
    (sock, rx)
}

#[test]
fn knock_raises_the_running_window() {
    let dir = sandbox("knock");
    let (sock, asked) = listener_saying(&dir, true);
    let probe = sock.clone();

    let ok = under_watchdog("стук в живого слушателя", Duration::from_secs(10), move || {
        si::activate_running_instance(&probe)
    });

    assert!(ok, "живой слушатель поднял окно — стук обязан вернуть true");
    assert!(
        asked.recv_timeout(Duration::from_secs(5)).is_ok(),
        "слушатель обязан был спросить окно, а не ответить сам"
    );
    assert!(std::path::Path::new(&sock).exists());
}

#[test]
fn knock_tells_the_truth_when_window_did_not_rise() {
    // Тот самый дефект 06.09.2026 наоборот: соврать «поднял» и оставить
    // экран пустым нельзя — второй запуск обязан узнать правду и отправить
    // человека в лоток.
    let dir = sandbox("refused");
    let (sock, _asked) = listener_saying(&dir, false);

    let ok = under_watchdog("стук в отказавшего", Duration::from_secs(10), move || {
        si::activate_running_instance(&sock)
    });
    assert!(!ok, "«не поднял» обязан дойти до второго запуска как false");
}

#[test]
fn knock_without_listener_is_false_not_a_hang() {
    let dir = sandbox("nobody");
    let sock = si::socket_path(dir.to_str().unwrap());
    let probe = sock.clone();
    // Rexon старее этой правки: замок держит, слушателя нет. Стук обязан
    // сразу вернуть false, а не ждать.
    let ok = under_watchdog("стук в пустоту", Duration::from_secs(5), move || {
        si::activate_running_instance(&probe)
    });
    assert!(!ok);

    // Мёртвый файл сокета от прошлой сессии — тоже отказ, а не зависание.
    std::fs::write(&sock, b"").expect("подделка мёртвого файла");
    let ok = under_watchdog("стук в мёртвый файл", Duration::from_secs(5), move || {
        si::activate_running_instance(&sock)
    });
    assert!(!ok);
}

#[test]
fn stray_word_does_not_touch_the_window() {
    // Чужой стук в наш адрес — не повод дёргать экран покупателю.
    let dir = sandbox("stray");
    let sock = si::socket_path(dir.to_str().unwrap());
    let (tx, asked) = mpsc::channel();
    si::ActivationListener::spawn(&sock, move || {
        let _ = tx.send(true);
        true
    })
    .expect("слушатель");

    let reply = under_watchdog("чужое слово", Duration::from_secs(10), move || {
        let mut conn = UnixStream::connect(&sock).expect("соединение");
        conn.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        conn.write_all(b"hide\n").unwrap();
        let mut buf = [0u8; 16];
        let n = conn.read(&mut buf).unwrap_or(0);
        buf[..n].to_vec()
    });

    assert_eq!(reply, si::REPLY_FAILED, "на чужое слово — отказ");
    assert!(
        asked.recv_timeout(Duration::from_millis(300)).is_err(),
        "окно трогать не смели"
    );
}

// ============================================================================
// Кража общего сокета — то, чего делать НЕЛЬЗЯ
// ============================================================================

#[test]
fn bind_never_steals_a_live_socket() {
    // ⚠️⚠️ Ради этой сверки модуль и не повторяет `QLocalServer::removeServer`:
    // тот сносит файл по адресу БЕЗУСЛОВНО. В общем на весь сеанс каталоге
    // это означает отнять адрес у живой программы — уже случалось.
    let dir = sandbox("steal");
    let (sock, _asked) = listener_saying(&dir, true);

    let second = si::ActivationListener::bind(&sock);
    assert!(
        matches!(second, Err(si::BindError::Busy)),
        "второй слушатель обязан уйти ни с чем, а не забрать адрес"
    );

    // И главное: первый по-прежнему на месте и отвечает.
    let probe = sock.clone();
    let ok = under_watchdog("первый жив после отказа", Duration::from_secs(10), move || {
        si::activate_running_instance(&probe)
    });
    assert!(ok, "живой слушатель обязан был уцелеть");
}

#[test]
fn bind_clears_only_a_dead_socket_and_never_a_stranger_file() {
    let dir = sandbox("stale");
    let sock = si::socket_path(dir.to_str().unwrap());

    // Мёртвый сокет прошлой сессии: снести можно, и слушатель встаёт.
    {
        let dead = si::ActivationListener::bind(&sock).expect("первый");
        drop(dead);
    }
    // Специально оставляем файл на месте, как это бывает после убитого
    // процесса (Drop не отработал).
    leave_dead_socket(&sock);
    let _fresh = si::ActivationListener::bind(&sock).expect("мёртвый сокет не помеха");

    // А вот НЕ сокет по адресу — руками не сносим: чужой файл не наш.
    let dir2 = sandbox("notsock");
    let plain = si::socket_path(dir2.to_str().unwrap());
    std::fs::write(&plain, "чужое").expect("подделка обычного файла");
    assert!(
        matches!(
            si::ActivationListener::bind(&plain),
            Err(si::BindError::NotASocket)
        ),
        "обычный файл по адресу сноситься не должен"
    );
    assert!(std::path::Path::new(&plain).exists(), "файл обязан уцелеть");
}

#[test]
fn clearing_the_address_spares_a_live_socket() {
    // Та же функция, что снимает мёртвый сокет перед Python-слушателем
    // (вместо безусловного `QLocalServer::removeServer`).
    let dir = sandbox("clear");
    let sock = si::socket_path(dir.to_str().unwrap());
    assert!(si::clear_stale_socket(&sock).is_ok(), "пустой адрес свободен");

    let (sock, _asked) = listener_saying(&dir, true);
    assert!(
        matches!(si::clear_stale_socket(&sock), Err(si::BindError::Busy)),
        "живой сокет сносить нельзя"
    );
    let probe = sock.clone();
    let ok = under_watchdog("живой после отказа", Duration::from_secs(10), move || {
        si::activate_running_instance(&probe)
    });
    assert!(ok, "живой слушатель обязан уцелеть и отвечать");

    // Мёртвый сокет (процесс убит, файл остался) — снести можно.
    let dir2 = sandbox("clear-dead");
    let dead = si::socket_path(dir2.to_str().unwrap());
    leave_dead_socket(&dead);
    assert!(std::path::Path::new(&dead).exists());
    let cleared = si::clear_stale_socket(&dead);
    assert!(cleared.is_ok(), "мёртвый сокет — не помеха, а вернулось {cleared:?}");
    assert!(!std::path::Path::new(&dead).exists(), "мёртвый сокет обязан уйти");

    // Не сокет — не трогаем.
    let dir3 = sandbox("clear-file");
    let plain = si::socket_path(dir3.to_str().unwrap());
    std::fs::write(&plain, "чужое").expect("подделка обычного файла");
    assert!(matches!(
        si::clear_stale_socket(&plain),
        Err(si::BindError::NotASocket)
    ));
    assert!(std::path::Path::new(&plain).exists(), "файл обязан уцелеть");
}

#[test]
fn dropped_listener_takes_only_its_own_socket() {
    let dir = sandbox("drop");
    let sock = si::socket_path(dir.to_str().unwrap());

    // Свой сокет за собой убираем: следующий запуск не должен разбираться с
    // нашим мусором.
    let ours = si::ActivationListener::bind(&sock).expect("наш слушатель");
    drop(ours);
    assert!(
        !std::path::Path::new(&sock).exists(),
        "свой сокет обязан уйти вместе со слушателем"
    );

    // А ЧУЖОЙ сокет, успевший встать на тот же адрес, наш Drop трогать не
    // смеет — это та же кража общего адреса, только задним ходом. Поэтому
    // сверяется inode, а не путь: подменяем сокет под живым слушателем и
    // смотрим, что после его смерти чужой уцелел.
    let ours = si::ActivationListener::bind(&sock).expect("наш слушатель");
    std::fs::remove_file(&sock).expect("снять наш");
    let stranger = std::os::unix::net::UnixListener::bind(&sock).expect("чужой слушатель");
    drop(ours);
    assert!(
        std::path::Path::new(&sock).exists(),
        "чужой сокет обязан уцелеть после нашего Drop"
    );
    drop(stranger);
}

// ============================================================================
// Длинный XDG_RUNTIME_DIR и чужой пользователь
// ============================================================================

/// Подставной runtime-каталог, у которого основной адрес за пределом.
fn long_runtime_dir(dir: &PathBuf) -> String {
    let long = dir.join("d".repeat(120));
    std::fs::create_dir_all(&long).expect("длинный подставной каталог");
    let long = long.to_str().unwrap().to_string();
    assert!(si::preferred_socket_path(&long).len() > SUN_PATH_MAX);
    long
}

#[test]
fn knock_reaches_the_listener_on_a_long_runtime_dir() {
    let dir = sandbox("long");
    let runtime = long_runtime_dir(&dir);
    let lock = si::lock_path(&runtime);
    // Слушатель и стук считают адрес ОДНОЙ функцией, каждый сам.
    let listen_at = si::socket_path(&runtime);
    let knock_at = si::socket_path(&runtime);
    assert_eq!(listen_at, knock_at);
    let fallback_dir = si::fallback_socket_dir(si::current_uid());
    assert!(listen_at.starts_with(&format!("{fallback_dir}/")), "{listen_at}");

    // Слушатель обслуживает ОДИН стук и уходит: запасной каталог общий для
    // пользователя и не принадлежит стенду, и навечно поднятый слушатель
    // оставлял бы там файл сокета от каждого прогона.
    let server = si::ActivationListener::bind(&listen_at)
        .unwrap_or_else(|e| panic!("слушатель на длинном каталоге обязан встать: {e}"));
    let (tx, asked) = mpsc::channel();
    let serving = std::thread::spawn(move || {
        server.serve_one_for(si::Holder::Rust, &mut |action| {
            let _ = tx.send(action);
            true
        })
    });

    let out = under_watchdog("стук на длинном каталоге", Duration::from_secs(10), move || {
        si::knock(&knock_at, Some(("settings", "")))
    });
    assert_eq!(out, si::KnockOutcome::Raised);
    assert_eq!(
        asked.recv_timeout(Duration::from_secs(5)).expect("спросил"),
        si::ListenerAction::Raise(Some(("settings".to_string(), String::new())))
    );
    assert_eq!(serving.join().expect("поток слушателя"), Some(true));
    assert!(
        !std::path::Path::new(&listen_at).exists(),
        "свой сокет в запасном каталоге обязан уйти вместе со слушателем"
    );

    // Запасной каталог — личный: этого пользователя и 0700.
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(&fallback_dir).expect("запасной каталог");
    assert!(meta.is_dir());
    assert_eq!(meta.uid(), si::current_uid());
    assert_eq!(meta.mode() & 0o777, 0o700);
    // Замок длины не боится (flock путь в sun_path не кладёт) и остаётся в
    // runtime-каталоге сеанса. Сам замок здесь не берём: он на процесс один,
    // и захват вне `LOCK_TESTS` отнял бы его у соседней сверки.
    assert_eq!(std::path::Path::new(&lock).parent(), Some(std::path::Path::new(&runtime)));
}

#[test]
fn stranger_knock_is_refused_without_touching_the_window() {
    let dir = sandbox("stranger");
    let sock = si::socket_path(dir.to_str().unwrap());
    let stranger = si::current_uid().wrapping_add(1);
    // Слушатель, считающий своим другого пользователя, — то же, что стук
    // от чужого: uid стучащего не совпадает с хозяином.
    let server = si::ActivationListener::bind(&sock)
        .expect("слушатель")
        .with_owner_uid(stranger);
    let (tx, asked) = mpsc::channel();
    std::thread::spawn(move || {
        server.serve_forever_for(si::Holder::Rust, move |action| {
            let _ = tx.send(action);
            true
        })
    });

    let (s1, s2) = (sock.clone(), sock.clone());
    let lock = si::lock_path(dir.to_str().unwrap());
    let (show, handover) = under_watchdog("чужой стук", Duration::from_secs(10), move || {
        (
            si::knock(&s1, None),
            si::request_handover(&s2, &lock, Duration::from_millis(200)),
        )
    });
    // Отказ «no» либо оборванное соединение — слушатель отвечает, не читая
    // слова, и закрывает; какой из двух дойдёт, зависит от порядка потоков.
    // Важно одно: не «поднял».
    assert_ne!(show, si::KnockOutcome::Raised, "чужому — отказ");
    assert_eq!(handover, si::HandoverOutcome::Failed, "чужой не вправе забрать место");
    assert!(
        asked.recv_timeout(Duration::from_millis(300)).is_err(),
        "окно на чужой стук трогать не смели"
    );
}

#[test]
fn knock_goes_only_to_a_listener_of_the_same_user() {
    let dir = sandbox("owner");
    let (sock, asked) = listener_saying(&dir, true);
    let stranger = si::current_uid().wrapping_add(1);
    let out = under_watchdog("стук чужому слушателю", Duration::from_secs(10), move || {
        si::knock_expecting_owner(&sock, None, stranger)
    });
    match out {
        si::KnockOutcome::NoAnswer(reason) => {
            assert!(reason.contains("не наш процесс"), "причина: {reason}")
        }
        other => panic!("слово чужому слушателю уходить не должно: {other:?}"),
    }
    assert!(asked.recv_timeout(Duration::from_millis(300)).is_err(), "слово не ушло");
}

#[test]
fn private_dir_refuses_what_is_not_ours() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let dir = sandbox("private");
    let uid = si::current_uid();

    let fresh = dir.join("fresh");
    let fresh = fresh.to_str().unwrap();
    si::ensure_private_dir(fresh, uid).expect("новый каталог");
    assert_eq!(std::fs::metadata(fresh).unwrap().mode() & 0o777, 0o700);

    let loose = dir.join("loose");
    std::fs::create_dir(&loose).unwrap();
    std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o755)).unwrap();
    si::ensure_private_dir(loose.to_str().unwrap(), uid).expect("свой каталог с лишними правами");
    assert_eq!(std::fs::metadata(&loose).unwrap().mode() & 0o777, 0o700);

    let reason = |r: Result<(), si::BindError>| match r {
        Err(e @ si::BindError::UnsafeDir { .. }) => e.to_string(),
        other => panic!("обязан отказать, а вернулось {other:?}"),
    };
    let foreign = reason(si::ensure_private_dir(fresh, uid.wrapping_add(1)));
    assert!(foreign.contains("владелец"), "{foreign}");

    let file = dir.join("file");
    std::fs::write(&file, "чужое").unwrap();
    assert!(reason(si::ensure_private_dir(file.to_str().unwrap(), uid)).contains("не каталог"));

    let link = dir.join("link");
    std::os::unix::fs::symlink(fresh, &link).unwrap();
    assert!(reason(si::ensure_private_dir(link.to_str().unwrap(), uid)).contains("ссылка"));
}

#[test]
fn too_long_address_names_path_length_and_limit() {
    let path = format!("/{}", "x".repeat(199));
    match si::clear_stale_socket(&path) {
        Err(e @ si::BindError::TooLong(_)) => {
            let text = e.to_string();
            assert!(text.contains("200 байт") && text.contains("107") && text.contains(&path));
        }
        other => panic!("длинный адрес обязан быть назван, а вернулось {other:?}"),
    }
    assert!(matches!(si::ActivationListener::bind(&path), Err(si::BindError::TooLong(_))));
    match si::knock(&path, None) {
        si::KnockOutcome::NoAnswer(reason) => assert!(reason.contains("200 байт"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn busy_lock_texts_are_honest_and_plain() {
    let _lang = i18n_pin::ru();
    let lock = "/run/user/1000/rexon.lock";
    let said = |k: Option<&si::KnockOutcome>| si::said_on_busy_lock(lock, k).join("\n");

    assert_eq!(said(Some(&si::KnockOutcome::Raised)), si::said_raised());
    let silent = said(Some(&si::KnockOutcome::NoAnswer("техническая причина".into())));
    assert_eq!(silent, si::said_not_responding());
    assert!(!silent.contains("техническая"), "причина — в журнал, не человеку");
    assert!(silent.contains("не отвечает") && silent.contains("перезапустите"));
    assert!(said(Some(&si::KnockOutcome::Refused)).contains(&si::said_look_in_tray()));
    assert!(!said(None).contains(&si::said_look_in_tray()), "скрытый запуск окна не просил");

    // Правило #13: в том, что читает человек при сломанной двери, — ни
    // одного технического слова.
    for jargon in [
        "сокет", "socket", "sun_path", "sun_len", "xdg", "/tmp", "/run", "flock", "uid", "байт",
        "замок", "трее",
    ] {
        assert!(
            !silent.to_lowercase().contains(jargon),
            "жаргон «{jargon}» в тексте: {silent}"
        );
    }
}

/// По-русски — дословно как было до переноса строк в каталог.
#[test]
fn busy_lock_texts_in_russian_as_before() {
    let _lang = i18n_pin::ru();
    let lock = "/run/user/1000/rexon.lock";
    assert_eq!(
        si::said_on_busy_lock(lock, Some(&si::KnockOutcome::Raised)),
        vec!["↑ Rexon уже запущен — поднял его окно."]
    );
    assert_eq!(
        si::said_on_busy_lock(lock, Some(&si::KnockOutcome::Refused)),
        vec![
            "⚠️  Rexon уже запущен — второй значок в трее не создаю.".to_string(),
            format!("    Замок: {lock}"),
            "    Поднять окно не удалось — ищите значок Rexon в системном лотке.".to_string(),
        ]
    );
    assert_eq!(
        si::said_on_busy_lock(lock, Some(&si::KnockOutcome::NoAnswer("x".into()))),
        vec![
            "⚠️  Rexon уже открыт, но не отвечает на просьбу показать окно. \
             Откройте его из значка в системном лотке или перезапустите Rexon."
        ]
    );
}

#[test]
fn english_busy_lock_texts_have_no_russian() {
    let _lang = i18n_pin::en();
    let lock = "/run/user/1000/rexon.lock";
    let mut all = Vec::new();
    for k in [
        None,
        Some(si::KnockOutcome::Raised),
        Some(si::KnockOutcome::Refused),
        Some(si::KnockOutcome::NoAnswer("x".into())),
    ] {
        all.extend(si::said_on_busy_lock(lock, k.as_ref()));
    }
    for line in &all {
        assert!(
            !line.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')),
            "русская буква в английском тексте: {line}"
        );
    }
    assert_eq!(
        si::said_on_busy_lock(lock, Some(&si::KnockOutcome::Refused)),
        vec![
            "⚠️  Rexon is already running — not adding a second tray icon.".to_string(),
            format!("    Lock: {lock}"),
            "    Couldn't bring up the window — look for the Rexon icon in the system tray."
                .to_string(),
        ]
    );
    let silent = si::said_not_responding();
    assert_eq!(
        silent,
        "⚠️  Rexon is already open but doesn't respond to the request to show its window. \
         Open it from the icon in the system tray or restart Rexon."
    );
    for jargon in ["socket", "sun_path", "xdg", "/tmp", "/run", "flock", "uid", "bytes", "lock"] {
        assert!(!silent.to_lowercase().contains(jargon), "жаргон «{jargon}» в тексте: {silent}");
    }
}

// ============================================================================
// Укус сторожа
// ============================================================================

/// Сторож обязан УБИВАТЬ зависшую сверку, и это проверяется по-настоящему:
/// дочерний прогон этого же файла ждёт `accept`, которого не будет, и обязан
/// умереть кодом 97 со своей строкой, а не висеть.
///
/// Отдельным процессом — потому что укус по определению убивает процесс:
/// внутри своего он унёс бы весь прогон.
#[test]
fn watchdog_bites() {
    if std::env::var("REXON_SINGLE_INSTANCE_WATCHDOG_VICTIM").is_ok() {
        // Роль жертвы: висим внутри сторожа заведомо дольше отпущенного.
        under_watchdog("заведомо зависшая работа", Duration::from_millis(300), || {
            std::thread::sleep(Duration::from_secs(30));
        });
        eprintln!("сторож не сработал — жертва дожила до конца");
        std::process::exit(0);
    }

    let out = std::process::Command::new(std::env::current_exe().expect("свой бинарь"))
        .args(["watchdog_bites", "--exact", "--nocapture"])
        .env("REXON_SINGLE_INSTANCE_WATCHDOG_VICTIM", "1")
        .output()
        .expect("дочерний прогон");
    let code = out.status.code();
    let text = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        code,
        Some(97),
        "сторож обязан убить зависшую сверку кодом 97, а не {code:?}; stderr: {text}"
    );
    assert!(
        text.contains("сторож:") && text.contains("не уложилась"),
        "укус обязан назвать причину; stderr: {text}"
    );
}

// ============================================================================
// Перезапуск: новый процесс ждёт ухода прежнего (живой случай 08.10.2026)
// ============================================================================

/// Держатель замка — настоящий чужой процесс `flock(1)`, спящий `secs`.
fn hold_lock_for(lock: &str, secs: &str) -> std::process::Child {
    let child = std::process::Command::new("/usr/bin/flock")
        .args(["-x", lock, "sleep", secs])
        .spawn()
        .expect("flock(1) есть в util-linux");
    let probe = lock.to_string();
    under_watchdog("держатель взял замок", Duration::from_secs(10), move || {
        while std::process::Command::new("/usr/bin/flock")
            .args(["-n", &probe, "true"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    child
}

#[test]
fn restart_waits_for_previous_and_takes_the_lock() {
    let _turn = LOCK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = sandbox("restart-wait");
    let lock = si::lock_path(dir.to_str().unwrap());
    let mut old = hold_lock_for(&lock, "1");
    let pid = old.id();
    assert_eq!(si::lock_holder_pid(&lock).is_some(), true, "держатель виден в /proc/locks");
    assert!(si::process_alive(pid));
    let (l, p) = (lock.clone(), pid);
    let out = under_watchdog("ожидание прежнего", Duration::from_secs(15), move || {
        si::wait_for_previous(&l, p, Duration::from_secs(10))
    });
    assert!(matches!(out, si::PreviousWait::Acquired(d) if d >= Duration::from_millis(500)), "{out:?}");
    let _ = old.wait();
}

#[test]
fn restart_gives_up_when_previous_hangs() {
    let _turn = LOCK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = sandbox("restart-hang");
    let lock = si::lock_path(dir.to_str().unwrap());
    let mut old = hold_lock_for(&lock, "5");
    let (l, p) = (lock.clone(), old.id());
    let out = under_watchdog("сдача по сроку", Duration::from_secs(10), move || {
        si::wait_for_previous(&l, p, Duration::from_millis(600))
    });
    assert_eq!(out, si::PreviousWait::PreviousStillRunning);
    let _ = old.kill();
    let _ = old.wait();
}

#[test]
fn restart_notices_another_holder_after_previous_left() {
    let _turn = LOCK_TESTS.lock().unwrap_or_else(|e| e.into_inner());
    let dir = sandbox("restart-other");
    let lock = si::lock_path(dir.to_str().unwrap());
    // «Прежний» уже умер (и прибран), а замок держит посторонний.
    let mut gone = std::process::Command::new("true").spawn().expect("true");
    let gone_pid = gone.id();
    let _ = gone.wait();
    assert!(!si::process_alive(gone_pid));
    let mut other = hold_lock_for(&lock, "5");
    let (l, p) = (lock.clone(), gone_pid);
    let out = under_watchdog("посторонний держатель", Duration::from_secs(10), move || {
        si::wait_for_previous(&l, p, Duration::from_secs(5))
    });
    assert!(matches!(out, si::PreviousWait::OtherHolder(Some(_))), "{out:?}");
    let _ = other.kill();
    let _ = other.wait();
}

#[test]
fn proc_locks_line_is_parsed() {
    let text = "1: POSIX  ADVISORY  WRITE 999 00:1a:1 0 EOF\n\
                2: FLOCK  ADVISORY  WRITE 4242 00:2c:77 0 EOF\n\
                2: -> FLOCK  ADVISORY  WRITE 5555 00:2c:77 0 EOF\n";
    assert_eq!(si::lock_holder_in(text, 0, 0x2c, 77), Some(4242));
    assert_eq!(si::lock_holder_in(text, 0, 0x2c, 78), None);
}

// ============================================================================
// Rexon обновился, а работает прежняя копия
// ============================================================================

fn stamp(ino: u64, mtime: i64) -> si::FileStamp {
    si::FileStamp { dev: 1, ino, mtime_sec: mtime, mtime_nsec: 0 }
}

/// Решение по наблюдениям: хвост « (deleted)», смена inode или времени
/// изменения — замена; файла нет — перезапускаться не во что.
#[test]
fn exe_change_reads_deleted_suffix_and_stamps() {
    let a = stamp(10, 100);
    let exe = "/usr/lib/rexon/rexon-main-window";
    assert_eq!(si::exe_change(exe, Some(a), Some(a)), si::ExeChange::Same);
    assert_eq!(
        si::exe_change(exe, Some(a), Some(stamp(11, 100))),
        si::ExeChange::Replaced { path: exe.into(), stamp: stamp(11, 100) }
    );
    assert_eq!(
        si::exe_change(exe, Some(a), Some(stamp(10, 101))),
        si::ExeChange::Replaced { path: exe.into(), stamp: stamp(10, 101) },
        "переписан на месте — тоже замена"
    );
    let deleted = format!("{exe}{}", si::DELETED_EXE_SUFFIX);
    assert_eq!(
        si::exe_change(&deleted, None, Some(a)),
        si::ExeChange::Replaced { path: exe.into(), stamp: a },
        "снят с диска, по пути новый файл — путь без хвоста"
    );
    assert_eq!(si::exe_change(&deleted, Some(a), None), si::ExeChange::Gone);
    assert_eq!(si::exe_change(exe, Some(a), None), si::ExeChange::Same, "нечего сравнивать — молчим");
    assert_eq!(si::exe_change(exe, None, Some(a)), si::ExeChange::Same);
}

/// Живая сверка на подставной ссылке вместо `/proc/self/exe`: замена файла
/// переименованием (как у пакетного менеджера) и ссылка на снятый файл.
#[test]
fn exe_change_at_on_substitute_link() {
    let dir = sandbox("self-update");
    let exe = dir.join("rexon-main-window");
    let link = dir.join("exe");
    std::fs::write(&exe, "old").unwrap();
    std::os::unix::fs::symlink(&exe, &link).unwrap();
    let running = si::FileStamp::of(&link);
    assert!(running.is_some());
    assert_eq!(si::exe_change_at(&link, running), si::ExeChange::Same);

    let fresh = dir.join("rexon-main-window.new");
    std::fs::write(&fresh, "new").unwrap();
    std::fs::rename(&fresh, &exe).unwrap();
    let new_stamp = si::FileStamp::of(&exe).unwrap();
    assert_ne!(running, Some(new_stamp));
    assert_eq!(
        si::exe_change_at(&link, running),
        si::ExeChange::Replaced { path: exe.to_string_lossy().into_owned(), stamp: new_stamp }
    );

    // Как ядро показывает снятый файл: ссылка с хвостом « (deleted)».
    let deleted = dir.join("deleted-exe");
    let target = format!("{}{}", exe.to_string_lossy(), si::DELETED_EXE_SUFFIX);
    std::os::unix::fs::symlink(&target, &deleted).unwrap();
    assert!(matches!(si::exe_change_at(&deleted, None), si::ExeChange::Replaced { .. }));
    std::fs::remove_file(&exe).unwrap();
    assert_eq!(si::exe_change_at(&deleted, None), si::ExeChange::Gone);
    // Ссылки нет вовсе — молчим.
    assert_eq!(si::exe_change_at(&dir.join("nope"), None), si::ExeChange::Same);
}

/// Уведомление — один раз на обновление и только после подтверждения
/// вторым опросом; следующее обновление — снова один раз.
#[test]
fn self_update_notice_once_per_update() {
    let mut n = si::SelfUpdateNotice::default();
    let r = |s| si::ExeChange::Replaced { path: "/x".into(), stamp: s };
    let (v2, v3) = (stamp(2, 2), stamp(3, 3));
    assert!(!n.step(&si::ExeChange::Same));
    assert!(!n.step(&r(v2)), "первый опрос — ещё не устоялось");
    assert!(n.step(&r(v2)), "второй — показать");
    assert!(!n.step(&r(v2)) && !n.step(&r(v2)), "больше не показывать");
    assert!(!n.step(&r(v3)));
    assert!(n.step(&r(v3)), "новое обновление — снова один раз");
    // Замена посреди транзакции: отпечаток скачет — ждём, пока устоится.
    let mut m = si::SelfUpdateNotice::default();
    assert!(!m.step(&r(v2)));
    assert!(!m.step(&r(v3)));
    assert!(!m.step(&si::ExeChange::Gone));
    assert!(!m.step(&r(v3)));
    assert!(m.step(&r(v3)));
}

#[test]
fn update_restart_opens_window() {
    let args: Vec<String> =
        ["--tray", "--selftest", "--section=settings", "--page=backups", "--classic"].iter().map(|s| s.to_string()).collect();
    assert_eq!(si::update_restart_args(&args), vec!["--classic".to_string()]);
    assert_eq!(si::self_update_actions()[0].0, si::SELF_UPDATE_RESTART_ACTION);
    assert!(!si::self_update_title().is_empty() && !si::self_update_body().is_empty());
    assert!(!si::self_update_no_buttons().is_empty());
}

/// Второй запуск уходит без окна и обязан сам сказать панели «запуск
/// окончен» (01.11.2026: «Запускается Rexon» и думающий курсор до таймаута).
/// Формат — спецификация startup-notification: «remove: ID=…», куски по 20
/// байт, ноль в конце.
#[test]
fn startup_notification_remove_message() {
    assert_eq!(si::startup_id_from(None), None);
    assert_eq!(si::startup_id_from(Some("  ")), None);
    assert_eq!(
        si::startup_id_from(Some("mate-panel-1-host-rexon-1_TIME42")).as_deref(),
        Some("mate-panel-1-host-rexon-1_TIME42")
    );
    assert_eq!(si::startup_remove_message("test_TIME0"), "remove: ID=test_TIME0");
    assert_eq!(si::startup_remove_message("a b\"c\\"), "remove: ID=\"a b\\\"c\\\\\"");

    let msg = si::startup_remove_message("mate-panel-3371040-host-rexon-1_TIME123456");
    let chunks = si::startup_message_chunks(&msg);
    assert_eq!(chunks.len(), (msg.len() + 1).div_ceil(20));
    let joined: Vec<u8> = chunks.concat();
    assert_eq!(&joined[..msg.len()], msg.as_bytes());
    assert_eq!(joined[msg.len()], 0, "сообщение обязано кончаться нулём");
    // Ровно 20 байт с нулём — один кусок, а не два.
    assert_eq!(si::startup_message_chunks(&"x".repeat(19)).len(), 1);
    assert_eq!(si::startup_message_chunks(&"x".repeat(20)).len(), 2);
}
