//! Guard of the vendored translation engine: the multiplication kernel is chosen BY
//! THE PROCESSOR, not pinned at build time.
//!
//! Why a separate file. `fxtranslate` 0.4.2 hard-codes `-mavx2` in its
//! `build.rs`, and the crate has no CPUID checks anywhere. An engine built this way on
//! x86-64 without AVX2 (Intel before Haswell, AMD before Zen, low-end Celeron/Pentium/
//! Atom) gives SIGILL — and not at startup: a bench measurement showed "model
//! load: 0.031 s" and exit code 132 = 128 + 4 on the FIRST matrix
//! multiplication. For a product this is the worst kind of failure: the program showed its window and
//! crashed on the first description translation.
//!
//! The cure is our vendor patch (`rust/vendor/fxtranslate/patches/`): the shim
//! is compiled four times (avx2, avxvnni, avx512bw, avx512vnni) and the set
//! is chosen ONCE on first use via `is_x86_feature_detected!`.
//! The multi-arch build is switched on by the variable `FXT_GEMM_X86_MULTI=1` from
//! `rust/.cargo/config.toml`, and that is the easiest thing to forget: a single-arch
//! build looks and works fine — right until someone else's processor.
//!
//! ⭐ The guard catches exactly this. `FXT_GEMM_FORCE_SCALAR=1` is understood ONLY by our
//! dispatcher; in a single-arch build the set is pinned at build time and there is
//! nobody to read the variable. So "the kernel switched to scalar" proves
//! that the dispatcher is in the binary, and the proof is machine-independent:
//! it needs neither AVX-512 nor even AVX2.
//!
//! ⚠️ Every check runs as a CHILD process, and this is not overcaution: the set
//! choice is cached by `OnceLock`, i.e. within one process it happens once
//! and for all, and a second environment variant can no longer be checked in it.

/// Marker of a child run (also a ban on running the body by hand).
const РЕБЁНОК: &str = "REXON_FX_ЯДРО_РЕБЁНОК";

/// How the child run reports the name of the chosen kernel.
const МЕТКА: &str = "ЯДРО=";

/// Directory with the Firefox en→ru model for the `#[ignore]` live translation check.
/// The model is NOT put in the tree (its place is the distribution), so the path is set by
/// the environment, and the bench one is taken by default.
///
/// The variable name comes from the core: the live model lookup reads the same one
/// ([`rexon_core::translator_engine::pick_model_live`]). There must be no copy of it
/// here — otherwise the bench and the product would look for the model along different paths.
const МОДЕЛЬ_ENV: &str = rexon_core::translator_engine::MODEL_DIR_ENV;
fn default_model_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/file-verify/mt-bench/models/firefox-enru")
}

/// Ask a child process which kernel the dispatcher chose in the given
/// environment.
fn ядро_ребёнком(доп: &[(&str, &str)]) -> String {
    let mut cmd = std::process::Command::new(std::env::current_exe().expect("путь бинаря"));
    cmd.args([
        "--exact",
        "печать_ядра_ребёнком",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ])
    .env(РЕБЁНОК, "1");
    // So that the run does not depend on the environment of the machine it was started on.
    cmd.env_remove("FXT_GEMM_X86_ARCH");
    cmd.env_remove("FXT_GEMM_FORCE_SCALAR");
    for (имя, значение) in доп {
        cmd.env(имя, значение);
    }
    let out = cmd.output().expect("дочерний прогон");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "дочерний прогон не прошёл (окружение {доп:?}):\n{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // The marker is searched INSIDE the line, not at its start: libtest prints its
    // "test name ... " without a newline, and the check's output lands at its tail.
    stdout
        .lines()
        .find_map(|l| l.split_once(МЕТКА).map(|(_, хвост)| хвост))
        .unwrap_or_else(|| panic!("дочерний прогон не назвал ядро:\n{stdout}"))
        .trim()
        .to_string()
}

/// The probe body runs only in the child process (see the header about `OnceLock`).
#[test]
#[ignore = "run as a child process by ядро_умножения_выбирается_по_cpuid"]
fn печать_ядра_ребёнком() {
    assert!(std::env::var_os(РЕБЁНОК).is_some(), "только дочерний прогон");
    println!("{МЕТКА}{}", fxtranslate::gemm::backend());
}

