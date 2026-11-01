//! «Управление шрифтами»: фильтры списка (производитель, тип файла,
//! письменность), таблица символов по блокам Юникода и свой текст образца
//! (`qt_settings`). `fc-list`/`fc-query` не запускаются: их вывод подставлен
//! строками ниже (снят с настоящего fontconfig 2.17 и сокращён).
//!
//! Итоги сверяются с эталоном `golden/font_library_cases.json`; переснять:
//! `REXON_GOLDEN_WRITE=1 cargo test-core --test font_library`, затем
//! прочитать разницу глазами.

mod i18n_pin;

use rexon_core::qt_settings as qs;
use serde_json::{json, Value};

const GOLDEN: &str = "tests/golden/font_library_cases.json";

/// Вывод `fc-list` в формате [`qs::FC_FONT_FORMAT`].
const FC_LIST: &str = "\
Noto Sans\tRegular\t/usr/share/fonts/noto/NotoSans-Regular.ttf\tGOOG\taa|be|en|ru|uk|el|vi\n\
Noto Sans\tBold\t/usr/share/fonts/noto/NotoSans-Bold.ttf\tGOOG\taa|be|en|ru|uk|el|vi\n\
Noto Sans CJK JP\tRegular\t/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc\tGOOG\tja|ko|zh-tw|en\n\
Noto Sans Gunjala Gondi\tRegular\t/usr/share/fonts/noto/NotoSansGunjalaGondi-Regular.ttf\tGOOG\t\n\
Schumacher Clean\tRegular\t/usr/share/fonts/misc/clB8x10.pcf.gz\tSchumacher\tfj|ho|ia\n\
Nimbus Roman\tRegular\t/usr/share/fonts/urw-base35/NimbusRoman-Regular.t1\tURW \ten|ru\n\
Nimbus Sans\tRegular\t/usr/share/fonts/urw-base35/NimbusSans-Regular.pfb\turw\ten|ru|el\n\
Inter\tRegular\t/home/u/.local/share/fonts/Inter.otf\tRSMS\ten|ru|el|vi\n\
Mine\tRegular\t/home/u/.fonts/mine.woff2\t\ten\n\
Old Thing\tRegular\t/home/u/.fonts/old.TTF\tUKWN\the|ar\n\
Bitmap Fixed\tRegular\t/usr/share/fonts/misc/fixed.bdf\tNONE\ten\n";

/// Вывод пробы рунического письма (`fc-list :charset=16a0`).
const PROBE_RUNIC: &str = "/usr/share/fonts/noto/NotoSans-Regular.ttf\n/usr/share/fonts/misc/clB8x10.pcf.gz\n";

/// Вывод `fc-query` набора символов коллекции из двух лиц.
const CHARSET: &str = "\
Regular\t20-7e a0-ff 401-44f 2116 e000-e002 1f600 20000-20001 d800 zz 110000\n\
Bold\t41-43\n";

fn names(v: &[qs::FontFamilyGroup]) -> Vec<String> {
    v.iter().map(|g| g.family.clone()).collect()
}

fn fonts() -> Vec<qs::InstalledFont> {
    let dirs = qs::font_user_dirs("/home/u", "/home/u/.local/share");
    let mut fonts = qs::parse_fc_list_fonts(FC_LIST, &dirs);
    qs::font_add_probed_script(&mut fonts, "runic", PROBE_RUNIC);
    fonts
}

fn view(filter: Value) -> Vec<String> {
    let groups = qs::group_font_families(&fonts());
    names(&qs::font_group_view_filtered(&groups, &qs::FontListFilter::from_json(&filter.to_string())))
}

fn pairs(v: &[(String, String)]) -> Value {
    v.iter().map(|(id, label)| json!([id, label])).collect()
}

