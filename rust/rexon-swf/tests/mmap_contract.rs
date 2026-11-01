//! Договор mmap-кадра и счётчика поколения (без GPU). Читатель ниже написан независимо от писателя
//! и повторяет общий блок чтения `extension.js` Rexon: счётчик до → копия кадра → счётчик после.

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rexon_swf::{MmapSpec, MmapWriter};

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rexon-swf-mmap-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn spec(d: &Path, w: u32, h: u32, with_gen: bool) -> MmapSpec {
    MmapSpec {
        frame_path: d.join("rexon-wp-frame.rgba"),
        generation_path: with_gen.then(|| d.join("rexon-wp-frame.rgba.gen")),
        width: w,
        height: h,
        file_mode: 0o600,
    }
}

/// Независимый читатель (как extension.js): None — кадр не показывать в этот тик.
fn read_whole_frame(frame: &Path, gen_path: &Path, expected: usize) -> Option<Vec<u8>> {
    let generation = |p: &Path| -> Option<u64> {
        let b = std::fs::read(p).ok()?;
        (b.len() >= 8).then(|| u64::from_le_bytes(b[..8].try_into().unwrap()))
    };
    let before = generation(gen_path);
    if before.is_some_and(|g| g & 1 == 1) {
        return None;
    }
    let data = std::fs::read(frame).ok()?;
    if data.len() < expected {
        return None;
    }
    if generation(gen_path) != before {
        return None;
    }
    Some(data)
}

#[test]
fn fresh_files_mode_and_initial_state() {
    let d = tmp("fresh");
    let s = spec(&d, 8, 4, true);
    std::fs::write(&s.frame_path, vec![7u8; 8 * 4 * 4]).unwrap(); // кадр прежних обоев
    let old = std::fs::File::open(&s.frame_path).unwrap();
    let w = MmapWriter::create(&s).unwrap();
    let meta = std::fs::metadata(&s.frame_path).unwrap();
    assert_eq!(meta.len(), 0, "до первого кадра файл нулевой длины — расширение держит свой чёрный фон");
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(s.generation_path.as_ref().unwrap()).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::read(s.generation_path.as_ref().unwrap()).unwrap(), 0u64.to_le_bytes());
    assert_eq!(old.metadata().unwrap().nlink(), 0, "прежний inode отцеплён: старый хост пишет мимо экрана");
    assert_eq!(read_whole_frame(&s.frame_path, s.generation_path.as_ref().unwrap(), w.frame_len()), None);
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn write_frame_bumps_generation_by_two() {
    let d = tmp("write");
    let s = spec(&d, 3, 2, true);
    let g = s.generation_path.clone().unwrap();
    let mut w = MmapWriter::create(&s).unwrap();
    let frame: Vec<u8> = (0..24).collect();
    assert!(w.write_frame(&frame[..23]).is_err(), "неверная длина — ошибка, не panic");
    assert_eq!(w.generation(), 0, "после отказа по длине счётчик не тронут");
    w.write_frame(&frame).unwrap();
    assert_eq!(w.generation(), 2);
    assert_eq!(std::fs::read(&g).unwrap(), 2u64.to_le_bytes());
    assert_eq!(read_whole_frame(&s.frame_path, &g, 24).unwrap(), frame);
    w.write_frame(&frame).unwrap();
    assert_eq!((w.generation(), w.frames()), (4, 2));
    std::fs::remove_dir_all(d).unwrap();
}

#[test]
fn without_generation_file() {
    let d = tmp("nogen");
    let s = spec(&d, 2, 2, false);
    let mut w = MmapWriter::create(&s).unwrap();
    w.write_frame(&[1u8; 16]).unwrap();
    assert!(!d.join("rexon-wp-frame.rgba.gen").exists());
    // читатель без счётчика ведёт себя как прежнее расширение
    assert_eq!(read_whole_frame(&s.frame_path, &d.join("rexon-wp-frame.rgba.gen"), 16).unwrap(), vec![1u8; 16]);
    std::fs::remove_dir_all(d).unwrap();
}

/// Писатель пишет однотонные кадры, читатели копируют без замка.
/// Со счётчиком — ни одного рваного кадра; без счётчика рваные бывают (печатается для сравнения).
#[test]
fn concurrent_reader_never_sees_torn_frame() {
    let d = tmp("race");
    let (w, h) = (512u32, 512u32);
    let s = spec(&d, w, h, true);
    let len = (w * h * 4) as usize;
    let mut writer = MmapWriter::create(&s).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (frame, gen_path) = (s.frame_path.clone(), s.generation_path.clone().unwrap());
    let stop2 = stop.clone();
    let reader = std::thread::spawn(move || {
        let (mut accepted, mut skipped, mut torn, mut raw_torn, mut raw_reads) = (0u32, 0u32, 0u32, 0u32, 0u32);
        while !stop2.load(Ordering::Relaxed) {
            match read_whole_frame(&frame, &gen_path, len) {
                Some(data) => {
                    accepted += 1;
                    if data.iter().any(|b| *b != data[0]) {
                        torn += 1;
                    }
                }
                None => skipped += 1,
            }
            if let Ok(data) = std::fs::read(&frame)
                && data.len() >= len
            {
                raw_reads += 1;
                raw_torn += data.iter().any(|b| *b != data[0]) as u32;
            }
        }
        (accepted, skipped, torn, raw_reads, raw_torn)
    });
    let mut buf = vec![0u8; len];
    // Между кадрами пауза, как у хоста (при 60 к/с — 16 мс; здесь 2 мс, чтобы гонка была частой).
    for i in 0..300u32 {
        buf.fill((i % 251) as u8);
        writer.write_frame(&buf).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    stop.store(true, Ordering::Relaxed);
    let (accepted, skipped, torn, raw_reads, raw_torn) = reader.join().unwrap();
    println!("seqlock reader: accepted {accepted}, skipped {skipped}, torn {torn}; plain reader: {raw_reads} reads, {raw_torn} torn");
    assert_eq!(torn, 0, "кадр, принятый по счётчику, всегда целый");
    assert!(accepted > 0, "читатель хоть что-то принял");
    assert_eq!(writer.generation(), 600);
    std::fs::remove_dir_all(d).unwrap();
}
