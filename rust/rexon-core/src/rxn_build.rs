//! Port of `rexon/scripts/build_rxn_v4.py` — building adaptive wallpapers into
//! an archive of the `.rxn` format, version 4.
//!
//! This holds the whole build process: the layout of files inside the archive,
//! parsing the audio track arguments, the order and content of the checks, the
//! choice of fallback tracks, the manifest text, message texts and exit codes.
//! It is exposed by [`plan_build`] — a ready list of what to put into the
//! archive. Only the input/output itself is left to the executor: checking
//! whether files exist (through [`RxnBuildIo`]), creating the directory and
//! writing the ZIP.

use serde_json::{Map, Value};

/// Resolutions in the order in which they are listed in the archive and in the
/// manifest. The order matters: the choice of fallback video depends on it when
/// there is no `1080p`.
pub const RESOLUTIONS: &[&str] = &["720p", "1080p", "1440p", "2160p", "4320p"];

/// The resolution taken as the fallback if it is present.
pub const PREFERRED_FALLBACK: &str = "1080p";

/// Audio extensions that are known to play. The list does not forbid anything:
/// a file with another extension is still packed, but the executor warns that
/// playback is worth checking by hand.
pub const AUDIO_EXTS: &[&str] = &[
    ".aac", ".flac", ".m4a", ".mp3", ".mp4a", ".oga", ".ogg", ".opus", ".wav",
    ".wma",
];

/// Path of a video inside the archive.
pub fn video_archive_path(resolution: &str) -> String {
    format!("data/video/{resolution}.mp4")
}

/// Path of an audio track inside the archive (the extension is kept — the
/// player figures it out by itself).
pub fn audio_archive_path(lang: &str, ext: &str) -> String {
    format!("data/audio/{lang}{ext}")
}

/// Whether the audio extension is known. Case does not matter.
pub fn audio_ext_known(ext: &str) -> bool {
    let lower = ext.to_lowercase();
    AUDIO_EXTS.contains(&lower.as_str())
}

/// A parsed track argument `LANG=path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioSpec {
    pub lang: String,
    pub path: String,
}

/// Parse `LANG=path`. The FIRST equals sign is taken as the separator — it may
/// legitimately occur in the path.
pub fn parse_audio_spec(spec: &str) -> Option<AudioSpec> {
    let (lang, path) = spec.split_once('=')?;
    Some(AudioSpec {
        lang: lang.to_string(),
        path: path.to_string(),
    })
}

/// Exit codes of the builder — scripts read them, so the values are fixed.
pub const EXIT_VIDEO_NOT_FOUND: i32 = 1;
pub const EXIT_NO_VIDEO: i32 = 2;
pub const EXIT_BAD_AUDIO_SPEC: i32 = 3;
pub const EXIT_AUDIO_NOT_FOUND: i32 = 4;

/// Error messages — the texts are kept verbatim.
pub fn msg_video_not_found(path: &str) -> String {
    format!("❌ video file not found: {path}")
}

/// The "no video specified" message.
pub const MSG_NO_VIDEO: &str = "❌ хотя бы одно --video-XXXX обязательно";

/// Message about a wrong track argument format.
pub fn msg_bad_audio_spec(spec: &str) -> String {
    format!("❌ --audio формат: LANG=path.ext (got: {spec})")
}

/// The "audio file not found" message.
pub fn msg_audio_not_found(path: &str) -> String {
    format!("❌ audio file not found: {path}")
}