/// Всё, что сверяется с эталоном.
fn cases() -> Value {
    let _lang = i18n_pin::en();
    let fonts = fonts();
    let per_font: Vec<Value> = fonts
        .iter()
        .map(|f| json!({ "family": f.family, "foundry": f.foundry, "type": qs::font_file_type(&f.file), "scripts": f.scripts }))
        .collect();
    let choices = qs::font_filter_choices(&fonts);
    let filters = json!({
        "cyrillic": view(json!({ "script": "cyrillic" })),
        "cyrillic_opentype": view(json!({ "script": "cyrillic", "type": "opentype" })),
        "chinese_simplified": view(json!({ "script": "chinese_simplified" })),
        "runic_probe": view(json!({ "script": "runic" })),
        "other_script": view(json!({ "script": "other" })),
        "foundry_urw_any_case": view(json!({ "foundry": "URW" })),
        "foundry_unknown": view(json!({ "foundry": "unknown" })),
        "type1": view(json!({ "type": "type1" })),
        "collection": view(json!({ "type": "collection" })),
        "personal_cyrillic": view(json!({ "origin": "user", "script": "cyrillic" })),
        "query_and_foundry": view(json!({ "query": "noto bold", "foundry": "goog" })),
        "nothing": view(json!({ "foundry": "GOOG", "type": "pcf" })),
        "garbage_json": view(json!("not an object")),
    });
    let blocks: Vec<Value> = qs::font_char_blocks(&qs::fc_charset_for_style(CHARSET, "regular"))
        .into_iter()
        .map(|b| json!({ "start": b.start, "name": b.name, "count": b.chars.len(), "first": b.chars.first(), "last": b.chars.last() }))
        .collect();
    let bold: Vec<u32> = qs::font_char_blocks(&qs::fc_charset_for_style(CHARSET, "Bold"))
        .into_iter()
        .flat_map(|b| b.chars)
        .collect();
    let unknown_style: Vec<String> =
        qs::font_char_blocks(&qs::fc_charset_for_style(CHARSET, "Italic")).into_iter().map(|b| b.name).collect();
    json!({
        "fonts": per_font,
        "choices": {
            "foundries": pairs(&choices.foundries),
            "file_types": pairs(&choices.file_types),
            "scripts": pairs(&choices.scripts),
        },
        "filters": filters,
        "char_blocks": blocks,
        "char_blocks_bold": bold,
        "char_blocks_unknown_style_falls_back_to_first": unknown_style,
    })
}

#[test]
fn эталон_фильтров_и_символов() {
    let got = cases();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    if std::env::var_os("REXON_GOLDEN_WRITE").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
    }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("эталон есть")).unwrap();
    assert_eq!(got, want, "эталон {GOLDEN} разошёлся");
}

#[test]
fn формат_и_разбор_fc_list() {
    assert_eq!(qs::fc_list_fonts_argv()[2], "%{family[0]}\t%{style[0]}\t%{file}\t%{foundry}\t%{lang}\n");
    assert_eq!(qs::fc_query_fonts_argv("/x/a.ttf"), vec!["fc-query", "--format", qs::FC_FONT_FORMAT, "/x/a.ttf"]);
    assert_eq!(qs::fc_list_char_argv(0x16A0), vec!["fc-list", "--format", "%{file}\n", ":charset=16a0"]);
    assert_eq!(qs::fc_query_charset_argv("/x/a.ttf")[2], "%{style[0]}\t%{charset}\n");
    // Прежний трёхполевой вывод разбирается: производитель «не указан»,
    // письменностей нет.
    let old = qs::parse_fc_list_fonts("A\tRegular\t/usr/share/fonts/a.ttf\n", &[]);
    assert_eq!(old[0].file, "/usr/share/fonts/a.ttf");
    assert_eq!(old[0].foundry, qs::FONT_FOUNDRY_UNKNOWN);
    assert!(old[0].scripts.is_empty());
    // Проба не дописывает повтор и не принимает «other» и чужое.
    let mut f = fonts();
    qs::font_add_probed_script(&mut f, "runic", PROBE_RUNIC);
    qs::font_add_probed_script(&mut f, "other", PROBE_RUNIC);
    qs::font_add_probed_script(&mut f, "klingon", PROBE_RUNIC);
    let noto = f.iter().find(|x| x.file.ends_with("NotoSans-Regular.ttf")).unwrap();
    assert_eq!(noto.scripts.iter().filter(|s| **s == "runic").count(), 1);
    assert_eq!(noto.scripts.last(), Some(&"runic"), "порядок — как в FONT_SCRIPTS");
    // Каждая письменность и каждый тип подписаны.
    let _lang = i18n_pin::en();
    for s in qs::FONT_SCRIPTS {
        assert_ne!(qs::font_script_label(s), *s, "{s}");
    }
    assert_eq!(qs::font_script_label("other"), "Symbols and other");
    for (t, _) in qs::FONT_FILE_TYPES {
        assert_ne!(qs::font_file_type_label(t), *t, "{t}");
    }
}

