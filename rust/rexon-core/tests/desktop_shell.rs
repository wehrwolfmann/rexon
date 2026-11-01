//! Сверка связки «композитор + шелл» и владельца слоя обоев.
//!
//! ⛔ Ни Noctalia, ни DankMaterialShell на машине не установлены, и ставить
//! их запрещено. Поэтому сверка идёт подставными наблюдениями: сюда подаются
//! ровно те имена файлов, юнитов, процессов и переменных, которые эти шеллы
//! оставляют в сеансе. Живьём связка НЕ проверялась — это честная граница
//! сверки, а не её пробел: ядро по устройству ничего не спрашивает у машины,
//! добывать наблюдения — работа вызывающего, и её проверяет уже он.
//!
//! Проверяется: каждый признак ПООТДЕЛЬНОСТИ поднимает шелл; их комбинации
//! не мешают друг другу; посторонние имена не путают; имя процесса само по
//! себе шелла не означает; при чужом владельце ответ «не наш» и есть готовый
//! текст покупателю.

mod i18n_pin;

use rexon_core::system::{
    detect_current_de, detect_shell, noctalia_lock_name, noctalia_socket_name, DesktopShell,
    ShellFacts, SHELL_ENV_MARKERS, SHELL_ENV_NAMES,
};
use rexon_core::desktop_shell::{give_label, return_label};
use rexon_core::wallpaper_engine::{
    shell_owner_message, wallpaper_layer_owner, WallpaperOwner,
};

fn s(items: &[&str]) -> Vec<String> {
    items.iter().map(|x| (*x).to_string()).collect()
}

/// Наблюдения пустые — шелла нет.
#[test]
fn пустой_сеанс_шелла_не_даёт() {
    assert_eq!(detect_shell(&ShellFacts::default()), None);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        ..Default::default()
    };
    assert_eq!(detect_shell(&facts), None);
}

/// Голый niri и niri с шеллом — ОДНО значение композитора и разные связки.
/// Ровно ради этого различия функция и заведена.
#[test]
fn композитор_один_а_связки_разные() {
    assert_eq!(detect_current_de("niri", "", true).as_deref(), Some("niri"));

    let голый = ShellFacts {
        wayland_display: "wayland-1",
        ..Default::default()
    };
    let файлы = s(&["noctalia-wayland-1.sock"]);
    let с_шеллом = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        ..Default::default()
    };
    assert_eq!(detect_shell(&голый), None);
    assert_eq!(
        detect_shell(&с_шеллом).map(|d| d.shell),
        Some(DesktopShell::Noctalia)
    );

    // И владелец фона у них РАЗНЫЙ.
    assert!(wallpaper_layer_owner("niri", "wayland", None, None).is_ours());
    assert!(!wallpaper_layer_owner(
        "niri",
        "wayland",
        Some(DesktopShell::Noctalia),
        None
    )
    .is_ours());
}

// ── Noctalia: каждый признак по отдельности ────────────────────────────────

