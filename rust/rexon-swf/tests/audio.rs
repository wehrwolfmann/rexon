//! Звук без устройства (фича `sound`): декодирование, микширование, громкость и тишина.
#![cfg(feature = "sound")]

use std::path::Path;

/// Ролик из тестов Ruffle (`tests/tests/swfs/audio/stream_incomplete_loop`, MIT OR Apache-2.0):
/// потоковый звук, который Ruffle декодирует без mp3/aac.
fn movie() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/stream_incomplete_loop.swf")
}

#[test]
fn ползунок_линеен_как_у_видео_и_ноль_молчит() {
    // ⚠️ Сверяется не «тише/громче», а ПРОПОРЦИЯ. Своя шкала Ruffle
    // логарифмическая: половина ползунка даёт там 0,1 уровня, четверть — 0,025.
    // Проверка «half < loud» такую шкалу пропускала бы зелёной, а человек
    // слышал бы, что Flash на середине ползунка почти молчит, тогда как видео
    // на той же середине играет вполголоса.
    let swf = movie();
    let (loud, peak) = rexon_swf::probe_audio(&swf, 4.0, 1.0).unwrap();
    let (half, _) = rexon_swf::probe_audio(&swf, 4.0, 0.5).unwrap();
    let (quarter, _) = rexon_swf::probe_audio(&swf, 4.0, 0.25).unwrap();
    let (zero, zpeak) = rexon_swf::probe_audio(&swf, 4.0, 0.0).unwrap();
    println!(
        "rms 1.0 → {loud:.5} (пик {peak:.3}), 0.5 → {half:.5} ({:.3} от целого), \
         0.25 → {quarter:.5} ({:.3} от целого), 0.0 → {zero:.5}",
        half / loud,
        quarter / loud
    );
    assert!(loud > 0.1, "ролик обязан звучать на целой громкости");
    assert!(
        (half / loud - 0.5).abs() < 0.02,
        "половина ползунка дала {:.3} уровня вместо 0,5",
        half / loud
    );
    assert!(
        (quarter / loud - 0.25).abs() < 0.02,
        "четверть ползунка дала {:.3} уровня вместо 0,25",
        quarter / loud
    );
    assert_eq!((zero, zpeak), (0.0, 0.0), "ноль обязан быть тишиной, а не «очень тихо»");
}

#[test]
fn missing_movie_is_error() {
    assert!(matches!(rexon_swf::probe_audio(Path::new("/nonexistent.swf"), 1.0, 1.0), Err(rexon_swf::Error::MovieLoad(_))));
}