/// Warning about an unknown audio extension.
///
/// Warning — FIXED (rule #16). The list of extensions is printed the way the
/// original printed it — in single quotes (the `repr` of a list of strings in
/// Python). The debug output of a Rust slice gives double quotes, and the
/// message diverged from the reference on every unknown extension. The former
/// golden test did not catch this: it did not check the warning text at all.
pub fn msg_unknown_audio_ext(file_name: &str, ext: &str) -> String {
    let listed = AUDIO_EXTS
        .iter()
        .map(|e| format!("'{e}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "⚠️  audio {file_name}: extension {ext} не в списке [{listed}] — упаковываю \
         как есть, проверьте воспроизведение на месте"
    )
}

/// Everything needed for the manifest; paths inside the archive are already laid out.
#[derive(Debug, Clone, Default)]
pub struct ManifestInput {
    pub title: String,
    pub description: String,
    pub author: String,
    /// Whether there is a preview — the file is always put under the name `preview.png`.
    pub has_preview: bool,
    /// Resolution → path in the archive, in the order of [`RESOLUTIONS`].
    pub videos: Vec<(String, String)>,
    /// Language → path in the archive, in the order given on the command line.
    pub audio_tracks: Vec<(String, String)>,
    pub default_locale: String,
    pub audio_volume_default: f64,
}

/// Fallback video: preferably `1080p`, otherwise the first in resolution order.
/// Called only when there is at least one video.
pub fn fallback_video(videos: &[(String, String)]) -> Option<&str> {
    videos
        .iter()
        .find(|(res, _)| res == PREFERRED_FALLBACK)
        .or_else(|| videos.first())
        .map(|(_, path)| path.as_str())
}

/// Fallback track: the default language, otherwise the first one given.
pub fn fallback_audio<'a>(
    audio_tracks: &'a [(String, String)],
    default_locale: &str,
) -> Option<&'a str> {
    audio_tracks
        .iter()
        .find(|(lang, _)| lang == default_locale)
        .or_else(|| audio_tracks.first())
        .map(|(_, path)| path.as_str())
}

/// Volume in the manifest.
///
/// Warning — FIXED (rule #16). On the command line `--volume` is declared as an
/// INTEGER (`type=int`, default 50), so the reference manifest text contains
/// `"audio_volume_default": 50`. An intermediate layer cast the value to a
/// float, and through the bridge the very same call produced `50.0` — the
/// manifest diverged from the reference path byte for byte on EVERY ordinary
/// build. Here an integer value stays an integer and a float stays a float.
fn volume_value(volume: f64) -> Value {
    if volume.is_finite() && volume.fract() == 0.0 && volume.abs() < 9.007_199_254_740_992e15 {
        Value::from(volume as i64)
    } else {
        Value::from(volume)
    }
}

/// The archive manifest.
///
/// Warning: KEY ORDER matters: the manifest is written as indented JSON and
/// compared byte for byte. Keys with a `None` value were dropped in the
/// original — here they are simply not added: that is `preview` when there is
/// no preview, and `fallback_audio` when there are no tracks.
pub fn manifest(input: &ManifestInput) -> Value {
    let mut m = Map::new();
    m.insert("format_version".into(), Value::from(4));
    m.insert("type".into(), Value::from("wallpaper-video-adaptive"));
    m.insert("title".into(), Value::from(input.title.clone()));
    m.insert("description".into(), Value::from(input.description.clone()));
    m.insert("author".into(), Value::from(input.author.clone()));
    if input.has_preview {
        m.insert("preview".into(), Value::from("preview.png"));
    }

    let mut videos = Map::new();
    for (res, path) in &input.videos {
        videos.insert(res.clone(), Value::from(path.clone()));
    }
    m.insert("videos".into(), Value::Object(videos));

    let mut audio = Map::new();
    for (lang, path) in &input.audio_tracks {
        audio.insert(lang.clone(), Value::from(path.clone()));
    }
    m.insert("audio_tracks".into(), Value::Object(audio));

    if let Some(path) = fallback_video(&input.videos) {
        m.insert("fallback_video".into(), Value::from(path));
    }
    if let Some(path) = fallback_audio(&input.audio_tracks, &input.default_locale) {
        m.insert("fallback_audio".into(), Value::from(path));
    }
    m.insert("default_locale".into(), Value::from(input.default_locale.clone()));
    m.insert("audio_volume_default".into(), volume_value(input.audio_volume_default));
    m.insert("loop".into(), Value::from(true));
    m.insert(
        "tags".into(),
        Value::Array(vec![
            Value::from("adaptive"),
            Value::from("multi-language"),
            Value::from("multi-resolution"),
        ]),
    );
    Value::Object(m)
}

/// Text of `manifest.json` — as `json.dumps(..., indent=2, ensure_ascii=False)`:
/// Cyrillic is not escaped, there is no trailing newline.
pub fn manifest_json(input: &ManifestInput) -> String {
    serde_json::to_string_pretty(&manifest(input)).unwrap_or_else(|_| "{}".into())
}

// ── Build process ────────────────────────────────────────────────────────
//
// Below is what used to remain in the Python builder: the order of checks,
// which files go into the archive and in what order, what gets printed along
// the way. Input/output is reduced to a single operation — "does the file
// exist".