#[test]
fn noctalia_по_файлу_блокировки() {
    // Сокет снимается только в деструкторе и после падения остаётся; flock
    // на .lock — то, чем шелл держит единственность запуска.
    let файлы = s(&["noctalia-wayland-1.lock"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("файл блокировки не опознан");
    assert_eq!(d.shell, DesktopShell::Noctalia);
    assert_eq!(d.evidence, vec!["файл noctalia-wayland-1.lock в каталоге сеанса"]);
}

#[test]
fn noctalia_по_имени_на_шине() {
    // Имя на шине протухшим не бывает — самый надёжный признак.
    for имя in ["dev.noctalia.Debug", "dev.noctalia.Mpris"] {
        let имена = s(&["org.freedesktop.Notifications", имя]);
        let facts = ShellFacts {
            dbus_names: &имена,
            ..Default::default()
        };
        let d = detect_shell(&facts).unwrap_or_else(|| panic!("{имя} не опознано"));
        assert_eq!(d.shell, DesktopShell::Noctalia, "{имя}");
        assert_eq!(d.evidence, vec![format!("имя {имя} на шине")]);
    }
    // Ничьи и условные имена шеллом не считаются: org.freedesktop.Notifications
    // берут все подряд, а KWinActiveWindow бывает только на KDE.
    for чужое in [
        "org.freedesktop.Notifications",
        "org.kde.StatusNotifierWatcher",
        "dev.noctalia.Other",
    ] {
        let имена = s(&[чужое]);
        let facts = ShellFacts {
            dbus_names: &имена,
            ..Default::default()
        };
        assert_eq!(detect_shell(&facts), None, "{чужое}");
    }
}

#[test]
fn noctalia_по_сокету_с_точным_именем() {
    let файлы = s(&["bus", "noctalia-wayland-1.sock", "pipewire-0"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("сокет Noctalia не опознан");
    assert_eq!(d.shell, DesktopShell::Noctalia);
    assert_eq!(d.evidence, vec!["файл noctalia-wayland-1.sock в каталоге сеанса"]);
    // Имя сокета собирается одной формулой, а не переписывается по месту.
    assert_eq!(noctalia_socket_name("wayland-1"), "noctalia-wayland-1.sock");
}

#[test]
fn noctalia_по_сокету_без_переменной_дисплея() {
    // Нас могли запустить без WAYLAND_DISPLAY — слепнуть из-за этого нельзя,
    // форма имени узнаётся и так. Сам шелл при пустой переменной подставляет
    // wayland-0, и формула имени делает то же.
    assert_eq!(noctalia_socket_name(""), "noctalia-wayland-0.sock");
    let файлы = s(&["noctalia-wayland-0.sock"]);
    let facts = ShellFacts {
        runtime_files: &файлы,
        ..Default::default()
    };
    assert_eq!(
        detect_shell(&facts).map(|d| d.shell),
        Some(DesktopShell::Noctalia)
    );
}

/// Штатного юнита у Noctalia нет (пакет ставит бинарь и .desktop с
/// `Exec=noctalia --daemon`), `noctalia.service` заводит только модуль
/// NixOS. Признак редкий, но если юнит есть — он доказательство.
#[test]
fn noctalia_по_юниту() {
    let юниты = s(&["pipewire.service", "noctalia.service"]);
    let facts = ShellFacts {
        active_user_units: &юниты,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("юнит Noctalia не опознан");
    assert_eq!(d.shell, DesktopShell::Noctalia);
    assert_eq!(d.evidence, vec!["запущен юнит noctalia.service"]);
}

// ── DankMaterialShell: каждый признак по отдельности ───────────────────────

#[test]
fn dms_по_каждому_служебному_файлу() {
    for имя in [
        "danklinux-4242.sock",
        "danklinux-4242.pid",
        "danklinux-4242.session",
        // ⚠️ Имени без номера процесса («danklinux.sock») здесь НЕТ намеренно.
        // Такой вариант проверить не удалось: путь сокета шелл берёт из
        // переменной `DMS_SOCKET`, а собирает её Go-бэкенд, исходников
        // которого в открытых репозиториях DankMaterialShell и danklinux
        // нет. Опознавать по неподтверждённому образцу нельзя: ложное
        // «шелл есть» заставит продукт отказаться ставить обои и советовать
        // выключить чужие — на машине, где этого шелла нет вовсе.
        // Встретится живьём — расширить `detect_shell` и дописать сюда.
    ] {
        let файлы = s(&["bus", имя]);
        let facts = ShellFacts {
            runtime_files: &файлы,
            ..Default::default()
        };
        let d = detect_shell(&facts).unwrap_or_else(|| panic!("{имя} не опознан"));
        assert_eq!(d.shell, DesktopShell::DankMaterialShell, "{имя}");
        assert_eq!(d.evidence, vec![format!("файл {имя} в каталоге сеанса")]);
    }
}

#[test]
fn dms_по_юниту() {
    let юниты = s(&["dms.service"]);
    let facts = ShellFacts {
        active_user_units: &юниты,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("юнит dms не опознан");
    assert_eq!(d.shell, DesktopShell::DankMaterialShell);
    assert_eq!(d.evidence, vec!["запущен юнит dms.service"]);
}

#[test]
fn dms_по_метке_окружения() {
    // Нас запустили ИЗ шелла — метку мы унаследовали.
    for имя in ["DMS_SOCKET", "DMS_EXECUTABLE"] {
        let окружение = [(имя, "/run/user/1000/danklinux-77.sock")];
        let facts = ShellFacts {
            env: &окружение,
            ..Default::default()
        };
        let d = detect_shell(&facts).unwrap_or_else(|| panic!("{имя} не опознана"));
        assert_eq!(d.shell, DesktopShell::DankMaterialShell, "{имя}");
        assert_eq!(d.evidence, vec![format!("переменная {имя} в окружении")]);
    }
    // `QS_APP_ID` — ПЕРЕМЕННАЯ окружения процесса `qs` (её ставит модуль
    // запуска `dankgo`), и признак она — только с ТОЧНЫМ значением DMS.
    let окружение = [("QS_APP_ID", "com.danklinux.dms")];
    let facts = ShellFacts {
        env: &окружение,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("QS_APP_ID=com.danklinux.dms не опознана");
    assert_eq!(d.shell, DesktopShell::DankMaterialShell);
    assert_eq!(d.evidence, vec!["переменная QS_APP_ID=com.danklinux.dms в окружении"]);

    // Пустое значение — не признак; чужие DMS_* — тоже (это входные
    // настройки самого шелла, их выставляет пользователь); `QS_APP_ID`
    // другой программы на том же модуле запуска (пример из комментария
    // самого `dankgo`) — тоже не DMS.
    for (имя, значение) in [
        ("DMS_SOCKET", ""),
        ("DMS_LOG_LEVEL", "debug"),
        ("DMS_SHELL_DIR", "/opt/dms"),
        ("QS_APP_ID", "com.danklinux.dankcalendar"),
        ("QS_APP_ID", ""),
    ] {
        let окружение = [(имя, значение)];
        let facts = ShellFacts {
            env: &окружение,
            ..Default::default()
        };
        assert_eq!(detect_shell(&facts), None, "{имя}={значение}");
    }
}

/// Список переменных для живого сбора не разъехался с таблицей признаков.
#[test]
fn имена_переменных_для_сбора_совпадают_с_таблицей() {
    let mut из_таблицы: Vec<&str> = SHELL_ENV_MARKERS
        .iter()
        .flat_map(|(_, m)| m.iter().map(|e| e.name))
        .collect();
    let mut собираемые: Vec<&str> = SHELL_ENV_NAMES.to_vec();
    из_таблицы.sort();
    собираемые.sort();
    assert_eq!(из_таблицы, собираемые);
}

/// Второй сокет демона Noctalia (`noctalia-dmenu-<дисплей>.sock`) тоже
/// шелл (его создаёт сам демон), но в подтверждении первым стоит ТОЧНОЕ имя
/// управляющего сокета, если он есть.
#[test]
fn noctalia_точное_имя_раньше_dmenu() {
    let файлы = s(&["noctalia-dmenu-wayland-1.sock"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("dmenu-сокет не опознан");
    assert_eq!(d.shell, DesktopShell::Noctalia);
    assert_eq!(d.evidence, vec!["файл noctalia-dmenu-wayland-1.sock в каталоге сеанса"]);

    // Порядок в каталоге — алфавитный, dmenu идёт раньше; подтверждение
    // всё равно называет точный сокет нашего дисплея.
    let файлы = s(&["noctalia-dmenu-wayland-1.sock", "noctalia-wayland-1.sock"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        ..Default::default()
    };
    assert_eq!(
        detect_shell(&facts).unwrap().evidence,
        vec!["файл noctalia-wayland-1.sock в каталоге сеанса"]
    );
    // Точное имя блокировки — тоже раньше запасного.
    let файлы = s(&["noctalia-dmenu-wayland-1.sock", "noctalia-wayland-1.lock"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        ..Default::default()
    };
    assert_eq!(
        detect_shell(&facts).unwrap().evidence,
        vec!["файл noctalia-wayland-1.lock в каталоге сеанса"]
    );
    assert_eq!(noctalia_lock_name("wayland-1"), "noctalia-wayland-1.lock");
    assert_eq!(noctalia_lock_name(""), "noctalia-wayland-0.lock");
}

// ── имя процесса: подтверждение, но не доказательство ──────────────────────

#[test]
fn одно_имя_процесса_шелла_не_означает() {
    // `noctalia msg …` и `dms ipc call …` — это командная строка шелла, а не
    // сам шелл: процесс с тем же именем живёт миллисекунды. Принимать такое
    // за живой шелл нельзя, иначе своя же проверка состояния объявила бы
    // владельцем фона чужого.
    // `qs` и `quickshell` не берём вовсе: под ними ходит ЛЮБОЙ шелл на
    // Quickshell, а поднять его можно и руками, без DankMaterialShell.
    for имя in ["noctalia", "dms", "qs", "quickshell"] {
        let процессы = s(&[имя, "kwin_wayland"]);
        let facts = ShellFacts {
            process_names: &процессы,
            ..Default::default()
        };
        assert_eq!(detect_shell(&facts), None, "имя процесса {имя}");
    }
}

#[test]
fn имя_процесса_попадает_в_подтверждения_к_доказательству() {
    let файлы = s(&["noctalia-wayland-1.sock"]);
    let процессы = s(&["noctalia"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        process_names: &процессы,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("шелл не опознан");
    assert_eq!(
        d.evidence,
        vec![
            "файл noctalia-wayland-1.sock в каталоге сеанса".to_string(),
            "процесс noctalia".to_string(),
        ]
    );
}

// ── комбинации и посторонние имена ─────────────────────────────────────────

#[test]
fn все_признаки_разом_складываются_а_не_путаются() {
    let файлы = s(&["danklinux-77.sock", "danklinux-77.pid"]);
    let юниты = s(&["dms.service"]);
    let процессы = s(&["dms", "qs"]);
    let окружение = [("DMS_SOCKET", "/run/user/1000/danklinux-77.sock")];
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        active_user_units: &юниты,
        dbus_names: &[],
        process_names: &процессы,
        env: &окружение,
    };
    let d = detect_shell(&facts).expect("шелл не опознан");
    assert_eq!(d.shell, DesktopShell::DankMaterialShell);
    assert_eq!(
        d.evidence,
        vec![
            "файл danklinux-77.sock в каталоге сеанса".to_string(),
            "запущен юнит dms.service".to_string(),
            "переменная DMS_SOCKET в окружении".to_string(),
            "процесс dms".to_string(),
        ]
    );
}

#[test]
fn похожие_имена_не_считаются() {
    // Ни одно из этих имён шеллом не является.
    let файлы = s(&[
        "noctalia.sock",        // без имени дисплея — не наша форма
        "noctalia-wayland-1",   // без расширения
        "danklinux77.sock",     // без разделителя
        "danklinux-77.log",     // чужое расширение
        "dbus-1",
    ]);
    let юниты = s(&["dms.socket", "noctalia-backup.service", "dms-shell.timer"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        active_user_units: &юниты,
        ..Default::default()
    };
    assert_eq!(detect_shell(&facts), None);
}

#[test]
fn признаки_обоих_шеллов_разом_решаются_числом_доказательств() {
    // Остатки прошлого сеанса: один файл от Noctalia и полный набор DMS.
    let файлы = s(&["noctalia-wayland-1.sock", "danklinux-9.sock"]);
    let юниты = s(&["dms.service"]);
    let facts = ShellFacts {
        wayland_display: "wayland-1",
        runtime_files: &файлы,
        active_user_units: &юниты,
        ..Default::default()
    };
    let d = detect_shell(&facts).expect("шелл не опознан");
    assert_eq!(d.shell, DesktopShell::DankMaterialShell);
    // А главный ответ от выбора не зависит: фон в обоих случаях чужой.
    for шелл in DesktopShell::all() {
        assert!(!wallpaper_layer_owner("niri", "wayland", Some(*шелл), None).is_ours());
    }
}

// ── владелец слоя обоев ────────────────────────────────────────────────────

#[test]
fn без_шелла_фон_наш() {
    assert_eq!(
        wallpaper_layer_owner("niri", "wayland", None, None),
        WallpaperOwner::Rexon
    );
    assert_eq!(
        wallpaper_layer_owner("kde", "wayland", None, Some(true)),
        WallpaperOwner::Rexon
    );
}

#[test]
fn с_шеллом_фон_чужой() {
    let o = wallpaper_layer_owner("niri", "wayland", Some(DesktopShell::Noctalia), None);
    assert_eq!(o, WallpaperOwner::Shell(DesktopShell::Noctalia));
    assert!(!o.is_ours());
    assert_eq!(o.shell(), Some(DesktopShell::Noctalia));
    // Noctalia живёт и на Plasma — композитор тут ни при чём.
    assert!(!wallpaper_layer_owner("kde", "wayland", Some(DesktopShell::Noctalia), None)
        .is_ours());
}

#[test]
fn выключенные_обои_шелла_возвращают_фон_нам() {
    // Покупатель сделал то, что мы просили, — фон снова наш.
    assert_eq!(
        wallpaper_layer_owner(
            "niri",
            "wayland",
            Some(DesktopShell::DankMaterialShell),
            Some(false)
        ),
        WallpaperOwner::Rexon
    );
    // «Не узнали» — это НЕ «выключено»: осторожная сторона.
    assert_eq!(
        wallpaper_layer_owner("niri", "wayland", Some(DesktopShell::DankMaterialShell), None),
        WallpaperOwner::Shell(DesktopShell::DankMaterialShell)
    );
}

#[test]
fn на_x11_и_под_mutter_фон_всегда_наш() {
    // Оба шелла — только Wayland; на X11 обои кладутся окном.
    assert_eq!(
        wallpaper_layer_owner("niri", "x11", Some(DesktopShell::Noctalia), None),
        WallpaperOwner::Rexon
    );
    // Mutter и Muffin не умеют wlr-layer-shell: занять слой обоев там
    // нечем, путь один — расширение оболочки.
    for de in ["gnome", "cinnamon"] {
        assert_eq!(
            wallpaper_layer_owner(de, "wayland", Some(DesktopShell::Noctalia), None),
            WallpaperOwner::Rexon,
            "{de}"
        );
    }
}

// ── текст покупателю ───────────────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn при_чужом_владельце_есть_что_сказать() {
    // Текст — по-русски из настоящего каталога.
    let _lang = i18n_pin::ru();
    for шелл in DesktopShell::all() {
        let t = shell_owner_message(*шелл);
        assert!(t.contains("Фон сейчас рисует"), "{t}");
        assert!(t.contains(шелл.label()), "{t}");
        // Что нажать — та самая кнопка, тем же словом, что на кнопке.
        assert!(t.contains("«Отдать обои Rexon»"), "{t}");
        assert!(t.contains("«Wallpaper Engine»"), "{t}");
        assert!(t.contains("примените обои Rexon ещё раз"), "{t}");
        // Жаргону (пути, ключи, идентификаторы) в тексте не место.
        for жаргон in ["~/", "settings.toml", "screenPreferences", "enabled", "noctalia:"] {
            assert!(!t.contains(жаргон), "{жаргон} в «{t}»");
        }
    }
    assert_eq!(give_label(), "Отдать обои Rexon");
    assert_eq!(return_label(), "Вернуть оболочке");
}

/// Идентификаторы шеллов не разъезжаются с именами.
#[test]
fn идентификаторы_и_имена() {
    assert_eq!(DesktopShell::Noctalia.as_str(), "noctalia");
    assert_eq!(DesktopShell::DankMaterialShell.as_str(), "dms");
    assert_eq!(DesktopShell::Noctalia.label(), "Noctalia");
    assert_eq!(DesktopShell::DankMaterialShell.label(), "DankMaterialShell");
    assert_eq!(DesktopShell::all().len(), 2);
    assert_eq!(
        serde_json::to_string(&DesktopShell::DankMaterialShell).unwrap(),
        "\"dms\""
    );
}

/// Тот же совет по-английски: без русских букв, с той же кнопкой.
#[test]
fn english_shell_owner_message_has_no_russian() {
    let _lang = i18n_pin::en();
    for шелл in DesktopShell::all() {
        let t = shell_owner_message(*шелл);
        assert!(
            !t.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')),
            "{t}"
        );
        assert!(t.starts_with("The background is currently drawn by "), "{t}");
        assert!(t.contains("press “Give Wallpaper to Rexon”"), "{t}");
        assert!(t.ends_with("apply the Rexon wallpaper again."), "{t}");
        assert!(t.contains(шелл.label()), "{t}");
    }
    assert_eq!(give_label(), "Give Wallpaper to Rexon");
    assert_eq!(return_label(), "Return to Shell");
}

// ════════════════════════════════════════════════════════════════════════════
// Шаг 2: «Отдать обои Rexon» / «Вернуть оболочке»
// ════════════════════════════════════════════════════════════════════════════
//
// ⛔ Живьём шеллов на машине нет. Правка проверяется на подставных файлах
// тех форм, в каких их пишут сами шеллы (Noctalia — toml++, DMS —
// `JSON.stringify` с отступом), и на приватном HOME во временном каталоге:
// «до → после → обратно» обязано вернуть файл БИТ В БИТ.

use rexon_core::desktop_shell::{
    backup_path, dms_disable, dms_draws_wallpaper, dms_restore, give_back, hand_over,
    live_runtime_files, noctalia_disable, noctalia_draws_wallpaper, noctalia_restore,
    read_handover, shell_banner, shell_draws_wallpaper, shell_wallpaper_state, EditError,
    Previous, Restored, ShellHome,
};

/// Туда-обратно на одном тексте: выключено, затем ровно исходник.
fn noctalia_round_trip(before: Option<&str>) -> (String, Previous) {
    let (after, prev) = noctalia_disable(before).expect("правка отвергнута");
    assert_eq!(
        noctalia_draws_wallpaper(&[], Some(&after)),
        Some(false),
        "после правки обои Noctalia всё ещё включены:\n{after}"
    );
    match (noctalia_restore(Some(&after), &prev).expect("возврат отвергнут"), before) {
        (Restored::Write(back), Some(b)) => assert_eq!(back, b, "возврат не бит в бит"),
        (Restored::Delete, None) => {}
        (r, b) => panic!("возврат {r:?} при исходнике {b:?}"),
    }
    (after, prev)
}

#[test]
fn noctalia_правка_во_всех_формах_файла_и_возврат_бит_в_бит() {
    // Файла состояния нет — заводим, возврат удаляет.
    let (after, prev) = noctalia_round_trip(None);
    assert_eq!(after, "[wallpaper]\nenabled = false\n");
    assert_eq!(prev, Previous::FileAbsent);

    // Так пишет toml++: таблицы через пустую строку, подтаблицы обоев без
    // самой `[wallpaper]` (у неё нет своих ключей).
    let toml_pp = "config_version = 3\n\n[theme]\nmode = \"dark\"\n\n\
                   [wallpaper.default]\npath = \"/home/u/a.png\"\n\n\
                   [wallpaper.last]\npath = \"/home/u/a.png\"\n";
    let (after, prev) = noctalia_round_trip(Some(toml_pp));
    assert!(after.starts_with(toml_pp), "остальной файл тронут:\n{after}");
    assert_eq!(&after[toml_pp.len()..], "\n[wallpaper]\nenabled = false\n");
    assert_eq!(prev, Previous::KeyAbsent { parent_created: true, prefix: "\n".into() });

    // Таблица есть, ключа нет — строка встаёт сразу под заголовок.
    let hdr = "[wallpaper]\nfill_mode = \"crop\"\n\n[bar.default]\nposition = \"top\"\n";
    let (after, prev) = noctalia_round_trip(Some(hdr));
    assert_eq!(
        after,
        "[wallpaper]\nenabled = false\nfill_mode = \"crop\"\n\n[bar.default]\nposition = \"top\"\n"
    );
    assert_eq!(prev, Previous::KeyAbsent { parent_created: false, prefix: String::new() });

    // Ключ есть — меняется ТОЛЬКО значение, комментарий и отступы на месте.
    let key = "[wallpaper]\n  enabled   =   true   # из настроек\nfill_mode = \"crop\"\n";
    let (after, prev) = noctalia_round_trip(Some(key));
    assert_eq!(
        after,
        "[wallpaper]\n  enabled   =   false   # из настроек\nfill_mode = \"crop\"\n"
    );
    assert_eq!(prev, Previous::Value { raw: "true".into() });

    // Без перевода строки в конце — и заголовок без него.
    noctalia_round_trip(Some("[theme]\nmode = \"dark\""));
    noctalia_round_trip(Some("[wallpaper]"));
    // Пустой файл.
    noctalia_round_trip(Some(""));
    // «[wallpaper]» внутри многострочной строки — не заголовок.
    noctalia_round_trip(Some("note = \"\"\"\n[wallpaper]\nenabled = true\n\"\"\"\n"));
    // Помониторная таблица — не наша.
    noctalia_round_trip(Some("[wallpaper.monitor.DP-1]\nenabled = true\n"));
}

#[test]
fn noctalia_небезопасные_формы_отвергаются_без_порчи() {
    for (текст, ошибка) in [
        ("wallpaper.enabled = true\n", EditError::UnsafeForm),
        ("wallpaper = { enabled = true }\n", EditError::UnsafeForm),
        ("[wallpaper]\nenabled = \"yes\"\n", EditError::UnsafeForm),
        ("wallpaper = 5\n", EditError::UnsafeForm),
        ("[wallpaper\nenabled = true\n", EditError::Unparsable),
    ] {
        assert_eq!(noctalia_disable(Some(текст)).err(), Some(ошибка), "{текст}");
    }
}

#[test]
fn noctalia_возврат_после_того_как_шелл_переписал_файл() {
    // Мы поставили false в пустой таблице; шелл потом переписал файл своим
    // форматом и дописал ключи. Возврат снимает только наш ключ.
    let prev = Previous::KeyAbsent { parent_created: true, prefix: "\n".into() };
    let rewritten = "[theme]\nmode = \"dark\"\n\n[wallpaper]\nenabled = false\n\n[wallpaper.last]\npath = \"/x.png\"\n";
    let Restored::Write(back) = noctalia_restore(Some(rewritten), &prev).unwrap() else {
        panic!("файл не должен удаляться");
    };
    assert_eq!(back, "[theme]\nmode = \"dark\"\n\n\n[wallpaper.last]\npath = \"/x.png\"\n");
    assert_eq!(noctalia_draws_wallpaper(&[], Some(&back)), Some(true));

    // Было значение, а шелл ключ снял — ставим значение обратно.
    let prev = Previous::Value { raw: "true".into() };
    let Restored::Write(back) = noctalia_restore(Some("[theme]\nmode = \"dark\"\n"), &prev).unwrap() else {
        panic!()
    };
    assert_eq!(noctalia_draws_wallpaper(&[], Some(&back)), Some(true));
    assert!(back.starts_with("[theme]\nmode = \"dark\"\n"), "{back}");

    // Файл удалили без нас: «файла не было» так и остаётся.
    assert_eq!(noctalia_restore(None, &Previous::FileAbsent).unwrap(), Restored::Delete);
}

#[test]
fn noctalia_слои_настроек_и_кто_побеждает() {
    let on = "[wallpaper]\nenabled = true\n".to_string();
    let off = "[wallpaper]\nenabled = false\n".to_string();
    assert_eq!(noctalia_draws_wallpaper(&[], None), Some(true), "по умолчанию включены");
    assert_eq!(noctalia_draws_wallpaper(&[off.clone()], None), Some(false));
    // Последний слой конфигурации побеждает предыдущий.
    assert_eq!(noctalia_draws_wallpaper(&[off.clone(), on.clone()], None), Some(true));
    // Файл состояния перебивает ~/.config — ровно поэтому пишем в него.
    assert_eq!(noctalia_draws_wallpaper(&[off.clone()], Some(&on)), Some(true));
    assert_eq!(noctalia_draws_wallpaper(&[on.clone()], Some(&off)), Some(false));
    // Помониторный выключатель общего ответа не меняет: на остальных
    // выходах шелл рисует.
    let per = "[wallpaper.monitor.DP-1]\nenabled = false\n";
    assert_eq!(noctalia_draws_wallpaper(&[], Some(per)), Some(true));
    // Сломанный слой — «не знаю», а не догадка.
    assert_eq!(noctalia_draws_wallpaper(&["[[[".to_string()], None), None);
}

/// Так пишет DMS: `JSON.stringify(…, null, 2)`.
const DMS_PRETTY: &str = "{\n  \"currentThemeName\": \"purple\",\n  \"screenPreferences\": {\n    \"dankBar\": [\n      \"all\"\n    ],\n    \"wallpaper\": [\n      \"all\"\n    ]\n  },\n  \"showOnLastDisplay\": {}\n}\n";

fn dms_round_trip(before: Option<&str>) -> (String, Previous) {
    let (after, prev) = dms_disable(before).expect("правка отвергнута");
    assert_eq!(dms_draws_wallpaper(Some(&after)), Some(false), "после правки:\n{after}");
    match (dms_restore(Some(&after), &prev).expect("возврат отвергнут"), before) {
        (Restored::Write(back), Some(b)) => assert_eq!(back, b, "возврат не бит в бит"),
        (Restored::Delete, None) => {}
        (r, b) => panic!("возврат {r:?} при исходнике {b:?}"),
    }
    (after, prev)
}

#[test]
fn dms_правка_во_всех_формах_файла_и_возврат_бит_в_бит() {
    // Ключ есть — меняется только его значение, многострочное целиком.
    let (after, prev) = dms_round_trip(Some(DMS_PRETTY));
    assert_eq!(after, DMS_PRETTY.replace("\"wallpaper\": [\n      \"all\"\n    ]", "\"wallpaper\": []"));
    assert_eq!(prev, Previous::Value { raw: "[\n      \"all\"\n    ]".into() });

    // Обои только на одном экране — тоже рисует, тоже выключаем и
    // возвращаем ровно этот экран.
    let one = "{\"screenPreferences\":{\"wallpaper\":[\"DP-1\"]}}";
    let (_, prev) = dms_round_trip(Some(one));
    assert_eq!(prev, Previous::Value { raw: "[\"DP-1\"]".into() });

    // Объект есть, ключа нет.
    let (after, prev) = dms_round_trip(Some("{\n  \"screenPreferences\": {\n    \"dankBar\": []\n  }\n}\n"));
    assert_eq!(after, "{\n  \"screenPreferences\": {\n    \"wallpaper\": [],\n    \"dankBar\": []\n  }\n}\n");
    assert_eq!(prev, Previous::KeyAbsent { parent_created: false, prefix: String::new() });

    // Объекта нет вовсе.
    let (_, prev) = dms_round_trip(Some("{\n  \"currentThemeName\": \"purple\"\n}\n"));
    assert_eq!(prev, Previous::KeyAbsent { parent_created: true, prefix: String::new() });
    // Пустые объекты.
    dms_round_trip(Some("{}"));
    dms_round_trip(Some("{\"screenPreferences\": {}}\n"));
    // Строки с фигурными скобками и экранированными кавычками не сбивают
    // разбор границ.
    dms_round_trip(Some("{\"a\": \"}{\\\"]\", \"screenPreferences\": {\"wallpaper\": [\"x}\"]}}"));
    // Файла нет — заводим, возврат удаляет.
    let (_, prev) = dms_round_trip(None);
    assert_eq!(prev, Previous::FileAbsent);
}

#[test]
fn dms_небезопасные_формы_отвергаются_без_порчи() {
    for (текст, ошибка) in [
        ("{\"screenPreferences\": null}", EditError::UnsafeForm),
        ("{\"screenPreferences\": []}", EditError::UnsafeForm),
        ("[1, 2]", EditError::UnsafeForm),
        ("{\"screenPreferences\": {", EditError::Unparsable),
    ] {
        assert_eq!(dms_disable(Some(текст)).err(), Some(ошибка), "{текст}");
    }
}

#[test]
fn dms_кто_рисует() {
    assert_eq!(dms_draws_wallpaper(None), Some(true), "без файла — умолчания, обои есть");
    assert_eq!(dms_draws_wallpaper(Some("{}")), Some(true));
    assert_eq!(dms_draws_wallpaper(Some(DMS_PRETTY)), Some(true));
    assert_eq!(dms_draws_wallpaper(Some("{\"screenPreferences\":{\"wallpaper\":null}}")), Some(true));
    assert_eq!(dms_draws_wallpaper(Some("{\"screenPreferences\":{\"wallpaper\":[\"DP-1\"]}}")), Some(true));
    assert_eq!(dms_draws_wallpaper(Some("{\"screenPreferences\":{\"wallpaper\":[]}}")), Some(false));
    assert_eq!(dms_draws_wallpaper(Some("{")), None);
}

// ── на диске, с приватным HOME ────────────────────────────────────────────

/// Свой временный HOME на сверку (во временном каталоге — ОЗУ; файлы
/// крошечные). Живой профиль не трогается: все корни указывают сюда.
struct PrivateHome {
    root: std::path::PathBuf,
    home: ShellHome,
}

impl PrivateHome {
    fn new(tag: &str) -> PrivateHome {
        let root = std::env::temp_dir().join(format!("rexon-shell-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let home = ShellHome { home: root.to_string_lossy().into_owned(), ..Default::default() };
        PrivateHome { root, home }
    }
}

impl Drop for PrivateHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn put(path: &std::path::Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn на_диске_noctalia_до_после_обратно_бит_в_бит() {
    let _lang = i18n_pin::en();
    let h = PrivateHome::new("noct");
    let state = h.home.noctalia_state_file();
    assert!(state.ends_with(".local/state/noctalia/settings.toml"), "{}", state.display());
    let before = "config_version = 3\n\n[wallpaper.default]\npath = \"/a.png\"\n";
    put(&state, before);
    // Конфигурация в ~/.config включает обои явно — состояние перебьёт.
    put(&h.home.noctalia_config_dir().join("config.toml"), "[wallpaper]\nenabled = true\n");
    std::fs::set_permissions(&state, std::os::unix::fs::PermissionsExt::from_mode(0o600)).unwrap();

    assert_eq!(shell_draws_wallpaper(DesktopShell::Noctalia, &h.home), Some(true));
    let msg = hand_over(DesktopShell::Noctalia, &h.home).expect("не отдали");
    assert!(msg.contains("Noctalia"), "{msg}");
    assert_eq!(shell_draws_wallpaper(DesktopShell::Noctalia, &h.home), Some(false));
    // Резервная копия — прежнее содержимое; права файла сохранены.
    assert_eq!(std::fs::read_to_string(backup_path(&state)).unwrap(), before);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&state).unwrap().permissions().mode() & 0o777, 0o600);
    // ~/.config не тронут.
    assert_eq!(
        std::fs::read_to_string(h.home.noctalia_config_dir().join("config.toml")).unwrap(),
        "[wallpaper]\nenabled = true\n"
    );
    let rec = read_handover(&h.home, DesktopShell::Noctalia).expect("памятки нет");
    assert_eq!(rec.shell, "noctalia");
    // Повторное «Отдать» не портит памятку: прежнее значение остаётся тем,
    // что стояло ДО Rexon.
    hand_over(DesktopShell::Noctalia, &h.home).unwrap();
    assert_eq!(read_handover(&h.home, DesktopShell::Noctalia), Some(rec));

    let msg = give_back(DesktopShell::Noctalia, &h.home).expect("не вернули");
    assert!(msg.contains("back on"), "{msg}");
    assert_eq!(std::fs::read_to_string(&state).unwrap(), before, "возврат не бит в бит");
    assert!(read_handover(&h.home, DesktopShell::Noctalia).is_none(), "памятка осталась после возврата");
    assert_eq!(shell_draws_wallpaper(DesktopShell::Noctalia, &h.home), Some(true));
    // Второй возврат — честное «нечего возвращать».
    assert!(give_back(DesktopShell::Noctalia, &h.home).is_err());
}

#[test]
fn на_диске_noctalia_без_файла_состояния_и_xdg_корни() {
    let _lang = i18n_pin::en();
    let mut h = PrivateHome::new("noct-xdg");
    h.home.xdg_state_home = h.root.join("st").to_string_lossy().into_owned();
    h.home.noctalia_state_home = h.root.join("nst").to_string_lossy().into_owned();
    // NOCTALIA_STATE_HOME главнее XDG_STATE_HOME — как у самого шелла.
    assert_eq!(h.home.noctalia_state_file(), h.root.join("nst/noctalia/settings.toml"));
    // А памятка Rexon — в XDG_STATE_HOME.
    assert_eq!(h.home.handover_file(), h.root.join("st/rexon/shell-wallpaper.json"));
    hand_over(DesktopShell::Noctalia, &h.home).unwrap();
    assert!(h.home.noctalia_state_file().is_file());
    assert!(!backup_path(&h.home.noctalia_state_file()).exists(), "копировать было нечего");
    give_back(DesktopShell::Noctalia, &h.home).unwrap();
    assert!(!h.home.noctalia_state_file().exists(), "заведённый Rexon файл не убран");
}

#[test]
fn на_диске_dms_до_после_обратно_бит_в_бит_и_ссылка_цела() {
    let _lang = i18n_pin::en();
    let h = PrivateHome::new("dms");
    let file = h.home.dms_settings_file();
    assert!(file.ends_with(".config/DankMaterialShell/settings.json"));
    // Настройки лежат в «хранилище» и подключены ссылкой.
    let real = h.root.join("dotfiles/dms.json");
    put(&real, DMS_PRETTY);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real, &file).unwrap();

    hand_over(DesktopShell::DankMaterialShell, &h.home).unwrap();
    assert!(std::fs::symlink_metadata(&file).unwrap().file_type().is_symlink(), "ссылку заменили файлом");
    assert_eq!(dms_draws_wallpaper(Some(&std::fs::read_to_string(&real).unwrap())), Some(false));
    give_back(DesktopShell::DankMaterialShell, &h.home).unwrap();
    assert_eq!(std::fs::read_to_string(&real).unwrap(), DMS_PRETTY, "возврат не бит в бит");
    assert!(std::fs::symlink_metadata(&file).unwrap().file_type().is_symlink());
}

#[test]
fn на_диске_только_для_чтения_отказ_и_ничего_не_тронуто() {
    let _lang = i18n_pin::en();
    let h = PrivateHome::new("ro");
    let file = h.home.dms_settings_file();
    put(&file, DMS_PRETTY);
    std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o444)).unwrap();
    let err = hand_over(DesktopShell::DankMaterialShell, &h.home).expect_err("правка закрытого файла");
    assert!(err.contains("read-only"), "{err}");
    assert!(err.contains("Nothing was changed"), "{err}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), DMS_PRETTY);
    assert!(read_handover(&h.home, DesktopShell::DankMaterialShell).is_none(), "памятка осталась при отказе");
    assert!(!backup_path(&file).exists());
}

#[test]
fn на_диске_неразборный_файл_отказ_и_ничего_не_тронуто() {
    let _lang = i18n_pin::en();
    let h = PrivateHome::new("broken");
    let file = h.home.noctalia_state_file();
    put(&file, "[wallpaper\n");
    let err = hand_over(DesktopShell::Noctalia, &h.home).expect_err("правка сломанного файла");
    assert!(err.contains("Noctalia"), "{err}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "[wallpaper\n");
    assert!(read_handover(&h.home, DesktopShell::Noctalia).is_none());
}

// ── живой сбор и полоса на вкладке ────────────────────────────────────────

#[test]
fn протухшие_служебные_файлы_отсеиваются() {
    let files = s(&[
        "bus",
        "danklinux-100.sock",
        "danklinux-100.pid",
        "danklinux-200.sock",
        "noctalia-wayland-1.sock",
        "noctalia-wayland-1.lock",
    ]);
    let alive = |pid: u32| pid == 100;
    assert_eq!(
        live_runtime_files(&files, alive, false),
        s(&["bus", "danklinux-100.sock", "danklinux-100.pid"])
    );
    assert_eq!(
        live_runtime_files(&files, |_| false, true),
        s(&["bus", "noctalia-wayland-1.sock", "noctalia-wayland-1.lock"])
    );
}

#[test]
fn полоса_только_при_живом_шелле() {
    let _lang = i18n_pin::en();
    let n = DesktopShell::Noctalia;
    // Шелла нет — полосы нет, даже если памятка осталась от прошлого сеанса.
    assert!(!shell_banner(None, WallpaperOwner::Rexon, Some(n)).visible);
    // Фон держит шелл — «Отдать».
    let b = shell_banner(Some(n), WallpaperOwner::Shell(n), None);
    assert!(b.visible && b.can_give && !b.can_return, "{b:?}");
    assert!(b.text.contains("Noctalia") && b.text.contains("Give Wallpaper to Rexon"), "{}", b.text);
    assert_eq!(b.shell, "noctalia");
    // Фон наш, потому что Rexon выключил обои этого шелла, — «Вернуть».
    let b = shell_banner(Some(n), WallpaperOwner::Rexon, Some(n));
    assert!(b.visible && !b.can_give && b.can_return, "{b:?}");
    assert!(b.text.contains("Return to Shell"), "{}", b.text);
    // Человек выключил обои шелла сам — спорить не о чем.
    assert!(!shell_banner(Some(n), WallpaperOwner::Rexon, None).visible);
    // Памятка про ДРУГОЙ шелл к этому не относится.
    assert!(!shell_banner(Some(n), WallpaperOwner::Rexon, Some(DesktopShell::DankMaterialShell)).visible);
}

#[test]
fn состояние_сеанса_по_файлам_до_и_после_передачи() {
    let _lang = i18n_pin::en();
    let h = PrivateHome::new("state");
    let det = |shell| Some(rexon_core::system::ShellDetection { shell, evidence: vec![] });
    let st = shell_wallpaper_state("niri", "wayland", det(DesktopShell::DankMaterialShell), &h.home);
    assert_eq!(st.owner, WallpaperOwner::Shell(DesktopShell::DankMaterialShell));
    assert!(st.banner.can_give);

    hand_over(DesktopShell::DankMaterialShell, &h.home).unwrap();
    let st = shell_wallpaper_state("niri", "wayland", det(DesktopShell::DankMaterialShell), &h.home);
    assert!(st.owner.is_ours(), "после передачи фон всё ещё чужой");
    assert_eq!(st.handed_over, Some(DesktopShell::DankMaterialShell));
    assert!(st.banner.can_return && !st.banner.can_give);

    give_back(DesktopShell::DankMaterialShell, &h.home).unwrap();
    let st = shell_wallpaper_state("niri", "wayland", det(DesktopShell::DankMaterialShell), &h.home);
    assert!(!st.owner.is_ours());
    // На X11 тот же шелл фона не держит — и полосы нет.
    let st = shell_wallpaper_state("niri", "x11", det(DesktopShell::DankMaterialShell), &h.home);
    assert!(st.owner.is_ours() && !st.banner.visible);
}

/// Памятки двух шеллов живут рядом: выключение обоев одного не затирает
/// память о другом, а возврат одного не трогает второй.
#[test]
fn на_диске_два_шелла_памятки_не_мешают_друг_другу() {
    let _lang = i18n_pin::en();
    let h = PrivateHome::new("two");
    let noct_before = "[theme]\nmode = \"dark\"\n";
    put(&h.home.noctalia_state_file(), noct_before);
    put(&h.home.dms_settings_file(), DMS_PRETTY);
    hand_over(DesktopShell::Noctalia, &h.home).unwrap();
    hand_over(DesktopShell::DankMaterialShell, &h.home).unwrap();
    assert!(read_handover(&h.home, DesktopShell::Noctalia).is_some());
    assert!(read_handover(&h.home, DesktopShell::DankMaterialShell).is_some());

    give_back(DesktopShell::DankMaterialShell, &h.home).unwrap();
    assert_eq!(std::fs::read_to_string(h.home.dms_settings_file()).unwrap(), DMS_PRETTY);
    assert!(read_handover(&h.home, DesktopShell::DankMaterialShell).is_none());
    // Noctalia по-прежнему выключена нами, и её памятка цела.
    assert_eq!(shell_draws_wallpaper(DesktopShell::Noctalia, &h.home), Some(false));
    give_back(DesktopShell::Noctalia, &h.home).unwrap();
    assert_eq!(std::fs::read_to_string(h.home.noctalia_state_file()).unwrap(), noct_before);
    assert!(!h.home.handover_file().exists(), "пустой файл памяток не убран");
}
