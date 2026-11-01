// Compiles the gemmology SIMD shim (src/gemmology_shim.cpp) when the `gemmology`
// feature is on and a kernel is wired for the target. On success it emits
// `--cfg gemmology_simd`; otherwise it falls back to the portable scalar kernel
// *without failing the build*, so the fast default still builds and runs on any
// target. aarch64 uses the i8mm dot-product kernel, x86_64 the AVX2 kernel.
//
// Setting FXTRANSLATE_REQUIRE_SIMD flips that fallback into a hard error: any
// reason we'd otherwise drop to the scalar kernel (unsupported arch, missing
// headers, a failed C++ build, `portable`) becomes a panic. CI sets it so a green
// build proves the SIMD kernel is actually compiled rather than silently absent.
//
// The gemmology/xsimd headers are vendored under vendor/; GEMMOLOGY_DIR /
// XSIMD_INCLUDE_DIR override the include paths.

use std::path::PathBuf;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(gemmology_simd)");
    // `fast_gemm` is the single "a `gemm::PreparedB` fast path exists" switch the
    // engine/weights dispatch keys off. It is set by *either* accelerated backend:
    // the native gemmology FFI kernel (`gemmology_simd`, below) or the pure-Rust
    // wasm SIMD128 kernel (wasm32 target built with `+simd128`). Keeping them under
    // one cfg means the call sites don't fork per-backend.
    println!("cargo::rustc-check-cfg=cfg(fast_gemm)");
    println!("cargo:rerun-if-env-changed=FXTRANSLATE_REQUIRE_SIMD");
    println!("cargo:rerun-if-env-changed=FXT_GEMM_X86_ARCH");
    println!("cargo:rerun-if-env-changed=FXT_GEMM_X86_MULTI");
    println!("cargo::rustc-check-cfg=cfg(fxt_multiarch)");

    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    // The pure-Rust wasm SIMD128 int8 kernel needs no feature and no C++ toolchain:
    // it compiles whenever the wasm32 target is built with `simd128` enabled
    // (`-C target-feature=+simd128`). Detect it here so the plain scalar wasm build
    // (step 5) keeps the scalar path. `portable` still forces scalar everywhere.
    let target_features = std::env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default();
    let wasm_simd128 = arch == "wasm32"
        && target_features.split(',').any(|f| f == "simd128")
        && std::env::var_os("CARGO_FEATURE_PORTABLE").is_none();
    if wasm_simd128 {
        println!("cargo::rustc-cfg=fast_gemm");
    }

    // When required, falling back to scalar is a build failure, not a warning.
    let require_simd = std::env::var_os("FXTRANSLATE_REQUIRE_SIMD").is_some();
    let bail = |reason: String| {
        if require_simd {
            panic!("fxtranslate: FXTRANSLATE_REQUIRE_SIMD is set but {reason}");
        }
        println!("cargo::warning=fxtranslate: {reason} — using the scalar kernel.");
    };

    // Scalar kernel unless the SIMD kernel is both requested and not opted out.
    if std::env::var_os("CARGO_FEATURE_GEMMOLOGY").is_none() {
        return;
    }
    if std::env::var_os("CARGO_FEATURE_PORTABLE").is_some() {
        bail("`portable` is set, which disables the SIMD kernel (no SIMD, no C++)".into());
        return;
    }

    // Pick the arch-specific xsimd kernel + the `-m` flags to compile it with. The
    // define selects the `Arch` in the shim (see gemmology_shim.cpp). Targets not
    // listed here have no wired kernel and fall back to the scalar path.
    let (arch_define, arch_flag) = match arch.as_str() {
        // -march=armv8.4-a+i8mm enables __ARM_FEATURE_MATMUL_INT8 (usdot).
        "aarch64" => ("FXT_GEMM_I8MM", "-march=armv8.4-a+i8mm"),
        // AVX2 baseline: runs on every x86-64-v2+ CPU. The exact VNNI kernels
        // behind CPUID dispatch are a possible follow-up.
        // FXT_GEMM_X86_ARCH (опыт замера): avx2 | avxvnni | avx512bw | avx512vnni
        "x86_64" => match std::env::var("FXT_GEMM_X86_ARCH").unwrap_or_default().as_str() {
            "avxvnni" => ("FXT_GEMM_AVXVNNI", "-mavx2 -mavxvnni"),
            "avx512bw" => ("FXT_GEMM_AVX512BW", "-mavx512f -mavx512cd -mavx512bw -mavx512dq -mavx512vl"),
            "avx512vnni" => (
                "FXT_GEMM_AVX512VNNI",
                "-mavx512f -mavx512cd -mavx512bw -mavx512dq -mavx512vl -mavx512vnni",
            ),
            _ => ("FXT_GEMM_AVX2", "-mavx2"),
        },
        _ => {
            bail(format!("no SIMD kernel wired for `{arch}` yet"));
            return;
        }
    };

    // ── Сборка под несколько наборов инструкций с выбором по CPUID ───────────
    // FXT_GEMM_X86_MULTI=1: шим компилируется по разу на каждый набор из списка,
    // имена функций получают суффикс (FXT_SYM_SUFFIX), а диспетчер в gemm.rs
    // выбирает набор во время работы. Каждая единица трансляции собирается с
    // `-fvisibility=hidden`, после чего objcopy --localize-hidden делает все её
    // прочие символы локальными: иначе компоновщик мог бы слить одинаково
    // названные шаблонные копии из разных наборов и подсунуть сборке под AVX2
    // тело, скомпилированное под AVX-512.
    let multi = arch == "x86_64" && std::env::var_os("FXT_GEMM_X86_MULTI").is_some();
    if multi {
        let vendor = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("vendor");
        let gemmology = std::env::var_os("GEMMOLOGY_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| vendor.join("gemmology"));
        let xsimd = std::env::var_os("XSIMD_INCLUDE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| vendor.join("xsimd/include"));
        if !gemmology.join("gemmology.h").exists() {
            bail("gemmology headers not found".into());
            return;
        }
        println!("cargo:rerun-if-changed=src/gemmology_shim.cpp");
        let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let targets: [(&str, &str, &str); 4] = [
            ("avx2", "FXT_GEMM_AVX2", "-mavx2"),
            ("avxvnni", "FXT_GEMM_AVXVNNI", "-mavx2 -mavxvnni"),
            (
                "avx512bw",
                "FXT_GEMM_AVX512BW",
                "-mavx512f -mavx512cd -mavx512bw -mavx512dq -mavx512vl",
            ),
            (
                "avx512vnni",
                "FXT_GEMM_AVX512VNNI",
                "-mavx512f -mavx512cd -mavx512bw -mavx512dq -mavx512vl -mavx512vnni",
            ),
        ];
        for (name, define, flags) in targets {
            let mut build = cc::Build::new();
            build
                .cpp(true)
                .std("c++17")
                .flags(flags.split(' '))
                .flag("-fvisibility=hidden")
                .flag("-fvisibility-inlines-hidden")
                .define(define, None)
                .define("FXT_SYM_SUFFIX", format!("_{name}").as_str())
                .opt_level(3)
                .include(&gemmology)
                .include(&xsimd)
                .out_dir(out_dir.join(format!("shim-{name}")))
                .file("src/gemmology_shim.cpp");
            if std::env::var_os("CARGO_FEATURE_GEMM_THREADS").is_some() {
                build.define("FXT_GEMM_THREADS", None);
            }
            let objs = match build.try_compile_intermediates() {
                Ok(o) => o,
                Err(e) => {
                    bail(format!("сборка шима под {name} не удалась ({e})"));
                    return;
                }
            };
            // Сделать локальными все скрытые символы: только 7 точек входа с
            // суффиксом остаются глобальными, шаблонные копии — нет.
            for o in &objs {
                let st = std::process::Command::new("objcopy")
                    .arg("--localize-hidden")
                    .arg(o)
                    .status();
                match st {
                    Ok(s) if s.success() => {}
                    _ => {
                        bail(format!("objcopy --localize-hidden не сработал на {}", o.display()));
                        return;
                    }
                }
            }
            // Заархивировать в свою статическую библиотеку и слинковать.
            let lib = out_dir.join(format!("libgemmology_shim_{name}.a"));
            let _ = std::fs::remove_file(&lib);
            let ar = std::env::var("AR").unwrap_or_else(|_| "ar".into());
            let mut cmd = std::process::Command::new(&ar);
            cmd.arg("crs").arg(&lib);
            for o in &objs {
                cmd.arg(o);
            }
            match cmd.status() {
                Ok(s) if s.success() => {}
                _ => {
                    bail(format!("ar не собрал библиотеку под {name}"));
                    return;
                }
            }
            println!("cargo:rustc-link-lib=static=gemmology_shim_{name}");
        }
        println!("cargo:rustc-link-search=native={}", out_dir.display());
        // Стандартная библиотека C++ нужна один раз на всех.
        println!("cargo:rustc-link-lib=dylib=stdc++");
        println!("cargo::rustc-cfg=gemmology_simd");
        println!("cargo::rustc-cfg=fast_gemm");
        println!("cargo::rustc-cfg=fxt_multiarch");
        return;
    }

    let vendor = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("vendor");
    let gemmology = std::env::var_os("GEMMOLOGY_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| vendor.join("gemmology"));
    let xsimd = std::env::var_os("XSIMD_INCLUDE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| vendor.join("xsimd/include"));

    let header = gemmology.join("gemmology.h");
    if !header.exists() {
        bail(format!(
            "gemmology headers not found at {}",
            header.display()
        ));
        return;
    }

    println!("cargo:rerun-if-changed=src/gemmology_shim.cpp");
    println!("cargo:rerun-if-changed={}", header.display());
    println!("cargo:rerun-if-env-changed=GEMMOLOGY_DIR");
    println!("cargo:rerun-if-env-changed=XSIMD_INCLUDE_DIR");

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .flags(arch_flag.split(' '))
        .define(arch_define, None)
        .opt_level(3)
        .include(&gemmology)
        .include(&xsimd)
        .file("src/gemmology_shim.cpp");
    // Intra-op GEMM parallelism (feature `gemm-threads`): compile the persistent
    // pool in the shim. Off otherwise, so the shim has no threading code at all.
    if std::env::var_os("CARGO_FEATURE_GEMM_THREADS").is_some() {
        build.define("FXT_GEMM_THREADS", None);
    }
    let result = build.try_compile("gemmology_shim");

    match result {
        Ok(()) => {
            println!("cargo::rustc-cfg=gemmology_simd");
            // Native FFI kernel is live: the shared "fast PreparedB exists" switch.
            println!("cargo::rustc-cfg=fast_gemm");
        }
        Err(e) => bail(format!("gemmology C++ build failed ({e})")),
    }
}
