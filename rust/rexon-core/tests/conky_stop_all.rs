//! «Остановить все» на настоящих процессах: подставной conky
//! (`tools/de_matrix_stand/fake_conky.c`) запускается дважды — виджетом
//! Rexon и «чужим» из `~/.conky` в частном домашнем каталоге. Оба видны
//! списком и гасятся по номерам. Чужие процессы машины (виджет человека)
//! в проверку не попадают: гасится только то, что запустила сама проверка.

use rexon_core::animations::SystemIo;
use rexon_core::conky_procs;
use rexon_core::de_apply::Ctx;

fn build_fake(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/de_matrix_stand/fake_conky.c");
    let bin_dir = dir.join("bin");
    std::fs::create_dir_all(&bin_dir).ok()?;
    let bin = bin_dir.join("conky");
    let ok = std::process::Command::new("cc").arg("-O2").arg("-o").arg(&bin).arg(&src).status().ok()?.success();
    ok.then_some(bin)
}

#[test]
fn stop_all_stops_own_and_foreign_widgets_by_pid() {
    let dir = std::env::temp_dir().join(format!("rexon-conky-stopall-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let home = dir.join("home");
    let Some(fake) = build_fake(&dir) else {
        eprintln!("НЕ ПРОВЕРЕНО: нет компилятора C для подставного conky");
        return;
    };
    let own = home.join(".config/conky/Mine/conky.conf");
    let foreign = home.join(".conky/Gotham/Gotham");
    for f in [&own, &foreign] {
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, "conky.text = [[x]]\n").unwrap();
    }
    // Без `-d`: номер процесса — номер ребёнка, его и ждём.
    let mut kids: Vec<std::process::Child> = [&own, &foreign]
        .iter()
        .map(|c| std::process::Command::new(&fake).arg("-c").arg(c).spawn().expect("подставной conky"))
        .collect();
    let ours: Vec<String> = kids.iter().map(|k| k.id().to_string()).collect();
    std::thread::sleep(std::time::Duration::from_millis(200));

    let ctx = Ctx { home: home.to_string_lossy().into_owned(), xdg_config_home: None, wayland: false, de: String::new() };
    let listed: Vec<conky_procs::ConkyProc> =
        conky_procs::all(&SystemIo).into_iter().filter(|p| ours.contains(&p.pid)).collect();
    assert_eq!(listed.len(), 2, "оба подставных conky видны: {listed:?}");
    rexon_core::i18n::set_language(Some("en"));
    let (_, question) = conky_procs::stop_all_question(&ctx, &[], &listed).unwrap();
    assert!(question.contains("• Mine") && question.contains("• Gotham (not from Rexon)"), "{question}");

    let pids: Vec<String> = listed.iter().map(|p| p.pid.clone()).collect();
    let (ok, msg) = conky_procs::stop_all(&SystemIo, &pids);
    assert!(ok, "{msg}");
    for k in &mut kids {
        let st = k.wait().unwrap();
        assert!(!st.success(), "процесс погашен сигналом, а не вышел сам");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