/// The only thing the core needs from the file system: `Path(...).is_file()`.
pub trait RxnBuildIo {
    /// `pathlib.Path(path).is_file()`.
    fn is_file(&mut self, path: &str) -> bool;
}

/// Builder arguments — exactly what the command line provides.
#[derive(Debug, Clone, Default)]
pub struct BuildArgs {
    pub title: String,
    pub description: String,
    pub author: String,
    /// Resolution → path to the source file. The order does NOT matter here: in the
    /// archive and in the manifest resolutions follow the [`RESOLUTIONS`] table.
    pub videos: Vec<(String, String)>,
    /// Raw track arguments `LANG=path`, in command-line order.
    pub audio: Vec<String>,
    pub preview: Option<String>,
    pub default_locale: String,
    pub volume: f64,
}

/// A file that goes into the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    /// Path to the source file on disk.
    pub source: String,
    /// Path inside the archive.
    pub archive_path: String,
    /// The line the original printed after adding the file. Empty — it did not
    /// print (that is how the preview behaves).
    pub line: String,
}

/// What to put into the archive and in what order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    /// Text of `manifest.json`. It is written into the archive FIRST, before
    /// [`Self::entries`].
    pub manifest_json: String,
    /// Files: the preview, then videos by the resolution table, then tracks in
    /// command-line order.
    pub entries: Vec<ArchiveEntry>,
    /// Warnings for stderr, in order of appearance.
    pub warnings: Vec<String>,
}

/// Result of argument parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildOutcome {
    /// Everything adds up — packing can proceed.
    Ready(Box<BuildPlan>),
    /// The build will not happen: an exit code and a line for stderr.
    Failed { code: i32, message: String },
}

/// Parse the arguments and build the archive plan.
///
/// The order of checks is preserved verbatim, because it decides WHICH exit
/// code the caller gets when several things are wrong at once: first all videos
/// by the resolution table, then "no video at all", then tracks in command-line
/// order.
pub fn plan_build<I: RxnBuildIo + ?Sized>(args: &BuildArgs, io: &mut I) -> BuildOutcome {
    let mut warnings = Vec::new();
    let mut entries = Vec::new();

    // The preview goes into the archive first and silently — the original did not
    // print anything about it.
    if let Some(preview) = args.preview.as_deref().filter(|p| !p.is_empty()) {
        entries.push(ArchiveEntry {
            source: preview.to_string(),
            archive_path: "preview.png".to_string(),
            line: String::new(),
        });
    }

    // ── Video ──
    let mut videos: Vec<(String, String)> = Vec::new();
    let mut video_entries = Vec::new();
    for res in RESOLUTIONS {
        let Some((_, source)) = args
            .videos
            .iter()
            .find(|(r, s)| r == res && !s.is_empty())
        else {
            continue;
        };
        if !io.is_file(source) {
            return BuildOutcome::Failed {
                code: EXIT_VIDEO_NOT_FOUND,
                message: msg_video_not_found(source),
            };
        }
        videos.push(((*res).to_string(), video_archive_path(res)));
        video_entries.push(ArchiveEntry {
            source: source.clone(),
            // The path is written as a separate expression rather than via
            // video_archive_path: that is how it was in the original, and the match with
            // the manifest is checked by the golden test, not assumed.
            archive_path: format!("data/video/{res}.mp4"),
            line: format!("  + {:<5} {}", res, path_name(source)),
        });
    }
    if videos.is_empty() {
        return BuildOutcome::Failed {
            code: EXIT_NO_VIDEO,
            message: MSG_NO_VIDEO.to_string(),
        };
    }
    entries.extend(video_entries);

    // ── Audio tracks ──
    let mut audio_tracks: Vec<(String, String)> = Vec::new();
    for spec in &args.audio {
        let Some(parsed) = parse_audio_spec(spec) else {
            return BuildOutcome::Failed {
                code: EXIT_BAD_AUDIO_SPEC,
                message: msg_bad_audio_spec(spec),
            };
        };
        if !io.is_file(&parsed.path) {
            return BuildOutcome::Failed {
                code: EXIT_AUDIO_NOT_FOUND,
                message: msg_audio_not_found(&parsed.path),
            };
        }
        let ext = path_suffix(&parsed.path).to_lowercase();
        if !audio_ext_known(&ext) {
            warnings.push(msg_unknown_audio_ext(path_name(&parsed.path), &ext));
        }
        let archive_path = audio_archive_path(&parsed.lang, &ext);
        // Warning — ADDED (rule #16). A language given twice was silently swallowed by
        // the original: the manifest got the last track, while the archive got both,
        // and the extra one stayed as dead weight. We do not make this an error (the
        // archive can still be built), but staying silent about it is not acceptable.
        //
        // The language's position in the manifest does NOT change: a Python dict,
        // when a key is assigned again, updates the value and leaves the key where it
        // first appeared.
        match audio_tracks.iter_mut().find(|(l, _)| *l == parsed.lang) {
            Some(slot) => {
                let previous = slot.1.clone();
                slot.1 = archive_path.clone();
                if previous == archive_path {
                    // Warning — FIXED (rule #16). Not only the language but also the extension
                    // matched — which means the path INSIDE THE ARCHIVE matched too. The original
                    // put BOTH files under that name: the archive came out with two entries of the
                    // same name, unpacking kept the last one, and the first simply vanished — a
                    // silent data loss that the package author only noticed at the buyer's end.
                    // There is no second entry any more: the archive gets the track the manifest
                    // refers to, and we say out loud which one was dropped.
                    warnings.push(msg_overwritten_lang(&parsed.lang, &archive_path));
                    entries.retain(|e| e.archive_path != archive_path);
                } else {
                    warnings.push(msg_duplicate_lang(&parsed.lang, &previous, &archive_path));
                }
            }
            None => audio_tracks.push((parsed.lang.clone(), archive_path.clone())),
        }
        entries.push(ArchiveEntry {
            source: parsed.path.clone(),
            archive_path,
            line: format!("  + audio[{}] {}", parsed.lang, path_name(&parsed.path)),
        });
    }

    let manifest_json = manifest_json(&ManifestInput {
        title: args.title.clone(),
        description: args.description.clone(),
        author: args.author.clone(),
        has_preview: args.preview.as_deref().is_some_and(|p| !p.is_empty()),
        videos,
        audio_tracks,
        default_locale: args.default_locale.clone(),
        audio_volume_default: args.volume,
    });

    BuildOutcome::Ready(Box::new(BuildPlan {
        manifest_json,
        entries,
        warnings,
    }))
}