/// The instruction set is chosen by the processor, and this choice is made at
/// run time, not at build time.
#[cfg(target_arch = "x86_64")]
#[test]
fn ядро_умножения_выбирается_по_cpuid() {
    // 1. The dispatcher IS IN THE BINARY. `FXT_GEMM_FORCE_SCALAR` is
    //    our patch's emergency exit (for support, if on the buyer's
    //    machine the SIMD path misbehaves); a single-arch build does not know this
    //    variable, and the kernel would have stayed SIMD. The check
    //    requires nothing from the processor, so it works on any machine.
    let скаляр = ядро_ребёнком(&[("FXT_GEMM_FORCE_SCALAR", "1")]);
    assert!(
        скаляр.contains("scalar"),
        "аварийный скалярный выход не сработал (ядро «{скаляр}»). \
         Похоже, сборка ОДНОАРОЧНАЯ: потерялась переменная FXT_GEMM_X86_MULTI=1 \
         из rust/.cargo/config.toml. На процессоре без нужного набора это SIGILL \
         после загрузки модели — см. rust/vendor/fxtranslate/ATTRIBUTION.md."
    );

    // 2. By default the best set THIS processor supports is taken.
    let своё = ядро_ребёнком(&[]);
    let ждём = if is_x86_feature_detected!("avx512vnni")
        && is_x86_feature_detected!("avx512bw")
        && is_x86_feature_detected!("avx512vl")
        && is_x86_feature_detected!("avx512dq")
    {
        "avx512vnni"
    } else if is_x86_feature_detected!("avx512bw")
        && is_x86_feature_detected!("avx512vl")
        && is_x86_feature_detected!("avx512dq")
    {
        "avx512bw"
    } else if is_x86_feature_detected!("avxvnni") && is_x86_feature_detected!("avx2") {
        "avxvnni"
    } else if is_x86_feature_detected!("avx2") {
        "avx2"
    } else {
        // We have no processor without AVX2, but the path is described: there will be no table and
        // the engine will fall back to the crate's scalar kernel — a slowdown, not a crash.
        "scalar"
    };
    assert!(
        своё.contains(ждём),
        "диспетчер выбрал «{своё}», а по флагам процессора ждали «{ждём}»"
    );

    // 3. The kernel copies are DIFFERENT and lie side by side: forced AVX2 must give
    //    exactly AVX2, even when the processor can do more. This is also a check
    //    that `objcopy --localize-hidden` did not let the linker merge the copies.
    if is_x86_feature_detected!("avx2") {
        let навязанный = ядро_ребёнком(&[("FXT_GEMM_X86_ARCH", "avx2")]);
        assert_eq!(
            навязанный, "avx2",
            "принудительный AVX2 дал «{навязанный}» — копии наборов не изолированы"
        );
        if ждём != "avx2" {
            assert_ne!(
                навязанный, своё,
                "навязанный набор совпал с выбранным сам — похоже, копия в файле одна"
            );
        }
    }
}

/// Live translation of one en→ru phrase on a MANUALLY DOWNLOADED Firefox model.
///
/// `#[ignore]` on purpose: the model is not and will not be in the tree (43 MB, its place is
/// the distribution, a separate step). To run:
///
/// ```sh
/// cargo test -p rexon-core --release --test fxtranslate_vendor \
///     -- --ignored --nocapture перевод_одной_фразы
/// ```
///
/// The model directory is `REXON_TRANSLATE_MODEL_DIR` (the bench one by default). Needed:
/// `model.enru.intgemm.alphas.bin` and `vocab.enru.spm`; the short list
/// (`lex.*.bin`) is deliberately NOT used: it costs 0.4 chrF++ and 2.8 MB of
/// distribution, and the speedup on dozens of short descriptions is not noticeable.
#[test]
#[ignore = "needs the downloaded Firefox en→ru model (REXON_TRANSLATE_MODEL_DIR), not kept in the tree"]
fn перевод_одной_фразы() {
    let корень = std::env::var(МОДЕЛЬ_ENV).unwrap_or_else(|_| default_model_dir());
    let модель = std::path::Path::new(&корень).join("model.enru.intgemm.alphas.bin");
    let словарь = std::path::Path::new(&корень).join("vocab.enru.spm");
    assert!(
        модель.is_file() && словарь.is_file(),
        "модели нет: {} — задайте каталог через {МОДЕЛЬ_ENV}",
        корень
    );

    let движок = fxtranslate::engine::Engine::load_mmapped(&модель, &словарь, &словарь)
        .expect("загрузка модели")
        .with_threads(1);
    let источник = "This global theme brings a warm, muted palette to the whole desktop.";
    let начало = std::time::Instant::now();
    let перевод = движок.translate(источник);
    println!("ядро умножения : {}", fxtranslate::gemm::backend());
    println!("источник       : {источник}");
    println!("перевод ({:.3} с): {перевод}", начало.elapsed().as_secs_f64());

    assert!(!перевод.trim().is_empty(), "перевод пустой");
    assert!(
        перевод.chars().any(|c| ('а'..='я').contains(&c)),
        "в переводе нет русских букв: {перевод}"
    );
    // ⭐ Damage to the first word is the very defect our patch cures
    // (16.0 % of FLORES-200 lines lost the start of the first word: «ная глобальная
    // тема» instead of «эта глобальная тема»). The first word must be whole.
    let первое = перевод.split_whitespace().next().unwrap_or_default();
    assert!(
        первое.chars().count() > 2,
        "первое слово перевода обрублено («{первое}») — похоже, потерялась \
         починка первого токена из патча вендора"
    );
}
