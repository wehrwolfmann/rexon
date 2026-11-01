//! Ошибки запуска без окна и без GPU: перечисление и коды выхода, никаких panic.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use rexon_swf::{Error, GpuContext, MmapSpec, OutputTarget, Settings, Wallpaper};

/// ⚠️ Проверки идут по очереди, и это не аккуратность ради аккуратности.
/// libtest запускает их параллельно, каждая заводит свой `VkInstance` — и
/// драйвер Vulkan падает по SIGSEGV внутри себя на одновременном создании
/// нескольких инстансов (замер 16.09.2026: `--test-threads=1` — зелено,
/// без него — signal 11 на второй же проверке). Падение libtest выглядит как
/// «Flash сломан», хотя ломается стенд, — поэтому Vulkan здесь берётся под
/// общим замком.
static VULKAN: Mutex<()> = Mutex::new(());

fn vulkan() -> MutexGuard<'static, ()> {
    // Отравленный замок — след ЧУЖОГО упавшего теста; свой ход от этого не
    // портится, и прятать его новым падением незачем.
    VULKAN.lock().unwrap_or_else(|e| e.into_inner())
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rexon-swf-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn mmap_target(dir: &PathBuf, w: u32, h: u32) -> OutputTarget {
    OutputTarget::Mmap(MmapSpec { frame_path: dir.join("f.rgba"), generation_path: Some(dir.join("f.rgba.gen")), width: w, height: h, file_mode: 0o600 })
}

#[test]
fn no_outputs() {
    let _vulkan = vulkan();
    let e = unsafe { Wallpaper::new(GpuContext::new(), &Settings::new("x.swf"), vec![]) }.err().unwrap();
    assert!(matches!(e, Error::NoOutputs));
    assert_eq!(e.exit_code(), 2);
}

#[test]
fn invalid_size_before_anything_else() {
    let _vulkan = vulkan();
    let d = tmp("size");
    for (w, h) in [(0, 10), (10, 0), (20000, 10)] {
        let e = unsafe { Wallpaper::new(GpuContext::new(), &Settings::new("/nonexistent.swf"), vec![mmap_target(&d, w, h)]) }.err().unwrap();
        assert!(matches!(e, Error::InvalidSize { .. }), "{e}");
    }
    assert!(!d.join("f.rgba").exists(), "файл кадра не создаётся при ошибке");
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn missing_and_garbage_movie() {
    let _vulkan = vulkan();
    let d = tmp("movie");
    let e = unsafe { Wallpaper::new(GpuContext::new(), &Settings::new(d.join("missing.swf")), vec![mmap_target(&d, 64, 64)]) }.err().unwrap();
    assert!(matches!(e, Error::MovieLoad(_)), "{e}");
    assert_eq!(e.exit_code(), 8);
    let junk = d.join("junk.swf");
    std::fs::write(&junk, b"definitely not a flash movie").unwrap();
    let e = unsafe { Wallpaper::new(GpuContext::new(), &Settings::new(&junk), vec![mmap_target(&d, 64, 64)]) }.err().unwrap();
    assert!(matches!(e, Error::MovieLoad(_)), "{e}");
    assert!(!d.join("f.rgba").exists(), "ролик проверяется до GPU и до файлов");
    std::fs::remove_dir_all(d).unwrap();
}