#[test]
fn блоки_юникода_по_порядку() {
    let blocks = qs::UNICODE_BLOCKS;
    assert_eq!(blocks[0], (0x0000, 0x007F, "Basic Latin"));
    for w in blocks.windows(2) {
        assert!(w[0].1 < w[1].0, "блоки идут по возрастанию и не пересекаются: {:?} {:?}", w[0], w[1]);
    }
    assert!(blocks.iter().all(|(a, b, _)| a <= b && *b <= 0x10FFFF));
    assert!(blocks.iter().any(|(_, _, n)| *n == "Cyrillic"));
    assert!(!blocks.iter().any(|(_, _, n)| n.contains("Surrogates")), "суррогатов в таблице нет");
    assert!(!qs::font_char_printable(0x1F) && !qs::font_char_printable(0x85) && !qs::font_char_printable(0xFFFE));
    assert!(qs::font_char_printable(0x20) && qs::font_char_printable(0x416));
}

#[test]
fn свой_текст_образца() {
    let _lang = i18n_pin::en();
    let default = qs::font_sample_default();
    assert_eq!(default, "The quick brown fox jumps over the lazy dog 0123456789");
    assert_eq!(qs::font_sample_text(None), default);
    assert_eq!(qs::font_sample_text(Some("   ")), default, "пусто — по умолчанию");
    assert_eq!(qs::font_sample_text(Some(" Съешь ещё ")), "Съешь ещё");
    assert_eq!(qs::font_sample_text_to_store("  a\nb\tc  "), "a b c");
    assert_eq!(qs::font_sample_text_to_store(&default), "", "образец по умолчанию не запоминается");
    let long = "ж".repeat(qs::FONT_SAMPLE_TEXT_MAX + 10);
    assert_eq!(qs::font_sample_text_to_store(&long).chars().count(), qs::FONT_SAMPLE_TEXT_MAX);
    assert_eq!(qs::font_sample_text(Some(&long)).chars().count(), qs::FONT_SAMPLE_TEXT_MAX);
    // Образец по умолчанию следует языку интерфейса.
    drop(_lang);
    let _ru = i18n_pin::ru();
    assert_ne!(qs::font_sample_default(), default);
}

#[test]
fn подписи_раздела() {
    let _lang = i18n_pin::en();
    let keys: Vec<&str> = qs::font_manager_texts().iter().map(|(k, _)| *k).collect();
    for k in [
        "filter_foundry_tip",
        "filter_type_tip",
        "filter_script_tip",
        "sample_text_tip",
        "sample_reset",
        "view_sample",
        "view_chars",
        "chars_style_tip",
        "chars_block_tip",
        "chars_none",
        "char_pick",
        "groups_tip",
        "group_new",
        "group_rename",
        "group_remove",
        "group_enable",
        "group_disable",
        "group_export",
        "group_add",
        "group_take",
    ] {
        assert!(keys.contains(&k), "{k}");
    }
}

// ── Свои группы шрифтов (`fontgroups.xml`, общий с KDE) ─────────────────

/// Файл групп в том виде, в каком его пишет KDE (порядок семейств у KDE
/// случайный), плюс то, что бывает в руках: сущности, пустая группа,
/// повтор группы, группа без имени, лишняя вложенность.
const KDE_GROUPS: &str = "\
<groups>
 <group name=\"Работа &amp; учёба\">
  <family>Noto Sans</family>
  <family>Inter</family>
 </group>
 <group name=\"Пусто\"/>
 <group>
  <family>Безымянная</family>
 </group>
 <group name=\"Работа &amp; учёба\">
  <family>Nimbus Roman</family>
  <family>  </family>
  <x><family>Глубже</family></x>
 </group>
 <group name=\"Символы\">
  <family>A &lt;B&gt; &#1046;</family>
 </group>
</groups>
";

fn group_names(groups: &[qs::FontGroup]) -> Vec<String> {
    groups.iter().map(|g| g.name.clone()).collect()
}

