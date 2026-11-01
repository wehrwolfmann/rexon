//! Рендер ролика в mmap-кадр без окна (нужен Vulkan-адаптер; без него тест пропускается с пометкой).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rexon_swf::{Error, GpuContext, MmapSpec, OutputTarget, Settings, Wallpaper};

/// Ролик из тестов Ruffle (`tests/tests/swfs/avm2/morph_shape`, MIT OR Apache-2.0):
/// таймлайн из 24 кадров без кода — анимация видна на любом снимке.
fn swf() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/morph_shape.swf")
}

fn read_frame(frame: &Path, gen_path: &Path, len: usize) -> Option<Vec<u8>> {
    let g = |p: &Path| std::fs::read(p).ok().filter(|b| b.len() >= 8).map(|b| u64::from_le_bytes(b[..8].try_into().unwrap()));
    let before = g(gen_path);
    if before.is_some_and(|v| v & 1 == 1) {
        return None;
    }
    let data = std::fs::read(frame).ok().filter(|d| d.len() >= len)?;
    (g(gen_path) == before).then_some(data)
}

#[test]
fn renders_animation_into_mmap_with_generation() {
    let d = std::env::temp_dir().join(format!("rexon-swf-render-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let (w, h) = (320u32, 180u32);
    let spec = MmapSpec { frame_path: d.join("f.rgba"), generation_path: Some(d.join("f.rgba.gen")), width: w, height: h, file_mode: 0o600 };
    let mut settings = Settings::new(swf());
    settings.max_fps = 24.0;
    let mut wp = match unsafe { Wallpaper::new(GpuContext::new(), &settings, vec![OutputTarget::Mmap(spec.clone())]) } {
        Ok(wp) => wp,
        Err(e @ Error::NoAdapter(_)) => {
            eprintln!("SKIPPED: {e}");
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let len = (w * h * 4) as usize;
    let t0 = Instant::now();
    let mut shots = Vec::new();
    for k in 0..48u64 {
        let r = wp.tick(t0 + Duration::from_millis(k * 42)).unwrap();
        if r.rendered && k % 12 == 6 {
            shots.push(read_frame(&spec.frame_path, spec.generation_path.as_ref().unwrap(), len).expect("целый кадр"));
        }
    }
    // Разрыв как после SIGSTOP/SIGCONT: ролик двигается не больше dt_cap.
    let before = wp.stats().movie_time;
    let r = wp.tick(t0 + Duration::from_secs(10)).unwrap();
    assert_eq!(r.gap.map(|g| g > Duration::from_secs(7)), Some(true));
    assert_eq!(wp.stats().movie_time - before, Duration::from_millis(100));
    // Пауза внутри процесса.
    wp.set_paused(true);
    let frames = wp.stats().frames;
    assert!(!wp.tick(t0 + Duration::from_secs(20)).unwrap().rendered);
    assert_eq!(wp.stats().frames, frames);
    wp.set_paused(false);
    // resize mmap-вывода пересоздаёт файлы под новый размер
    wp.resize(0, 160, 90).unwrap();
    assert!(wp.resize(3, 10, 10).is_err());
    let gen_after_resize = std::fs::read(spec.generation_path.as_ref().unwrap()).unwrap();
    assert_eq!(gen_after_resize, 0u64.to_le_bytes());
    std::thread::sleep(Duration::from_millis(50));
    let r = wp.tick(Instant::now()).unwrap();
    assert!(r.rendered);
    assert_eq!(std::fs::metadata(&spec.frame_path).unwrap().len(), 160 * 90 * 4);

    assert!(shots.len() >= 3, "shots {}", shots.len());
    for s in &shots {
        let opaque_nonblack = s.chunks_exact(4).filter(|p| p[3] == 255 && (p[0] | p[1] | p[2]) != 0).count();
        assert!(opaque_nonblack > len / 8, "кадр не пустой: {opaque_nonblack}");
    }
    let differs = shots.windows(2).filter(|p| p[0] != p[1]).count();
    assert!(differs >= 1, "анимация идёт");
    println!("shots {}, differing pairs {differs}, stats {:?}", shots.len(), (wp.stats().frames, wp.stats().gaps));
    drop(wp);
    std::fs::remove_dir_all(d).unwrap();
}
