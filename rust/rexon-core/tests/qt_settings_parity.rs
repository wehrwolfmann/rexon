//! Golden-сверка `qt_settings.rs` — вкладки «Настройки».
//!
//! ЧТО ЗДЕСЬ ЭТАЛОН (с 02.10.2026). Python в Rexon — только архив: он не
//! эталон и не мост. `golden/qt_settings_cases.json` и
//! `golden/qt_settings_ru_before.json` — замороженные СОБСТВЕННЫЕ эталоны
//! Rust; сверки с исходником виджетной вкладки сняты, их числа и цвета
//! записаны здесь (`LAYOUT_FROZEN`, `status_colors_are_frozen`).
//!
//! Откуда снимок. Он снят прогоном прежнего модуля
//! `rexon/qt_settings/window.py` (коммит `94d8d57`) под offscreen-Qt:
//!   * `_detect_wallpaper_format()` — двадцать путей, включая ловушки
//!     («каталог с точкой в имени», файл без расширения, ВЕРХНИЙ регистр);
//!   * звуковые темы — ПРАВИЛО на подставном дереве (`sound_rules`), а не
//!     состав машины: что считается темой, кто в списке исключений, как
//!     склеиваются два каталога, какое расширение побеждает, что бывает на
//!     пустом месте, порядок кнопок «послушать», значки и подписи событий.
//!     Раньше здесь стоял снимок ЖИВОЙ машины: пять установленных тем.
//!     Такая сверка врала в обе стороны — краснела там, где `budgie` и
//!     `mate` не поставлены, и зеленела при сломанном правиле, если нужные
//!     каталоги случайно оказались на месте; пересъём в сентябре 2026
//!     развалил четыре раздела, не поменяв ни строчки кода;
//!   * `_parse_font_str()` — двенадцать записей шрифта (GNOME, KDE, дробный
//!     кегль, мусор вместо кегля) и три таблицы ключей DE;
//!   * `_pretty_desktop_id()`, `_pretty_backup_ts()`, `_files_word()`.
//!
//! Санкционированные расхождения (починки оригинала, правило #16) в голдене
//! НЕ отражены — там записано поведение Python как есть; каждая проверяется
//! отдельным тестом внизу файла:
//!   1. кегль терялся, если строка шрифта была в кавычках или с хвостовым
//!      пробелом (`gsettings get` отдаёт именно кавычки);
//!   2. `_pretty_backup_ts` теперь считает длину месяца — «30 февраля»
//!      остаётся как есть (у Python это делал `strptime`, сверяем, что и у
//!      нас так же);
//!   3. пользовательский каталог звуковых тем берётся от `XDG_DATA_HOME`.
//!
//! ГРАНИЦА СВЕРКИ ПО ЗВУКУ. Оригинал зашил пару каталогов намертво
//! (`/usr/share/sounds` и `~/.local/share/sounds`), поэтому у генератора
//! они подменяются, а ЗДЕСЬ пара собирается [`qs::sound_theme_dirs`] —
//! это и есть починка 3. Что именно кладётся в пару, сверяет
//! `sound_dirs_follow_xdg_data_home`; правила обхода этой пары — сверки
//! ниже; сам факт, что продукт зовёт `sound_theme_dirs` от XDG-каталога
//! пользователя, ни одной сверкой ядра не покрыт.
//!
//! ГЕЙТ «голден обязан ловить регрессию» проверен мутацией: девять точечных
//! поломок реализации (`gif` отнесён к статике; `.tif` потерян из списка
//! расширений; поиск звука без приоритета расширений; showcase без
//! узнаваемых событий вперёд; `pretty_desktop_id` берёт первый сегмент
//! вместо последнего; в `pretty_backup_ts` переставлены день и месяц;
//! «alsa» и прочие не-темы попадают в список тем; звуки KDE уходят по
//! ветке gsettings; склонение «файлов» считается по последней цифре) —
//! каждая красит тесты. Две из них пережили первую редакцию проверок и
//! потребовали доводки: перебор по самому списку расширений оказался
//! тавтологией (теперь фильтр пришпилен строкой), а склонение проверялось
//! только напрямую, минуя строку копии (добавлен случай на 11).
//!
//! ЯЗЫК СВЕРКИ (с 28.09.2026). Строки вкладки перенесены в каталог перевода:
//! в ядре — английские `msgid`, русский — в `po/ru.po`. Эталон Python помнит
//! РУССКИЕ тексты, поэтому сверки с ним идут на русском, собранном из
//! настоящего `po/ru.po` (`i18n_pin::ru()`), — они и есть проверка «по-русски
//! как было». Отдельно: `russian_catalog_keeps_old_texts` сверяет ВЕСЬ каталог
//! вкладки со снимком до переноса (`golden/qt_settings_ru_before.json`), а
//! `english_*` — английский интерфейс (`i18n_pin::en()`).

mod i18n_pin;

use std::collections::BTreeMap;
use std::fs;

use rexon_core::i18n;
use rexon_core::qt_settings as qs;
use serde_json::Value;

/// Русская форма слова по числу: «1 файл», «3 файла», «5 файлов» — свой
/// оракул сверки, независимый от каталога перевода, который она проверяет.
///
/// Правило — русское из CLDR (one / few / many): последняя цифра 1, кроме
/// 11, — первая форма; 2–4, кроме 12–14, — вторая; остальное — третья.
fn plural<'a>(n: i64, one: &'a str, few: &'a str, many: &'a str) -> &'a str {
    let n = n.unsigned_abs();
    let (last, last_two) = (n % 10, n % 100);
    if last == 1 && last_two != 11 {
        one
    } else if (2..=4).contains(&last) && !(12..=14).contains(&last_two) {
        few
    } else {
        many
    }
}

/// ⭐ СТОРОЖ ПУСТОГО ОБРАЗЦА: «сверять было нечего» — это провал, а не успех.
///
/// ⚠️ Это класс, а не один случай. Сверка «в тексте НЕТ запрещённой строки»
/// на пустом входе проходит сама собой: в пустоте нет ничего, в том числе и
/// запрещённого. Полностью пустой файл сторожа ловили и раньше — рядом
/// падали положительные сверки, — а вот ПРОИЗВОДНЫЙ текст (вырезанный
/// кусок, отфильтрованные строки) мог опустеть незаметно. Замер 19.09:
/// исходник страницы из ТРЁХ строк-комментариев, в которых стоят искомые
/// вызовы, проходил сторож плашки ЗЕЛЁНЫМ — положительные сверки читали
/// комментарии, а запреты смотрели в пустоту, оставшуюся после фильтра.
///
/// Поэтому каждый образец — и прочитанный с диска, и производный — обязан
/// сперва доказать, что он непуст, и только потом участвовать в сравнениях.
fn nonempty<'a>(text: &'a str, what: &str) -> &'a str {
    assert!(
        !text.trim().is_empty(),
        "{what}: образец пуст — сверка прошла бы впустую, не проверив ничего"
    );
    text
}

/// Прочитать исходник для текстовой сверки и доказать, что читать было что.
fn read_sample(path: &str, what: &str) -> String {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{what}: не удалось прочитать {path}: {e}"));
    nonempty(&text, what);
    text
}

fn golden() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/qt_settings_cases.json"
    );
    let text = read_sample(path, "эталон qt_settings_cases.json");
    serde_json::from_str(&text).expect("эталон не разбирается как JSON")
}

/// Подставное дерево звуковых тем из эталона: строится во временном
/// каталоге ровно по списку `sound_rules.tree`, который записал генератор,
/// и возвращает пару каталогов «системный, пользовательский» — в том же
/// порядке, в каком их перебирал оригинал.
///
/// ⚠️ Каталог уникален по имени теста: сверки внутри одного двоичного файла
/// libtest гоняет ПАРАЛЛЕЛЬНО, и общее дерево они бы вычищали друг у друга.
struct SoundTree {
    root: std::path::PathBuf,
}

impl SoundTree {
    fn new(tag: &str, entries: &[&str]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "rexon-snd-{}-{tag}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        for rel in entries {
            let full = root.join(rel);
            if rel.ends_with('/') {
                fs::create_dir_all(&full).unwrap();
            } else {
                fs::create_dir_all(full.parent().unwrap()).unwrap();
                fs::write(&full, b"").unwrap();
            }
        }
        // Оба каталога обязаны существовать даже пустыми: у оригинала они
        // тоже всегда есть, просто бывают без содержимого.
        fs::create_dir_all(root.join("sys")).unwrap();
        fs::create_dir_all(root.join("user")).unwrap();
        Self { root }
    }

    /// Дерево из эталона.
    fn from_golden(tag: &str, g: &Value) -> Self {
        let entries: Vec<&str> = g["sound_rules"]["tree"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        Self::new(tag, &entries)
    }

    /// Пара каталогов в порядке оригинала: сперва системный, затем свой.
    fn dirs(&self) -> Vec<String> {
        vec![
            self.root.join("sys").to_string_lossy().into_owned(),
            self.root.join("user").to_string_lossy().into_owned(),
        ]
    }

    /// Путь внутри дерева — таким его записал генератор.
    fn rel(&self, path: &str) -> String {
        path.strip_prefix(&format!("{}/", self.root.display()))
            .unwrap_or(path)
            .to_string()
    }
}