fn group_cases() -> Value {
    let _lang = i18n_pin::en();
    let parsed = qs::parse_font_groups(KDE_GROUPS).expect("файл KDE разбирается");
    let families = groups_json(&parsed);
    let xml = qs::font_groups_xml(&parsed);
    assert_eq!(qs::parse_font_groups(&xml).unwrap(), parsed, "запись читается обратно без потерь");

    // Цепочка правок, как их делает человек.
    let mut groups = parsed.clone();
    let mut steps = Vec::new();
    for edit in [
        qs::FontGroupEdit::Create("  Новая\n группа ".into()),
        qs::FontGroupEdit::Create("Новая группа".into()),
        qs::FontGroupEdit::Create("   ".into()),
        qs::FontGroupEdit::Create("Personal".into()),
        qs::FontGroupEdit::Create("Unclassified".into()),
        qs::FontGroupEdit::Add { group: "Новая группа".into(), families: vec!["Inter".into(), "Mine".into()] },
        qs::FontGroupEdit::Add { group: "Новая группа".into(), families: vec!["Inter".into()] },
        qs::FontGroupEdit::Add { group: "Нет такой".into(), families: vec!["Inter".into()] },
        qs::FontGroupEdit::Take { group: "Новая группа".into(), families: vec!["Mine".into()] },
        qs::FontGroupEdit::Take { group: "Новая группа".into(), families: vec!["Mine".into()] },
        qs::FontGroupEdit::Rename { from: "Новая группа".into(), to: "Символы".into() },
        qs::FontGroupEdit::Rename { from: "Новая группа".into(), to: "Мои".into() },
        qs::FontGroupEdit::Remove("Пусто".into()),
        qs::FontGroupEdit::Remove("Пусто".into()),
    ] {
        let (next, outcome) = qs::font_groups_apply(&groups, &edit);
        if !outcome.changed() {
            assert_eq!(next, groups, "отказ не меняет список: {edit:?}");
        }
        steps.push(json!({ "status": qs::font_group_status(&outcome), "changed": outcome.changed(), "groups": group_names(&next) }));
        groups = next;
    }

    // Фильтр «место» со своими группами.
    let all = qs::group_font_families(&fonts());
    let in_view = |origin: &str, extra: Value| {
        let mut f = extra.as_object().cloned().unwrap_or_default();
        f.insert("origin".into(), Value::from(origin));
        names(&qs::font_group_view_in(&all, &qs::FontListFilter::from_json(&Value::Object(f).to_string()), &groups))
    };
    let view = json!({
        "group_mine": in_view("group:Мои", json!({})),
        "group_work": in_view("group:Работа & учёба", json!({})),
        "group_work_cyrillic_query": in_view("group:Работа & учёба", json!({ "query": "noto bold" })),
        "unclassified": in_view("unclassified", json!({})),
        "unclassified_personal_script_mix": in_view("unclassified", json!({ "script": "hebrew" })),
        "unknown_group": in_view("group:Нет такой", json!({})),
        "user_unchanged": in_view("user", json!({})),
    });
    let choices: Vec<Value> = qs::font_origin_choices_with_groups(&groups).into_iter().map(|(id, l, c)| json!([id, l, c])).collect();
    let no_groups: Vec<Value> = qs::font_origin_choices_with_groups(&[]).into_iter().map(|(id, l, c)| json!([id, l, c])).collect();

    let work = groups.iter().find(|g| g.name == "Работа & учёба").unwrap();
    let files = qs::font_group_files(&fonts(), work);
    let exists = |p: &str| p.ends_with("NimbusRoman-Regular.afm") || p.ends_with("b/Dup.pfm");
    let export = qs::font_group_export_entries(
        &[
            "/a/Dup.ttf".to_string(),
            "/b/dup.ttf".to_string(),
            "/c/Dup.ttf".to_string(),
            "/b/Dup.pfb".to_string(),
            "/usr/share/fonts/urw/NimbusRoman-Regular.pfb".to_string(),
            "/a/Dup.ttf".to_string(),
        ],
        &exists,
    );
    let export: Vec<Value> = export.into_iter().map(|(n, p)| json!([n, p])).collect();

    json!({
        "parsed": families,
        "xml": xml,
        "steps": steps,
        "final_xml": qs::font_groups_xml(&groups),
        "view": view,
        "choices": choices,
        "choices_without_groups": no_groups,
        "of_family_inter": qs::font_groups_of_family(&groups, "Inter"),
        "work_files": files,
        "toggle": [
            qs::font_group_toggle_status("Мои", 1, true),
            qs::font_group_toggle_status("Мои", 3, false),
            qs::font_group_empty_status("Пусто"),
        ],
        "disable_confirm": qs::font_group_disable_confirm("Мои", &["Noto Sans CJK KR".to_string()]),
        "disable_confirm_none": qs::font_group_disable_confirm("Мои", &[]),
        "remove_confirm": qs::font_group_remove_confirm("Мои"),
        "export_entries": export,
        "export_names": [
            qs::font_group_export_name("Работа / учёба"),
            qs::font_group_export_name(".скрытая"),
            qs::font_group_export_name("  "),
        ],
        "export_paths": [qs::font_group_export_path("/h/a"), qs::font_group_export_path("/h/a.ZIP")],
        "export_status": [
            qs::font_group_export_status("Мои", "/h/Мои.zip", &Ok(2)),
            qs::font_group_export_status("Мои", "/h/Мои.zip", &Err("Permission denied".to_string())),
        ],
    })
}

