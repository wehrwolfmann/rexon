//! Sound without a device (feature `sound`): decoding, mixing, volume and silence.
#![cfg(feature = "sound")]

use std::path::Path;

/// A movie from the Ruffle tests (`tests/tests/swfs/audio/stream_incomplete_loop`, MIT OR Apache-2.0):
/// streaming sound that Ruffle decodes without mp3/aac.
fn movie() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/stream_incomplete_loop.swf")
}

#[test]
fn slider_linear_like_video_and_zero_is_silent() {
    // ⚠️ What is checked is not "quieter/louder" but the PROPORTION. Ruffle's own scale
    // is logarithmic: half the slider gives 0.1 of the level there, a quarter — 0.025.
    // A "half < loud" check would let such a scale pass green, while the person
    // would hear Flash almost silent in the middle of the slider, whereas video
    // at the same middle plays at half volume.
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