/// Warning about a language given more than once (the tracks went into the
/// archive under different names — the extensions differed).
pub fn msg_duplicate_lang(lang: &str, previous: &str, current: &str) -> String {
    format!(
        "⚠️  язык {lang} указан не один раз: {previous} останется в архиве \
         мёртвым грузом, манифест сошлётся на {current}"
    )
}

/// Warning about a language given more than once with the same extension: there
/// is one place in the archive, and the previous track will no longer get there.
pub fn msg_overwritten_lang(lang: &str, archive_path: &str) -> String {
    format!(
        "⚠️  язык {lang} указан не один раз с тем же расширением: место \
         {archive_path} в архиве одно — в пакет пойдёт ПОСЛЕДНЯЯ дорожка, \
         предыдущая не попадёт в него вовсе"
    )
}

/// The line before packing starts; `output_name` — the archive file name without a path.
pub fn msg_start(output_name: &str) -> String {
    format!("📦 Сборка {output_name}…")
}

/// The final line: the archive path and its size, as `{MB:.1f}`.
pub fn msg_done(output_path: &str, size_bytes: u64) -> String {
    let mb = size_bytes as f64 / (1024.0 * 1024.0);
    format!("✅ {output_path} ({mb:.1} MB)")
}

/// `pathlib.Path(p).name`: the last part of the path, trailing "/" dropped.
fn path_name(p: &str) -> &str {
    let trimmed = p.trim_end_matches('/');
    if trimmed.is_empty() {
        return "";
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

/// `pathlib.Path(p).suffix`: leading dots of the name are dropped, and
/// everything from the last remaining dot counts as the extension.
///
/// Warning: the rule is taken from Python 3.14 (`name.lstrip('.')` +
/// `rfind('.')`), on which the golden data was captured. Up to 3.13 it was
/// different: a trailing dot did not count as an extension, and `sound.` gave
/// an empty string instead of `.`. The divergence only concerns names without a
/// normal extension; we take the behavior of the version the product runs on.
fn path_suffix(p: &str) -> &str {
    let name = path_name(p).trim_start_matches('.');
    match name.rfind('.') {
        Some(i) => &name[i..],
        None => "",
    }
}