impl Drop for SoundTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn str_vec(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn wallpaper_format_matches_python() {
    for case in golden()["wallpaper_format"].as_array().unwrap() {
        let path = case[0].as_str().unwrap();
        let want = case[1].as_str().unwrap();
        assert_eq!(
            qs::detect_wallpaper_format(path),
            want,
            "формат обоев для {path:?}"
        );
    }
}

/// Список тем: правило «что считается темой», а не состав машины.
///
/// ЧТО ЛОМАЛОСЬ. Прежняя редакция звала `qs::list_sound_themes` на ЖИВЫХ
/// `/usr/share/sounds` и `$HOME/.local/share/sounds` — список тем был
/// свойством машины, а не кода. Затем сверка шла по эталону Python, где темой
/// считался и каталог с одними звуками внутри; Python с 02.10.2026 — архив, а
/// правило оказалось неверным: наборы GNOME/MATE/Budgie без `index.theme` и
/// обёртки архивов попадали в список, хотя KDE и libcanberra их темами не
/// считают и подменяют выбранное имя на `freedesktop`.
///
/// КАК ТЕПЕРЬ (03.10.2026). Дерево строится здесь же. Тема — каталог
/// ВЕРХНЕГО уровня с `index.theme` и разделом `[Sound Theme]`; `alsa`,
/// `speech-dispatcher` и `sox` не темы даже с ним; каталоги склеиваются без
/// повторов; порядок — по возрастанию; от имён правило не зависит.
#[test]
fn sound_themes_are_only_real_themes() {
    const THEME: &str = "[Sound Theme]\nName=T\nDirectories=stereo\n[stereo]\nOutputProfile=stereo\n";
    for own in ["oxygen", "чужая-тема"] {
        let root = std::env::temp_dir().join(format!("rexon-snd-{}-real-{own}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let put = |rel: &str, text: &str| {
            let p = root.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, text).unwrap();
        };
        put(&format!("sys/{own}/index.theme"), THEME);
        put(&format!("sys/{own}/stereo/bell.oga"), "");
        put("sys/alsa/index.theme", THEME);
        put("sys/gnome/default/alerts/bark.ogg", "");
        put("sys/mate/default/alerts/drip.ogg", "");
        put("sys/budgie/default/alerts/x.oga", "");
        put("sys/значки/index.theme", "[Icon Theme]\nName=I\n");
        put("sys/не-каталог.oga", "");
        fs::create_dir_all(root.join("sys/пустая")).unwrap();
        put("user/Ocean/index.theme", THEME);
        put(&format!("user/{own}/index.theme"), THEME);
        put("user/zorinos2/sounds/Zorin/index.theme", THEME);
        put("user/тема с пробелом/index.theme", THEME);
        let dirs = vec![
            root.join("sys").to_string_lossy().into_owned(),
            root.join("user").to_string_lossy().into_owned(),
        ];
        let mut want = vec!["Ocean".to_string(), own.to_string(), "тема с пробелом".to_string()];
        want.sort();
        assert_eq!(qs::list_sound_themes(&dirs), want, "темы при имени {own}");
        let _ = fs::remove_dir_all(&root);
    }
}

/// Пустое место: обе стороны подставляют тему по умолчанию, а не отдают
/// покупателю выпадающий список без единого пункта.
#[test]
fn sound_themes_on_empty_dirs_fall_back() {
    let g = golden();
    let tree = SoundTree::new("empty", &[]);
    assert_eq!(
        qs::list_sound_themes(&tree.dirs()),
        str_vec(&g["sound_rules"]["themes_when_empty"]),
        "пустые каталоги"
    );
    assert_eq!(
        qs::list_sound_themes(&[]),
        vec![qs::DEFAULT_SOUND_THEME.to_string()],
        "каталогов нет вовсе"
    );
}

#[test]
fn theme_events_match_python() {
    let g = golden();
    let tree = SoundTree::from_golden("events", &g);
    let dirs = tree.dirs();
    for (theme, events) in g["sound_rules"]["theme_events"].as_object().unwrap() {
        assert_eq!(
            qs::list_theme_events(&dirs, theme),
            str_vec(events),
            "события темы {theme}"
        );
    }
}

/// Поиск файла события: приоритет расширений (`.oga` → `.ogg` → `.wav`)
/// сильнее глубины вложения, а порядок каталогов сильнее расширения —
/// системный отвечает первым, даже когда у пользователя файл «лучше».
#[test]
fn find_sound_file_matches_python() {
    let g = golden();
    let tree = SoundTree::from_golden("find", &g);
    let dirs = tree.dirs();
    for (theme, probes) in g["sound_rules"]["find_file"].as_object().unwrap() {
        for (event, expected) in probes.as_object().unwrap() {
            let got = qs::find_sound_file(&dirs, theme, event).map(|p| tree.rel(&p));
            assert_eq!(
                got.as_deref(),
                expected.as_str(),
                "файл события {event} в теме {theme}"
            );
        }
    }
}

#[test]
fn showcase_matches_python() {
    let g = golden();
    for (theme, want) in g["sound_rules"]["showcase"].as_object().unwrap() {
        let events = str_vec(&g["sound_rules"]["theme_events"][theme]);
        assert_eq!(
            qs::showcase_events(&events, 5),
            str_vec(want),
            "кнопки «послушать» темы {theme}"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn event_meta_matches_python() {
    let _lang = i18n_pin::ru();
    for case in golden()["sound_event_meta"].as_array().unwrap() {
        let event = case[0].as_str().unwrap();
        let want = (
            case[1][0].as_str().unwrap().to_string(),
            case[1][1].as_str().unwrap().to_string(),
        );
        assert_eq!(qs::event_meta(event), want, "подпись события {event:?}");
    }
}

/// Кегль `0` в ядре значит «в строке его не было» — Qt подставляет свой
/// дефолт. Сверяем только семейство и кегль там, где он в строке ЕСТЬ.
#[test]
fn parse_font_matches_python() {
    // Две записи вынесены в починку `font_spec_keeps_size_with_quotes`.
    let fixed = ["'Noto Sans 11'", "Ubuntu 11 "];
    for case in golden()["parse_font"].as_array().unwrap() {
        let raw = case[0].as_str().unwrap();
        if fixed.contains(&raw) {
            continue;
        }
        let want_family = case[1][0].as_str().unwrap();
        let want_size = case[1][1].as_i64().unwrap() as i32;
        let (family, size) = qs::parse_font_spec(raw);
        if raw.is_empty() {
            assert_eq!(family, "", "пустая строка шрифта");
            assert_eq!(size, 0);
            continue;
        }
        assert_eq!(family, want_family, "семейство из {raw:?}");
        if size != 0 {
            assert_eq!(size, want_size, "кегль из {raw:?}");
        }
    }
}

#[test]
fn font_backend_matches_python() {
    let g = golden();
    for (target, keys) in g["font_keys"].as_object().unwrap() {
        match qs::font_backend("gnome", target) {
            qs::SoundBackend::GSettings { schema, key } => {
                assert_eq!(schema, keys["gnome"][0].as_str().unwrap(), "{target}");
                assert_eq!(key, keys["gnome"][1].as_str().unwrap(), "{target}");
            }
            other => panic!("для GNOME ожидался gsettings, а не {other:?}"),
        }
        match qs::font_backend("kde", target) {
            qs::SoundBackend::KConfig { group, key, .. } => {
                assert_eq!(group, keys["kde"][0].as_str().unwrap(), "{target}");
                assert_eq!(key, keys["kde"][1].as_str().unwrap(), "{target}");
            }
            other => panic!("для KDE ожидался kdeglobals, а не {other:?}"),
        }
        match qs::font_backend("xfce", target) {
            qs::SoundBackend::Xfconf { channel, prop } => {
                assert_eq!(channel, keys["xfce"][0].as_str().unwrap(), "{target}");
                assert_eq!(prop, keys["xfce"][1].as_str().unwrap(), "{target}");
            }
            other => panic!("для XFCE ожидался xfconf, а не {other:?}"),
        }
    }
}

#[test]
fn pretty_desktop_id_matches_python() {
    for case in golden()["pretty_desktop_id"].as_array().unwrap() {
        let raw = case[0].as_str().unwrap();
        assert_eq!(
            qs::pretty_desktop_id(raw),
            case[1].as_str().unwrap(),
            "имя ярлыка {raw:?}"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn pretty_backup_ts_matches_python() {
    let _lang = i18n_pin::ru();
    for case in golden()["pretty_backup_ts"].as_array().unwrap() {
        let raw = case[0].as_str().unwrap();
        assert_eq!(
            qs::pretty_backup_ts(raw),
            case[1].as_str().unwrap(),
            "метка копии {raw:?}"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn files_word_matches_python() {
    let _lang = i18n_pin::ru();
    // Счёт файлов в строке копии теперь склоняет каталог перевода
    // (`Plural-Forms` в `po/ru.po`), а не своя таблица: сверяем, что его
    // формы дают те же слова, что и `_files_word` оригинала.
    for case in golden()["files_word"].as_array().unwrap() {
        let n = case[0].as_i64().unwrap();
        let want = case[1].as_str().unwrap();
        assert_eq!(plural(n, "файл", "файла", "файлов"), want, "склонение для {n}");
        assert_eq!(
            i18n::ntr("%n file", "%n files", n as u64),
            format!("{n} {want}"),
            "форма каталога для {n}"
        );
    }
}

// ── Починки оригинала (в голдене не отражены) ────────────────────────────

/// Починка 1: `gsettings get` отдаёт значение В КАВЫЧКАХ, а хвостовой пробел
/// оставляет любой ручной ввод. Оригинал в обоих случаях терял кегль и
/// возвращал шрифт «по умолчанию».
#[test]
fn font_spec_keeps_size_with_quotes() {
    assert_eq!(
        qs::parse_font_spec("'Noto Sans 11'"),
        ("Noto Sans".to_string(), 11)
    );
    assert_eq!(qs::parse_font_spec("Ubuntu 11 "), ("Ubuntu".to_string(), 11));
}

/// Починка 2: длина месяца. `strptime` её знал, ручной разбор мог бы не
/// знать — сверяем, что «30 февраля» осталось нерасшифрованным.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn pretty_backup_ts_checks_month_length() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::pretty_backup_ts("20260230_010203"), "20260230_010203");
    assert_eq!(qs::pretty_backup_ts("20260229_010203"), "20260229_010203");
    assert_eq!(qs::pretty_backup_ts("20240229_010203"), "29.02.2024 в 01:02");
}

/// Починка 3: пользовательский каталог тем — от `XDG_DATA_HOME`, а не от
/// `~/.local/share` наглухо.
#[test]
fn sound_dirs_follow_xdg_data_home() {
    assert_eq!(
        qs::sound_theme_dirs("/custom/data"),
        vec![
            "/usr/share/sounds".to_string(),
            "/custom/data/sounds".to_string()
        ]
    );
}

// ── Логика, у которой в Python не было отдельной функции ─────────────────

#[test]
fn sound_backend_covers_every_de() {
    // Тайловые и незнакомые сеансы обязаны попадать на общий gsettings-путь,
    // а не оставаться без звука (правило #12).
    for de in ["hyprland", "niri", "sway", "unknown", "", "labwc"] {
        assert!(matches!(
            qs::sound_backend(de, qs::SoundSetting::Theme),
            qs::SoundBackend::GSettings { .. }
        ));
    }
    assert_eq!(
        qs::sound_backend("kde", qs::SoundSetting::EventSounds),
        qs::SoundBackend::KConfig {
            file: "kdeglobals".into(),
            group: "Sounds".into(),
            // KF6 Notifications читает `[Sounds] Enable` (notifybyaudio.cpp);
            // прежний `EnableNotificationSounds` не читал никто.
            key: "Enable".into(),
        }
    );
    // На чистом профиле ключа нет, а KF6 считает его включённым — читаем
    // с тем же умолчанием.
    assert_eq!(
        qs::sound_read_argv_for("kde", qs::SoundSetting::EventSounds, "/usr/bin/kreadconfig6")
            .unwrap()
            .last()
            .unwrap(),
        "true"
    );
    assert_eq!(
        qs::sound_read_argv_for("kde", qs::SoundSetting::Theme, "/usr/bin/kreadconfig6")
            .unwrap()
            .last()
            .unwrap(),
        ""
    );
}

#[test]
fn sound_argv_needs_kde_tool() {
    let kde = qs::sound_backend("kde", qs::SoundSetting::Theme);
    assert!(qs::sound_read_argv(&kde, "").is_none());
    assert!(qs::sound_write_argv(&kde, "ocean", "").is_none());
    assert_eq!(
        qs::sound_write_argv(&kde, "ocean", "/usr/bin/kwriteconfig6").unwrap(),
        vec![
            "/usr/bin/kwriteconfig6",
            "--file",
            "kdeglobals",
            "--group",
            "Sounds",
            "--key",
            "Theme",
            "--notify",
            "ocean"
        ]
    );
    let gnome = qs::sound_backend("gnome", qs::SoundSetting::Theme);
    assert_eq!(
        qs::sound_read_argv(&gnome, "").unwrap(),
        vec!["gsettings", "get", "org.gnome.desktop.sound", "theme-name"]
    );
}

/// План смены звуковой темы: основной механизм окружения плюс общая ветка
/// gsettings для GTK-программ, и обязательная команда проверки.
#[test]
fn sound_theme_plan_writes_both_and_verifies() {
    // KDE: kdeglobals + gsettings (GTK-программы читают libcanberra).
    let plan = qs::sound_theme_plan(
        "kde", "zorinos2", "/usr/bin/kwriteconfig6", "/usr/bin/kreadconfig6", true);
    assert_eq!(plan.write.len(), 2);
    assert_eq!(plan.write[0][0], "/usr/bin/kwriteconfig6");
    assert_eq!(plan.write[0].last().unwrap(), "zorinos2");
    assert_eq!(
        plan.write[1],
        vec!["gsettings", "set", "org.gnome.desktop.sound", "theme-name", "zorinos2"]
    );
    assert_eq!(plan.verify.unwrap()[0], "/usr/bin/kreadconfig6");

    // Нет gsettings — вспомогательной команды не предлагаем.
    let plan = qs::sound_theme_plan(
        "kde", "zorinos2", "/usr/bin/kwriteconfig6", "/usr/bin/kreadconfig6", false);
    assert_eq!(plan.write.len(), 1);

    // GNOME: основной механизм и есть общая ветка — дубля быть не должно.
    let plan = qs::sound_theme_plan("gnome", "ocean", "", "", true);
    assert_eq!(plan.write.len(), 1);
    assert_eq!(plan.write[0][2], "org.gnome.desktop.sound");

    // KDE без kwriteconfig6: писать основным механизмом нечем, но общая
    // ветка остаётся — единственный доступный путь, без падения.
    let plan = qs::sound_theme_plan("kde", "ocean", "", "", true);
    assert_eq!(plan.write.len(), 1);
    assert_eq!(plan.write[0][0], "gsettings");
    assert!(plan.verify.is_none());

    // Не-KDE окружение с kwriteconfig6: тема доходит и до KDE/Qt-программ
    // (kdeglobals [Sounds] Theme, с уведомлением запущенных).
    let plan = qs::sound_theme_plan(
        "cinnamon-mint", "ocean", "/usr/bin/kwriteconfig6", "/usr/bin/kreadconfig6", true);
    assert_eq!(
        plan.write,
        vec![
            svec(&["gsettings", "set", "org.cinnamon.desktop.sound", "theme-name", "ocean"]),
            svec(&["gsettings", "set", "org.gnome.desktop.sound", "theme-name", "ocean"]),
            svec(&[
                "/usr/bin/kwriteconfig6", "--file", "kdeglobals", "--group", "Sounds",
                "--key", "Theme", "--notify", "ocean",
            ]),
        ]
    );
    assert_eq!(
        plan.verify.unwrap(),
        svec(&["gsettings", "get", "org.cinnamon.desktop.sound", "theme-name"])
    );
}

fn svec(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// «Звуки событий» — тот же общий план: родной ключ, общий слой GNOME и
/// KDE, в KDE ещё снятие мусорного ключа прежних версий.
#[test]
fn event_sounds_plan_per_desktop() {
    let kw = "/usr/bin/kwriteconfig6";
    let kr = "/usr/bin/kreadconfig6";
    let kde_enable = |v: &str| {
        svec(&[kw, "--file", "kdeglobals", "--group", "Sounds", "--key", "Enable", "--notify", v])
    };
    let drop_stale = svec(&[
        kw, "--file", "kdeglobals", "--group", "Sounds", "--key", "EnableNotificationSounds",
        "--delete",
    ]);
    let gnome = |v: &str| svec(&["gsettings", "set", "org.gnome.desktop.sound", "event-sounds", v]);

    let plan = qs::sound_plan("kde", qs::SoundSetting::EventSounds, "false", kw, kr, true);
    assert_eq!(plan.write, vec![kde_enable("false"), gnome("false"), drop_stale.clone()]);
    assert_eq!(plan.verify.unwrap()[0], kr);

    let plan = qs::sound_plan("gnome", qs::SoundSetting::EventSounds, "true", kw, kr, true);
    assert_eq!(plan.write, vec![gnome("true"), kde_enable("true"), drop_stale.clone()]);

    let plan = qs::sound_plan("mate", qs::SoundSetting::EventSounds, "false", "", "", true);
    assert_eq!(
        plan.write,
        vec![
            svec(&["gsettings", "set", "org.mate.sound", "event-sounds", "false"]),
            gnome("false"),
        ]
    );

    let plan = qs::sound_plan("xfce", qs::SoundSetting::EventSounds, "true", "", "", false);
    assert_eq!(
        plan.write,
        vec![svec(&[
            "xfconf-query", "-c", "xsettings", "-p", "/Net/EnableEventSounds", "-n", "-t", "bool", "-s",
            "true",
        ])]
    );
}

/// Д6 (проверка окружений 02.10.2026): шрифт заголовков XFCE на профиле, где
/// его не меняли, — свойства `/general/title_font` в канале нет, и запись без
/// `-n -t string` отказывала. Запись обязана уметь завести свойство.
#[test]
fn xfce_font_write_creates_missing_property() {
    for (target, channel, prop) in [
        ("title", "xfwm4", "/general/title_font"),
        ("system", "xsettings", "/Gtk/FontName"),
        ("mono", "xsettings", "/Gtk/MonospaceFontName"),
    ] {
        assert_eq!(
            qs::font_write_argv("xfce", target, "Noto Sans", 12, "").unwrap(),
            svec(&["xfconf-query", "-c", channel, "-p", prop, "-n", "-t", "string", "-s", "Noto Sans 12"]),
            "{target}"
        );
    }
    let plan = qs::sound_plan("xfce", qs::SoundSetting::Theme, "ocean", "", "", false);
    assert_eq!(
        plan.write[0],
        svec(&["xfconf-query", "-c", "xsettings", "-p", "/Net/SoundThemeName", "-n", "-t", "string", "-s", "ocean"])
    );
}

/// Незаданная тема доходит до подписи «— не задана —», заданная — как есть.
///
/// ЧТО ЛОМАЛОСЬ (живая проверка вкладки 18.09). Пустой ответ подменялся на
/// `freedesktop`, и на чистом профиле строка «Звуковая тема» показывала имя
/// темы, которой человек не выбирал; строка-заглушка была недостижима.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn unset_sound_theme_reaches_the_label() {
    let _lang = i18n_pin::ru();
    // Нечего показывать — так и сказано пустой строкой: строку-заглушку
    // страница ставит ровно по этому признаку.
    for raw in ["", "  \n", "''\n", "\"\"\n"] {
        assert_eq!(qs::parse_sound_theme(raw), "", "ответ {raw:?}");
    }
    // Подпись, которую страница поставит вместо пустого места, — в ядре.
    assert_eq!(
        qs::text("snd.theme_unset", &[]).as_deref(),
        Some("— не задана —")
    );
    // Тема задана — показывается она сама, без всякой подстановки.
    assert_eq!(qs::parse_sound_theme("'ocean'\n"), "ocean");
    assert_eq!(qs::parse_sound_theme("zorinos2\n"), "zorinos2");
}

/// Звуку умолчание по-прежнему нужно — но берётся оно ВТОРЫМ путём, у
/// поиска файла, а не у подписи: пустое имя темы иначе означало бы каталог
/// тем целиком, и «послушать» играло бы звук чужой темы.
#[test]
fn unset_theme_plays_freedesktop_and_only_it() {
    let tree = SoundTree::new(
        "effective",
        &[
            "sys/freedesktop/stereo/bell.oga",
            "sys/ocean/stereo/complete.oga",
        ],
    );
    let dirs = tree.dirs();
    assert_eq!(qs::effective_sound_theme(""), "freedesktop");
    assert_eq!(qs::effective_sound_theme("  "), "freedesktop");
    assert_eq!(qs::effective_sound_theme("ocean"), "ocean");
    assert_eq!(
        qs::list_theme_events(&dirs, ""),
        vec!["bell".to_string()],
        "события незаданной темы = события freedesktop"
    );
    assert_eq!(
        qs::find_sound_file(&dirs, "", "bell").map(|p| tree.rel(&p)),
        Some("sys/freedesktop/stereo/bell.oga".to_string())
    );
    assert!(
        qs::find_sound_file(&dirs, "", "complete").is_none(),
        "звук чужой темы при незаданной своей брать не вправе"
    );
}

#[test]
fn parse_sound_values() {
    assert_eq!(qs::parse_sound_theme("'ocean'\n"), "ocean");
    assert_eq!(qs::parse_sound_theme("  \n"), "");
    assert!(qs::parse_event_sounds("true\n"));
    assert!(qs::parse_event_sounds("'True'"));
    assert!(qs::parse_event_sounds("1"));
    assert!(!qs::parse_event_sounds("false"));
    assert!(!qs::parse_event_sounds(""));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn compliance_summary_words() {
    let _lang = i18n_pin::ru();
    let case = |pairs: &[(&str, &str)]| {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        qs::compliance_summary(&owned)
    };
    assert_eq!(case(&[]), "Для этого рабочего стола проверок пока нет");
    assert_eq!(
        case(&[("a", "ok"), ("b", "ok")]),
        "✅ Всё на месте (проверено пунктов: 2)"
    );
    assert_eq!(case(&[("a", "fixed")]), "✅ Доустановлено: 1");
    assert_eq!(
        case(&[("a", "fixed"), ("b", "failed"), ("c", "needs_action")]),
        "⚠️ Доустановлено: 1, не удалось: 1, осталось доустановить: 1"
    );
    assert_eq!(
        case(&[("a", "skipped")]),
        "✅ Проверка пакетов доступна только на Arch/CachyOS"
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn backup_row_label_reads_like_a_sentence() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        qs::backup_row_label("20260620_011005", "manual", 3),
        "📁 20.06.2026 в 01:10  ·  вручную  ·  3 файла"
    );
    assert_eq!(
        qs::backup_row_label("20260620_011005", "before_install", 1),
        "📁 20.06.2026 в 01:10  ·  авто, перед установкой  ·  1 файл"
    );
    // 11 — та самая ловушка русского счёта: «11 файлов», не «11 файл».
    assert_eq!(
        qs::backup_row_label("20260620_011005", "manual", 11),
        "📁 20.06.2026 в 01:10  ·  вручную  ·  11 файлов"
    );
    assert_eq!(qs::backup_reason_label("своё"), "своё");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn de_display_never_shows_unknown() {
    let _lang = i18n_pin::ru();
    // Имя стола — как на вкладке «Рабочие столы», а не сокращение.
    assert_eq!(qs::de_display("kde", "KDE", ""), "KDE Plasma");
    assert_eq!(qs::de_display("unknown", "niri:wlroots", ""), "NIRI");
    assert_eq!(qs::de_display("unknown", "", "sway"), "SWAY");
    // Починка (правило #13): запасное имя было «ВАШ WM» — сокращение из
    // словаря разработчика, покупателю оно ничего не говорит.
    assert_eq!(qs::de_display("unknown", "", ""), "ВАШ РАБОЧИЙ СТОЛ");
    assert_eq!(qs::de_display("", "", ""), "ВАШ РАБОЧИЙ СТОЛ");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn youtube_lists_are_paired() {
    let _lang = i18n_pin::ru();
    let by_label: BTreeMap<String, &str> = qs::yt_qualities().into_iter().collect();
    assert_eq!(by_label["4K"], "4k");
    assert_eq!(by_label["Лучшее"], "best");
    // Починка (правило #13): было «Стриминг» — калька, а не русское слово.
    assert_eq!(qs::yt_modes()[0], ("Онлайн".to_string(), "stream"));
    assert_eq!(qs::yt_modes()[1], ("Скачать".to_string(), "download"));
}

#[test]
fn font_gtk_sync_only_for_system() {
    assert!(qs::font_syncs_to_gtk("system"));
    assert!(!qs::font_syncs_to_gtk("mono"));
    assert!(!qs::font_syncs_to_gtk("title"));
    assert_eq!(qs::gnome_font_spec("Noto Sans", 11), "Noto Sans 11");
    // ⚠️ Расхождение с Python-оригиналом намеренное: его запись
    // `Noto Sans,11,-1,5,Regular,0,0,0,0,0` Qt 6 читает с весом 100 (Thin) —
    // десять полей разбираются по старой шкале, «Regular» → 0. Запись Qt 6
    // из шестнадцати полей даёт обычный вес 400, как у самой Plasma 6.
    assert_eq!(
        qs::kde_font_spec("Noto Sans", 11),
        "Noto Sans,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"
    );
    // А разбор понимает обе записи — старые конфиги читаются как прежде.
    assert_eq!(
        qs::parse_font_spec("Noto Sans,11,-1,5,Regular,0,0,0,0,0"),
        ("Noto Sans".to_string(), 11)
    );
    assert_eq!(
        qs::parse_font_spec(&qs::kde_font_spec("Noto Sans", 11)),
        ("Noto Sans".to_string(), 11)
    );
}

/// Что читается и пишется в каждом окружении: родное хранилище, спутники,
/// команды-спутники. Источники ключей — в документации `font_backend` и
/// `font_plan` (схемы gsettings, исходники Cinnamon, MATE, Budgie, COSMIC,
/// LXQt, qt6ct).
#[test]
fn font_plan_table_per_desktop() {
    use qs::{FontFile, FontStore, SoundBackend};
    let gs = |schema: &str, key: &str| {
        FontStore::Cmd(SoundBackend::GSettings {
            schema: schema.to_string(),
            key: key.to_string(),
        })
    };
    let kc = |group: &str, key: &str| {
        FontStore::Cmd(SoundBackend::KConfig {
            file: "kdeglobals".to_string(),
            group: group.to_string(),
            key: key.to_string(),
        })
    };
    let xf = |channel: &str, prop: &str| {
        FontStore::Cmd(SoundBackend::Xfconf {
            channel: channel.to_string(),
            prop: prop.to_string(),
        })
    };
    let cosmic = |key: &str| FontStore::File(FontFile::CosmicTk { key: key.to_string() });
    let lxqt = |key: &str| {
        FontStore::File(FontFile::QtIni {
            path: "lxqt/lxqt.conf".to_string(),
            group: "Qt".to_string(),
            key: key.to_string(),
            create: true,
        })
    };
    let qt6ct = |key: &str| {
        FontStore::File(FontFile::QtIni {
            path: "qt6ct/qt6ct.conf".to_string(),
            group: "Fonts".to_string(),
            key: key.to_string(),
            create: false,
        })
    };
    const IFACE: &str = "org.gnome.desktop.interface";
    let g_sys = || gs(IFACE, "font-name");
    let g_mono = || gs(IFACE, "monospace-font-name");
    let g_title = || gs("org.gnome.desktop.wm.preferences", "titlebar-font");

    // (окружение, цель, родное хранилище, спутники)
    let table: Vec<(&str, &str, FontStore, Vec<FontStore>)> = vec![
        ("kde", "system", kc("General", "font"), vec![g_sys()]),
        ("kde", "mono", kc("General", "fixed"), vec![g_mono()]),
        ("kde", "title", kc("WM", "activeFont"), vec![]),
        ("xfce", "system", xf("xsettings", "/Gtk/FontName"), vec![g_sys(), qt6ct("general")]),
        ("xfce", "mono", xf("xsettings", "/Gtk/MonospaceFontName"), vec![g_mono(), qt6ct("fixed")]),
        ("xfce", "title", xf("xfwm4", "/general/title_font"), vec![]),
        ("gnome", "system", g_sys(), vec![qt6ct("general")]),
        ("gnome", "mono", g_mono(), vec![qt6ct("fixed")]),
        ("gnome", "title", g_title(), vec![]),
        ("budgie", "system", g_sys(), vec![qt6ct("general")]),
        ("budgie", "mono", g_mono(), vec![qt6ct("fixed")]),
        ("budgie", "title", g_title(), vec![]),
        (
            "cinnamon-mint",
            "system",
            gs("org.cinnamon.desktop.interface", "font-name"),
            vec![g_sys(), qt6ct("general")],
        ),
        ("cinnamon-mint", "mono", g_mono(), vec![qt6ct("fixed")]),
        (
            "cinnamon-mint",
            "title",
            gs("org.cinnamon.desktop.wm.preferences", "titlebar-font"),
            vec![],
        ),
        ("mate", "system", gs("org.mate.interface", "font-name"), vec![g_sys(), qt6ct("general")]),
        (
            "mate",
            "mono",
            gs("org.mate.interface", "monospace-font-name"),
            vec![g_mono(), qt6ct("fixed")],
        ),
        ("mate", "title", gs("org.mate.Marco.general", "titlebar-font"), vec![]),
        ("cosmic", "system", cosmic("interface_font"), vec![qt6ct("general")]),
        ("cosmic", "mono", cosmic("monospace_font"), vec![qt6ct("fixed")]),
        ("lxqt", "system", lxqt("font"), vec![g_sys()]),
        ("lxqt", "mono", lxqt("fixedFont"), vec![g_mono()]),
        ("sway", "system", g_sys(), vec![qt6ct("general")]),
        ("hyprland", "mono", g_mono(), vec![qt6ct("fixed")]),
        ("niri", "system", g_sys(), vec![qt6ct("general")]),
        ("", "system", g_sys(), vec![qt6ct("general")]),
    ];
    for (de, target, primary, also) in table {
        let plan = qs::font_plan(de, target);
        assert_eq!(plan.primary, primary, "{de}/{target}: родное хранилище");
        assert_eq!(plan.also, also, "{de}/{target}: спутники");
        assert_eq!(plan.gtk_ini, target == "system", "{de}/{target}: settings.ini");
        // Родное хранилище-команда совпадает с тем, что отдаёт font_backend
        // (им до сих пор пользуется мост Python).
        if let FontStore::Cmd(backend) = &plan.primary {
            assert_eq!(*backend, qs::font_backend(de, target), "{de}/{target}");
        }
    }

    // Заголовок: вместе с titlebar-font снимается titlebar-uses-system-font
    // той же схемы — иначе ключ шрифта игнорируется.
    for (de, schema) in [
        ("gnome", "org.gnome.desktop.wm.preferences"),
        ("budgie", "org.gnome.desktop.wm.preferences"),
        ("cinnamon-mint", "org.cinnamon.desktop.wm.preferences"),
        ("mate", "org.mate.Marco.general"),
    ] {
        assert_eq!(
            qs::font_plan(de, "title").extra,
            vec![vec![
                "gsettings".to_string(),
                "set".to_string(),
                schema.to_string(),
                "titlebar-uses-system-font".to_string(),
                "false".to_string(),
            ]],
            "{de}"
        );
    }
    for de in ["kde", "xfce"] {
        assert!(qs::font_plan(de, "title").extra.is_empty(), "{de}");
    }
    for de in ["gnome", "cinnamon-mint", "mate", "cosmic", "lxqt", "kde", "xfce"] {
        for target in ["system", "mono"] {
            assert!(qs::font_plan(de, target).extra.is_empty(), "{de}/{target}");
        }
    }
}

/// Шрифт заголовков предлагается только там, где его есть куда записать;
/// «маленький», «панели инструментов» и «меню» — везде (вне KDE их несёт
/// общий слой Rexon).
#[test]
fn font_title_offered_only_where_a_wm_reads_it() {
    for de in ["kde", "xfce", "gnome", "budgie", "cinnamon-mint", "mate"] {
        assert_eq!(
            qs::font_targets_for(de),
            vec!["system", "mono", "small", "toolbar", "menu", "title"],
            "{de}"
        );
    }
    for de in ["cosmic", "lxqt", "sway", "hyprland", "niri", "deepin", ""] {
        assert_eq!(qs::font_targets_for(de), vec!["system", "mono", "small", "toolbar", "menu"], "{de}");
    }
}

/// «Маленький», «панели инструментов», «меню»: в KDE — родные ключи
/// kdeglobals, во всех остальных окружениях — общий слой Rexon: наш блок в
/// gtk.css GTK 3 (он же родной — из него шрифт читается обратно) и GTK 4,
/// таблица стилей qt6ct/qt5ct (кроме LXQt, чья платформенная тема таблиц не
/// читает) и kdeglobals для KDE-программ. Кнопки включены везде.
#[test]
fn extra_fonts_reach_every_desktop() {
    use qs::{FontFile, FontStore, SoundBackend};
    let css = |v: &str, t: &str| FontStore::File(FontFile::GtkCss { version: v.to_string(), target: t.to_string() });
    let qss = |tool: &str, t: &str| FontStore::File(FontFile::QtCtQss { tool: tool.to_string(), target: t.to_string() });
    for (target, key) in [("small", "smallestReadableFont"), ("toolbar", "toolBarFont"), ("menu", "menuFont")] {
        let kde = FontStore::Cmd(SoundBackend::KConfig {
            file: "kdeglobals".to_string(),
            group: "General".to_string(),
            key: key.to_string(),
        });
        let plan = qs::font_plan("kde", target);
        assert_eq!(plan.primary, kde, "{target}");
        assert_eq!(plan.also, vec![css("3.0", target), css("4.0", target)], "kde/{target}");
        assert!(plan.extra.is_empty() && !plan.gtk_ini, "{target}");
        for de in ["gnome", "xfce", "cinnamon-mint", "mate", "budgie", "pantheon", "cosmic", "deepin", "hyprland", "sway", "niri", ""] {
            assert!(qs::font_target_available(de, target), "{de}/{target}");
            let plan = qs::font_plan(de, target);
            assert_eq!(plan.primary, css("3.0", target), "{de}/{target}");
            assert_eq!(
                plan.also,
                vec![css("4.0", target), qss("qt6ct", target), qss("qt5ct", target), kde.clone()],
                "{de}/{target}"
            );
            assert!(plan.extra.is_empty() && !plan.gtk_ini, "{de}/{target}");
        }
        let plan = qs::font_plan("lxqt", target);
        assert_eq!(plan.primary, css("3.0", target), "lxqt/{target}");
        assert_eq!(plan.also, vec![css("4.0", target), kde.clone()], "lxqt/{target}");
    }
    assert_eq!(
        qs::font_targets_shown("gnome"),
        vec![("system", true), ("mono", true), ("small", true), ("toolbar", true), ("menu", true), ("title", true)]
    );
    // Заголовок там, где писать некуда, по-прежнему не показывается.
    assert!(qs::font_targets_shown("sway").iter().all(|(t, ok)| *t != "title" && *ok));
    assert!(qs::font_targets_shown("kde").iter().all(|(_, ok)| *ok));
}

/// Подпись «на что действует» — честная по окружению.
#[test]
fn extra_fonts_scope_is_honest() {
    let _lang = i18n_pin::en();
    let gnome = qs::extra_fonts_scope_note("gnome");
    assert!(gnome.contains("gtk.css") && gnome.contains("qt6ct") && gnome.contains("GNOME Shell"), "{gnome}");
    let cosmic = qs::extra_fonts_scope_note("cosmic");
    assert!(cosmic.contains("In COSMIC apps only the interface and monospace fonts change, without the size"), "{cosmic}");
    let lxqt = qs::extra_fonts_scope_note("lxqt");
    assert!(lxqt.contains("LXQt's Qt apps don't take them") && !lxqt.contains("qt6ct"), "{lxqt}");
    let kde = qs::extra_fonts_scope_note("kde");
    assert!(kde.contains("GTK apps") && !kde.contains("qt6ct"), "{kde}");
    for de in ["gnome", "cosmic", "lxqt", "kde", "xfce", ""] {
        assert!(qs::extra_fonts_scope_note(de).contains("Electron"), "{de}");
    }
}

/// Блок в gtk.css: чужие правила и `@import` на месте, наш блок один и в
/// конце, три шрифта уживаются, читаются обратно; разорванный блок не
/// трогается; ссылка на тему заменяется файлом с `@import` этой темы.
#[test]
fn extra_fonts_gtk_css_block() {
    use qs::{FontFile, FontFileIo};
    struct Fake(Vec<(String, String)>, Option<String>);
    impl FontFileIo for Fake {
        fn read(&self, rel: &str) -> Option<String> {
            self.0.iter().find(|(p, _)| p == rel).map(|(_, t)| t.clone())
        }
        fn link_target(&self, _rel: &str) -> Option<String> {
            self.1.clone()
        }
    }
    let menu = FontFile::GtkCss { version: "3.0".to_string(), target: "menu".to_string() };
    let small = FontFile::GtkCss { version: "3.0".to_string(), target: "small".to_string() };
    let before = "@import 'colors.css';\n\nbutton { color: red; }\n";
    let text = menu.new_text(Some(before), "Noto Sans", 11);
    assert!(text.starts_with("@import 'colors.css';\n\nbutton { color: red; }\n\n/* >>> Rexon: fonts"), "{text}");
    assert!(text.contains("menubar, menubar > menuitem, menu, menuitem, popover.menu, popover.menu modelbutton {\n  font-family: \"Noto Sans\";\n  font-size: 11pt;\n}"), "{text}");
    let text = small.new_text(Some(&text), "Cantarell", 8);
    let text = menu.new_text(Some(&text), "Fira Sans", 12);
    assert_eq!(text.matches("/* >>> Rexon: fonts").count(), 1, "{text}");
    assert_eq!(qs::parse_font_spec(&menu.read_spec(&text)), ("Fira Sans".to_string(), 12));
    assert_eq!(qs::parse_font_spec(&small.read_spec(&text)), ("Cantarell".to_string(), 8));
    assert!(text.contains("statusbar, .caption, .caption-heading {"), "{text}");
    assert!(text.trim_end().ends_with("/* <<< Rexon: fonts */"), "{text}");
    assert!(text.contains("button { color: red; }"));
    // Кавычки в имени экранируются.
    let odd = menu.new_text(None, "Odd \"Name\"", 10);
    assert!(odd.contains("font-family: \"Odd \\\"Name\\\"\";"), "{odd}");
    // GTK 4 — свои узлы.
    let gtk4 = FontFile::GtkCss { version: "4.0".to_string(), target: "toolbar".to_string() };
    assert!(gtk4.new_text(None, "Inter", 10).contains(".toolbar, .toolbar button {"));
    // Разорванный блок: не трогаем.
    let broken = "a {}\n/* >>> Rexon: fonts (managed by Rexon, edits are overwritten) */\nb {}\n";
    assert_eq!(menu.try_new_text(Some(broken), "X", 10), None);
    assert_eq!(menu.file_writes("/c", &Fake(vec![("gtk-3.0/gtk.css".into(), broken.into())], None), "X", 10), None);
    // Ссылка на тему: обычный файл с @import темы и нашим блоком.
    let w = menu
        .file_writes("/c", &Fake(vec![], Some("/home/u/.themes/My Theme/gtk-3.0/gtk.css".into())), "X", 10)
        .unwrap();
    assert_eq!(w.len(), 1);
    assert!(w[0].replace_link);
    assert!(w[0].text.starts_with("@import url(\"file:///home/u/.themes/My%20Theme/gtk-3.0/gtk.css\");\n"), "{}", w[0].text);
}

/// qt6ct: таблица пишется, только если qt6ct настроен (`qt6ct.conf` есть);
/// её путь дописывается в `[Interface] stylesheets` к чужим, повторно — нет.
#[test]
fn extra_fonts_qtct_stylesheet() {
    use qs::{FontFile, FontFileIo};
    struct Fake(Vec<(String, String)>);
    impl FontFileIo for Fake {
        fn read(&self, rel: &str) -> Option<String> {
            self.0.iter().find(|(p, _)| p == rel).map(|(_, t)| t.clone())
        }
        fn link_target(&self, _rel: &str) -> Option<String> {
            None
        }
    }
    let menu = FontFile::QtCtQss { tool: "qt6ct".to_string(), target: "menu".to_string() };
    assert_eq!(menu.rel_path(), "qt6ct/qss/rexon-fonts.qss");
    assert_eq!(menu.file_writes("/c", &Fake(vec![]), "Noto Sans", 10), None, "qt6ct не настроен");
    let conf = "[Appearance]\nstyle=Fusion\n\n[Interface]\nstylesheets=/usr/share/qt6ct/qss/fusion-fixes.qss\n";
    let w = menu.file_writes("/c", &Fake(vec![("qt6ct/qt6ct.conf".into(), conf.into())]), "Noto Sans", 10).unwrap();
    assert_eq!(w.len(), 2);
    assert!(w[0].text.contains("QMenuBar, QMenu {\n  font-family: \"Noto Sans\";\n  font-size: 10pt;\n}"), "{}", w[0].text);
    assert!(!w[0].text.contains("//"), "qt6ct вырезает «//…» до конца строки");
    assert_eq!(
        qs::qtct_conf_stylesheets(&w[1].text),
        vec!["/usr/share/qt6ct/qss/fusion-fixes.qss".to_string(), "/c/qt6ct/qss/rexon-fonts.qss".to_string()]
    );
    assert!(w[1].text.contains("style=Fusion"));
    // Уже вписана — conf не переписывается; второй шрифт уживается с первым.
    let fs = Fake(vec![("qt6ct/qt6ct.conf".into(), w[1].text.clone()), ("qt6ct/qss/rexon-fonts.qss".into(), w[0].text.clone())]);
    let tool = FontFile::QtCtQss { tool: "qt6ct".to_string(), target: "toolbar".to_string() };
    let w2 = tool.file_writes("/c", &fs, "Inter", 9).unwrap();
    assert_eq!(w2.len(), 1);
    assert!(w2[0].text.contains("QToolButton {") && w2[0].text.contains("QMenuBar, QMenu {"), "{}", w2[0].text);
    // Пустой список QSettings и пути с запятой.
    assert_eq!(qs::qtct_conf_with_stylesheet("[Interface]\nstylesheets=@Invalid()\n", "/a.qss").map(|t| qs::qtct_conf_stylesheets(&t)), Some(vec!["/a.qss".to_string()]));
    let t = qs::qtct_conf_with_stylesheet("[Interface]\nstylesheets=\"/x,y.qss\", /z.qss\n", "/a.qss").unwrap();
    assert_eq!(qs::qtct_conf_stylesheets(&t), vec!["/x,y.qss".to_string(), "/z.qss".to_string(), "/a.qss".to_string()]);
}

/// Запись в каждое хранилище: argv команд и текст файлов.
#[test]
fn font_store_writes() {
    use qs::{FontFile, FontStore};
    // gsettings — «Семейство Кегль», родной ключ Cinnamon.
    let plan = qs::font_plan("cinnamon-mint", "system");
    assert_eq!(
        qs::font_store_write_argv(&plan.primary, "Noto Sans", 11, ""),
        Some(vec![
            "gsettings".to_string(),
            "set".to_string(),
            "org.cinnamon.desktop.interface".to_string(),
            "font-name".to_string(),
            "Noto Sans 11".to_string(),
        ])
    );
    // kdeglobals — запись QFont, без kwriteconfig6 писать нечем.
    let plan = qs::font_plan("kde", "system");
    assert_eq!(
        qs::font_store_write_argv(&plan.primary, "Noto Sans", 11, "/usr/bin/kwriteconfig6")
            .unwrap()
            .last()
            .unwrap(),
        "Noto Sans,11,-1,5,400,0,0,0,0,0,0,0,0,0,0,1"
    );
    assert_eq!(qs::font_store_write_argv(&plan.primary, "Noto Sans", 11, ""), None);
    // Файл командой не пишется.
    let cosmic = FontStore::File(FontFile::CosmicTk { key: "interface_font".to_string() });
    assert_eq!(qs::font_store_write_argv(&cosmic, "Noto Sans", 11, ""), None);

    // COSMIC: RON-структура FontConfig, как её пишет cosmic-config.
    let FontStore::File(file) = qs::font_plan("cosmic", "mono").primary else {
        panic!("у COSMIC родное хранилище — файл");
    };
    assert_eq!(file.rel_path(), "cosmic/com.system76.CosmicTk/v1/monospace_font");
    let text = file.new_text(None, "Fira Mono", 12);
    assert_eq!(
        text,
        "// Rexon: font (managed by Rexon)\n(\n    family: \"Fira Mono\",\n    weight: Normal,\n    stretch: Normal,\n    style: Normal,\n)"
    );
    assert_eq!(file.read_spec(&text), "Fira Mono");
    assert_eq!(qs::font_value_text(&file.read_spec(&text)).contains("Fira Mono"), true);
    assert!(file.may_create());

    // LXQt: [Qt] font в lxqt.conf, строка QFont в кавычках; соседние ключи и
    // секции не трогаются.
    let FontStore::File(file) = qs::font_plan("lxqt", "system").primary else {
        panic!("у LXQt родное хранилище — файл");
    };
    assert_eq!(file.rel_path(), "lxqt/lxqt.conf");
    let before = "[General]\ntheme=frost\n\n[Qt]\nfont=\"Sans,11,-1,5,50,0,0,0,0,0\"\nstyle=Fusion\n";
    let text = file.new_text(Some(before), "Noto Sans", 10);
    assert!(text.contains("font=\"Noto Sans,10,-1,5,400,0,0,0,0,0,0,0,0,0,0,1\""), "{text}");
    assert!(text.contains("theme=frost") && text.contains("style=Fusion"), "{text}");
    assert!(!text.contains("Sans,11,"), "{text}");
    assert_eq!(qs::parse_font_spec(&file.read_spec(&text)), ("Noto Sans".to_string(), 10));
    // fixedFont не путается с font.
    let FontStore::File(mono) = qs::font_plan("lxqt", "mono").primary else {
        panic!("у LXQt родное хранилище — файл");
    };
    let text = mono.new_text(Some(&text), "Hack", 9);
    assert!(text.contains("font=\"Noto Sans,10,"), "{text}");
    assert!(text.contains("fixedFont=\"Hack,9,"), "{text}");
    assert_eq!(qs::parse_font_spec(&mono.read_spec(&text)), ("Hack".to_string(), 9));

    // qt6ct — только в уже существующий файл.
    let plan = qs::font_plan("sway", "system");
    let Some(FontStore::File(qt6ct)) = plan.also.last() else {
        panic!("последний спутник — qt6ct");
    };
    assert!(!qt6ct.may_create());
    assert_eq!(qt6ct.rel_path(), "qt6ct/qt6ct.conf");
    let text = qt6ct.new_text(Some("[Appearance]\nstyle=Fusion\n"), "Noto Sans", 10);
    assert!(text.contains("[Fonts]\ngeneral=\"Noto Sans,10,-1,5,400,"), "{text}");
}

/// Фильтр окна выбора файла собран из тех же списков, что и разбор формата.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn dialog_filter_lists_every_known_ext() {
    let _lang = i18n_pin::ru();
    // Набор пришпилен строкой: перебор по самому списку был бы тавтологией
    // и пропустил бы потерю расширения (проверено мутацией «.tif забыт»).
    // Окно открывается на «родной» группе кнопки — она первая, как и была
    // до появления остальных групп.
    assert!(qs::wallpaper_dialog_filter("static").starts_with(
        "Изображения (*.jpg *.jpeg *.png *.webp *.bmp *.tiff *.tif);;"
    ));
    assert!(qs::wallpaper_dialog_filter("video").starts_with(
        "Видео (*.mp4 *.webm *.mkv *.avi *.mov *.m4v *.wmv *.mpg *.mpeg *.gif);;"
    ));
    let images = qs::wallpaper_dialog_filter("static");
    for ext in qs::STATIC_EXTS {
        assert!(images.contains(&format!("*.{ext}")), "{images} без {ext}");
        assert_eq!(qs::detect_wallpaper_format(&format!("x.{ext}")), "static");
    }
    let video = qs::wallpaper_dialog_filter("video");
    for ext in qs::VIDEO_EXTS {
        assert!(video.contains(&format!("*.{ext}")), "{video} без {ext}");
        assert_eq!(qs::detect_wallpaper_format(&format!("x.{ext}")), "video");
    }
}

/// Flash достижим из окна выбора файла, а не только из манифеста `.rxn`.
///
/// Движок разбирает формат `swf` (`WallpaperApplier.apply`, `case "swf"`), но
/// вкладка о нём не знала: разбор возвращал `"static"`, фильтр `.swf` не
/// показывал, и выбранный человеком ролик уезжал в показ статичной картинки.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn swf_is_reachable_from_file_dialog() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::detect_wallpaper_format("/home/w/ролик.swf"), "swf");
    assert_eq!(qs::detect_wallpaper_format("/home/w/РОЛИК.SWF"), "swf");
    // Кнопок для Flash во вкладке нет, поэтому .swf обязан быть досягаем
    // из-под ЛЮБОЙ существующей кнопки — иначе формат так и остаётся без
    // входа. Порядок групп при этом не меняется: окно по-прежнему
    // открывается на «родной» группе нажатой кнопки.
    for kind in ["static", "video", "auto"] {
        let f = qs::wallpaper_dialog_filter(kind);
        assert!(f.contains("Flash (*.swf)"), "{kind}: {f}");
        assert!(f.contains("Все обои ("), "{kind}: {f}");
    }
    assert!(qs::wallpaper_dialog_filter("swf").starts_with("Flash (*.swf);;"));
    // Ни одна группа не повторяется дважды.
    let f = qs::wallpaper_dialog_filter("static");
    assert_eq!(f.matches("Изображения (").count(), 1, "{f}");
}

/// Окно «выбрать обои» показывает ВСЕ применимые файлы одним списком.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn auto_dialog_shows_every_wallpaper_kind() {
    let _lang = i18n_pin::ru();
    let f = qs::wallpaper_dialog_filter("auto");
    assert!(f.starts_with("Все обои ("), "{f}");
    for ext in qs::all_wallpaper_exts() {
        assert!(f.contains(&format!("*.{ext}")), "{f} без {ext}");
        // Каждое расширение из фильтра вкладка умеет разобрать в формат,
        // который движок принимает (не «static по умолчанию» для .swf).
        let fmt = qs::detect_wallpaper_format(&format!("x.{ext}"));
        assert!(
            ["static", "video", "swf"].contains(&fmt),
            "{ext} → {fmt}"
        );
    }
    // Группы для тех, кто хочет сузить, никуда не делись.
    for group in ["Изображения (", "Видео (", "Flash ("] {
        assert!(f.contains(group), "{f} без группы {group}");
    }
    assert!(qs::wallpaper_dialog_title("auto").contains("обои"));
    assert!(qs::wallpaper_dialog_title("swf").contains("Flash"));
}

// ── Списки и тексты вкладки ─────────────────────────────────────────────

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn fm_choices_match_python() {
    let _lang = i18n_pin::ru();
    let g = golden();
    let want: Vec<(String, String)> = g["fm_options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c[0].as_str().unwrap().to_string(),
                c[1].as_str().unwrap().to_string(),
            )
        })
        .collect();
    // Починка (правило #13): пункт «Автоматически (определять по DE)» называл
    // рабочий стол сокращением из словаря разработчика. Сверяем с эталоном,
    // подставив новую формулировку — остальные пункты обязаны совпасть
    // дословно.
    let want: Vec<(String, String)> = want
        .into_iter()
        .map(|(short, label)| {
            if short == "auto" {
                assert_eq!(label, "⚙️  Автоматически (определять по DE)");
                (short, "⚙️  Автоматически (по рабочему столу)".to_string())
            } else {
                (short, label)
            }
        })
        .collect();
    assert_eq!(qs::fm_choices(), want, "пункты списка файловых менеджеров");
}

/// Набор пунктов обязан совпадать с набором, который знает ядро: новый
/// файловый менеджер, добавленный в `appdefaults`, не должен потеряться по
/// дороге в окно (правило #12).
#[test]
fn fm_display_covers_every_known_manager() {
    use std::collections::BTreeSet;
    let shown: BTreeSet<&str> = qs::FM_DISPLAY_ORDER.iter().copied().collect();
    let known: BTreeSet<&str> = rexon_core::appdefaults::FM_SHORT_NAMES
        .iter()
        .copied()
        .collect();
    assert_eq!(shown, known, "список в окне разошёлся со списком ядра");
    for short in qs::FM_DISPLAY_ORDER {
        let (icon, label) = qs::fm_label(short);
        assert!(!label.is_empty(), "{short} без подписи");
        assert!(!icon.is_empty(), "{short} без значка");
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn font_target_texts_match_python() {
    let _lang = i18n_pin::ru();
    let g = golden();
    // Починка (правило #13): две подсказки были написаны для разработчика —
    // «Шрифт UI приложений…» и перечень оконных менеджеров в скобках.
    // Подписи кнопок при этом обязаны совпадать с оригиналом дословно.
    let tip_fixes: &[(&str, &str)] = &[
        ("system", "Шрифт UI приложений (заголовки, кнопки, текст)"),
        ("title", "Шрифт заголовков окон (KWin/Mutter/Xfwm)"),
    ];
    for case in g["font_targets"].as_array().unwrap() {
        let target = case[0].as_str().unwrap();
        let (button, tip, _title) = qs::font_target_texts(target);
        assert_eq!(button, case[1].as_str().unwrap(), "кнопка {target}");
        match tip_fixes.iter().find(|(t, _)| *t == target) {
            Some((_, was)) => {
                assert_eq!(case[2].as_str().unwrap(), *was, "эталон {target}");
                assert_ne!(tip, *was, "{target}: починка подсказки не применена");
            }
            None => assert_eq!(tip, case[2].as_str().unwrap(), "подсказка {target}"),
        }
    }
    for (target, title) in g["font_titles"].as_object().unwrap() {
        let (_b, _t, got) = qs::font_target_texts(target);
        assert_eq!(got, title.as_str().unwrap(), "заголовок окна {target}");
    }
    // Три шрифта оригинала остались, порядок их тот же; к ним добавлены
    // «маленький», «панели инструментов» и «меню» (как у страницы KDE).
    let listed: Vec<&str> = g["font_targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c[0].as_str().unwrap())
        .collect();
    let kept: Vec<&str> = qs::FONT_TARGETS
        .iter()
        .copied()
        .filter(|t| !qs::EXTRA_FONT_TARGETS.contains(t))
        .collect();
    assert_eq!(listed, kept);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn wallpaper_status_matches_python() {
    let _lang = i18n_pin::ru();
    for case in golden()["wallpaper_status"].as_array().unwrap() {
        let st = &case[0];
        let running = st["running"].as_bool().unwrap_or(false);
        let format = st.get("format").and_then(|v| v.as_str());
        let path = st.get("path").and_then(|v| v.as_str());
        // Починка (правило #13): оригинал вставлял в строку служебное слово
        // движка — «Активны живые обои: video». Имя для человека берётся у
        // общего `wallpaper_engine::format_label`, поэтому эталонную строку
        // сверяем, подставив в неё то же имя.
        let want = case[1].as_str().unwrap().to_string();
        let want = match format.filter(|f| !f.is_empty()) {
            Some(fmt) => {
                let label = rexon_core::wallpaper_engine::format_label(fmt);
                assert_ne!(label, fmt, "{fmt}: имя формата осталось служебным");
                want.replace(&format!("</b> {fmt} —"), &format!("</b> {label} —"))
            }
            None => want,
        };
        assert_eq!(
            qs::wallpaper_status_text(running, format, path),
            want,
            "строка состояния обоев для {st}"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn restore_native_message_matches_python() {
    let _lang = i18n_pin::ru();
    for case in golden()["restore_native"].as_array().unwrap() {
        let restored = case[0].as_u64().unwrap() as usize;
        let missing: Vec<String> = case[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        // Починка (правило #13): счёт стоял с одним окончанием на все числа
        // («Восстановлено 1 приложений»), теперь склоняется общим `plural`.
        let want = case[3].as_str().unwrap().replace(
            &format!("{restored} приложений"),
            &format!(
                "{restored} {}",
                plural(restored as i64, "приложение", "приложения", "приложений")
            ),
        );
        assert_eq!(
            qs::restore_native_message(restored, &missing, case[2].as_str().unwrap()),
            want
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn protection_status_matches_python() {
    let _lang = i18n_pin::ru();
    for case in golden()["protection_status"].as_array().unwrap() {
        let changed: Vec<(String, String, String)> = case[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c[0].as_str().unwrap().to_string(),
                    c[1].as_str().unwrap().to_string(),
                    c[2].as_str().unwrap().to_string(),
                )
            })
            .collect();
        // Эталон снят с оригинала, у которого эталона защиты как понятия не
        // было: все его случаи — это «что-то уже запомнено». Их формулировки
        // сохранены дословно; случай «не запомнено ничего» проверяет
        // `protection_reports_green_only_after_a_snapshot` ниже.
        assert_eq!(
            qs::protection_status_text(1, &changed),
            case[1].as_str().unwrap()
        );
    }
}

/// ⭐ «✅ Всё на месте» говорится ТОЛЬКО после того, как эталон снят.
///
/// Дефект (вкладка «Настройки», раздел «Защита приложений по умолчанию»):
/// пустой список расхождений считался успехом. На чистом профиле, где не
/// запомнено ещё ничего и сверять не с чем, раздел показывал зелёное «Всё на
/// месте» — покупатель читал это как «защита работает», а защищён он не был
/// никак. Сверка держит обе половины: без эталона зелёного ответа нет и
/// названа кнопка, которая это чинит; с эталоном прежние формулировки не
/// тронуты.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn protection_reports_green_only_after_a_snapshot() {
    let _lang = i18n_pin::ru();
    let nothing = qs::protection_status_text(0, &[]);
    assert!(
        !nothing.contains("Всё на месте") && !nothing.contains('✅'),
        "без эталона раздел рапортует успех: {nothing}"
    );
    assert!(
        nothing.contains("не запомнил"),
        "не сказано, что запоминать ещё нечего: {nothing}"
    );
    // Названа ровно та кнопка, что стоит рядом в разделе, — иначе совет
    // разъедется с окном при первой же правке подписи.
    let button = qs::text("prot.snapshot", &[]).unwrap();
    assert!(nothing.contains(&button), "не названа кнопка «{button}»: {nothing}");

    // Эталон снят, расхождений нет — вот теперь зелено.
    assert!(qs::protection_status_text(2, &[]).contains("✅ Всё на месте"));
    // Эталон снят и приложение подменили — разговор прежний.
    let changed = vec![(
        "Терминал".to_string(),
        "konsole".to_string(),
        "gnome-terminal".to_string(),
    )];
    let text = qs::protection_status_text(2, &changed);
    assert!(text.contains("Терминал: konsole → gnome-terminal"), "{text}");
}

/// Починка 4: окно выбора файла темы не показывало `.gadget`, хотя
/// импортёр такие файлы принимает. Все прежние группы сохранены дословно,
/// добавлена недостающая.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn import_filter_keeps_old_groups_and_adds_gadget() {
    let _lang = i18n_pin::ru();
    use rexon_core::importers;
    let golden_filter = golden()["import_filters"].as_str().unwrap().to_string();
    let now = importers::import_dialog_filter();
    for group in golden_filter.split(";;") {
        if group.starts_with("Все темы Windows") {
            // Сводная группа выросла ровно на .gadget и (03.10.2026) на
            // архивы наборов курсоров с файлом ролей .crs — сразу за курсорами.
            let want = group
                .trim_end_matches(')')
                .replace("*.curxptheme", "*.curxptheme *.zip *.rar *.7z *.crs")
                + " *.gadget)";
            assert!(now.contains(&want), "сводная группа: {now}");
            continue;
        }
        assert!(now.contains(group), "потеряна группа «{group}» из {now}");
    }
    assert!(now.contains("Гаджеты рабочего стола (*.gadget)"), "{now}");
    assert!(now.contains("Наборы курсоров (*.zip *.rar *.7z *.crs)"), "{now}");
    assert!(now.ends_with("Все файлы (*)"), "{now}");
    // Список принимаемых расширений — тот же, что у диспетчера.
    for ext in [
        ".themepack", ".THEME", ".msstyles", ".ico", ".7tsp", ".cur", ".inf", ".dream", ".wmv",
        ".gadget", ".zip", ".rar", ".7z", ".crs",
    ] {
        assert!(importers::is_importable_ext(ext), "{ext} потерян");
    }
    // .zip принимается только как набор курсоров: иной ZIP импортёр
    // отклоняет, заглянув внутрь (`cursor_can_import`).
    for ext in [".rxn", ".png", ""] {
        assert!(!importers::is_importable_ext(ext), "{ext} принят зря");
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn ambient_texts() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::ambient_sound_id("/a/b/дождь.ogg"), "manual_дождь.ogg");
    assert_eq!(qs::ambient_sound_id("дождь.ogg"), "manual_дождь.ogg");
    assert!(qs::ambient_file_filter().contains("*.opus"));
    assert!(qs::ambient_file_filter().starts_with("Аудио (*.ogg"));
    assert!(qs::ambient_file_filter().ends_with(";;Все файлы (*)"));
}

// ============================================================================
// Вторая партия переноса: секции, каталог подписей, решения вкладки
// ============================================================================
//
// Эталон снят с версии вкладки ДО этого переноса (коммит `d305e9d`) РАЗБОРОМ
// ИСХОДНИКА: в `tab_phrases` попали и сообщения, которые видны только по
// нажатию, и секция MATE, которой на машине сверки не бывает. F-строки
// приведены к виду с «{}» вместо мест подстановки — с тем же видом
// сравниваются шаблоны каталога.

/// Шаблон каталога в том же виде, в каком эталон помнит f-строку Python.
///
/// Места подстановки каталога — `%1`, `%2` (в стиле Qt, так их пишет
/// `po/ru.po`); прежняя запись `{0}` тоже понимается.
fn as_phrase(template: &str) -> String {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' && chars.peek().is_some_and(|d| d.is_ascii_digit() && *d != '0') {
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
            out.push_str("{}");
            continue;
        }
        if c == '{' {
            // «{0}», «{1}», … — место подстановки
            let mut digits = String::new();
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() {
                    digits.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            if !digits.is_empty() && chars.peek() == Some(&'}') {
                chars.next();
                out.push_str("{}");
                continue;
            }
            out.push('{');
            out.push_str(&digits);
            continue;
        }
        out.push(c);
    }
    out
}

/// Подписи, которых в оригинале нет намеренно (правило #16 — баги при порте
/// чиним): жаргон в лицо покупателю заменён человеческими словами.
const TEXT_FIXES: &[(&str, &str)] = &[
    ("prot.snapshot_error", "⚠️ Ошибка snapshot: {}"),
    ("plasma.backup_error", "⚠️ Ошибка backup: {}"),
    // Подсказка перечисляла форматы, а Flash в ней не было — при том что
    // движок его проигрывает. Умолчание в перечне читается как «не умеем».
    (
        "wp.file_hint",
        "Картинка: JPG, PNG, WebP.  Видео: MP4, WebM, MKV, GIF.",
    ),
    // Галочка звалась «Звук видео-обоев», а распоряжается звуком и Flash-обоев
    // тоже: ключи настроек у них одни (16.09.2026). Подпись про видео означала
    // бы, что человек выключает звук Flash кнопкой, которая про Flash молчит.
    ("wp.video_audio", "🔊 Звук видео-обоев"),
    // Название шрифта подставлялось в середину фразы, приведённым к нижнему
    // регистру, — в немецком так ломаются существительные. Теперь оно идёт
    // как есть, в начале строки (01.10.2026).
    ("font.failed", "⚠️ Не удалось применить {}"),
    // ── Проход 06.09: жаргон разработчика по всей вкладке (правило #13) ──
    //
    // Вкладку смотрели живьём, и покупатель читал в ней «wallpaper-pack'и»,
    // «синкается в GTK», «ищи в AUR». Это не три отдельные опечатки, а один
    // класс: строки писались для себя. Поэтому переписан ВЕСЬ каталог, а не
    // замеченные места (правило #12). Имена технологий (GTK, AUR, mpv,
    // Pillow, Vimeo, YouTube) остались латиницей — менялась обвязка вокруг
    // них.
    (
        "skin.hint",
        "💡 «Системный» использует тему Plasma/Qt как есть. Остальные скины \
         задают свою палитру независимо от системной темы.",
    ),
    (
        "ob.no_main_window",
        "🎓 Обучение доступно в главном окне Rexon — запустите его без флага \
         одиночной вкладки",
    ),
    ("fm.auto_hint", "← по DE"),
    (
        "quickfix.unknown_de",
        "Для окружения «{}» набор родных приложений неизвестен. Используй \
         ручное управление в блоке «Защита» ниже.",
    ),
    ("yt.url_label", "URL:"),
    (
        "yt.url_hint",
        "https://youtube.com/watch?v=…  или  vimeo / другой поддерживаемый",
    ),
    ("yt.no_url", "⚠️ Вставь URL видео"),
    (
        "yt.no_ytdlp",
        "⚠️ yt-dlp не установлен — нажми «Установить yt-dlp»",
    ),
    (
        "yt.audio_next",
        "🔊 Звук YouTube: {} (применится к следующему ▶️ Применить)",
    ),
    (
        "web.hint",
        "URL веб-страницы / WebGL-визуализации или путь к локальному HTML.",
    ),
    ("web.no_url", "⚠️ Вставь URL или путь к HTML"),
    (
        "lib.empty",
        "Пусто. Установи wallpaper-pack через Магазин тем OCS или перенеси \
         .rxn в окно «Расширения».",
    ),
    ("lib.preview_tip", "Открыть файл системно (предпросмотр)"),
    (
        "blur.no_pillow",
        "⚠️ Pillow не установлен — размытие недоступно (<tt>pip install \
         Pillow</tt> или пакет python-pillow)",
    ),
    // ── Проход 18.09: остатки жаргона в том, что читает покупатель ──
    //
    // Значение ползунка силы размытия подписывалось «20px». Радиус в
    // пикселях — величина для того, кто пишет фильтр, а не для того, кто
    // двигает ползунок: по ней ничего нельзя решить, и единицы измерения в
    // подписи рядом со словом «Сила:» только сбивают. Осталось голое деление
    // шкалы. ⭐Расхождение с оригиналом сознательное: Python говорил
    // жаргоном, и мы не подгоняем эталон, а расходимся с ним в пользу
    // человека (правило #13).
    ("blur.px", "{}px"),
    // Образцом пути в поле веб-обоев стоял английский «/path/to/index.html».
    // Покупатель, пришедший с Windows, читает в своём окне чужой язык и
    // служебное слово «path» — при том что подсказка строкой выше уже
    // объяснила всё по-русски. ⭐Расхождение с оригиналом сознательное.
    (
        "web.url_hint",
        "https://example.com  или  /path/to/index.html",
    ),
    ("blur.apply", "🌫️ Размыть текущие"),
    ("blur.remove", "🔆 Убрать блюр"),
    ("blur.applying", "⏳ Размываю текущие обои (radius={})…"),
    ("blur.removing", "⏳ Убираю блюр, восстанавливаю обои…"),
    ("blur.failed", "Не удалось размыть (нет статичных обоев / нет Pillow)"),
    ("blur.removed", "Блюр снят, обои восстановлены"),
    ("amb.no_manager", "⚠️ Менеджер звуков недоступен (нет движка обоев)"),
    (
        "amb.no_mpv",
        "⚠️ mpv не установлен — фоновые звуки недоступны (установи пакет \
         <tt>mpv</tt>)",
    ),
    ("amb.add_tip", "Запустить любой аудиофайл фоном (mpv loop)"),
    ("amb.dialog_title", "Выбери аудиофайл"),
    (
        "amb.volume_hint",
        "Громкость применяется к <b>новому</b> добавляемому звуку. Для \
         активного — кнопка 🔊 у его строки переприменит.",
    ),
    ("amb.empty", "Нет активных фоновых звуков. Нажми «Добавить файл…»."),
    ("amb.setvol_tip", "Применить громкость слайдера к этому звуку"),
    (
        "snd.themes_hint",
        "💡 Мало тем? Установи: <tt>sound-theme-freedesktop</tt>, \
         <tt>sound-theme-smooth</tt> или ищи в AUR.",
    ),
    ("snd.event_tip", "Event: {}"),
    // «(не задан)» читалось как «шрифта нет»; на деле действует шрифт стола
    // по умолчанию — так и написано (приёмка 10.10.2026).
    ("font.unset", "<i>(не задан)</i>"),
    ("mate.icons_on", "✅ Иконки ВКЛ"),
    ("mate.icons_off", "⏹️ Иконки ВЫКЛ"),
    ("mate.no_api", "⚠️ MATE-API недоступно"),
    (
        "plasma.backup_tip",
        "Технически: копируются plasma-org.kde.plasma.desktop-appletsrc, \
         plasmashellrc, kglobalshortcutsrc",
    ),
    (
        "plasma.confirm_text",
        "Восстановить Plasma-панель из бэкапа «{}»?\n\nТекущая конфигурация \
         будет перезаписана.",
    ),
];

/// Подписи, которых в оригинале вкладки НЕ БЫЛО и быть не могло: они
/// появились вместе с настройками звука обоев, которых у вкладки не
/// существовало (была одна галочка без хранения и без громкости). Список
/// закрытый — новая строка обязана попасть сюда осознанно, а не проехать
/// мимо сверки.
const TEXT_ADDITIONS: &[&str] = &[
    // Отказ фонового звука при установленном mpv: у оригинала на любой отказ
    // был один ответ «нужен mpv» — неправда, когда не играет сам файл.
    "amb.start_unplayable",
    // Смена громкости звука, который уже не играет: оригинал об этом молчал.
    "amb.not_playing",
    // Отказы кнопок MATE: оригинал рапортовал успех, не глядя на итог.
    "mate.icons_failed",
    "mate.caja_failed",
    // Подтверждение возврата копии панели переехало из окна Qt внутрь
    // страницы (QML-вкладка): подписи ответов теперь свои.
    "dialog.yes",
    "dialog.no",
    "wp.audio_live",
    "wp.audio_volume",
    "wp.audio_mute_other",
    "wp.audio_mute_covered",
    "wp.audio_hint",
    // Раздел «Значок в системном лотке»: у оригинала вкладки значка не было
    // вовсе, поэтому и подписей к нему быть не могло.
    "tray.autostart",
    "tray.autostart_hint",
    "tray.autostart_on",
    "tray.autostart_off",
    "tray.autostart_failed",
    "tray.missing",
    // «Тема не задана»: оригинал вкладки такой строки не знал — при пустой
    // или неизвестной теме он молча показывал первую установленную.
    "snd.theme_unset",
    // В системе названа тема, которой на диске нет: оригинал показывал первую
    // установленную, Rust-вкладка до 10.10.2026 — «не задана».
    "snd.theme_missing",
    // Раздел «Язык» (30.09): выбора языка не было ни у оригинала вкладки,
    // ни в снимке русских текстов — см. [`AFTER_RU_SNAPSHOT_TEXTS`].
    "lang.row",
    "lang.system",
    "lang.restart_hint",
    "lang.restart",
    "lang.saved",
    "lang.restart_failed",
    // Сглаживание шрифтов (04.10): у оригинала вкладки были только три
    // шрифта, отрисовки не было вовсе — см. [`AFTER_RU_SNAPSHOT_TEXTS`].
    "font.render_title",
    "font.aa",
    "font.aa_exclude",
    "font.aa_to",
    "font.pt",
    "font.subpixel",
    "font.hinting",
    "font.render_hint",
    "font.render_saved",
    "font.render_native_failed",
    "font.fc_refused",
    "font.fc_failed",
    // «Изменить все шрифты» и принудительное DPI (04.10).
    "font.all_mode",
    "font.dpi",
    // Докачка значков и курсора к теме (17.10): сторожа оформления сеанса
    // (`theme_watch`) у оригинала не было.
    "tray.theme_deps",
    "tray.theme_deps_hint",
    "tray.theme_deps_on",
    "tray.theme_deps_off",
    // Полоса «Rexon может изменять систему до закрытия окна» и «Запретить
    // сейчас» (Б13, 29.10): сеанса прав у оригинала не было.
    "admin.info",
    "admin.revoke",
];

/// Подписи, появившиеся ПОСЛЕ снимка русских текстов
/// (`golden/qt_settings_ru_before.json`): «как было» у них нет, их русский
/// сверяется отдельно ([`language_section_russian`]).
const AFTER_RU_SNAPSHOT_TEXTS: &[&str] = &[
    "snd.theme_missing",
    "lang.row",
    "lang.system",
    "lang.restart_hint",
    "lang.restart",
    "lang.saved",
    "lang.restart_failed",
    // Сглаживание шрифтов — русский сверяется в `font_rendering_texts_russian`.
    "font.render_title",
    "font.aa",
    "font.aa_exclude",
    "font.aa_to",
    "font.pt",
    "font.subpixel",
    "font.hinting",
    "font.render_hint",
    "font.render_saved",
    "font.render_native_failed",
    "font.fc_refused",
    "font.fc_failed",
    // «Изменить все шрифты» и принудительное DPI (04.10).
    "font.all_mode",
    "font.dpi",
    "tray.theme_deps",
    "tray.theme_deps_hint",
    "tray.theme_deps_on",
    "tray.theme_deps_off",
    "admin.info",
    "admin.revoke",
];

/// Разделы, появившиеся после снимка русских текстов.
const AFTER_RU_SNAPSHOT_SECTIONS: &[&str] = &["langGroup"];

/// Разделы, чьё описание переписано ПОСЛЕ снимка русских текстов, потому что
/// раздел стал больше: `(раздел, новое русское описание)`. Снимок обязан
/// помнить прежнее, а каталог — отдавать новое слово в слово.
const AFTER_RU_SNAPSHOT_REWORDED: &[(&str, &str)] = &[(
    // 04.10: к трём шрифтам добавлены маленький, панели инструментов, меню
    // и сглаживание текста.
    "fontsGroup",
    "Шрифты интерфейса, терминала, мелких подписей, панелей инструментов, меню и \
     заголовков окон, а также сглаживание текста. Применяется к текущему рабочему столу \
     и к программам на других библиотеках интерфейса",
)];

/// Разделы, которых в оригинале вкладки НЕ БЫЛО. Список закрытый по той же
/// причине, что и [`TEXT_ADDITIONS`]: новый раздел обязан попасть сюда
/// осознанно, а не проехать мимо сверки.
const SECTION_ADDITIONS: &[&str] = &[
    // Значка в лотке у Rexon не было ни в одной версии вкладки.
    "trayGroup",
    // Выбора языка интерфейса тоже: Rexon был только русским.
    "langGroup",
];

/// Разделы, чей заголовок или описание переписаны с жаргона разработчика на
/// человеческий русский (правило #13, проход 06.09) — тройка «имя рамки,
/// прежний заголовок, прежнее описание». Проверка та же, что у
/// [`TEXT_FIXES`]: эталон обязан помнить прежнюю формулировку, а новая
/// обязана от неё отличаться, иначе «починка» осталась в комментарии.
const SECTION_FIXES: &[(&str, &str, &str)] = &[
    (
        "fmGroup",
        "📁 Файловый менеджер",
        "Файловый менеджер по умолчанию для папок и MIME-типа inode/directory",
    ),
    (
        "libraryGroup",
        "📚 Библиотека обоев",
        "Установленные wallpaper-pack'и (.rxn). «Применить» ставит обой; \
         ставятся через Магазин тем OCS или окно «Расширения»",
    ),
    (
        "blurGroup",
        "🌫️ Размытие обоев (Blur)",
        "Размыть текущие статичные обои для эстетики (Gaussian blur). \
         Оригинал сохраняется — блюр можно убрать одной кнопкой",
    ),
    (
        "ambientGroup",
        "🔊 Фоновые звуки (ambient)",
        "Зацикленные фоновые звуки (дождь, ветер, музыка) через mpv. \
         Громкость регулируется тут или системным микшером",
    ),
    (
        "soundThemeGroup",
        "🔔 Звуки событий",
        "Системная звуковая тема для {} (клики, уведомления, ошибки). \
         Применяется через нативные API DE",
    ),
    (
        "fontsGroup",
        "🔤 Системные шрифты",
        "Выбрать шрифт интерфейса, моноширинный и шрифт заголовков окон. \
         Применяется к текущему DE и синкается в GTK-приложения",
    ),
];

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn every_tab_text_comes_from_python() {
    let _lang = i18n_pin::ru();
    let g = golden();
    let known: BTreeMap<String, ()> = g["tab_phrases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| (v.as_str().unwrap().to_string(), ()))
        .collect();
    for (key, _) in qs::TEXTS {
        let template = &qs::text_template(key).unwrap();
        if TEXT_ADDITIONS.contains(key) {
            // Добавленная подпись обязана быть именно НОВОЙ: совпала с
            // эталонной — значит она не добавление, а порча существующей.
            assert!(
                !known.contains_key(&as_phrase(template)),
                "«{key}» помечена как новая, но такая подпись во вкладке уже была"
            );
            continue;
        }
        if let Some((_, was)) = TEXT_FIXES.iter().find(|(k, _)| k == key) {
            // Починенная строка обязана отличаться от прежней — иначе
            // «починка» осталась в комментарии, а не в коде.
            assert!(
                known.contains_key(*was),
                "эталон не помнит прежней формулировки {key}"
            );
            assert_ne!(as_phrase(template), *was, "{key}: починка не применена");
            continue;
        }
        assert!(
            known.contains_key(&as_phrase(template)),
            "подпись «{key}» = {template:?} расходится с оригиналом вкладки"
        );
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn tab_sections_match_python() {
    let _lang = i18n_pin::ru();
    let g = golden();
    let want = g["tab_sections"].as_object().unwrap();
    assert_eq!(
        want.len() + SECTION_ADDITIONS.len(),
        qs::SECTIONS.len(),
        "число секций разошлось с оригиналом"
    );
    for (id, title, desc) in qs::SECTIONS {
        // В таблице — английские `msgid`; эталон помнит русский.
        // Пустой `msgid` gettext отдаёт заголовком каталога — не переводим.
        let title = &if title.is_empty() { String::new() } else { i18n::tr(title) };
        let desc = &i18n::tr(desc);
        if SECTION_ADDITIONS.contains(id) {
            // Добавленный раздел обязан быть именно НОВЫМ: нашёлся в
            // эталоне — значит это не добавление, а порча существующего.
            assert!(!want.contains_key(*id), "{id} помечен новым, а он был");
            continue;
        }
        let row = want
            .get(*id)
            .unwrap_or_else(|| panic!("в оригинале нет секции {id}"));
        let want_desc = row[1].as_str().unwrap_or("");
        if let Some((_, was_title, was_desc)) = SECTION_FIXES.iter().find(|(k, _, _)| k == id) {
            // Переписанный раздел: эталон обязан помнить прежние слова, а
            // новые — от них отличаться хотя бы в одном из двух полей.
            assert_eq!(want_desc, *was_desc, "эталон не помнит описания {id}");
            assert_eq!(row[0].as_str().unwrap_or(""), *was_title, "эталон {id}");
            assert!(
                as_phrase(desc) != *was_desc || *title != *was_title,
                "{id}: починка формулировки не применена"
            );
            continue;
        }
        assert_eq!(
            as_phrase(desc),
            want_desc,
            "описание секции {id} разошлось с оригиналом"
        );
        match row[0].as_str() {
            Some(want_title) => assert_eq!(
                *title, want_title,
                "заголовок секции {id} разошёлся с оригиналом"
            ),
            // Заголовок секции обучения оригинал складывал на месте —
            // теперь его собирает ядро из текстов самого обучения.
            None => {
                assert!(title.is_empty(), "{id}: заголовок обязан быть у обучения");
                let (built, _) = qs::section_texts(id, "KDE").unwrap();
                assert_eq!(
                    built,
                    format!("🎓 {}", rexon_core::onboarding::settings_label())
                );
            }
        }
    }
}

#[test]
fn section_order_matches_python() {
    let g = golden();
    let want: Vec<String> = g["tab_section_order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let ours: Vec<String> = qs::section_order()
        .into_iter()
        .filter(|id| !SECTION_ADDITIONS.contains(id))
        .map(str::to_owned)
        .collect();
    assert_eq!(ours, want, "порядок секций вкладки");
    // Добавленный раздел обязан стоять в порядке показа, а не «где-нибудь».
    for id in SECTION_ADDITIONS {
        assert!(qs::section_order().iter().any(|s| s == id), "{id} не в порядке показа");
    }
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn sound_section_names_current_de() {
    let _lang = i18n_pin::ru();
    let (_, desc) = qs::section_texts("soundThemeGroup", "COSMIC").unwrap();
    assert!(
        desc.contains("для COSMIC"),
        "имя окружения не подставилось: {desc}"
    );
    assert!(!desc.contains("%1"), "место подстановки осталось: {desc}");
    assert!(qs::section_texts("нет такой секции", "KDE").is_none());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn text_lookup_is_loud_about_unknown_keys() {
    let _lang = i18n_pin::ru();
    // Пустая надпись в окне выглядит как «так и задумано» — ядро обязано
    // ответить «нет такого ключа», а не пустой строкой.
    assert!(qs::text("такого.ключа.нет", &[]).is_none());
    assert_eq!(
        qs::text("fm.current", &["nemo".to_string()]).unwrap(),
        "Текущий файловый менеджер: <b>nemo</b>"
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn clip_counts_letters_not_bytes() {
    let _lang = i18n_pin::ru();
    // Русская строка, обрезанная по байтам, разваливается посреди буквы.
    let text = "яяяяя";
    assert_eq!(qs::clip(text, 3), "яяя");
    assert_eq!(qs::clip(text, 99), text);
    assert_eq!(qs::wallpaper_done(true, &"я".repeat(200)).chars().count(), 160 + "✅ Обои применены: ".chars().count());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn ytdlp_button_follows_system() {
    let _lang = i18n_pin::ru();
    // Починка оригинала: после установки кнопка обязана стать «Обновить».
    let (label, button, action) = qs::ytdlp_row(true, "2026.07.04");
    assert!(label.contains("2026.07.04"));
    assert_eq!(action, "update");
    assert!(button.contains("Обновить"));
    let (label, button, action) = qs::ytdlp_row(false, "");
    assert!(label.contains("не установлен"));
    assert_eq!(action, "install");
    assert!(button.contains("Установить"));

    let (btn, status) = qs::ytdlp_working("update");
    assert!(btn.contains("Обновляю") && status.contains("пароль"));
    let (btn, _) = qs::ytdlp_working("install");
    assert!(btn.contains("Устанавливаю"));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn video_audio_reapplies_only_for_running_video() {
    let _lang = i18n_pin::ru();
    assert!(qs::wallpaper_audio_reapply(true, "video", true));
    assert!(!qs::wallpaper_audio_reapply(false, "video", true));
    assert!(!qs::wallpaper_audio_reapply(true, "static", true));
    // Файла нет — переприменять нечего, только сообщение «к следующему».
    assert!(!qs::wallpaper_audio_reapply(true, "video", false));
    assert!(qs::wallpaper_audio_status(true, false).contains("к следующему видео"));
    assert!(qs::wallpaper_audio_status(true, true).contains("Включаю"));
    assert!(qs::wallpaper_audio_status(false, true).contains("Выключаю"));
}

#[test]
fn library_preview_covers_every_video_ext() {
    // Починка: свой список плитки знал только часть видео.
    for fmt in qs::VIDEO_EXTS {
        assert!(
            qs::library_shows_preview(fmt),
            "{fmt}: движок это проигрывает, а кнопки предпросмотра нет"
        );
    }
    for fmt in ["video", "swf", "MP4", "gif"] {
        assert!(qs::library_shows_preview(fmt), "{fmt}");
    }
    for fmt in ["static", "web", "scene", "", "application"] {
        assert!(!qs::library_shows_preview(fmt), "{fmt}");
    }
    // Прежний набор оригинала — каждый его элемент по-прежнему с кнопкой.
    for fmt in ["video", "gif", "swf", "webm", "mkv", "mp4"] {
        assert!(qs::library_shows_preview(fmt), "потеряли прежний {fmt}");
    }
}

#[test]
fn web_url_becomes_file_scheme() {
    assert_eq!(
        qs::web_wallpaper_url("/home/u/index.html", true),
        "file:///home/u/index.html"
    );
    // Файла нет — путь остаётся как есть, движок скажет об этом сам.
    assert_eq!(qs::web_wallpaper_url("/nope.html", false), "/nope.html");
    assert_eq!(
        qs::web_wallpaper_url("https://example.com", false),
        "https://example.com"
    );
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn import_failure_speaks_human() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::import_failure_reason(true, "нечто"), "файл не найден");
    assert_eq!(qs::import_failure_reason(false, "  "), 
               "файл повреждён или формат не поддерживается");
    assert_eq!(qs::import_failure_reason(false, " битый zip "), "битый zip");
    let (note, status) = qs::import_success_texts("  ", &[]);
    assert!(note.contains("«тема»") && status.contains("тема"));
    let (note, _) = qs::import_success_texts(" Aero ", &[]);
    assert!(note.contains("«Aero»"));
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn blur_words_match_the_action() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::blur_result("blur", true), "Текущие обои размыты");
    assert!(qs::blur_result("blur", false).contains("Не удалось размыть"));
    assert_eq!(
        qs::blur_result("unblur", true),
        "Размытие снято, обои восстановлены"
    );
    assert!(qs::blur_result("unblur", false).contains("Нет сохранённых"));
    let (low, high, step, default) = qs::BLUR_RANGE;
    assert!(low < default && default < high && step > 0);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn protection_types_keep_terminal_last() {
    let _lang = i18n_pin::ru();
    let types = qs::protection_types();
    assert_eq!(types.last(), Some(&"terminal"));
    assert_eq!(types.len(), rexon_core::defaults::MIME_TYPES.len() + 1);
    // Подпись каждой галочки — из ядра, а не из окна.
    let (label, tip) = qs::protection_row_texts("terminal", "org.kde.konsole.desktop");
    assert_eq!(label, "🖥️ Терминал   ·  запомнено: Konsole");
    assert_eq!(tip, "Файл ярлыка: org.kde.konsole.desktop");
    let (label, tip) = qs::protection_row_texts("browser", "");
    assert_eq!(label, "🌐 Браузер   ·  пока не запомнено");
    assert!(tip.is_empty());
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn statuses_pick_the_right_sign() {
    let _lang = i18n_pin::ru();
    assert!(qs::restore_native_status("x", true).starts_with("⚠️"));
    assert!(qs::restore_native_status("x", false).starts_with("✅"));
    assert!(qs::compliance_status("x", false).starts_with("ℹ️"));
    assert_eq!(qs::compliance_status("x", true), "x");
    assert!(qs::plasma_status(true, "готово").starts_with("✅"));
    assert!(qs::plasma_status(false, "нет копии").starts_with("⚠️"));
    assert!(qs::fm_apply_status(true).starts_with("✅"));
    assert!(qs::sound_theme_status(false, "ocean").contains("ocean"));
    assert!(qs::event_sounds_status(true, false).contains("выключены"));
    assert!(qs::event_sounds_status(false, true).contains("Не удалось"));
    assert!(qs::mate_icons_status(true).contains("включены"));
    assert!(qs::font_value_text("  ").contains("по умолчанию"));
    assert_eq!(qs::font_value_text("Noto Sans 11"), "<i>Noto Sans 11</i>");
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn plasma_backups_are_counted_and_confirmed() {
    let _lang = i18n_pin::ru();
    assert_eq!(
        qs::plasma_backups_header(10),
        "<b>Сохранённые копии (10):</b>"
    );
    let (title, question) = qs::plasma_restore_confirm("20260620_011005");
    assert_eq!(title, "Восстановить?");
    // ⭐Сознательное расхождение с оригиналом (правило #13): Python
    // подставлял в вопрос машинную метку «20260620_011005», хотя строку
    // списка, на которую человек только что нажал, он же подписывал
    // «20.06.2026 в 01:10». Одну и ту же копию нельзя звать двумя именами в
    // одном окне, поэтому вопрос тоже спрашивает человеческой датой.
    assert!(
        question.contains("20.06.2026 в 01:10") && question.contains("будет заменён"),
        "вопрос о возврате копии показывает машинную метку времени: {question}"
    );
    // Неразобранная метка остаётся как есть — вопрос не должен превратиться
    // в «из копии от ?».
    let (_, broken) = qs::plasma_restore_confirm("мусор");
    assert!(broken.contains("мусор"));
    assert_eq!(qs::PLASMA_BACKUPS_SHOWN, 5);
}

#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn mate_buttons_are_three_known_actions() {
    let _lang = i18n_pin::ru();
    let buttons = qs::mate_buttons();
    let actions: Vec<&str> = buttons.iter().map(|(_, a)| a.as_str()).collect();
    assert_eq!(actions, vec!["on", "off", "restart"]);
    assert!(buttons[2].0.contains("Caja"));
}

// ============================================================================
// Состав разделов и цвета состояний (перенос вкладки на QML, 2026-08-23)
// ============================================================================

/// Условия показа разделов в ОРИГИНАЛЕ вкладки — те самые `if`-ы, что стояли
/// в постройке секций до переноса решения в ядро.
///
/// Пара «раздел → факт, без которого его не рисовали»:
///   * `obGroup` — `onboarding.available()`;
///   * `mateIconsGroup` — `self._de != "mate"` → выход;
///   * `plasmaGroup` — `PlasmaProtector.is_plasma_running() or has_backups()`;
///   * `aeroGroup` — `detect_plasma_forks()`.
/// Остальные разделы рисовались всегда.
const CONDITIONAL_SECTIONS: &[&str] = &["obGroup", "mateIconsGroup", "plasmaGroup", "aeroGroup"];

#[test]
fn sections_without_conditions_are_always_shown() {
    // Ни один факт не подтверждён — остаются ровно безусловные разделы.
    let nothing = qs::SectionFacts::default();
    let shown = qs::visible_sections(&nothing);
    for id in qs::section_order() {
        let conditional = CONDITIONAL_SECTIONS.contains(&id);
        assert_eq!(
            shown.contains(&id),
            !conditional,
            "раздел {id}: показан не так, как в оригинале вкладки"
        );
    }
}

#[test]
fn every_condition_opens_exactly_its_own_section() {
    // Каждый факт по отдельности: включает свой раздел и НИ ОДНОГО чужого.
    let cases: &[(&str, qs::SectionFacts)] = &[
        ("obGroup", qs::SectionFacts::new(true, false, false, false)),
        ("mateIconsGroup", qs::SectionFacts::new(false, true, false, false)),
        ("plasmaGroup", qs::SectionFacts::new(false, false, true, false)),
        ("aeroGroup", qs::SectionFacts::new(false, false, false, true)),
    ];
    for (id, facts) in cases {
        let shown = qs::visible_sections(facts);
        assert!(shown.contains(id), "{id}: свой факт раздел не открыл");
        for other in CONDITIONAL_SECTIONS {
            if other == id {
                continue;
            }
            assert!(
                !shown.contains(other),
                "{id}: заодно открылся чужой раздел {other}"
            );
        }
    }
}

#[test]
fn all_facts_show_every_section_in_core_order() {
    // Всё подтверждено — видны ВСЕ разделы, и порядок тот же, что в таблице.
    let everything = qs::SectionFacts::new(true, true, true, true);
    assert_eq!(qs::visible_sections(&everything), qs::section_order());
}

/// Цвета состояний вкладки — эталон записан здесь (прежде их сличали с
/// виджетной вкладкой на Python; с 02.10.2026 Python — только архив).
#[test]
fn status_colors_are_frozen() {
    assert_eq!(qs::COLOR_OK, "#4ade80", "всё на месте");
    assert_eq!(qs::COLOR_BAD, "#ef4444", "нет / остановить");
    assert_eq!(qs::COLOR_WARN, "#f59e0b", "внимание");

    assert_eq!(qs::fm_badge_color(true), qs::COLOR_OK);
    assert_eq!(qs::fm_badge_color(false), qs::COLOR_BAD);
    // Отметка и её цвет — пара: «установлен» зелёный, «не установлен» красный.
    assert!(qs::fm_badge(true).starts_with("✅"));
    assert!(qs::fm_badge(false).starts_with("❌"));
}

/// Каждый раздел, который знает ядро, страница обязана уметь построить.
///
/// Виджетная вкладка ловит это на запуске (`raise RuntimeError`), у QML —
/// `switch` в `SettingsPage.qml`. Сторож читает саму разметку: раздел,
/// добавленный в ядро и забытый в странице, красит тест, а не показывает
/// покупателю пустую рамку (правило #12).
#[test]
fn qml_page_builds_every_section() {
    let qml = read_sample(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../rexon-gui/qml/SettingsPage.qml"),
        "QML-страница «Настройки»",
    );

    // Пустая таблица разделов прогнала бы цикл ни разу и объявила страницу
    // годной, не проверив ни одного раздела.
    assert!(
        !qs::section_order().is_empty(),
        "ядро не знает ни одного раздела: сверять страницу не с чем"
    );
    for id in qs::section_order() {
        assert!(
            qml.contains(&format!("case \"{id}\":")),
            "раздел {id} есть в ядре, но QML-страница его не строит"
        );
    }
}

/// Надпись под кнопкой шрифта не должна быть сырой записью конфига.
///
/// Починка при переносе вкладки (правило #16): KDE хранит сериализацию
/// QFont из десяти полей, и она уезжала покупателю как есть. Теперь строку
/// разбирает тот же `parse_font_spec`, которым открывается окно выбора.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn font_value_is_human_not_config_line() {
    let _lang = i18n_pin::ru();
    let kde = "Segoe UI,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,,0,0";
    let shown = qs::font_value_text(kde);
    assert!(
        shown.contains("Segoe UI 9"),
        "имя и кегль потерялись: {shown}"
    );
    assert!(
        !shown.contains(",-1,"),
        "сырая запись конфига видна покупателю: {shown}"
    );
    // Запись GNOME и так человеческая — её вид не меняется.
    assert_eq!(qs::font_value_text("Noto Sans 11"), "<i>Noto Sans 11</i>");
    // Ничего не записано — так и говорим.
    assert!(qs::font_value_text("   ").contains("по умолчанию"));
    // Совсем непонятная строка показывается как есть: это честнее, чем
    // «не задан» там, где что-то записано.
    assert!(qs::font_value_text("???").contains("???"));
}

/// Раскладка и краски вкладки, которые QML-страница берёт из ядра картой
/// `styles`. Прежде каждое число сличалось с исходником виджетной вкладки на
/// Python; с 02.10.2026 Python — только архив, и эталон — запись
/// [`LAYOUT_FROZEN`] ниже, в той же форме, в какой числа стояли в виджетах
/// (`setSpacing(8)`, `neutral_rgba(0.04)`). Сдвиг любого числа ядра красит
/// сверку, пока эталон не поправлен осознанно.
#[test]
fn layout_and_colors_are_frozen() {
    let (group_bg, group_border) = qs::group_colors();
    let (banner_bg, banner_border) = qs::polkit_banner_colors();
    let btn = qs::primary_button_colors();
    let (danger_off_bg, danger_off_text) = qs::danger_button_disabled_colors();
    let (lib_border, lib_hover) = qs::lib_card_colors(true);
    // `neutral_rgba(0.04)` в Python — вызов, в ядре — готовая строка: сверяем
    // по числу внутри, а не по записи.
    let neutral = |css: &str| {
        let alpha = css
            .trim_start_matches("rgba(127,127,127,")
            .trim_end_matches(')');
        format!("neutral_rgba({alpha})")
    };

    let expected = [
        format!(
            "setContentsMargins({}, {}, {}, {})",
            qs::PAGE_MARGIN, qs::PAGE_MARGIN, qs::PAGE_MARGIN, qs::PAGE_MARGIN
        ),
        format!("setSpacing({})", qs::PAGE_SPACING),
        format!(
            "setContentsMargins({}, {}, {}, {})",
            qs::GROUP_MARGIN_H, qs::GROUP_MARGIN_V, qs::GROUP_MARGIN_H, qs::GROUP_MARGIN_V
        ),
        format!("setSpacing({})", qs::GROUP_SPACING),
        format!("border-radius: {}px", qs::GROUP_RADIUS),
        neutral(&group_bg),
        neutral(&group_border),
        format!(
            "border-radius: {}px; padding: {}px",
            qs::POLKIT_BANNER_RADIUS, qs::POLKIT_BANNER_PADDING
        ),
        neutral(&banner_bg),
        neutral(&banner_border),
        format!("background-color: {}", btn.background),
        format!("background-color: {}", btn.hover),
        format!("background-color: {}", btn.pressed),
        format!("border: 1px solid {}", btn.border),
        format!(
            "padding: {}px {}px",
            qs::PRIMARY_BTN_PADDING_V, qs::PRIMARY_BTN_PADDING_H
        ),
        neutral(&btn.disabled_background),
        neutral(&btn.disabled_border),
        neutral(&btn.disabled_text),
        format!(
            "padding: {}px {}px",
            qs::DANGER_BTN_PADDING_V, qs::DANGER_BTN_PADDING_H
        ),
        neutral(&danger_off_bg),
        neutral(&danger_off_text),
        format!(
            "setContentsMargins({}, {}, {}, {})",
            qs::FM_ROW_MARGIN_H, qs::FM_ROW_MARGIN_V, qs::FM_ROW_MARGIN_H, qs::FM_ROW_MARGIN_V
        ),
        format!("setSpacing({})", qs::FM_ROW_SPACING),
        format!("border-radius: {}px", qs::FM_ROW_RADIUS),
        format!("background-color: {}", qs::fm_row_focus_color()),
        format!("setMinimumWidth({})", qs::FIELD_LABEL_MIN_WIDTH),
        format!("setMinimumWidth({})", qs::VOLUME_LABEL_MIN_WIDTH),
        format!("setMaximumWidth({})", qs::VOLUME_SLIDER_MAX_WIDTH),
        format!("setMinimumWidth({})", qs::URL_LABEL_MIN_WIDTH),
        format!("setMinimumWidth({})", qs::BLUR_LABEL_MIN_WIDTH),
        format!("setMinimumWidth({})", qs::BLUR_VALUE_MIN_WIDTH),
        format!("setMaximumWidth({})", qs::AMBIENT_SLIDER_MAX_WIDTH),
        format!("setMinimumWidth({})", qs::AMBIENT_PERCENT_MIN_WIDTH),
        format!(
            "setContentsMargins(0, {}, 0, 0)",
            qs::AMBIENT_LIST_TOP_MARGIN
        ),
        format!("setSpacing({})", qs::AMBIENT_LIST_SPACING),
        format!(
            "setContentsMargins({}, {}, {}, {})",
            qs::AMBIENT_ROW_MARGIN_H,
            qs::AMBIENT_ROW_MARGIN_V,
            qs::AMBIENT_ROW_MARGIN_H,
            qs::AMBIENT_ROW_MARGIN_V
        ),
        format!("background-color: {}", qs::ambient_row_color(true)),
        format!("setFixedWidth({})", qs::AMBIENT_ROW_BUTTON_WIDTH),
        format!(
            "rgba(220,80,80,{})",
            qs::AMBIENT_STOP_FILL_ALPHA
        ),
        format!(
            "rgba(220,80,80,{})",
            qs::AMBIENT_STOP_FILL_HOVER_ALPHA
        ),
        format!(
            "padding: {}px {}px",
            qs::AMBIENT_STOP_PADDING_V, qs::AMBIENT_STOP_PADDING_H
        ),
        format!("setSpacing({})", qs::LIB_GRID_SPACING),
        format!("setContentsMargins(0, {}, 0, 0)", qs::LIB_GRID_TOP_MARGIN),
        format!("border: 1px solid {}", lib_border),
        format!("border-color: {}", lib_hover),
        format!("border-radius: {}px", qs::LIB_CARD_RADIUS),
        format!(
            "setContentsMargins({}, {}, {}, {})",
            qs::LIB_INFO_MARGIN_H, qs::LIB_INFO_MARGIN_V, qs::LIB_INFO_MARGIN_H, qs::LIB_INFO_MARGIN_V
        ),
        format!("setSpacing({})", qs::LIB_INFO_SPACING),
        format!("setSpacing({})", qs::LIB_BUTTON_SPACING),
        format!("setFixedWidth({})", qs::LIB_PREVIEW_BUTTON_WIDTH),
        format!("font-size: {}pt", qs::LIB_PLACEHOLDER_POINT_SIZE),
        format!("background-color: {}", qs::lib_placeholder_color(true)),
        format!("padding: {}px;", qs::LIB_EMPTY_PADDING),
        format!("setSpacing({})", qs::SOUND_PREVIEW_SPACING),
        format!("setSpacing({})", qs::FONT_ROW_SPACING),
        format!("setSpacing({})", qs::FONT_COL_SPACING),
        format!("setSpacing({})", qs::PLASMA_ROW_SPACING),
        format!("setSpacing({})", qs::INFO_BOX_SPACING),
    ];
    assert_eq!(expected.to_vec(), LAYOUT_FROZEN, "раскладка или краски вкладки сдвинулись");
}

const LAYOUT_FROZEN: &[&str] = &[
    "setContentsMargins(24, 24, 24, 24)",
    "setSpacing(14)",
    "setContentsMargins(16, 14, 16, 14)",
    "setSpacing(8)",
    "border-radius: 6px",
    "neutral_rgba(0.04)",
    "neutral_rgba(0.14)",
    "border-radius: 8px; padding: 10px",
    "neutral_rgba(0.1)",
    "neutral_rgba(0.25)",
    "background-color: rgba(59,130,246,0.30)",
    "background-color: rgba(59,130,246,0.45)",
    "background-color: rgba(59,130,246,0.60)",
    "border: 1px solid rgba(59,130,246,0.45)",
    "padding: 6px 14px",
    "neutral_rgba(0.15)",
    "neutral_rgba(0.25)",
    "neutral_rgba(0.7)",
    "padding: 6px 12px",
    "neutral_rgba(0.12)",
    "neutral_rgba(0.6)",
    "setContentsMargins(8, 4, 8, 4)",
    "setSpacing(8)",
    "border-radius: 4px",
    "background-color: rgba(59,130,246,0.22)",
    "setMinimumWidth(150)",
    "setMinimumWidth(190)",
    "setMaximumWidth(200)",
    "setMinimumWidth(60)",
    "setMinimumWidth(60)",
    "setMinimumWidth(48)",
    "setMaximumWidth(160)",
    "setMinimumWidth(42)",
    "setContentsMargins(0, 4, 0, 0)",
    "setSpacing(4)",
    "setContentsMargins(8, 4, 8, 4)",
    "background-color: rgba(255,255,255,0.05)",
    "setFixedWidth(40)",
    "rgba(220,80,80,0.25)",
    "rgba(220,80,80,0.4)",
    "padding: 4px 6px",
    "setSpacing(8)",
    "setContentsMargins(0, 4, 0, 0)",
    "border: 1px solid rgba(255,255,255,0.12)",
    "border-color: rgba(255,255,255,0.30)",
    "border-radius: 6px",
    "setContentsMargins(10, 8, 10, 8)",
    "setSpacing(2)",
    "setSpacing(4)",
    "setFixedWidth(36)",
    "font-size: 26pt",
    "background-color: rgba(255,255,255,0.05)",
    "padding: 8px;",
    "setSpacing(6)",
    "setSpacing(6)",
    "setSpacing(2)",
    "setSpacing(6)",
    "setSpacing(0)",
];

/// Сторож согласия ВИДЖЕТНОЙ плашки всплывающего сообщения и того, что
/// уходит в QML. Образец — `widget_qss_and_qml_control_colors_agree`
/// вкладки «Оформление».
///
/// ⚠️ ЗАЧЕМ. Плашка ложится ПОВЕРХ содержимого, её тон не берётся из
/// палитры скина, и Qt сама её ничем не красит: обе стороны рисуют её
/// целиком своими руками. Пока QML брал краски токенами
/// (`bg_secondary`/`border`/`text_primary`), плашка выходила глухой вместо
/// полупрозрачной, со скруглением 6 вместо 8 и внизу ПО ЦЕНТРУ, тогда как
/// виджетная стоит внизу СЛЕВА. Сторож держит три места разом: числа внутри
/// таблицы стилей, дверь в QML (`styles().toast`) и саму разметку.
#[test]
fn toast_qss_and_qml_toast_style_agree() {
    use rexon_core::ui_base;

    // ── 1. Таблица стилей виджетной плашки собрана из двери ──────────────
    let prop = |body: &str, name: &str| -> String {
        body.split(';')
            .map(str::trim)
            .find_map(|part| part.strip_prefix(&format!("{name}:")))
            .unwrap_or_else(|| panic!("в таблице стилей тоста нет свойства {name}"))
            .trim()
            .to_string()
    };
    for dark in [true, false] {
        let qss = ui_base::toast_qss(dark);
        let c = ui_base::toast_colors(dark);
        let body = qss
            .split_once('{')
            .expect("таблица стилей тоста без тела правила")
            .1
            .rsplit_once('}')
            .expect("правило тоста не закрыто")
            .0;
        let body = nonempty(body, "тело правила таблицы стилей плашки");
        assert_eq!(prop(body, "background-color"), c.background, "заливка");
        assert_eq!(prop(body, "color"), c.text, "надпись");
        assert_eq!(
            prop(body, "border"),
            format!("{}px solid {}", ui_base::TOAST_BORDER_WIDTH, c.border),
            "рамка"
        );
        assert_eq!(
            prop(body, "border-radius"),
            format!("{}px", ui_base::TOAST_RADIUS),
            "скругление"
        );
        assert_eq!(
            prop(body, "padding"),
            format!(
                "{}px {}px",
                ui_base::TOAST_PADDING_V,
                ui_base::TOAST_PADDING_H
            ),
            "поля"
        );
    }
    // Тёмная и светлая обязаны РАЗЛИЧАТЬСЯ: одинаковые краски означали бы,
    // что дверь потеряла наблюдение о теме и одна из тем стала нечитаемой.
    assert_ne!(
        ui_base::toast_colors(true).background,
        ui_base::toast_colors(false).background,
        "краски тоста перестали зависеть от тёмности темы"
    );

    // ── 2. Дверь ОДНА на весь продукт ────────────────────────────────────
    //
    // ⚠️ Сторож смотрел сюда по старому адресу и упал, когда группу `toast`
    // вынесли из страницы «Настройки» в общий модуль (коммит про плашку,
    // одну на все вкладки). Замысел при этом не нарушился, а усилился:
    // копий чисел стало не две, а ноль. Поэтому адрес исправлен, а правило
    // ужесточено — теперь проверяется и то, что мимо двери никто не ходит.
    let door = read_sample(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../rexon-gui/src/lib.rs"),
        "исходник крейта GUI",
    );
    let toast_mod = door
        .split_once("pub mod toast {")
        .expect("в крейте GUI нет модуля `toast`: у плашки снова нет общей двери")
        .1
        .split_once("\n}")
        .expect("модуль `toast` не закрыт")
        .0;
    let toast_mod = nonempty(toast_mod, "модуль `toast` крейта GUI");
    for needle in [
        "ui_base::toast_colors(dark)",
        "col(skin.background)",
        "col(skin.border)",
        "col(skin.text)",
        "ui_base::TOAST_BORDER_WIDTH",
        "ui_base::TOAST_RADIUS",
        "ui_base::TOAST_PADDING_H",
        "ui_base::TOAST_PADDING_V",
        "ui_base::TOAST_MARGIN",
        "ui_base::TOAST_TIMEOUT_MS",
        "ui_base::toast_width(",
        "ui_base::TOAST_TEXT_SLACK",
        "ui_base::TOAST_MAX_WIDTH",
    ] {
        assert!(
            toast_mod.contains(needle),
            "дверь плашки не берёт «{needle}» из ядра — у плашки завёлся свой набор чисел"
        );
    }

    // ── 3. Обе страницы ходят ЧЕРЕЗ дверь, а не мимо ─────────────────────
    //
    // Дефект «сообщение не гаснет никогда» заводился трижды, и каждый раз —
    // потому что вкладка заводила плашку заново. Здесь это и стережётся:
    // страница обязана брать краски, срок и ширину у двери и не обязана
    // знать ни одной константы плашки.
    for (name, file) in [
        ("Настройки", "/../rexon-gui/src/settings_page.rs"),
        ("Steam", "/../rexon-gui/src/steam_page.rs"),
    ] {
        let src = read_sample(
            &format!("{}{}", env!("CARGO_MANIFEST_DIR"), file),
            &format!("исходник страницы «{name}»"),
        );
        // Запреты ниже проверяются по КОДУ, а не по прозе: страница обязана
        // объяснять в комментарии, откуда у плашки числа («срок приходит из
        // `ui_base::TOAST_TIMEOUT_MS`»), и наказывать её за это — значит
        // запрещать ей же и объяснение.
        let code: String = src
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // ⚠️ Вот он, пустой производный образец: запреты ниже смотрят
        // ИМЕННО сюда, и на одних комментариях им нечего было бы искать.
        nonempty(&code, &format!("код страницы «{name}» без комментариев"));
        for needle in [
            "crate::toast::style_map(",
            "crate::toast::timeout_ms()",
            "crate::toast::width(",
        ] {
            assert!(
                src.contains(needle),
                "страница «{name}» не ходит через дверь плашки: нет «{needle}»"
            );
        }
        // Своих красок и своих чисел у страницы быть не может — за этим
        // сторож и поставлен.
        for forbidden in [
            "ui_base::toast_colors",
            "ui_base::TOAST_RADIUS",
            "ui_base::TOAST_PADDING_H",
            "ui_base::TOAST_MARGIN",
            "ui_base::TOAST_TIMEOUT_MS",
        ] {
            assert!(
                !code.contains(forbidden),
                "страница «{name}» снова красит плашку сама: «{forbidden}»"
            );
        }
    }

    // ── 4. Разметка плашки берёт всё из скина двери ──────────────────────
    let qml = read_sample(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../rexon-gui/qml/RexonToast.qml"),
        "общая разметка плашки",
    );
    for needle in [
        "required property var skin",
        "required property int timeoutMs",
        "required property var metrics",
        "color: plate.skin.background",
        "border.color: plate.skin.border",
        "border.width: plate.skin.borderWidth",
        "radius: plate.skin.radius",
        "color: plate.skin.textColor",
        "anchors.leftMargin: plate.skin.margin",
        "anchors.bottomMargin: plate.skin.margin",
        "plate.metrics.toastWidth(",
        // ⭐ Ради этой строки сторож и переписан: без таймера плашка не
        // гаснет вовсе, а именно так она и висела 29 минут.
        "interval: plate.timeoutMs",
    ] {
        assert!(
            qml.contains(needle),
            "разметка плашки потеряла «{needle}»"
        );
    }
    // Токенов палитры и «привязки по центру» в плашке быть не может: и то,
    // и другое — ровно та дверь, которую закрывали.
    for forbidden in ["root.pal", "root.col", "anchors.horizontalCenter", "anchors.centerIn"] {
        assert!(
            !qml.contains(forbidden),
            "плашка снова рисует себя сама: «{forbidden}»"
        );
    }

    // ── 5. Вкладки ставят именно её, а не свой прямоугольник ─────────────
    for (name, file) in [
        ("Настройки", "/../rexon-gui/qml/SettingsPage.qml"),
        ("Steam", "/../rexon-gui/qml/SteamPage.qml"),
    ] {
        let page_qml = read_sample(
            &format!("{}{}", env!("CARGO_MANIFEST_DIR"), file),
            &format!("разметка вкладки «{name}»"),
        );
        for needle in ["RexonToast {", "timeoutMs: page.toastTimeoutMs", "metrics: page"] {
            assert!(
                page_qml.contains(needle),
                "вкладка «{name}» ставит не общую плашку: нет «{needle}»"
            );
        }
    }
}

/// Слова, после которых имя неопознанного стола обязано стоять в падеже.
/// Выписаны здесь заново, а не взяты из ядра: сверка со списком, который
/// сама же проверяет, не проверяла бы ничего.
const CASE_WORDS: &[&str] = &[
    "для", "на", "в", "во", "у", "с", "со", "к", "ко", "о", "об", "из", "от", "под", "при",
    "по", "до", "без", "окружения", "приложения",
];

/// Текст, если в нём имя неопознанного стола стоит после предлога либо в
/// кавычках — то есть прямо подставлено туда, где нужен падеж.
fn unknown_de_after_preposition(text: &str) -> Option<String> {
    let name = rexon_core::screenshots::unknown_de_name();
    let name = name.as_str();
    let mut from = 0;
    while let Some(off) = text[from..].find(name) {
        let pos = from + off;
        let head = &text[..pos];
        // Кавычки сами по себе падежа не требуют («Применяю «{0}»» — это
        // название обоев); ловится только имя стола в кавычках после слова.
        if head.to_lowercase().ends_with("окружения «") {
            return Some(text.to_string());
        }
        let head = head.strip_suffix('«').unwrap_or(head);
        if let Some(before) = head.strip_suffix(' ') {
            let word: String = before
                .chars()
                .rev()
                .take_while(|c| c.is_alphabetic())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if CASE_WORDS.contains(&word.to_lowercase().as_str()) {
                return Some(text.to_string());
            }
        }
        from = pos + name.len();
    }
    None
}

/// ⭐ Имя неопознанного стола не подставляется в фразу с предлогом как есть.
///
/// Дефект: «Системная звуковая тема для ВАШ РАБОЧИЙ СТОЛ». Заглушка имени
/// склоняется, а подстановка шла простой заменой. Сверка прогоняет ВЕСЬ
/// каталог вкладки, все разделы, итог восстановления приложений и отказ
/// движка обоев с именем неопознанного стола в каждом месте подстановки.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn unknown_desktop_is_never_put_after_a_preposition() {
    let _lang = i18n_pin::ru();
    let name = rexon_core::screenshots::unknown_de_name();
    assert_eq!(name, "ВАШ РАБОЧИЙ СТОЛ");
    let name = name.as_str();
    // Сама проверка ловит то, что обязана ловить, и не ловит лишнего.
    assert!(unknown_de_after_preposition(&format!("для {name}")).is_some());
    assert!(unknown_de_after_preposition(&format!("Для окружения «{name}»")).is_some());
    assert!(unknown_de_after_preposition(&format!("Текущее окружение: <b>{name}</b>")).is_none());

    let mut texts: Vec<String> = Vec::new();
    let args = vec![name.to_string(); 4];
    for (key, _) in qs::TEXTS {
        if qs::text_template(key).unwrap().contains("%1") {
            texts.push(qs::text(key, &args).unwrap());
        }
    }
    for id in qs::section_order() {
        texts.push(qs::section_texts(id, name).unwrap().1);
    }
    let unknown = rexon_core::screenshots::display_name("");
    texts.push(qs::restore_native_message(1, &Vec::<String>::new(), &unknown));
    texts.push(qs::restore_native_message(2, &["Терминал".to_string()], name));
    for de in ["", "unknown", "generic"] {
        for step in
            rexon_core::wallpaper_engine::static_plan(de, "wayland", "/tmp/a.png", &[], "swww")
        {
            texts.push(step.err_message);
        }
    }
    for text in &texts {
        assert!(
            unknown_de_after_preposition(text).is_none(),
            "имя неопознанного стола подставлено после предлога: {text}"
        );
    }

    // Грамотные формы действительно на месте.
    let (_, sound) = qs::section_texts("soundThemeGroup", name).unwrap();
    assert!(sound.contains("для вашего рабочего стола"), "{sound}");
    let quick = qs::text("quickfix.unknown_de", &[name.to_string()]).unwrap();
    assert!(quick.starts_with("Для вашего рабочего стола набор"), "{quick}");
    assert!(texts.iter().any(|t| t.contains("на вашем рабочем столе")), "отказ обоев не склонён");
    // Известное имя подставляется как есть.
    assert_eq!(
        qs::text("quickfix.button", &["KDE Plasma".to_string()]).unwrap(),
        "Восстановить приложения KDE Plasma"
    );
    assert_eq!(rexon_core::screenshots::display_name("unknown"), name);
}

/// ⭐ Отказ фонового звука называет настоящую причину: «нужен mpv» — только
/// когда mpv действительно нет; при установленном — что не играет файл.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn ambient_failure_names_the_real_reason() {
    let _lang = i18n_pin::ru();
    let without = qs::ambient_start_failed_status(false);
    let with = qs::ambient_start_failed_status(true);
    assert!(without.contains("mpv"), "{without}");
    assert!(!with.contains("mpv"), "при установленном mpv просят mpv: {with}");
    assert_ne!(with, without);
    assert!(!with.is_empty());
}

/// ⭐ Итог смены громкости: имя звука, а не служебный идентификатор; не
/// переживший перезапуск звук — отказ, а не «громкость задана»; звука уже
/// нет — так и сказано.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn ambient_volume_status_tells_the_truth() {
    let _lang = i18n_pin::ru();
    let ok = qs::ambient_volume_status(Some("дождь.ogg"), 30, true, true);
    assert_eq!(ok, "🔊 Громкость «дождь.ogg» → 30%");
    let failed = qs::ambient_volume_status(Some("дождь.ogg"), 30, false, true);
    assert_eq!(failed, qs::ambient_start_failed_status(true));
    let gone = qs::ambient_volume_status(None, 30, false, true);
    assert!(!gone.is_empty() && gone != ok && gone != failed, "{gone}");
    assert!(!ok.contains("manual_"), "в ответ попал идентификатор: {ok}");
}

/// ⭐ Итог остановки звука называет имя, а о звуке, которого уже нет, не
/// рапортует «остановлен».
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn ambient_stopped_status_tells_the_truth() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::ambient_stopped_status(Some("дождь.ogg")), "⏹️ Остановлен: дождь.ogg");
    let gone = qs::ambient_stopped_status(None);
    assert!(!gone.contains("Остановлен"), "о пропавшем звуке сказано «остановлен»: {gone}");
    assert_eq!(gone, qs::ambient_volume_status(None, 50, false, true));
}

/// ⭐ Кнопки MATE: успех — прежние слова, отказ — отказ, а не «включены».
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn mate_results_do_not_report_blind_success() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::mate_icons_result(true, true), qs::mate_icons_status(true));
    let failed = qs::mate_icons_result(true, false);
    assert!(!failed.contains("включены"), "{failed}");
    assert!(failed.starts_with("⚠️"), "{failed}");
    assert!(qs::caja_restart_result(true).contains("перезапущена"));
    assert!(qs::caja_restart_result(false).starts_with("⚠️"));
}

// ============================================================================
// Перевод: русский как было, английский — без единой русской буквы
// ============================================================================

/// Снимок русских текстов вкладки до переноса строк в каталог.
fn ru_before() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/qt_settings_ru_before.json"
    );
    serde_json::from_str(&read_sample(path, "снимок qt_settings_ru_before.json"))
        .expect("снимок не разбирается как JSON")
}

/// ⭐ Для русского интерфейса НИЧЕГО не поменялось: каждая подпись каталога,
/// каждый раздел и каждая собранная фраза вкладки слово в слово те же, что
/// стояли в коде до переноса (снимок снят с `qt_settings.rs` того дня).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn russian_catalog_keeps_old_texts() {
    let _lang = i18n_pin::ru();
    let before = ru_before();

    let texts = before["texts"].as_object().unwrap();
    assert_eq!(
        texts.len() + AFTER_RU_SNAPSHOT_TEXTS.len(),
        qs::TEXTS.len(),
        "ключи каталога разошлись со снимком"
    );
    for (key, want) in texts {
        assert_eq!(
            qs::text_template(key).as_deref(),
            Some(want.as_str().unwrap()),
            "подпись «{key}»"
        );
    }

    let sections = before["sections"].as_object().unwrap();
    assert_eq!(sections.len() + AFTER_RU_SNAPSHOT_SECTIONS.len(), qs::SECTIONS.len());
    for (id, title, desc) in qs::SECTIONS {
        if AFTER_RU_SNAPSHOT_SECTIONS.contains(id) {
            assert!(!sections.contains_key(*id), "{id} помечен новым, а в снимке он был");
            continue;
        }
        let want = &sections[*id];
        if !title.is_empty() {
            assert_eq!(i18n::tr(title), want[0].as_str().unwrap(), "заголовок {id}");
        }
        if let Some((_, now)) = AFTER_RU_SNAPSHOT_REWORDED.iter().find(|(k, _)| k == id) {
            assert_ne!(want[1].as_str().unwrap(), *now, "{id}: снимок уже знает новое описание");
            assert_eq!(i18n::tr(desc), *now, "описание {id}");
            continue;
        }
        assert_eq!(i18n::tr(desc), want[1].as_str().unwrap(), "описание {id}");
    }

    let ru = i18n_pin::ru_catalog();
    for pair in before["phrases"].as_array().unwrap() {
        let msgid = pair[0].as_str().unwrap();
        assert_eq!(ru.get(None, msgid), pair[1].as_str(), "фраза «{msgid}»");
    }

    // Собранные строки — целиком, как их видел человек.
    assert_eq!(qs::pretty_backup_ts("20260620_011005"), "20.06.2026 в 01:10");
    assert_eq!(
        qs::restore_native_message(2, &["Терминал".to_string()], "KDE"),
        "Восстановлено 2 приложения для KDE, не нашлось в системе: Терминал"
    );
    assert_eq!(
        qs::restore_native_message(5, &[], "KDE"),
        "Восстановлено 5 приложений для KDE"
    );
    assert_eq!(
        qs::msg_no_event_in_theme("ocean"),
        "В теме «ocean» нет звука для этого события"
    );
    assert_eq!(qs::wallpaper_dialog_title("static"), "Выбрать: картинка");
    assert_eq!(qs::wallpaper_dialog_title("auto"), "Выбрать: обои");
    assert_eq!(qs::event_meta("complete"), ("✅".to_string(), "Готово".to_string()));
    assert_eq!(
        qs::fm_choices()[0].1,
        "⚙️  Автоматически (по рабочему столу)"
    );
    assert_eq!(rexon_core::constants::type_label("theme-gtk").as_deref(), Some("Тема GTK"));
}

/// Английский интерфейс вкладки: ни одной русской буквы ни в каталоге, ни в
/// разделах, ни в собранных фразах — забытая строка видна здесь, а не
/// покупателю.
///
/// Исключение одно и названо: имя неопознанного рабочего стола
/// (`screenshots::UNKNOWN_DE_NAME`) живёт в чужом модуле и в перевод этой
/// вкладки не входит — поэтому подставляется известное имя.
#[test]
fn english_tab_has_no_russian() {
    let _lang = i18n_pin::en();
    let cyrillic = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
    let args = vec!["KDE".to_string(), "x".to_string(), "y".to_string()];
    let mut all: Vec<String> = Vec::new();
    for (key, _) in qs::TEXTS {
        all.push(qs::text(key, &args).unwrap());
    }
    for id in qs::section_order() {
        let (title, desc) = qs::section_texts(id, "KDE").unwrap();
        all.push(desc);
        // Заголовок раздела обучения — у самого обучения (уже переведён).
        all.push(title);
    }
    for (_, label) in qs::fm_choices() {
        all.push(label);
    }
    for (id, _, _) in qs::SOUND_EVENT_META {
        all.push(qs::event_meta(id).1);
    }
    for target in qs::FONT_TARGETS {
        let (a, b, c) = qs::font_target_texts(target);
        all.extend([a, b, c]);
    }
    for (label, _) in qs::yt_qualities().into_iter().chain(qs::yt_modes()) {
        all.push(label);
    }
    for kind in ["static", "video", "swf", "auto"] {
        all.push(qs::wallpaper_dialog_filter(kind));
        all.push(qs::wallpaper_dialog_title(kind));
    }
    let pairs = |p: &[(&str, &str)]| -> Vec<(String, String)> {
        p.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    };
    all.extend([
        qs::ambient_file_filter(),
        qs::msg_no_event_in_theme("ocean"),
        qs::pretty_backup_ts("20260620_011005"),
        qs::backup_row_label("20260620_011005", "manual", 3),
        qs::backup_row_label("20260620_011005", "before_install", 1),
        qs::compliance_summary(&[]),
        qs::compliance_summary(&pairs(&[("a", "ok")])),
        qs::compliance_summary(&pairs(&[
            ("a", "fixed"),
            ("b", "failed"),
            ("c", "needs_action"),
            ("d", "skipped"),
        ])),
        qs::wallpaper_status_text(false, None, None),
        qs::wallpaper_status_text(true, None, Some("/a/b.mp4")),
        qs::restore_native_message(2, &["Terminal".to_string()], "KDE"),
        qs::protection_status_text(0, &[]),
        qs::protection_status_text(1, &[]),
        qs::protection_status_text(
            1,
            &[("Browser".to_string(), "a".to_string(), "b".to_string())],
        ),
        qs::youtube_starting_status("best", "stream", true),
        qs::plasma_restore_confirm("20260620_011005").1,
        qs::onboarding_replay_button(),
    ]);
    for text in &all {
        assert!(!cyrillic(text), "русское в английском интерфейсе: {text}");
    }
    for (t, label) in rexon_core::constants::type_labels_ordered() {
        let shown = rexon_core::constants::type_label(t).unwrap();
        assert_eq!(shown, *label, "{t}: исходный язык — сам msgid");
        assert!(!cyrillic(&shown), "{t}: {shown}");
    }
    for f in rexon_core::constants::WALLPAPER_FORMATS {
        let shown = rexon_core::constants::wallpaper_format_label(f).unwrap();
        assert!(!cyrillic(&shown), "{f}: {shown}");
    }
}

/// Английские формулировки, собираемые из частей, читаются как фразы.
#[test]
fn english_phrases_read_naturally() {
    let _lang = i18n_pin::en();
    assert_eq!(qs::pretty_backup_ts("20260620_011005"), "20.06.2026 at 01:10");
    assert_eq!(
        qs::backup_row_label("20260620_011005", "manual", 1),
        "📁 20.06.2026 at 01:10  ·  manually  ·  1 file"
    );
    assert_eq!(
        qs::backup_row_label("20260620_011005", "before_install", 11),
        "📁 20.06.2026 at 01:10  ·  automatically, before installing  ·  11 files"
    );
    assert_eq!(
        qs::compliance_summary(&[
            ("a".to_string(), "fixed".to_string()),
            ("b".to_string(), "failed".to_string()),
        ]),
        "⚠️ Installed: 1, failed: 1"
    );
    assert_eq!(
        qs::restore_native_message(1, &[], "KDE"),
        "Restored 1 app for KDE"
    );
    assert_eq!(
        qs::restore_native_message(3, &["Terminal".to_string()], "GNOME"),
        "Restored 3 apps for GNOME, not found on the system: Terminal"
    );
    assert_eq!(
        qs::text("quickfix.button", &["KDE Plasma".to_string()]).unwrap(),
        "Restore KDE Plasma Apps"
    );
    let (_, desc) = qs::section_texts("soundThemeGroup", "COSMIC").unwrap();
    assert!(desc.starts_with("System sound theme for COSMIC:"), "{desc}");
    assert!(qs::wallpaper_dialog_filter("static").starts_with("Images (*.jpg"));
    // Неопознанный стол — запасное имя по-английски, тем же регистром.
    assert_eq!(qs::de_display("unknown", "", ""), "YOUR DESKTOP");
    assert_eq!(qs::de_display("unknown", "niri:wlroots", ""), "NIRI");
    assert_eq!(qs::wallpaper_dialog_title("video"), "Choose a video");
    assert_eq!(qs::font_apply_status(false, "System Font", "", 0), "⚠️ System Font: could not apply");
    assert_eq!(
        qs::youtube_starting_status("best", "stream", false),
        "⏳ YouTube (Best available, Stream, sound off) — this may take a while…"
    );
    // Кнопка в совете та же, что на экране.
    assert!(qs::protection_status_text(0, &[]).contains("“📌 Remember Now”"));
    assert_eq!(qs::event_meta("device-added").1, "Device connected");
}

// ============================================================================
// Раздел «Язык»
// ============================================================================

/// Русские подписи раздела «Язык» — ровно такие, какими их утвердили.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn language_section_russian() {
    let _lang = i18n_pin::ru();
    let (title, desc) = qs::section_texts("langGroup", "KDE").unwrap();
    assert_eq!(title, "🌐 Язык");
    assert_eq!(desc, "Язык окон, меню и сообщений Rexon");
    let one = |key: &str, a: &str| qs::text(key, &[a.to_string()]).unwrap();
    assert_eq!(one("lang.row", ""), "<b>Язык:</b>");
    assert_eq!(one("lang.system", "Русский"), "Как в системе (Русский)");
    assert_eq!(one("lang.restart_hint", ""), "Язык сменится после перезапуска Rexon.");
    assert_eq!(one("lang.restart", ""), "Перезапустить Rexon");
    assert_eq!(one("lang.saved", "English"), "🌐 Язык сохранён: English");
    assert_eq!(
        qs::language_restart_failed("нет файла"),
        "⚠️ Не удалось перезапустить Rexon: нет файла"
    );
}

/// Полоса права изменять систему (Б13): по-русски и слово в слово как пункты
/// меню значка — это одно состояние и одно действие.
#[test]
fn admin_strip_russian_matches_tray() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::text_template("admin.info").as_deref(), Some("Rexon может изменять систему до закрытия окна"));
    assert_eq!(qs::text_template("admin.revoke").as_deref(), Some("Запретить сейчас"));
    assert_eq!(qs::text_msgid("admin.info"), rexon_core::tray::label_msgid("admin.info"));
    assert_eq!(qs::text_msgid("admin.revoke"), rexon_core::tray::label_msgid("admin.revoke"));
}

/// Английские подписи раздела «Язык».
#[test]
fn language_section_english() {
    let _lang = i18n_pin::en();
    let (title, desc) = qs::section_texts("langGroup", "KDE").unwrap();
    assert_eq!(title, "🌐 Language");
    assert_eq!(desc, "The language of Rexon's windows, menus and messages");
    let one = |key: &str, a: &str| qs::text(key, &[a.to_string()]).unwrap();
    assert_eq!(one("lang.system", "English"), "Same as system (English)");
    assert_eq!(one("lang.restart_hint", ""), "The language will change after Rexon restarts.");
    assert_eq!(one("lang.restart", ""), "Restart Rexon");
}

/// Список языков: первым «как в системе», затем English, каждый язык —
/// самоназванием, а не переводом (иначе человек, случайно включивший чужой
/// язык, не нашёл бы свой).
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn language_choices_use_native_names() {
    let _lang = i18n_pin::ru();
    let choices = qs::language_choices();
    assert_eq!(choices[0].0, "system");
    assert!(choices[0].1.starts_with("Как в системе ("), "{:?}", choices[0]);
    assert_eq!(choices[1], ("en".to_string(), "English".to_string()));
    // Русский каталог сверка собрала сама — значит «Русский» в списке есть.
    assert!(choices.contains(&("ru".to_string(), "Русский".to_string())), "{choices:?}");
    assert!(qs::language_choice_valid("en"));
    assert!(qs::language_choice_valid("system"));
    assert!(!qs::language_choice_valid("xx"));
    assert_eq!(qs::language_saved_status("en"), "🌐 Язык сохранён: English");
    drop(_lang);
    let _lang = i18n_pin::en();
    let choices = qs::language_choices();
    assert_eq!(choices[1].1, "English");
    assert!(choices.contains(&("ru".to_string(), "Русский".to_string())));
    assert!(choices[0].1.starts_with("Same as system ("));
}

/// Перезапуск ради языка открывает окно снова на «Настройках» и не прячет
/// его в лоток.
#[test]
fn restart_args_reopen_settings() {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    assert_eq!(qs::restart_args(&[]), s(&["--section=settings"]));
    assert_eq!(
        qs::restart_args(&s(&["--tray", "--section=steam", "--page=compare", "--selftest", "-v"])),
        s(&["-v", "--section=settings"])
    );
}

/// Д5 (проверка окружений 02.10.2026): шрифт интерфейса переносится в
/// `~/.config/gtk-3.0` и `gtk-4.0`, а не в `gtk-gtk-3.0` — тот каталог GTK
/// не читает, и GTK-программы на LXQt/X11 и XWayland шрифт не получали.
#[test]
fn gtk_font_goes_to_real_gtk_dirs() {
    assert_eq!(
        qs::gtk_font_ini_paths("/h/.config/"),
        ["/h/.config/gtk-3.0/settings.ini", "/h/.config/gtk-4.0/settings.ini"]
    );
    assert_eq!(
        qs::stray_gtk_font_dirs("/h/.config"),
        [
            ("/h/.config/gtk-gtk-3.0".to_string(), "/h/.config/gtk-gtk-3.0/settings.ini".to_string()),
            ("/h/.config/gtk-gtk-4.0".to_string(), "/h/.config/gtk-gtk-4.0/settings.ini".to_string()),
        ]
    );
}

/// Мусорный каталог прежней записи снимается, только если в нём ровно наш
/// файл: один `settings.ini` с одной строкой `gtk-font-name`.
#[test]
fn stray_gtk_font_dir_removed_only_if_ours() {
    let only = vec!["settings.ini".to_string()];
    let ours = "[Settings]\ngtk-font-name=Noto Serif 13\n";
    assert!(qs::stray_gtk_font_dir_removable(&only, Some(ours)));
    // Лишний файл рядом — каталог не наш.
    assert!(!qs::stray_gtk_font_dir_removable(
        &["settings.ini".to_string(), "gtk.css".to_string()],
        Some(ours)
    ));
    // Лишний ключ — кто-то правил руками.
    assert!(!qs::stray_gtk_font_dir_removable(
        &only,
        Some("[Settings]\ngtk-font-name=Noto Serif 13\ngtk-theme-name=Adwaita\n")
    ));
    assert!(!qs::stray_gtk_font_dir_removable(&only, Some("[Other]\ngtk-font-name=X 1\n")));
    assert!(!qs::stray_gtk_font_dir_removable(&only, None));
    assert!(!qs::stray_gtk_font_dir_removable(&[], Some(ours)));
}

// ═══════════════════════════════════════════════════════════════════════════
//  Существование темы звуков перед применением (03.10.2026): живьём в
//  kdeglobals стояла тема «Sounds», которой нет, — вкладка сказала
//  «применена», а система молча играла freedesktop.
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn sound_theme_exists_only_on_top_level_with_index() {
    let base = std::env::temp_dir().join(format!("rexon_snd_exists_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let put = |rel: &str, text: &str| {
        let p = base.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    put("user/sounds/Ocean/index.theme", "[Sound Theme]\nName=Ocean\n");
    put("user/sounds/Sounds/Zorin/index.theme", "[Sound Theme]\nName=Zorin\n");
    put("user/sounds/NoIndex/stereo/bell.oga", "x");
    put("user/sounds/IconLike/index.theme", "[Icon Theme]\nName=x\n");
    put("sys/sounds/freedesktop/index.theme", "[Sound Theme]\nName=Default\n");
    let dirs = vec![
        base.join("user/sounds").to_string_lossy().into_owned(),
        base.join("sys/sounds").to_string_lossy().into_owned(),
    ];
    assert!(qs::sound_theme_exists(&dirs, "Ocean"));
    assert!(qs::sound_theme_exists(&dirs, "freedesktop"), "второй каталог XDG тоже ищется");
    for bad in ["Sounds", "Zorin", "NoIndex", "IconLike", "", "Sounds/Zorin", "..", "Нет"] {
        assert!(!qs::sound_theme_exists(&dirs, bad), "«{bad}» не тема на верхнем уровне");
    }
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn xdg_sound_theme_dirs_follow_spec_order() {
    assert_eq!(
        qs::xdg_sound_theme_dirs("/h/.local/share/", None),
        ["/h/.local/share/sounds", "/usr/local/share/sounds", "/usr/share/sounds"]
    );
    assert_eq!(
        qs::xdg_sound_theme_dirs("/h/d", Some("/opt/share:/usr/share::/opt/share/")),
        ["/h/d/sounds", "/opt/share/sounds", "/usr/share/sounds"]
    );
}

#[test]
fn missing_sound_theme_message_names_the_theme() {
    let _lang = i18n_pin::ru();
    let msg = qs::sound_theme_missing_message("Sounds");
    assert!(msg.contains("«Sounds»"), "{msg}");
    assert!(!msg.contains("примен"), "это отказ, а не «применена»: {msg}");
}

// ── Отрисовка шрифтов: сглаживание, субпиксели, хинтинг ─────────────────

/// Родные ключи отрисовки по окружениям. Источники — схемы gsettings
/// (`org.gnome.desktop.interface`, `org.cinnamon.settings-daemon.plugins.
/// xsettings`, `org.mate.font-rendering`), канал `xsettings` XFCE
/// (`xfce4-appearance-settings`) и `kdeglobals [General]` страницы «Шрифты»
/// Plasma 6.
#[test]
fn font_rendering_native_keys_per_desktop() {
    use qs::{RenderDialect, RenderKey, SoundBackend};
    let names = |de: &str| -> Option<(RenderDialect, Vec<(RenderKey, String)>)> {
        qs::render_native_store(de).map(|s| {
            (
                s.dialect,
                s.keys
                    .iter()
                    .map(|(k, b)| {
                        let name = match b {
                            SoundBackend::GSettings { schema, key } => format!("{schema} {key}"),
                            SoundBackend::Xfconf { channel, prop } => format!("{channel} {prop}"),
                            SoundBackend::KConfig { file, group, key } => format!("{file} [{group}] {key}"),
                            SoundBackend::Dde { .. } => "dde".to_string(),
                        };
                        (*k, name)
                    })
                    .collect(),
            )
        })
    };
    let (d, keys) = names("kde").unwrap();
    assert_eq!(d, RenderDialect::Kde);
    assert_eq!(
        keys,
        vec![
            (RenderKey::Antialias, "kdeglobals [General] XftAntialias".to_string()),
            (RenderKey::HintStyle, "kdeglobals [General] XftHintStyle".to_string()),
            (RenderKey::SubPixel, "kdeglobals [General] XftSubPixel".to_string()),
        ]
    );
    let (d, keys) = names("xfce").unwrap();
    assert_eq!(d, RenderDialect::Xfce);
    assert_eq!(
        keys.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>(),
        ["xsettings /Xft/Antialias", "xsettings /Xft/Hinting", "xsettings /Xft/HintStyle", "xsettings /Xft/RGBA"]
    );
    for (de, schema, prefix) in [
        ("gnome", "org.gnome.desktop.interface", "font-"),
        ("budgie", "org.gnome.desktop.interface", "font-"),
        ("pantheon", "org.gnome.desktop.interface", "font-"),
        ("cinnamon", "org.cinnamon.settings-daemon.plugins.xsettings", ""),
        ("cinnamon-mint", "org.cinnamon.settings-daemon.plugins.xsettings", ""),
        ("mate", "org.mate.font-rendering", ""),
    ] {
        let (d, keys) = names(de).unwrap();
        assert_eq!(d, RenderDialect::Gnome, "{de}");
        assert_eq!(
            keys.iter().map(|(_, n)| n.clone()).collect::<Vec<_>>(),
            ["antialiasing", "hinting", "rgba-order"]
                .iter()
                .map(|k| format!("{schema} {prefix}{k}"))
                .collect::<Vec<_>>(),
            "{de}"
        );
    }
    // Своей страницы нет — родным служит сам fonts.conf.
    for de in ["lxqt", "cosmic", "deepin", "hyprland", "sway", "niri", "unknown", ""] {
        assert!(qs::render_native_store(de).is_none(), "{de}");
    }
}

/// Значения в записи каждого хранилища и общий слой GNOME для GTK.
#[test]
fn font_rendering_plan_values() {
    let r = qs::FontRendering {
        antialias: true,
        exclude: None,
        subpixel: "bgr".to_string(),
        hinting: "full".to_string(),
    };
    let plan = qs::rendering_plan("xfce", &r, "", true);
    let xf = |prop: &str, ty: &str, v: &str| -> Vec<String> {
        ["xfconf-query", "-c", "xsettings", "-p", prop, "-n", "-t", ty, "-s", v]
            .iter()
            .map(|s| s.to_string())
            .collect()
    };
    assert_eq!(
        plan.primary,
        vec![
            xf("/Xft/Antialias", "int", "1"),
            xf("/Xft/Hinting", "int", "1"),
            xf("/Xft/HintStyle", "string", "hintfull"),
            xf("/Xft/RGBA", "string", "bgr"),
        ]
    );
    let gs = |k: &str, v: &str| -> Vec<String> {
        ["gsettings", "set", "org.gnome.desktop.interface", k, v]
            .iter()
            .map(|s| s.to_string())
            .collect()
    };
    assert_eq!(
        plan.also,
        vec![gs("font-antialiasing", "rgba"), gs("font-hinting", "full"), gs("font-rgba-order", "bgr")]
    );
    // Без gsettings общий слой не предлагается.
    assert!(qs::rendering_plan("xfce", &r, "", false).also.is_empty());
    // В GNOME общий слой и есть родной — дубля нет.
    let gnome = qs::rendering_plan("gnome", &r, "", true);
    assert_eq!(gnome.primary.len(), 3);
    assert!(gnome.also.is_empty());
    // KDE: без kwriteconfig6 писать нечем, с ним — kdeglobals с уведомлением.
    assert!(qs::rendering_plan("kde", &r, "", true).primary.is_empty());
    let kde = qs::rendering_plan("kde", &r, "/usr/bin/kwriteconfig6", true);
    assert_eq!(
        kde.primary[1],
        ["/usr/bin/kwriteconfig6", "--file", "kdeglobals", "--group", "General", "--key", "XftHintStyle", "--notify", "hintfull"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
    // Без сглаживания: GNOME «none», субпиксели сняты.
    let off = qs::FontRendering { antialias: false, ..r.clone() }.normalized();
    assert_eq!(off.subpixel, "none");
    assert_eq!(off.gnome_antialiasing(), "none");
    let gray = qs::FontRendering { subpixel: "none".to_string(), ..r.clone() };
    assert_eq!(gray.gnome_antialiasing(), "grayscale");
    assert_eq!(gray.gnome_rgba_order(), "rgb");
    // Тайловые сеансы: родного нет, но GTK получает ключи GNOME.
    let tiling = qs::rendering_plan("hyprland", &r, "", true);
    assert!(tiling.primary.is_empty());
    assert_eq!(tiling.also.len(), 3);
}

/// Ответы хранилищ разбираются в одну настройку; родное главнее fonts.conf.
#[test]
fn font_rendering_answers_and_merge() {
    use qs::{RenderDialect as D, RenderKey as K};
    let gnome = qs::parse_render_answers(
        D::Gnome,
        &[(K::Antialias, "'rgba'\n".into()), (K::HintStyle, "'medium'".into()), (K::SubPixel, "'vbgr'".into())],
    );
    assert_eq!(gnome.antialias, Some(true));
    assert_eq!(gnome.subpixel.as_deref(), Some("vbgr"));
    assert_eq!(gnome.hinting.as_deref(), Some("medium"));
    let gray = qs::parse_render_answers(D::Gnome, &[(K::Antialias, "'grayscale'".into())]);
    assert_eq!(gray.subpixel.as_deref(), Some("none"));
    let xfce = qs::parse_render_answers(
        D::Xfce,
        &[
            (K::Antialias, "0".into()),
            (K::Hinting, "0".into()),
            (K::HintStyle, "hintfull".into()),
            (K::SubPixel, "rgb".into()),
        ],
    );
    assert_eq!(xfce.antialias, Some(false));
    assert_eq!(xfce.hinting.as_deref(), Some("none"));
    // «-1» у XFCE — не задано.
    assert_eq!(qs::parse_render_answers(D::Xfce, &[(K::Antialias, "-1".into())]).antialias, None);
    let kde = qs::parse_render_answers(D::Kde, &[(K::Antialias, "".into()), (K::HintStyle, "hintslight".into())]);
    assert_eq!(kde.antialias, None);
    assert_eq!(kde.hinting.as_deref(), Some("slight"));

    let fc = qs::RenderingParts {
        antialias: Some(false),
        subpixel: Some("bgr".into()),
        hinting: Some("full".into()),
        exclude: Some((7, 12)),
        dpi: None,
    };
    let merged = qs::merge_rendering(&gnome, &fc);
    assert!(merged.antialias);
    assert_eq!(merged.subpixel, "vbgr");
    assert_eq!(merged.hinting, "medium");
    assert_eq!(merged.exclude, Some((7, 12)), "диапазон — только из fonts.conf");
    assert_eq!(qs::merge_rendering(&Default::default(), &Default::default()), qs::FontRendering::default());
}

const FOREIGN_FONTS_CONF: &str = r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <!-- своё правило человека -->
  <alias>
    <family>monospace</family>
    <prefer><family>Iosevka</family></prefer>
  </alias>
  <match target="font">
    <test name="family"><string>Terminus</string></test>
    <edit name="antialias" mode="assign"><bool>false</bool></edit>
  </match>
</fontconfig>
"#;

/// Наш блок вливается в чужой fonts.conf, не задевая чужого, заменяется
/// при повторной записи и читается обратно.
#[test]
fn fontconfig_block_merges_and_round_trips() {
    let r = qs::FontRendering {
        antialias: true,
        exclude: Some((8, 15)),
        subpixel: "rgb".to_string(),
        hinting: "slight".to_string(),
    };
    let block = qs::fontconfig_block(&r, None);
    // Файла нет — заводится целиком.
    let fresh = qs::fontconfig_with_block(None, &block).unwrap();
    assert!(fresh.starts_with("<?xml"));
    assert!(fresh.contains("<fontconfig>") && fresh.trim_end().ends_with("</fontconfig>"));
    let back = qs::fontconfig_rendering(&fresh);
    assert_eq!(back.antialias, Some(true));
    assert_eq!(back.subpixel.as_deref(), Some("rgb"));
    assert_eq!(back.hinting.as_deref(), Some("slight"));
    assert_eq!(back.exclude, Some((8, 15)));
    // Диапазон в пикселях — при 96 DPI 8pt = 11px, 15pt = 20px.
    assert!(fresh.contains("<double>11</double>") && fresh.contains("<double>20</double>"), "{fresh}");

    // Чужой файл: всё чужое на месте, наш блок — последним.
    let merged = qs::fontconfig_with_block(Some(FOREIGN_FONTS_CONF), &block).unwrap();
    for line in FOREIGN_FONTS_CONF.lines().filter(|l| *l != "</fontconfig>") {
        assert!(merged.contains(line), "пропало «{line}»:\n{merged}");
    }
    assert!(merged.find("Terminus").unwrap() < merged.find(">>> Rexon").unwrap());
    // Правило человека с <test name="family"> — не наше, его не читаем.
    assert_eq!(qs::fontconfig_rendering(&merged).antialias, Some(true));

    // Повторная запись заменяет блок, а не копит.
    let r2 = qs::FontRendering { hinting: "full".to_string(), exclude: None, ..r.clone() };
    let again = qs::fontconfig_with_block(Some(&merged), &qs::fontconfig_block(&r2, Some(120))).unwrap();
    assert_eq!(again.matches(">>> Rexon").count(), 1, "{again}");
    assert_eq!(again.matches("<<< Rexon").count(), 1);
    let back = qs::fontconfig_rendering(&again);
    assert_eq!(back.hinting.as_deref(), Some("full"));
    assert_eq!(back.exclude, None);
    assert_eq!(back.dpi, Some(120));
    // И то же самое дважды подряд — без дрейфа текста.
    let twice = qs::fontconfig_with_block(Some(&again), &qs::fontconfig_block(&r2, Some(120))).unwrap();
    assert_eq!(twice, again);
    // Чужое при этом не сдвинулось.
    assert!(again.starts_with(&FOREIGN_FONTS_CONF[..FOREIGN_FONTS_CONF.find("</fontconfig>").unwrap()]));
}

/// Битый файл и надрезанный блок — отказ, а не догадка: ошибка в fonts.conf
/// ломает шрифты всем программам.
#[test]
fn fontconfig_block_refuses_to_guess() {
    let block = qs::fontconfig_block(&qs::FontRendering::default(), None);
    for bad in [
        "<fontconfig><match></fontconfig>",
        "<?xml version=\"1.0\"?>\n<fontconfig>\n",
        "<other/>",
        "просто текст",
    ] {
        assert_eq!(
            qs::fontconfig_with_block(Some(bad), &block),
            Err(qs::FontconfigRefusal::Malformed),
            "{bad}"
        );
    }
    let cut = "<fontconfig>\n  <!-- >>> Rexon: font rendering (managed by Rexon, edits are overwritten) -->\n</fontconfig>\n";
    assert_eq!(qs::fontconfig_with_block(Some(cut), &block), Err(qs::FontconfigRefusal::BrokenBlock));
    // Пустой корень `<fontconfig/>` раскрывается.
    let out = qs::fontconfig_with_block(Some("<?xml version=\"1.0\"?>\n<fontconfig/>\n"), &block).unwrap();
    assert!(out.contains("<fontconfig>\n  <!-- >>> Rexon"), "{out}");
    assert_eq!(qs::fontconfig_rendering(&out).antialias, Some(true));
}

/// Файл, который ведёт страница «Шрифты» KDE (её форма записи), читается
/// так же, как наш блок: человек, настроивший шрифты в KDE, видит в Rexon
/// своё, а не умолчания.
#[test]
fn fontconfig_reads_kde_shaped_file() {
    let kde = r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
 <match target="pattern">
  <edit mode="assign" name="rgba">
   <const>vrgb</const>
  </edit>
 </match>
 <match target="pattern">
  <edit mode="assign" name="hintstyle">
   <const>hintmedium</const>
  </edit>
 </match>
 <match target="pattern">
  <test compare="more_eq" name="size" qual="any">
   <double>6</double>
  </test>
  <test compare="less_eq" name="size" qual="any">
   <double>10</double>
  </test>
  <edit mode="assign" name="antialias">
   <bool>false</bool>
  </edit>
 </match>
 <match target="pattern">
  <edit mode="assign" name="antialias">
   <bool>true</bool>
  </edit>
 </match>
</fontconfig>
"#;
    let parts = qs::fontconfig_rendering(kde);
    assert_eq!(parts.subpixel.as_deref(), Some("vrgb"));
    assert_eq!(parts.hinting.as_deref(), Some("medium"));
    assert_eq!(parts.exclude, Some((6, 10)));
    assert_eq!(parts.antialias, Some(true));
}

/// Подписи сглаживания по-русски: каталог их знает, а не отдаёт английский.
#[test]
#[cfg_attr(feature = "isolated-checks", ignore = "нужна живая машина: собирает po/ru.po настоящим msgfmt")]
fn font_rendering_texts_russian() {
    let _lang = i18n_pin::ru();
    assert_eq!(qs::text_template("font.aa").as_deref(), Some("Сглаживание"));
    assert_eq!(qs::text_template("font.aa_to").as_deref(), Some("до"));
    assert_eq!(qs::text_template("font.hinting").as_deref(), Some("Хинтинг:"));
    let hint: Vec<String> = qs::hinting_choices().into_iter().map(|(_, l)| l).collect();
    assert_eq!(hint, ["Нет", "Лёгкий", "Средний", "Полный"]);
    let sub: Vec<String> = qs::subpixel_choices().into_iter().map(|(_, l)| l).collect();
    assert_eq!(sub, ["Нет", "RGB", "BGR", "Вертикальная RGB", "Вертикальная BGR"]);
    let refused = qs::rendering_status(&qs::RenderingOutcome::FontconfigRefused("/h/.config/fontconfig/fonts.conf".into()));
    assert!(refused.contains("/h/.config/fontconfig/fonts.conf") && refused.contains("не тронул"), "{refused}");
    assert!(qs::rendering_status(&qs::RenderingOutcome::Saved).starts_with("✅ Сглаживание"));
}

// ── «Изменить все шрифты» и принудительное DPI ──────────────────────────

fn now(list: &[(&str, &str, i32)]) -> Vec<qs::FontNow> {
    list.iter()
        .map(|(t, f, s)| qs::FontNow { target: t.to_string(), family: f.to_string(), size: *s })
        .collect()
}

/// Правила «изменить все» как у страницы KDE: маленький — на 2 пт меньше
/// (от 9 пт), моноширинный не получает обычное семейство.
#[test]
fn change_all_fonts_rules() {
    let cur = now(&[
        ("system", "Noto Sans", 10),
        ("mono", "Hack", 10),
        ("small", "Noto Sans", 8),
        ("menu", "Noto Sans", 10),
        ("title", "Noto Sans", 10),
    ]);
    let (plan, kept) = qs::change_all_fonts(&cur, "Inter", 12, "both", false);
    let got: Vec<(&str, &str, i32)> = plan.iter().map(|f| (f.target.as_str(), f.family.as_str(), f.size)).collect();
    assert_eq!(
        got,
        [("system", "Inter", 12), ("mono", "Hack", 12), ("small", "Inter", 10), ("menu", "Inter", 12), ("title", "Inter", 12)]
    );
    assert!(kept);
    // Моноширинное семейство уходит и в моноширинный.
    let (plan, kept) = qs::change_all_fonts(&cur, "JetBrains Mono", 8, "both", true);
    assert!(!kept);
    assert_eq!(plan[1].family, "JetBrains Mono");
    assert_eq!(plan[2].size, 8, "мельче 9 пт маленький не уменьшается");
    // Только кегль — семейства свои.
    let (plan, _) = qs::change_all_fonts(&cur, "Inter", 11, "size", false);
    assert_eq!(plan[1].family, "Hack");
    assert_eq!(plan[0].family, "Noto Sans");
    assert_eq!(plan[2].size, 9);
    // Только семейство — кегли свои.
    let (plan, _) = qs::change_all_fonts(&cur, "Inter", 20, "family", false);
    assert_eq!(plan.iter().map(|f| f.size).collect::<Vec<_>>(), [10, 10, 8, 10, 10]);
    // Своего моноширинного нет, а выбран обычный — моноширинный не трогаем.
    let bare = now(&[("system", "Noto Sans", 10), ("mono", "", 0)]);
    let (plan, _) = qs::change_all_fonts(&bare, "Inter", 11, "both", false);
    assert_eq!(plan.len(), 1);
}

#[test]
fn fc_spacing_decides_monospace() {
    assert_eq!(qs::fc_spacing_argv("DejaVu Sans Mono"), ["fc-list", ":family=DejaVu Sans Mono", "spacing"]);
    assert_eq!(qs::fc_spacing_argv("Foo-Bar:X")[1], ":family=Foo\\-Bar\\:X");
    assert!(qs::parse_fc_spacing(":spacing=100\n:spacing=100\n"));
    assert!(qs::parse_fc_spacing(":spacing=90\n"));
    assert!(!qs::parse_fc_spacing(":\n:spacing=100\n"), "есть пропорциональное начертание");
    assert!(!qs::parse_fc_spacing(""), "семейство не найдено");
}

/// Хранилище DPI по окружениям и запись значений. GNOME-подобным — множитель
/// текста: из него служба настроек GNOME и считает Xft/DPI (96 × множитель).
#[test]
fn font_dpi_store_per_desktop() {
    use qs::FontDpiStore as S;
    assert_eq!(qs::font_dpi_store("kde", false), Some(S::Kde));
    assert_eq!(qs::font_dpi_store("kde", true), None, "Plasma Wayland: шрифты следуют масштабу экрана");
    assert_eq!(qs::font_dpi_store("xfce", true), Some(S::Xfce));
    assert_eq!(qs::font_dpi_store("mate", false), Some(S::Mate));
    assert_eq!(qs::font_dpi_store("lxqt", false), Some(S::Fontconfig));
    assert_eq!(
        qs::font_dpi_store("cinnamon-mint", false),
        Some(S::TextScaling { schema: "org.cinnamon.desktop.interface".into() })
    );
    for de in ["gnome", "budgie", "pantheon", "cosmic", "deepin", "hyprland", ""] {
        assert_eq!(
            qs::font_dpi_store(de, true),
            Some(S::TextScaling { schema: "org.gnome.desktop.interface".into() }),
            "{de}"
        );
    }
    let v = |argv: Option<Vec<String>>| argv.unwrap().join(" ");
    assert_eq!(
        v(qs::font_dpi_write_argv(&S::Kde, Some(120), "kwriteconfig6")),
        "kwriteconfig6 --file kcmfonts --group General --key forceFontDPI --notify 120"
    );
    assert_eq!(v(qs::font_dpi_write_argv(&S::Kde, None, "kwriteconfig6")).rsplit(' ').next(), Some("0"));
    assert_eq!(
        v(qs::font_dpi_write_argv(&S::Xfce, None, "")),
        "xfconf-query -c xsettings -p /Xft/DPI -n -t int -s -1"
    );
    assert_eq!(v(qs::font_dpi_write_argv(&S::Mate, Some(110), "")), "gsettings set org.mate.font-rendering dpi 110.0");
    let gnome = S::TextScaling { schema: "org.gnome.desktop.interface".into() };
    assert_eq!(
        v(qs::font_dpi_write_argv(&gnome, Some(120), "")),
        "gsettings set org.gnome.desktop.interface text-scaling-factor 1.25"
    );
    assert_eq!(qs::font_dpi_value(&gnome, Some(1000)), "3.00", "схема не пускает выше 3.0");
    assert_eq!(qs::font_dpi_value(&gnome, None), "1.0");
    assert!(qs::font_dpi_write_argv(&S::Fontconfig, Some(120), "").is_none());
    // Чтение обратно.
    assert_eq!(qs::parse_font_dpi(&gnome, "1.25\n"), Some(120));
    assert_eq!(qs::parse_font_dpi(&gnome, "1.0"), None);
    assert_eq!(qs::parse_font_dpi(&S::Xfce, "-1"), None);
    assert_eq!(qs::parse_font_dpi(&S::Kde, ""), None);
    assert_eq!(qs::parse_font_dpi(&S::Kde, "0"), None);
    assert_eq!(qs::parse_font_dpi(&S::Mate, "96.0"), Some(96));
    // Пояснение есть там, где действие не мгновенное или не буквальное.
    assert!(!qs::font_dpi_note(None).is_empty());
    assert!(!qs::font_dpi_note(Some(&gnome)).is_empty());
    assert!(qs::font_dpi_note(Some(&S::Xfce)).is_empty());
}

/// Deepin вне Wayland — сеанс X11, его Rexon не поддерживает: хранилища
/// нет, и подпись говорит «Rexon поддерживает Deepin только на Treeland», а
/// не про сеанс Plasma.
#[test]
fn deepin_x11_font_dpi_is_refused() {
    // Язык — общий на процесс, а сравниваются ответы разных вызовов: без
    // закрепления соседняя сверка переключает язык между ними.
    let _lang = i18n_pin::en();
    assert_eq!(qs::font_dpi_store("deepin", false), None);
    let note = qs::font_dpi_note_for("deepin", None);
    assert_eq!(note, rexon_core::dde::not_treeland_message());
    // У остальных подпись прежняя.
    assert_eq!(qs::font_dpi_note_for("kde", None), qs::font_dpi_note(None));
    let gnome = qs::FontDpiStore::TextScaling { schema: "org.gnome.desktop.interface".into() };
    assert_eq!(qs::font_dpi_note_for("deepin", Some(&gnome)), qs::font_dpi_note(Some(&gnome)));
}

/// Старый GNOME: отрисовка — в схеме службы настроек, те же перечисления,
/// что у новых ключей; пишется и читается, только если ключи есть.
#[test]
fn legacy_gnome_xsettings_rendering() {
    let r = qs::FontRendering {
        antialias: true,
        exclude: None,
        subpixel: "bgr".to_string(),
        hinting: "full".to_string(),
    };
    let argv: Vec<String> = qs::legacy_rendering_argv(&r).into_iter().map(|a| a.join(" ")).collect();
    assert_eq!(
        argv,
        vec![
            "gsettings set org.gnome.settings-daemon.plugins.xsettings antialiasing rgba",
            "gsettings set org.gnome.settings-daemon.plugins.xsettings hinting full",
            "gsettings set org.gnome.settings-daemon.plugins.xsettings rgba-order bgr",
        ]
    );
    assert_eq!(
        qs::legacy_render_probe_argv().join(" "),
        "gsettings writable org.gnome.settings-daemon.plugins.xsettings antialiasing"
    );
    assert!(qs::legacy_render_available("true\n"));
    assert!(!qs::legacy_render_available(""), "ключа нет — gsettings ничего не напечатал");
    let (dialect, reads) = qs::legacy_rendering_read_argv();
    assert_eq!(reads.len(), 3);
    let parts = qs::parse_render_answers(
        dialect,
        &reads.iter().map(|(k, _)| (*k, match k {
            qs::RenderKey::Antialias => "'grayscale'".to_string(),
            qs::RenderKey::HintStyle => "'medium'".to_string(),
            _ => "'rgb'".to_string(),
        })).collect::<Vec<_>>(),
    );
    assert_eq!(parts.antialias, Some(true));
    assert_eq!(parts.subpixel.as_deref(), Some("none"));
    assert_eq!(parts.hinting.as_deref(), Some("medium"));
    // Старые ключи читаются, только когда новые ничего не дали.
    assert!(qs::rendering_needs_legacy(&qs::RenderingParts::default(), true));
    assert!(!qs::rendering_needs_legacy(&qs::RenderingParts::default(), false));
    assert!(!qs::rendering_needs_legacy(&parts, true));
    for de in ["gnome", "budgie", "pantheon"] {
        assert!(qs::legacy_render_is_native(de), "{de}");
    }
    for de in ["kde", "xfce", "cinnamon-mint", "mate", "lxqt", ""] {
        assert!(!qs::legacy_render_is_native(de), "{de}");
    }
}

/// DPI шрифтов в сеансе X11 без своей службы настроек — `Xft.dpi` в своём
/// блоке `~/.Xresources`; LXQt в X11 — ещё и туда.
#[test]
fn font_dpi_xresources() {
    use qs::FontDpiStore as S;
    for de in ["", "i3", "openbox", "hyprland"] {
        assert_eq!(qs::font_dpi_store(de, false), Some(S::Xresources), "{de}");
    }
    // Deepin вне Wayland — отказ, см. `deepin_x11_font_dpi_is_refused`.
    for de in ["gnome", "budgie", "pantheon"] {
        assert_eq!(
            qs::font_dpi_store(de, false),
            Some(S::TextScaling { schema: "org.gnome.desktop.interface".into() }),
            "{de}: своя служба настроек раздаёт Xft/DPI сама"
        );
    }
    assert!(qs::font_dpi_xresources_also("lxqt", false));
    assert!(!qs::font_dpi_xresources_also("lxqt", true));
    assert!(!qs::font_dpi_xresources_also("xfce", false));
    assert_eq!(qs::font_dpi_write_argv(&S::Xresources, Some(120), ""), None, "это файл");

    let user = "! мои ресурсы\nXTerm*faceName: Hack\nXft.dpi: 96\n";
    let text = qs::xresources_with_dpi(Some(user), Some(144)).unwrap();
    assert!(text.starts_with(user.trim_end()), "{text}");
    assert!(text.ends_with("! >>> Rexon: font DPI (managed by Rexon, edits are overwritten)\nXft.dpi: 144\n! <<< Rexon: font DPI\n"), "{text}");
    assert_eq!(qs::xresources_dpi(&text), Some(144), "наш блок последний — он и действует");
    let again = qs::xresources_with_dpi(Some(&text), Some(120)).unwrap();
    assert_eq!(again.matches(">>> Rexon").count(), 1);
    assert_eq!(qs::xresources_dpi(&again), Some(120));
    let off = qs::xresources_with_dpi(Some(&again), None).unwrap();
    assert_eq!(off, user);
    assert_eq!(qs::xresources_dpi(&off), Some(96), "чужое значение остаётся");
    assert_eq!(qs::xresources_with_dpi(None, Some(110)).unwrap().lines().nth(1), Some("Xft.dpi: 110"));
    assert_eq!(qs::xresources_with_dpi(Some("! >>> Rexon: font DPI\nXft.dpi: 1\n"), Some(2)), None, "разорванный блок");
    assert_eq!(qs::xresources_dpi("! Xft.dpi: 200\n"), None, "комментарий");
    assert_eq!(qs::xrdb_merge_argv("/r/x.ad").join(" "), "xrdb -nocpp -merge /r/x.ad");
    let _lang = i18n_pin::en();
    assert!(qs::font_dpi_note(Some(&S::Xresources)).contains("~/.Xresources"));
}

/// «Управление шрифтами»: разбор fc-list, свои/системные, группировка,
/// поиск, проверка файла и место установки.
#[test]
fn font_manager_list_and_files() {
    let dirs = qs::font_user_dirs("/home/u", "/home/u/.local/share");
    assert_eq!(dirs, vec!["/home/u/.local/share/fonts".to_string(), "/home/u/.fonts".to_string()]);
    assert_eq!(qs::fc_list_fonts_argv().join(" "), "fc-list --format %{family[0]}\t%{style[0]}\t%{file}\t%{foundry}\t%{lang}\n");
    let out = "Noto Sans\tBold\t/usr/share/fonts/noto/NotoSans-Bold.ttf\n\
               Noto Sans\tRegular\t/usr/share/fonts/noto/NotoSans-Regular.ttf\n\
               Noto Sans\tThin Italic\t/usr/share/fonts/noto/NotoSans-ThinItalic.ttf\n\
               Noto Sans\tRegular\t/usr/share/fonts/noto/NotoSans-Regular.ttf\n\
               alpha Mono\t\t/home/u/.local/share/fonts/rexon-x/Alpha.otf\n\
               Old\tRegular\t/home/u/.fonts/old.pcf.gz\n\
               broken line\n\
               Evil\tRegular\t/home/u/.local/share/fonts/../../../../usr/share/fonts/x.ttf\n";
    let fonts = qs::parse_fc_list_fonts(out, &dirs);
    assert_eq!(fonts.len(), 6, "повтор снят, битая строка пропущена");
    assert!(fonts.iter().find(|f| f.family == "alpha Mono").unwrap().user);
    assert_eq!(fonts.iter().find(|f| f.family == "alpha Mono").unwrap().style, "Regular");
    assert!(fonts.iter().find(|f| f.family == "Old").unwrap().user);
    assert!(!fonts.iter().find(|f| f.family == "Evil").unwrap().user, "`..` не выводит из каталога");
    assert!(!fonts.iter().find(|f| f.family == "Noto Sans").unwrap().user);
    assert!(!qs::font_file_is_user("/home/u/.local/share/fonts", &dirs), "сам каталог — не файл");
    assert!(!qs::font_file_is_user("/home/u/.local/share/fontsX/a.ttf", &dirs));

    // Переменный шрифт: файл целиком (без имени) и его именованные
    // начертания — лишнего «Regular» у файла курсива нет.
    let var = "Adwaita Sans\t\t/f/AdwaitaSans-Italic.ttf\n\
               Adwaita Sans\tItalic\t/f/AdwaitaSans-Italic.ttf\n\
               Adwaita Sans\tBold Italic\t/f/AdwaitaSans-Italic.ttf\n\
               Adwaita Sans\t\t/f/AdwaitaSans-Regular.ttf\n\
               Adwaita Sans\tRegular\t/f/AdwaitaSans-Regular.ttf\n";
    let styles: Vec<(String, String)> =
        qs::parse_fc_list_fonts(var, &dirs).into_iter().map(|f| (f.style, f.file)).collect();
    assert_eq!(
        styles,
        vec![
            ("Italic".to_string(), "/f/AdwaitaSans-Italic.ttf".to_string()),
            ("Bold Italic".to_string(), "/f/AdwaitaSans-Italic.ttf".to_string()),
            ("Regular".to_string(), "/f/AdwaitaSans-Regular.ttf".to_string()),
        ]
    );

    let groups = qs::group_font_families(&fonts);
    let names: Vec<&str> = groups.iter().map(|g| g.family.as_str()).collect();
    assert_eq!(names, vec!["alpha Mono", "Evil", "Noto Sans", "Old"], "по алфавиту без учёта регистра");
    let noto = groups.iter().find(|g| g.family == "Noto Sans").unwrap();
    let styles: Vec<&str> = noto.styles.iter().map(|f| f.style.as_str()).collect();
    assert_eq!(styles, vec!["Thin Italic", "Regular", "Bold"], "от тонкого к жирному");
    assert!(noto.has_system() && !noto.has_user());
    assert!(qs::font_family_matches("Noto Sans Mono", "sans mono"));
    assert!(!qs::font_family_matches("Noto Sans", "serif"));

    assert!(qs::font_install_ext_ok("/x/A.TTF") && qs::font_install_ext_ok("a.woff2") && qs::font_install_ext_ok("b.pcf.gz"));
    assert!(!qs::font_install_ext_ok("a.zip"));
    assert!(qs::font_install_filter().contains("*.otf"));
    assert_eq!(qs::fc_query_families_argv("/x/a.ttc").join(" "), "fc-query --format %{family[0]}\n /x/a.ttc");
    assert_eq!(qs::parse_fc_query_families("A\nB\nA\n\n"), vec!["A".to_string(), "B".to_string()]);
    assert!(qs::parse_fc_query_families("").is_empty());
    assert_eq!(
        qs::font_install_dest("/home/u/.local/share", "/tmp/dl/My Font.ttf").as_deref(),
        Some("/home/u/.local/share/fonts/My Font.ttf")
    );
    assert_eq!(qs::font_install_dest("/d", "/"), None);
    assert_eq!(qs::fc_cache_argv("/d/fonts").join(" "), "fc-cache -f /d/fonts");
}

/// Подписи «Управления шрифтами»: вопрос перед удалением называет и
/// соседей по файлу (.ttc), системные удалить нельзя — сказано честно.
#[test]
fn font_manager_texts_english() {
    let _lang = i18n_pin::en();
    let (title, text) = qs::font_remove_confirm("Inter", 2, &["Inter".to_string(), "Inter Display".to_string()]);
    assert_eq!(title, "Remove Font");
    assert!(text.starts_with("Remove the font Inter (2 files)?"), "{text}");
    assert!(text.contains("Inter Display"), "{text}");
    let (_, one) = qs::font_remove_confirm("Inter", 1, &["Inter".to_string()]);
    assert!(one.starts_with("Remove the font Inter (1 file)?") && !one.contains("also hold"), "{one}");
    assert!(qs::font_remove_status("Inter", 0, 0).contains("together with that package"));
    assert_eq!(qs::font_remove_status("Inter", 2, 0), "✅ Font removed: Inter");
    assert_eq!(qs::font_manager_summary(3, 1), "3 font families, yours: 1");
    assert_eq!(
        qs::font_install_status(&qs::FontInstallOutcome::Installed(vec!["A".into(), "B".into()])),
        "✅ Font installed: A, B"
    );
    assert!(qs::font_install_status(&qs::FontInstallOutcome::NotAFont("x.txt".into())).contains("x.txt"));
    assert_eq!(
        qs::font_install_status(&qs::FontInstallOutcome::InstalledForAll(vec!["A".into()])),
        "✅ Font installed for all users: A"
    );
    assert!(qs::font_remove_shared_note().contains("password"));
    let keys: Vec<&str> = qs::font_manager_texts().iter().map(|(k, _)| *k).collect();
    for k in [
        "open", "close", "search", "install", "remove", "system", "user", "empty", "pick", "sample", "no_fclist",
        "shared", "install_all_tip", "scope_tip", "copy_shared",
    ] {
        assert!(keys.contains(&k), "{k}");
    }
}

/// «Управление шрифтами»: фильтр «Все / Личные / Системные» и поиск по
/// семейству и начертанию; кегль образца 12–48.
#[test]
fn font_manager_origin_filter_and_sample() {
    let dirs = qs::font_user_dirs("/home/u", "/home/u/.local/share");
    let out = "Inter\tRegular\t/usr/share/fonts/inter/Inter-Regular.otf\n\
               Inter\tItalic\t/home/u/.local/share/fonts/Inter-Italic.otf\n\
               Inter\tBold\t/usr/share/fonts/inter/Inter-Bold.otf\n\
               Mine\tRegular\t/home/u/.fonts/mine.ttf\n\
               Noto Sans\tRegular\t/usr/share/fonts/noto/NotoSans-Regular.ttf\n";
    let groups = qs::group_font_families(&qs::parse_fc_list_fonts(out, &dirs));
    let names = |v: &[qs::FontFamilyGroup]| v.iter().map(|g| g.family.clone()).collect::<Vec<_>>();
    assert_eq!(names(&qs::font_group_view(&groups, "all", "")), vec!["Inter", "Mine", "Noto Sans"]);
    let user = qs::font_group_view(&groups, "user", "");
    assert_eq!(names(&user), vec!["Inter", "Mine"]);
    assert_eq!(user[0].styles.len(), 1, "у смешанного семейства в «Личных» только свои начертания");
    assert_eq!(user[0].styles[0].style, "Italic");
    let system = qs::font_group_view(&groups, "system", "");
    assert_eq!(names(&system), vec!["Inter", "Noto Sans"]);
    assert_eq!(system[0].styles.len(), 2);
    assert_eq!(names(&qs::font_group_view(&groups, "nonsense", "")).len(), 3, "неизвестный фильтр — все");
    let italic = qs::font_group_view(&groups, "all", "INTER italic");
    assert_eq!(names(&italic), vec!["Inter"]);
    assert_eq!(italic[0].styles.iter().map(|f| f.style.as_str()).collect::<Vec<_>>(), vec!["Italic"]);
    assert!(qs::font_group_view(&groups, "system", "mine").is_empty());
    assert_eq!(names(&qs::font_group_view(&groups, "all", "sans")), vec!["Noto Sans"]);
    // «Для всех пользователей» (/usr/local/share/fonts): не свой и не из
    // пакета — удаляется с паролем; соседний каталог — не он.
    let shared = qs::parse_fc_list_fonts(
        "Ad\tRegular\t/usr/local/share/fonts/Ad.ttf\n\
         Ad\tBold\t/usr/local/share/fonts/sub/Ad-Bold.ttf\n\
         Ad\tItalic\t/usr/local/share/fonts2/Ad-Italic.ttf\n\
         Ad\tLight\t/usr/local/share/fonts/../fonts/Ad-Light.ttf\n",
        &dirs,
    );
    assert_eq!(shared.iter().map(|f| f.shared).collect::<Vec<_>>(), vec![true, true, false, false]);
    assert!(shared.iter().all(|f| !f.user));
    let g = &qs::group_font_families(&shared)[0];
    assert!(g.has_shared() && g.has_packaged() && g.has_system() && !g.has_user());
    assert!(groups.iter().all(|g| !g.has_shared()));
    assert_eq!(qs::FONT_ORIGINS, &["all", "user", "system"]);
    let _lang = i18n_pin::en();
    let labels: Vec<String> = qs::font_origin_choices().into_iter().map(|(_, l)| l).collect();
    assert_eq!(labels, vec!["All", "Personal", "System"]);
    assert_eq!(qs::FONT_SAMPLE_SIZE, (12, 48, 16));
    // «Для себя / для всех»: неизвестное значение — для себя (без пароля).
    assert_eq!(qs::FONT_INSTALL_SCOPES, &["user", "all"]);
    let scopes: Vec<String> = qs::font_install_scope_choices(true).into_iter().map(|(_, l)| l).collect();
    assert_eq!(scopes, vec!["Just for me", "For all users"]);
    // NixOS и SteamOS: «для всех» не предлагается (помощник там откажет).
    let only_me: Vec<&str> = qs::font_install_scope_choices(false).into_iter().map(|(id, _)| id).collect();
    assert_eq!(only_me, vec!["user"]);
    use rexon_core::system::PkgFamily;
    assert!(qs::font_shared_supported(PkgFamily::Arch, false));
    assert!(!qs::font_shared_supported(PkgFamily::Nix, false));
    assert!(!qs::font_shared_supported(PkgFamily::Arch, true));
    assert!(qs::font_install_scope_is_all("all"));
    assert!(!qs::font_install_scope_is_all("user") && !qs::font_install_scope_is_all("") && !qs::font_install_scope_is_all("ALL"));
    assert_eq!(qs::font_shared_dest("/home/u/Мой шрифт.ttf").as_deref(), Some("/usr/local/share/fonts/Мой шрифт.ttf"));
    assert_eq!(qs::font_shared_dest("/"), None);
    let texts = qs::font_manager_texts();
    let sample = &texts.iter().find(|(k, _)| *k == "sample_size").unwrap().1;
    assert!(sample.contains("%1"), "кегль подставляет страница: {sample}");
}

/// Выключение шрифта без удаления: наш блок `<rejectfont>` в fonts.conf
/// (fonts-conf(5)), чужое содержимое файла не трогается, пустой блок
/// снимается, файл без остатка — голый (его удаляют), разорванный блок и
/// битый файл — отказ.
#[test]
fn font_manager_disable_rejectfont_block() {
    let files = vec!["/home/u/.fonts/A&B.ttf".to_string(), "/usr/share/fonts/x/X.otf".to_string()];
    let block = qs::fontconfig_reject_block(&files);
    assert_eq!(
        block,
        "  <!-- >>> Rexon: disabled fonts (managed by Rexon, edits are overwritten) -->\n  <selectfont>\n    <rejectfont>\n      <glob>/home/u/.fonts/A&amp;B.ttf</glob>\n      <glob>/usr/share/fonts/x/X.otf</glob>\n    </rejectfont>\n  </selectfont>\n  <!-- <<< Rexon: disabled fonts -->\n"
    );
    assert_eq!(qs::fontconfig_reject_block(&[]), "");
    assert_eq!(qs::fontconfig_reject_block(&["/f/*.ttf".to_string(), "rel.ttf".to_string()]), "", "шаблон и относительный путь не пишутся");
    assert!(qs::font_glob_safe("/usr/share/fonts/a b.ttf"));
    assert!(!qs::font_glob_safe("/f/a?.ttf") && !qs::font_glob_safe("/f/../a.ttf") && !qs::font_glob_safe("a.ttf"));

    // Новый файл.
    let fresh = qs::fontconfig_with_disabled(None, &files).unwrap();
    assert!(fresh.starts_with("<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig"), "{fresh}");
    assert_eq!(qs::fontconfig_disabled_files(&fresh), files);
    // Чужой файл с блоком отрисовки и своим rejectfont: всё чужое на месте.
    let user = "<?xml version=\"1.0\"?>\n<fontconfig>\n  <selectfont><rejectfont><glob>/theirs/*.pcf.gz</glob></rejectfont></selectfont>\n  <match target=\"pattern\"><edit name=\"dpi\" mode=\"assign\"><double>96</double></edit></match>\n</fontconfig>\n";
    let with_render = qs::fontconfig_with_block(Some(user), &qs::fontconfig_block(&qs::FontRendering::default(), None)).unwrap();
    let text = qs::fontconfig_with_disabled(Some(&with_render), &files).unwrap();
    assert!(text.contains("<glob>/theirs/*.pcf.gz</glob>") && text.contains(">>> Rexon: font rendering"), "{text}");
    assert_eq!(qs::fontconfig_disabled_files(&text), files, "чужой rejectfont — не наш");
    assert_eq!(qs::fontconfig_rendering(&text), qs::fontconfig_rendering(&with_render), "отрисовка не задета");
    // Повтор — один блок; переписанная отрисовка не трогает выключенные.
    let again = qs::fontconfig_with_disabled(Some(&text), &files[..1]).unwrap();
    assert_eq!(again.matches(">>> Rexon: disabled fonts").count(), 1);
    assert_eq!(qs::fontconfig_disabled_files(&again), files[..1].to_vec());
    let rerender = qs::fontconfig_with_block(Some(&again), &qs::fontconfig_block(&qs::FontRendering::default(), Some(120))).unwrap();
    assert_eq!(qs::fontconfig_disabled_files(&rerender), files[..1].to_vec());
    // Сброс шрифтов снимает только блок отрисовки.
    let reset = qs::fontconfig_without_block(&rerender).unwrap();
    assert_eq!(qs::fontconfig_disabled_files(&reset), files[..1].to_vec());
    // Всё включено обратно: блок снят, чужое — как было.
    let back = qs::fontconfig_with_disabled(Some(&text), &[]).unwrap();
    assert_eq!(back, with_render);
    assert!(qs::fontconfig_is_bare(&qs::fontconfig_with_disabled(Some(&fresh), &[]).unwrap()), "наш файл без остатка — удалить");
    assert!(qs::fontconfig_is_bare(&qs::fontconfig_with_disabled(None, &[]).unwrap()));
    // Отказы.
    assert_eq!(qs::fontconfig_with_disabled(Some("<fontconfig><oops></fontconfig>"), &files), Err(qs::FontconfigRefusal::Malformed));
    let broken = "<fontconfig>\n  <!-- >>> Rexon: disabled fonts -->\n</fontconfig>\n";
    assert_eq!(qs::fontconfig_with_disabled(Some(broken), &files), Err(qs::FontconfigRefusal::BrokenBlock));
    assert!(qs::fontconfig_disabled_files(broken).is_empty());

    // Какие файлы переключаются и что выйдет.
    let dirs = qs::font_user_dirs("/home/u", "/home/u/.local/share");
    let fonts = qs::parse_fc_list_fonts(
        "Inter\tRegular\t/usr/share/fonts/Inter-Regular.otf\n\
         Inter\tBold\t/usr/share/fonts/Inter-Bold.otf\n\
         Mine\tRegular\t/home/u/.fonts/mine.ttf\n",
        &dirs,
    );
    assert_eq!(qs::font_toggle_files(&fonts, "Inter", ""), vec!["/usr/share/fonts/Inter-Bold.otf", "/usr/share/fonts/Inter-Regular.otf"]);
    assert_eq!(qs::font_toggle_files(&fonts, "Inter", "/usr/share/fonts/Inter-Bold.otf"), vec!["/usr/share/fonts/Inter-Bold.otf"]);
    assert!(qs::font_toggle_files(&fonts, "Nope", "").is_empty());
    let cur = vec!["/a".to_string()];
    assert_eq!(qs::font_disabled_after(&cur, &["/b".to_string(), "/a".to_string()], false), vec!["/a", "/b"]);
    assert_eq!(qs::font_disabled_after(&cur, &["/a".to_string()], true), Vec::<String>::new());
    // Защита: шрифт системы и «все шрифты» не выключаются; одно начертание — можно.
    let all_inter = qs::font_toggle_files(&fonts, "Inter", "");
    assert_eq!(qs::font_disable_refusal(&fonts, &all_inter, &["inter".to_string()]), Some(qs::FontToggleOutcome::InUse("Inter".to_string())));
    assert_eq!(qs::font_disable_refusal(&fonts, &all_inter[..1], &["Inter".to_string()]), None);
    assert_eq!(qs::font_disable_refusal(&fonts, &all_inter, &["Mine".to_string(), String::new()]), None);
    let everything: Vec<String> = fonts.iter().map(|f| f.file.clone()).collect();
    assert_eq!(qs::font_disable_refusal(&fonts, &everything, &[]), Some(qs::FontToggleOutcome::LastFonts));

    let _lang = i18n_pin::en();
    assert!(qs::font_toggle_status(&qs::FontToggleOutcome::Disabled("Inter".into())).starts_with("⏸️ Font disabled: Inter."));
    assert!(qs::font_toggle_status(&qs::FontToggleOutcome::InUse("Inter".into())).contains("system font"));
    assert!(qs::font_toggle_status(&qs::FontToggleOutcome::Refused("/c/fonts.conf".into())).contains("/c/fonts.conf"));
    assert_eq!(qs::font_disable_confirm("Inter", &["Inter".to_string()]), None, "обратимо — без вопроса");
    let (title, text) = qs::font_disable_confirm("Inter", &["Inter".to_string(), "Inter Display".to_string()]).unwrap();
    assert_eq!(title, "Disable Font");
    assert!(text.contains("Inter Display"), "{text}");
}

/// Дубликаты: одно семейство и начертание в разных файлах; ссылка на тот
/// же файл и выключенные — не копии; растровые не спрашиваются.
#[test]
fn font_manager_duplicates() {
    assert_eq!(
        qs::fc_list_scalable_argv().join(" "),
        "fc-list :scalable=true --format %{family[0]}\t%{style[0]}\t%{file}\t%{foundry}\t%{lang}\n"
    );
    let dirs = qs::font_user_dirs("/home/u", "/home/u/.local/share");
    let mut fonts = qs::parse_fc_list_fonts(
        "Goha\tRegular\t/usr/share/fonts/TTF/Goha.ttf\n\
         Goha\tRegular\t/usr/share/fonts/OTF/Goha.otf\n\
         Inter\tBold\t/home/u/.local/share/fonts/Inter-Bold.otf\n\
         Inter\tBold\t/usr/share/fonts/Inter-Bold.otf\n\
         Inter\tRegular\t/usr/share/fonts/Inter-Regular.otf\n\
         Inter\tRegular\t/home/u/.fonts/Inter-Regular.otf\n\
         Link\tRegular\t/home/u/.fonts/link.ttf\n\
         Link\tRegular\t/usr/share/fonts/link.ttf\n\
         Off\tRegular\t/home/u/.fonts/off.ttf\n\
         Off\tRegular\t/usr/share/fonts/off.ttf\n",
        &dirs,
    );
    for f in fonts.iter_mut().filter(|f| f.file == "/home/u/.fonts/off.ttf") {
        f.disabled = true;
    }
    let real = |p: &str| if p == "/home/u/.fonts/link.ttf" { "/usr/share/fonts/link.ttf".to_string() } else { p.to_string() };
    let dups = qs::find_font_duplicates(&fonts, &real);
    let keys: Vec<(String, String)> = dups.iter().map(|d| (d.family.clone(), d.style.clone())).collect();
    assert_eq!(
        keys,
        vec![
            ("Goha".to_string(), "Regular".to_string()),
            ("Inter".to_string(), "Regular".to_string()),
            ("Inter".to_string(), "Bold".to_string()),
        ]
    );
    let inter = &dups[2];
    assert_eq!(inter.copies.iter().map(|c| c.user).collect::<Vec<_>>(), vec![false, true], "системная — первой");
    assert!(dups[0].copies.iter().all(|c| !c.user), "системные дубликаты показываются, удалить нечего");
    let _lang = i18n_pin::en();
    assert_eq!(qs::font_duplicates_summary(3), "3 styles are installed more than once");
    assert!(qs::font_duplicates_summary(0).starts_with("No duplicates"));
}

/// COSMIC: родные программы (libcosmic) читают только `interface_font` и
/// `monospace_font` из `com.system76.CosmicTk/v1` — RON `FontConfig`
/// (family, weight, stretch, style), без кегля. Жирность и наклон из окна
/// выбора туда переносятся; читается запись обратно тем же разбором.
#[test]
fn cosmic_font_config_face() {
    use qs::{FontFace, FontFile, FontFileIo, FontStore};
    assert_eq!(qs::parse_font_face("Fira Sans,11,700,1"), FontFace { weight: 700, italic: true });
    assert_eq!(qs::parse_font_face("Fira Sans,11"), FontFace::default(), "старый ответ окна — без начертания");
    assert_eq!(qs::parse_font_spec("Fira Sans,11,700,1"), ("Fira Sans".to_string(), 11), "кегль читается по-прежнему");
    for (w, name) in [(0, "Normal"), (100, "Thin"), (200, "ExtraLight"), (300, "Light"), (400, "Normal"),
                      (500, "Medium"), (600, "Semibold"), (700, "Bold"), (800, "ExtraBold"), (900, "Black"), (650, "Bold")] {
        assert_eq!(qs::cosmic_weight_name(w), name, "{w}");
    }
    let FontStore::File(file) = qs::font_plan("cosmic", "system").primary else { panic!("файл") };
    assert_eq!(file, FontFile::CosmicTk { key: "interface_font".to_string() });
    let text = file.try_new_text_face(None, "Fira Sans", 11, FontFace { weight: 600, italic: true }).unwrap();
    assert_eq!(text, "// Rexon: font (managed by Rexon)\n(\n    family: \"Fira Sans\",\n    weight: Semibold,\n    stretch: Normal,\n    style: Italic,\n)");
    assert_eq!(file.read_spec(&text), "Fira Sans");
    assert_eq!(qs::cosmic_font_face_fields(&text), ("Semibold".to_string(), "Italic".to_string()));
    // Через план записи — тот же текст в тот же файл.
    struct NoFiles;
    impl FontFileIo for NoFiles {
        fn read(&self, _: &str) -> Option<String> { None }
        fn link_target(&self, _: &str) -> Option<String> { None }
    }
    let w = file.file_writes_face("/c", &NoFiles, "Fira Mono", 12, FontFace { weight: 700, italic: false }).unwrap();
    assert_eq!(w[0].rel_path, "cosmic/com.system76.CosmicTk/v1/interface_font");
    assert_eq!(qs::cosmic_font_face_fields(&w[0].text), ("Bold".to_string(), "Normal".to_string()));
    // LXQt держит начертание полями QFont (жирность, наклон).
    let FontStore::File(lxqt) = qs::font_plan("lxqt", "system").primary else { panic!("файл") };
    assert_eq!(
        lxqt.try_new_text_face(None, "A", 10, FontFace { weight: 700, italic: true }).as_deref(),
        Some("[Qt]\nfont=\"A,10,-1,5,700,1,0,0,0,0,0,0,0,0,0,1\"\n")
    );
}

/// План сброса шрифтов: снимается ровно наше — чужое остаётся, файл без
/// остатка удаляется, ссылки и разорванные руками блоки не трогаются.
#[test]
fn font_layers_reset_plan_takes_only_ours() {
    use qs::FontLayerUndo::{Relink, Remove, Write};
    use std::collections::{BTreeMap, BTreeSet};
    struct Mem {
        files: BTreeMap<String, String>,
        links: BTreeSet<String>,
    }
    impl qs::FontLayerFs for Mem {
        fn read(&self, p: &str) -> Option<String> { self.files.get(p).cloned() }
        fn is_link(&self, p: &str) -> bool { self.links.contains(p) }
    }
    let r = qs::FontRendering::default();
    let fc_foreign = "<?xml version=\"1.0\"?>\n<!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\">\n<fontconfig>\n  <!-- KDE -->\n  <match target=\"font\">\n    <edit name=\"embeddedbitmap\" mode=\"assign\">\n      <bool>false</bool>\n    </edit>\n  </match>\n</fontconfig>\n";
    let fc_ours = qs::fontconfig_with_block(Some(fc_foreign), &qs::fontconfig_block(&r, Some(110))).unwrap();
    let css_ours = qs::text_with_extra_fonts_block(
        Some("@import url(\"file:///usr/share/themes/A%20B/gtk-3.0/gtk.css\");\n"),
        "/* >>> Rexon: fonts (managed by Rexon, edits are overwritten) */\n/* <<< Rexon: fonts */\n",
    )
    .unwrap();
    let mut files = BTreeMap::new();
    files.insert("/c/fontconfig/fonts.conf".to_string(), fc_ours);
    files.insert("/c/gtk-3.0/gtk.css".to_string(), css_ours);
    files.insert("/usr/share/themes/A B/gtk-3.0/gtk.css".to_string(), "x".to_string());
    // Разорванный руками блок — не трогать.
    files.insert("/c/gtk-4.0/gtk.css".to_string(), "/* >>> Rexon: fonts */\nmenu {}\n".to_string());
    files.insert("/h/.Xresources".to_string(), qs::xresources_with_dpi(None, Some(120)).unwrap());
    // Ссылка — не наш файл, даже с меткой внутри.
    files.insert("/c/qt6ct/qss/rexon-fonts.qss".to_string(), "/* >>> Rexon: fonts */".to_string());
    let links: BTreeSet<String> = ["/c/qt6ct/qss/rexon-fonts.qss".to_string()].into();
    // COSMIC без метки — переписан «Параметрами», не наш.
    files.insert("/c/cosmic/com.system76.CosmicTk/v1/monospace_font".to_string(), "(family: \"X\")".to_string());
    files.insert(
        "/c/cosmic/com.system76.CosmicTk/v1/interface_font".to_string(),
        qs::cosmic_font_ron("Fira Sans"),
    );
    let plan = qs::font_layers_reset_plan("/c", "/h", &Mem { files, links });
    assert_eq!(
        plan,
        vec![
            Relink { path: "/c/gtk-3.0/gtk.css".into(), target: "/usr/share/themes/A B/gtk-3.0/gtk.css".into() },
            Write { path: "/c/fontconfig/fonts.conf".into(), text: fc_foreign.into() },
            Remove { path: "/h/.Xresources".into() },
            Remove { path: "/c/cosmic/com.system76.CosmicTk/v1/interface_font".into() },
        ]
    );
    // Пустой fonts.conf — только наш скелет.
    let bare = qs::fontconfig_with_block(None, &qs::fontconfig_block(&r, None)).unwrap();
    assert!(qs::fontconfig_is_bare(&qs::fontconfig_without_block(&bare).unwrap()));
    assert!(!qs::fontconfig_is_bare(fc_foreign));
    assert!(!qs::fontconfig_is_bare("<fontconfig>\n  <!-- моё -->\n</fontconfig>\n"), "комментарий человека — не пусто");
    // Чужая строка gtk.css до и после нашего блока остаётся.
    let mid = qs::text_without_extra_fonts_block(
        "a {}\n/* >>> Rexon: fonts (x) */\nmenu {}\n/* <<< Rexon: fonts */\nb {}\n",
    )
    .unwrap();
    assert_eq!(mid, "a {}\n\nb {}\n");
    assert_eq!(qs::text_without_extra_fonts_block("a {}\n"), None);
    assert_eq!(qs::gtk_css_import_only("@import url(\"colors.css\");"), None, "относительный адрес — не наша ссылка");
}

// ── Шрифт заголовков в LXQt и labwc ──────────────────────────────────────

/// Правка `rc.xml` Openbox и labwc по эталону `golden/rc_xml_title_font.json`:
/// меняются только имя и кегль у активного и неактивного окна, остальное в
/// файле — как было; что записано, то и читается обратно.
#[test]
fn rc_xml_title_font_golden() {
    let golden: Value = serde_json::from_str(include_str!("golden/rc_xml_title_font.json")).unwrap();
    for case in golden["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let wm = match case["wm"].as_str().unwrap() {
            "openbox" => qs::TitleWm::Openbox,
            _ => qs::TitleWm::Labwc,
        };
        let input = case["input"].as_str();
        let family = case["family"].as_str().unwrap();
        let size = case["size"].as_i64().unwrap() as i32;
        if let Some(before) = case["read_before"].as_array() {
            let want = (before[0].as_str().unwrap().to_string(), before[1].as_i64().unwrap() as i32);
            assert_eq!(qs::rc_xml_title_font(input.unwrap()), Some(want), "{name}");
        }
        let got = qs::rc_xml_with_title_font(input, wm, family, size);
        assert_eq!(got.as_deref(), case["expected"].as_str(), "{name}");
        if let Some(text) = got {
            assert_eq!(qs::rc_xml_title_font(&text), Some((family.to_string(), size)), "{name}: читается обратно");
            assert_eq!(qs::rc_xml_with_title_font(Some(&text), wm, family, size).as_deref(), Some(text.as_str()), "{name}: повтор ничего не меняет");
            let file = qs::FontFile::RcXml { wm };
            // Строка чтения несёт и начертание из `<weight>`/`<slant>` файла,
            // которых запись без начертания не трогает.
            let (f, s, face) = qs::parse_font_spec_face(&file.read_spec(&text));
            assert_eq!((f.as_str(), s), (family, size), "{name}");
            assert_eq!(Some(face), qs::rc_xml_title_font_face(&text).map(|t| t.2), "{name}");
        }
    }
}

/// Кто рисует заголовки: LXQt — по `session.conf` (свой раньше системного,
/// X11 и Wayland — разные ключи), голый labwc — сам labwc.
#[test]
fn title_wm_from_lxqt_session_conf() {
    use qs::TitleWm as W;
    let user = "[General]\nwindow_manager=openbox\ncompositor=labwc\n";
    assert_eq!(qs::title_wm_for("lxqt", &[user], false), Some(W::Openbox));
    assert_eq!(qs::title_wm_for("lxqt", &[user], true), Some(W::Labwc));
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\nwindow_manager=/usr/bin/xfwm4 --replace\n"], false), Some(W::Xfwm4));
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\nwindow_manager=kwin_x11\n"], false), Some(W::KWin));
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\ncompositor=kwin_wayland\n"], true), Some(W::KWin));
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\nwindow_manager=marco\n"], false), Some(W::Marco));
    // Свой файл без ключа — действует системный.
    let system = "[General]\nwindow_manager=openbox\n";
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\nleave_confirmation=true\n", system], false), Some(W::Openbox));
    // Не выбран, незнакомый или чужой сеансу — шрифт заголовка не предлагаем.
    assert_eq!(qs::title_wm_for("lxqt", &[], false), None);
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\nwindow_manager=icewm\n"], false), None);
    assert_eq!(qs::title_wm_for("lxqt", &["[General]\ncompositor=Hyprland\n"], true), None);
    assert_eq!(qs::title_wm_for("lxqt", &[user.replace("openbox", "labwc").as_str()], false), None, "labwc на X11 не бывает");
    assert_eq!(qs::title_wm_for("labwc", &[], true), Some(W::Labwc));
    assert_eq!(qs::title_wm_for("sway", &[user], true), Some(W::Sway));
}

/// План шрифта заголовков по менеджеру: Openbox/labwc — `rc.xml` и
/// `--reconfigure` после записи; xfwm4/Marco/KWin — их родные ключи.
#[test]
fn title_font_plan_follows_session_wm() {
    use qs::{FontFile, FontStore, TitleWm as W};
    for (wm, prog, path) in [(W::Openbox, "openbox", "openbox/rc.xml"), (W::Labwc, "labwc", "labwc/rc.xml")] {
        let plan = qs::font_plan_wm("lxqt", "title", Some(wm));
        assert_eq!(plan.primary, FontStore::File(FontFile::RcXml { wm }));
        assert!(plan.also.is_empty() && plan.extra.is_empty() && !plan.gtk_ini);
        assert_eq!(plan.notify, vec![vec![prog.to_string(), "--reconfigure".to_string()]]);
        assert_eq!(FontFile::RcXml { wm }.rel_path(), path);
        assert!(qs::font_target_available_wm("lxqt", "title", Some(wm)));
    }
    assert!(FontFile::RcXml { wm: W::Labwc }.may_create());
    assert!(!FontFile::RcXml { wm: W::Openbox }.may_create(), "свой rc.xml Openbox заменяет системный целиком");
    assert_eq!(qs::font_plan_wm("lxqt", "title", Some(W::Xfwm4)), qs::font_plan("xfce", "title"));
    assert_eq!(qs::font_plan_wm("lxqt", "title", Some(W::Marco)), qs::font_plan("mate", "title"));
    assert_eq!(qs::font_plan_wm("lxqt", "title", Some(W::KWin)), qs::font_plan("kde", "title"));
    assert_eq!(qs::font_plan_wm("labwc", "title", Some(W::Labwc)).primary, FontStore::File(FontFile::RcXml { wm: W::Labwc }));
    // Прочие цели в LXQt от менеджера не зависят.
    assert_eq!(qs::font_plan_wm("lxqt", "system", Some(W::Openbox)), qs::font_plan("lxqt", "system"));
    // Свои окружения менеджером сеанса не подменяются.
    assert_eq!(qs::font_plan_wm("mate", "title", Some(W::Openbox)), qs::font_plan("mate", "title"));
    assert_eq!(
        qs::font_targets_shown_wm("lxqt", Some(W::Openbox)).last(),
        Some(&("title", true))
    );
    assert!(qs::font_plan("kde", "title").notify.is_empty());
}

/// sway: шрифт заголовков — строка `font pango:…` в нашем файле, который
/// подключён управляемым блоком в конфиг sway (механизм акцента).
#[test]
fn sway_title_font_plan_and_pango() {
    use qs::{FontFace, FontFile, FontStore, TitleWm as W};
    assert!(qs::font_target_available_wm("sway", "title", Some(W::Sway)));
    assert_eq!(qs::font_targets_shown_wm("sway", Some(W::Sway)).last(), Some(&("title", true)));
    let plan = qs::font_plan_wm("sway", "title", Some(W::Sway));
    assert_eq!(plan.primary, FontStore::File(FontFile::SwayConf));
    assert!(plan.also.is_empty() && plan.extra.is_empty() && plan.notify.is_empty() && !plan.gtk_ini);
    assert_eq!(qs::font_plan_wm("sway", "system", Some(W::Sway)), qs::font_plan("sway", "system"));
    assert_eq!(FontFile::SwayConf.rel_path(), "sway/rexon-title-font.conf");
    // Одним файлом не пишется: нужен блок в конфиге с проверкой sway.
    struct NoFiles;
    impl qs::FontFileIo for NoFiles {
        fn read(&self, _: &str) -> Option<String> {
            None
        }
        fn link_target(&self, _: &str) -> Option<String> {
            None
        }
    }
    assert!(FontFile::SwayConf.file_writes("/h/.config", &NoFiles, "Noto Sans", 11).is_none());

    // Описание Pango: жирность и наклон словами, обычная — без слова.
    assert_eq!(qs::pango_font_desc("Noto Sans", 11, FontFace::default()), "Noto Sans 11");
    assert_eq!(qs::pango_font_desc("Noto Sans", 11, FontFace { weight: 700, italic: true }), "Noto Sans Bold Italic 11");
    assert_eq!(qs::pango_font_desc("Inter", 9, FontFace { weight: 600, italic: false }), "Inter Semi-Bold 9");
    assert_eq!(qs::pango_font_desc("Inter", 9, FontFace { weight: 400, italic: false }), "Inter 9");
    for (desc, family, size, weight, italic) in [
        ("pango:Noto Sans Bold Italic 11", "Noto Sans", 11, 700, true),
        ("monospace 10", "monospace", 10, 0, false),
        ("\"pango:DejaVu Sans Mono 9.5\"", "DejaVu Sans Mono", 10, 0, false),
        ("pango:Inter Semi-Bold 9", "Inter", 9, 600, false),
        ("Cantarell, Noto Sans 12", "Cantarell", 12, 0, false),
        ("pango:Bold 10", "Bold", 10, 0, false),
        ("Noto Sans", "Noto Sans", 10, 0, false),
    ] {
        let (f, s, face) = qs::parse_pango_font_desc(desc);
        assert_eq!((f.as_str(), s, face.weight, face.italic), (family, size, weight, italic), "{desc}");
    }
    let content = qs::sway_title_font_content("Noto Sans", 11, FontFace { weight: 700, italic: false });
    assert!(content.lines().any(|l| l == "font pango:Noto Sans Bold 11"), "{content}");
    assert_eq!(FontFile::SwayConf.read_spec(&content), "Noto Sans Bold 11");

    // Последняя `font` верхнего уровня; `bar { font … }` — шрифт панели.
    let config = "font pango:Old 8\n# font pango:Commented 20\nbar {\n    font pango:Bar 30\n    colors {\n    }\n}\nfont pango:New Bold 12\nmode \"resize\" {\n    font x 1\n}\n";
    assert_eq!(qs::sway_font_value(config).as_deref(), Some("pango:New Bold 12"));
    assert_eq!(qs::sway_font_value("bar {\n font pango:Bar 30\n}\n"), None);
}

/// Запись и чтение шрифта заголовков sway на подделке: наш файл, блок с
/// `include` в конфиге, `swaymsg font …`; чтение — из блока, иначе из
/// конфига, иначе шрифт sway по умолчанию.
#[test]
fn sway_title_font_apply_writes_block_and_reads_back() {
    use rexon_core::animations::{LookCtx, LookIo};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    struct Fake {
        files: RefCell<BTreeMap<String, String>>,
        ran: RefCell<Vec<Vec<String>>>,
    }
    impl LookIo for Fake {
        fn run(&self, argv: &[String]) -> (bool, String) {
            self.ran.borrow_mut().push(argv.to_vec());
            (true, String::new())
        }
        fn have(&self, tool: &str) -> bool {
            tool == "swaymsg"
        }
        fn read(&self, path: &str) -> Option<String> {
            self.files.borrow().get(path).cloned()
        }
        fn write(&self, path: &str, text: &str) -> bool {
            self.files.borrow_mut().insert(path.to_string(), text.to_string());
            true
        }
        fn list_dir(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
    }
    let ctx = LookCtx::new("sway", "/h", None);
    let config = "/h/.config/sway/config";
    let own = "/h/.config/sway/rexon-title-font.conf";
    let io = Fake { files: RefCell::new(BTreeMap::new()), ran: RefCell::new(Vec::new()) };
    // Конфига нет нигде — шрифт sway по умолчанию.
    assert_eq!(qs::sway_title_font_current(&io, &ctx), qs::SWAY_DEFAULT_FONT);
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "monospace 10");
    io.write(config, "font pango:Old Sans 8\nbar {\n    font pango:Bar 30\n}\n");
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "Old Sans 8");

    let face = qs::FontFace { weight: 700, italic: false };
    qs::sway_title_font_apply(&io, &ctx, "Noto Sans", 11, face).unwrap();
    let text = io.read(config).unwrap();
    assert!(text.starts_with("font pango:Old Sans 8\n"), "чужое не тронуто: {text}");
    assert!(text.contains(">>> Rexon: title bar font"), "{text}");
    assert!(text.lines().any(|l| l == format!("include {own}")), "{text}");
    assert_eq!(io.read(own).as_deref().and_then(qs::sway_font_value).as_deref(), Some("pango:Noto Sans Bold 11"));
    assert!(io.ran.borrow().iter().any(|a| a == &["swaymsg".to_string(), "font pango:Noto Sans Bold 11".to_string()]));
    assert!(!io.ran.borrow().iter().any(|a| a.iter().any(|s| s == "reload")), "без reload: он перезапускает swaybg");
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "Noto Sans Bold 11");

    // Повтор переписывает тот же блок, а не добавляет второй.
    qs::sway_title_font_apply(&io, &ctx, "Inter", 9, qs::FontFace::default()).unwrap();
    let text = io.read(config).unwrap();
    assert_eq!(text.matches(">>> Rexon: title bar font").count(), 1, "{text}");
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "Inter 9");

    // Блок сняли руками — наш файл уже не действует, читается конфиг.
    io.write(config, "font pango:Old Sans 8\n");
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "Old Sans 8");

    // Личного конфига нет — засевается системный, и блок ложится в него.
    let io = Fake { files: RefCell::new(BTreeMap::new()), ran: RefCell::new(Vec::new()) };
    io.write("/etc/sway/config", "font pango:System 10\n");
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "System 10");
    qs::sway_title_font_apply(&io, &ctx, "Noto Sans", 11, qs::FontFace::default()).unwrap();
    let text = io.read(config).unwrap();
    assert!(text.starts_with("font pango:System 10\n") && text.contains(&format!("include {own}")), "{text}");
    assert_eq!(qs::sway_title_font_spec(&io, &ctx), "Noto Sans 11");
}

/// Где кнопки шрифта заголовка нет — честная причина; где есть — молчим.
#[test]
fn title_font_note_is_honest() {
    // Язык — общий на процесс, а проверяются английские слова пояснения:
    // без закрепления соседняя сверка переключает язык на русский.
    let _lang = i18n_pin::en();
    assert_eq!(qs::title_font_note("kde", None), None);
    assert_eq!(qs::title_font_note("lxqt", Some(qs::TitleWm::Openbox)), None);
    assert!(qs::title_font_note("lxqt", None).unwrap().contains("Openbox"));
    assert!(qs::title_font_note("cosmic", None).unwrap().contains("interface font"));
    assert_eq!(qs::title_font_note("sway", Some(qs::TitleWm::Sway)), None);
    for de in ["hyprland", "niri"] {
        assert!(qs::title_font_note(de, None).unwrap().contains("no title bars"), "{de}");
    }
}

/// Шрифт с начертанием в обеих записях: Pango («Sans Bold 10» — gsettings,
/// xfconf) и QFont (kdeglobals, Qt 6 и старая Qt 5). Жирность и наклон
/// держатся отдельно от семейства и переносятся между записями без потерь.
#[test]
fn font_face_between_pango_and_qfont() {
    use qs::FontFace;
    let face = |weight, italic| FontFace { weight, italic };
    for (raw, family, size, f) in [
        ("'Sans Bold 10'", "Sans", 10, face(700, false)),
        ("Sans Italic 10", "Sans", 10, face(0, true)),
        ("Noto Sans Semi-Bold Italic 11", "Noto Sans", 11, face(600, true)),
        ("Ubuntu 11", "Ubuntu", 11, face(0, false)),
        ("Cantarell", "Cantarell", 0, face(0, false)),
        ("Noto Sans,11", "Noto Sans", 11, face(0, false)),
        ("Sans,10,-1,5,700,0,0,0,0,0,0,0,0,0,0,1", "Sans", 10, face(700, false)),
        ("Sans,10,-1,5,400,1,0,0,0,0,0,0,0,0,0,1", "Sans", 10, face(400, true)),
        ("Inter,9,-1,5,400,0,0,0,0,0,0,0,0,0,0,1,SemiBold", "Inter", 9, face(600, false)),
        // Qt 5: жирность по старой шкале (75 жирная, 63 полужирная).
        ("Sans,10,-1,5,75,0,0,0,0,0", "Sans", 10, face(700, false)),
        ("Sans,10,-1,5,63,1,0,0,0,0,Semibold Italic", "Sans", 10, face(600, true)),
        // Старая запись Rexon со словом в поле жирности.
        ("Noto Sans,11,-1,5,Regular,0,0,0,0,0", "Noto Sans", 11, face(400, false)),
    ] {
        assert_eq!(qs::parse_font_spec_face(raw), (family.to_string(), size, f), "{raw}");
    }
    assert_eq!(qs::kde_font_spec_face("Sans", 10, face(700, false)), "Sans,10,-1,5,700,0,0,0,0,0,0,0,0,0,0,1");
    assert_eq!(qs::kde_font_spec_face("Sans", 10, face(0, true)), "Sans,10,-1,5,400,1,0,0,0,0,0,0,0,0,0,1");
    assert_eq!(qs::kde_font_spec_face("Sans", 10, FontFace::default()), qs::kde_font_spec("Sans", 10));
    // Круг Pango → QFont → Pango.
    for desc in ["Sans Bold 10", "Sans Italic 10", "Noto Sans Semi-Bold Italic 11", "Ubuntu 11"] {
        let (family, size, f) = qs::parse_font_spec_face(desc);
        let (family, size, f) = qs::parse_font_spec_face(&qs::kde_font_spec_face(&family, size, f));
        assert_eq!(qs::pango_font_desc(&family, size, f), desc);
    }
    // Запись по хранилищам: KDE — поля QFont, gsettings/xfconf — слова Pango.
    let bold = face(700, true);
    assert_eq!(
        qs::font_write_argv_face("mate", "title", "Sans", 10, bold, "").unwrap().last().unwrap(),
        "Sans Bold Italic 10"
    );
    assert_eq!(
        qs::font_write_argv_face("xfce", "system", "Sans", 10, bold, "").unwrap().last().unwrap(),
        "Sans Bold Italic 10"
    );
    assert_eq!(
        qs::font_write_argv_face("kde", "title", "Sans", 10, bold, "kwriteconfig6").unwrap().last().unwrap(),
        "Sans,10,-1,5,700,1,0,0,0,0,0,0,0,0,0,1"
    );
    assert_eq!(
        qs::font_write_argv_face("mate", "system", "Sans", 10, FontFace::default(), ""),
        qs::font_write_argv("mate", "system", "Sans", 10, "")
    );
}

/// Начертание шрифта по кругу «запись → чтение» в каждом хранилище, куда
/// его пишет вкладка «Оформление»: gsettings MATE, xfconf, kdeglobals,
/// `rc.xml` Openbox и labwc, строка `font` sway. Жирный, наклонный и
/// полужирный возвращаются теми же; обычный после жирного — обычным.
#[test]
fn font_face_round_trip_every_store() {
    use qs::{FontFace, FontFile, TitleWm};
    let faces = [
        ("Bold", FontFace { weight: 700, italic: false }),
        ("Italic", FontFace { weight: 400, italic: true }),
        ("Semi-Bold", FontFace { weight: 600, italic: false }),
        ("Bold Italic", FontFace { weight: 700, italic: true }),
        ("Regular", FontFace { weight: 400, italic: false }),
    ];
    // Обычная жирность читается то как 0 (Pango, rc.xml), то как 400 (QFont).
    let norm = |(f, s, face): (String, i32, FontFace)| (f, s, if face.weight == 0 { 400 } else { face.weight }, face.italic);

    // Команды: значение — последний аргумент; так же его отдают `gsettings
    // get` (в кавычках), `xfconf-query` и `kreadconfig6`.
    for (de, target, quoted) in [
        ("mate", "title", true),
        ("mate", "system", true),
        ("mate", "mono", true),
        ("xfce", "title", false),
        ("xfce", "system", false),
        ("kde", "title", false),
        ("kde", "system", false),
    ] {
        for (name, face) in faces {
            let argv = qs::font_write_argv_face(de, target, "Noto Sans", 11, face, "kwriteconfig6").unwrap();
            let value = argv.last().unwrap();
            let raw = if quoted { format!("'{value}'\n") } else { format!("{value}\n") };
            assert_eq!(
                norm(qs::parse_font_spec_face(&raw)),
                ("Noto Sans".to_string(), 11, face.weight, face.italic),
                "{de}/{target} {name}: {raw}"
            );
        }
    }
    // Слова Pango и поля QFont — те, что пишут сами столы.
    let mate = qs::font_write_argv_face("mate", "title", "Sans", 10, faces[2].1, "").unwrap();
    assert_eq!(mate.last().map(String::as_str), Some("Sans Semi-Bold 10"));
    let kde = qs::font_write_argv_face("kde", "title", "Sans", 10, faces[0].1, "kwriteconfig6").unwrap();
    assert_eq!(kde.last().map(String::as_str), Some("Sans,10,-1,5,700,0,0,0,0,0,0,0,0,0,0,1"));

    // rc.xml: у Openbox жирность только `Bold`/`Normal` — полужирный
    // становится жирным; labwc держит ступени Pango.
    let openbox = "<?xml version=\"1.0\"?>\n<openbox_config>\n  <theme>\n    <name>Clearlooks</name>\n    <font place=\"ActiveWindow\">\n      <name>sans</name>\n      <size>8</size>\n      <weight>Bold</weight>\n      <slant>Normal</slant>\n    </font>\n  </theme>\n  <keyboard/>\n</openbox_config>\n";
    for (wm, start) in [(TitleWm::Openbox, Some(openbox)), (TitleWm::Labwc, None)] {
        let file = FontFile::RcXml { wm };
        let mut text = start.map(str::to_string);
        for (name, face) in faces {
            let written = file.try_new_text_face(text.as_deref(), "Noto Sans", 11, face).unwrap();
            let want = if wm == TitleWm::Openbox && face.weight == 600 { 700 } else { face.weight };
            assert_eq!(
                norm(qs::parse_font_spec_face(&file.read_spec(&written))),
                ("Noto Sans".to_string(), 11, want, face.italic),
                "{wm:?} {name}: {written}"
            );
            // Неактивное окно — тем же начертанием, второй элемент не заводится.
            assert_eq!(written.matches("place=\"InactiveWindow\"").count(), 1, "{written}");
            assert_eq!(written.matches("<weight>").count(), 2, "{written}");
            text = Some(written);
        }
        // Начертание не выбрано — `<weight>`/`<slant>` остаются как были.
        let bold = file.try_new_text_face(text.as_deref(), "Noto Sans", 11, faces[0].1).unwrap();
        let kept = file.try_new_text_face(Some(&bold), "Inter", 9, FontFace::default()).unwrap();
        assert_eq!(norm(qs::parse_font_spec_face(&file.read_spec(&kept))), ("Inter".to_string(), 9, 700, false));
    }
    let labwc = FontFile::RcXml { wm: TitleWm::Labwc }.try_new_text_face(None, "Inter", 9, faces[2].1).unwrap();
    assert!(labwc.contains("<weight>semibold</weight>") && labwc.contains("<slant>normal</slant>"), "{labwc}");
    let ob = FontFile::RcXml { wm: TitleWm::Openbox }.try_new_text_face(Some(openbox), "Inter", 9, faces[1].1).unwrap();
    assert!(ob.contains("<weight>Normal</weight>") && ob.contains("<slant>Italic</slant>"), "{ob}");
    assert!(ob.contains("<name>Clearlooks</name>") && ob.contains("<keyboard/>"), "чужое не тронуто: {ob}");

    // sway: наш файл со строкой `font pango:…`.
    for (name, face) in faces {
        let content = qs::sway_title_font_content("Noto Sans", 11, face);
        assert_eq!(
            norm(qs::parse_font_spec_face(&FontFile::SwayConf.read_spec(&content))),
            ("Noto Sans".to_string(), 11, face.weight, face.italic),
            "sway {name}: {content}"
        );
    }

    // Подпись во вкладке показывает начертание и из записи QFont.
    assert!(qs::font_value_text("Sans,10,-1,5,700,1,0,0,0,0,0,0,0,0,0,1").contains("Sans Bold Italic 10"));
    assert!(qs::font_value_text("'Sans Semi-Bold 10'").contains("Sans Semi-Bold 10"));
}