fn groups_json(groups: &[qs::FontGroup]) -> Value {
    groups.iter().map(|g| json!({ "name": g.name, "families": g.families })).collect()
}

#[test]
fn эталон_своих_групп() {
    const GROUPS_GOLDEN: &str = "tests/golden/font_groups_cases.json";
    let got = group_cases();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GROUPS_GOLDEN);
    if std::env::var_os("REXON_GOLDEN_WRITE").is_some() {
        std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
    }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("эталон есть")).unwrap();
    assert_eq!(got, want, "эталон {GROUPS_GOLDEN} разошёлся");
}

#[test]
fn файл_групп_не_xml_не_переписывается() {
    assert!(qs::parse_font_groups("<groups><group name=\"a\">").is_err(), "оборванный");
    assert!(qs::parse_font_groups("просто текст").is_err());
    assert!(qs::parse_font_groups("<groups><group name=\"a\"><family>&nbsp;</family></group></groups>").is_err());
    assert_eq!(qs::parse_font_groups("").unwrap(), vec![], "пустой файл — групп нет");
    assert_eq!(qs::parse_font_groups("<groups/>").unwrap(), vec![]);
    assert_eq!(qs::font_groups_xml(&[]), "<groups>\n</groups>\n", "как пишет KDE без групп");
}

#[test]
fn архив_группы_пишется_и_читается() {
    let dir = std::env::temp_dir().join(format!("rexon-font-groups-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("a")).unwrap();
    std::fs::create_dir_all(dir.join("b")).unwrap();
    std::fs::write(dir.join("a/X.ttf"), b"one").unwrap();
    std::fs::write(dir.join("b/X.ttf"), b"two").unwrap();
    let files = vec![
        dir.join("a/X.ttf").to_string_lossy().into_owned(),
        dir.join("b/X.ttf").to_string_lossy().into_owned(),
    ];
    let entries = qs::font_group_export_entries(&files, &|p| std::path::Path::new(p).is_file());
    let dest = dir.join("Мои.zip");
    assert_eq!(qs::write_font_group_zip(&entries, &dest).unwrap(), 2);
    assert!(!dir.join("Мои.zip.part").exists(), "временный файл убран");
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    let mut got = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut f, &mut buf).unwrap();
        got.push((f.name().to_string(), buf));
    }
    assert_eq!(got, vec![("X.ttf".to_string(), "one".to_string()), ("X-2.ttf".to_string(), "two".to_string())]);
    // Дата в архиве — время изменения файла, а не 1980 год.
    let year = zip.by_index(0).unwrap().last_modified().map(|d| d.year()).unwrap_or(0);
    assert!(year >= 2024, "год в архиве: {year}");
    let f = std::fs::File::options().write(true).open(dir.join("a/X.ttf")).unwrap();
    f.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_709_210_096)).unwrap(); // 2024-02-29 12:34:56 UTC
    // ZIP хранит местное время: пояс прогона прибит к UTC.
    std::env::set_var("TZ", "UTC");
    extern "C" {
        fn tzset();
    }
    // SAFETY: tzset (libc) только перечитывает TZ.
    unsafe { tzset() };
    drop(f);
    let dest3 = dir.join("date.zip");
    qs::write_font_group_zip(&entries[..1], &dest3).unwrap();
    let mut z3 = zip::ZipArchive::new(std::fs::File::open(&dest3).unwrap()).unwrap();
    let d = z3.by_index(0).unwrap().last_modified().unwrap();
    assert_eq!((d.year(), d.month(), d.day(), d.hour(), d.minute(), d.second()), (2024, 2, 29, 12, 34, 56));
    // Пропавший файл — ошибка, и под нужным именем ничего не остаётся.
    let bad = vec![("Y.ttf".to_string(), dir.join("нет.ttf").to_string_lossy().into_owned())];
    let dest2 = dir.join("bad.zip");
    assert!(qs::write_font_group_zip(&bad, &dest2).is_err());
    assert!(!dest2.exists() && !dir.join("bad.zip.part").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
