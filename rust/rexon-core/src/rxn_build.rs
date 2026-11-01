//! Порт `rexon/scripts/build_rxn_v4.py` — сборки adaptive-обоев в архив
//! формата `.rxn` версии 4.
//!
//! Здесь — весь ход сборки: раскладка файлов внутри архива, разбор указаний
//! на звуковые дорожки, порядок и содержание проверок, выбор запасных
//! дорожек, текст манифеста, тексты сообщений и коды возврата. Наружу это
//! отдаётся [`plan_build`] — готовым перечнем того, что положить в архив.
//! Исполнителю остаются только сам ввод-вывод: проверка существования
//! файлов (через [`RxnBuildIo`]), создание каталога и запись ZIP.

use serde_json::{Map, Value};

/// Разрешения в порядке, в котором они перечисляются в архиве и в манифесте.
/// Порядок значим: от него зависит выбор запасного видео, когда `1080p` нет.
pub const RESOLUTIONS: &[&str] = &["720p", "1080p", "1440p", "2160p", "4320p"];

/// Разрешение, которое берётся запасным, если оно есть.
pub const PREFERRED_FALLBACK: &str = "1080p";

/// Расширения звука, которые заведомо проигрываются. Список не запрещающий:
/// файл с другим расширением всё равно упаковывается, но исполнитель
/// предупреждает — проверить воспроизведение стоит вручную.
pub const AUDIO_EXTS: &[&str] = &[
    ".aac", ".flac", ".m4a", ".mp3", ".mp4a", ".oga", ".ogg", ".opus", ".wav",
    ".wma",
];

/// Путь видео внутри архива.
pub fn video_archive_path(resolution: &str) -> String {
    format!("data/video/{resolution}.mp4")
}

/// Путь звуковой дорожки внутри архива (расширение сохраняется — проигрыватель
/// разбирается сам).
pub fn audio_archive_path(lang: &str, ext: &str) -> String {
    format!("data/audio/{lang}{ext}")
}

/// Известное ли расширение звука. Регистр не важен.
pub fn audio_ext_known(ext: &str) -> bool {
    let lower = ext.to_lowercase();
    AUDIO_EXTS.contains(&lower.as_str())
}

/// Разобранное указание дорожки `ЯЗЫК=путь`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioSpec {
    pub lang: String,
    pub path: String,
}

/// Разобрать `ЯЗЫК=путь`. Разделителем считается ПЕРВЫЙ знак равенства —
/// в пути он встречается законно.
pub fn parse_audio_spec(spec: &str) -> Option<AudioSpec> {
    let (lang, path) = spec.split_once('=')?;
    Some(AudioSpec {
        lang: lang.to_string(),
        path: path.to_string(),
    })
}

/// Коды возврата сборщика — их читают скрипты, поэтому значения закреплены.
pub const EXIT_VIDEO_NOT_FOUND: i32 = 1;
pub const EXIT_NO_VIDEO: i32 = 2;
pub const EXIT_BAD_AUDIO_SPEC: i32 = 3;
pub const EXIT_AUDIO_NOT_FOUND: i32 = 4;

/// Сообщения об ошибках — тексты сохранены дословно.
pub fn msg_video_not_found(path: &str) -> String {
    format!("❌ video file not found: {path}")
}

/// Сообщение «ни одного видео не задано».
pub const MSG_NO_VIDEO: &str = "❌ хотя бы одно --video-XXXX обязательно";

/// Сообщение о неверном формате указания дорожки.
pub fn msg_bad_audio_spec(spec: &str) -> String {
    format!("❌ --audio формат: LANG=path.ext (got: {spec})")
}

/// Сообщение «звуковой файл не найден».
pub fn msg_audio_not_found(path: &str) -> String {
    format!("❌ audio file not found: {path}")
}

/// Предупреждение о незнакомом расширении звука.
///
/// ⚠️ ПОЧИНЕНО (правило #16). Список расширений печатается так, как его
/// печатал оригинал, — в одинарных кавычках (`repr` списка строк в Python).
/// Отладочный вывод среза Rust даёт двойные, и сообщение расходилось с
/// эталоном на каждом незнакомом расширении. Прежний голден этого не ловил:
/// текст предупреждения он не сверял вовсе.
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

/// Всё, что нужно для манифеста; пути внутри архива уже разложены.
#[derive(Debug, Clone, Default)]
pub struct ManifestInput {
    pub title: String,
    pub description: String,
    pub author: String,
    /// Есть ли превью — файл всегда кладётся под именем `preview.png`.
    pub has_preview: bool,
    /// Разрешение → путь в архиве, в порядке [`RESOLUTIONS`].
    pub videos: Vec<(String, String)>,
    /// Язык → путь в архиве, в порядке указания в командной строке.
    pub audio_tracks: Vec<(String, String)>,
    pub default_locale: String,
    pub audio_volume_default: f64,
}

/// Запасное видео: предпочтительно `1080p`, иначе первое по порядку
/// разрешений. Вызывается только когда хотя бы одно видео есть.
pub fn fallback_video(videos: &[(String, String)]) -> Option<&str> {
    videos
        .iter()
        .find(|(res, _)| res == PREFERRED_FALLBACK)
        .or_else(|| videos.first())
        .map(|(_, path)| path.as_str())
}

/// Запасная дорожка: язык по умолчанию, иначе первая указанная.
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

/// Громкость в манифесте.
///
/// ⚠️ ПОЧИНЕНО (правило #16). В командной строке `--volume` объявлена ЦЕЛЫМ
/// (`type=int`, по умолчанию 50), поэтому эталонный текст манифеста содержит
/// `"audio_volume_default": 50`. Промежуточный слой приводил значение к
/// дробному, и через мост тот же самый вызов давал `50.0` — манифест
/// расходился с эталонным путём побайтно на КАЖДОЙ обычной сборке. Здесь
/// целое значение остаётся целым, дробное — дробным.
fn volume_value(volume: f64) -> Value {
    if volume.is_finite() && volume.fract() == 0.0 && volume.abs() < 9.007_199_254_740_992e15 {
        Value::from(volume as i64)
    } else {
        Value::from(volume)
    }
}

/// Манифест архива.
///
/// ⚠️ ПОРЯДОК КЛЮЧЕЙ значим: манифест пишется JSON-ом с отступами и
/// сравнивается побайтно. Ключи со значением `None` в оригинале
/// выбрасывались — здесь они просто не добавляются: это `preview`, когда
/// превью нет, и `fallback_audio`, когда дорожек нет.
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

/// Текст `manifest.json` — как `json.dumps(..., indent=2, ensure_ascii=False)`:
/// кириллица не экранируется, перевода строки в конце нет.
pub fn manifest_json(input: &ManifestInput) -> String {
    serde_json::to_string_pretty(&manifest(input)).unwrap_or_else(|_| "{}".into())
}

// ── Ход сборки ──────────────────────────────────────────────────────────
//
// Ниже — то, что раньше оставалось в сборщике на Python: порядок проверок,
// какие файлы и в каком порядке попадают в архив, что при этом печатается.
// Ввод-вывод сведён к единственной операции — «существует ли файл».

/// Единственное, что ядру нужно от файловой системы: `Path(...).is_file()`.
pub trait RxnBuildIo {
    /// `pathlib.Path(path).is_file()`.
    fn is_file(&mut self, path: &str) -> bool;
}

/// Аргументы сборщика — ровно то, что даёт командная строка.
#[derive(Debug, Clone, Default)]
pub struct BuildArgs {
    pub title: String,
    pub description: String,
    pub author: String,
    /// Разрешение → путь к исходному файлу. Порядок здесь НЕ важен: в архиве
    /// и в манифесте разрешения идут по таблице [`RESOLUTIONS`].
    pub videos: Vec<(String, String)>,
    /// Сырые указания дорожек `ЯЗЫК=путь`, в порядке командной строки.
    pub audio: Vec<String>,
    pub preview: Option<String>,
    pub default_locale: String,
    pub volume: f64,
}

/// Файл, который кладётся в архив.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    /// Путь к исходному файлу на диске.
    pub source: String,
    /// Путь внутри архива.
    pub archive_path: String,
    /// Строка, которую оригинал печатал, добавив файл. Пусто — не печатал
    /// (так ведёт себя превью).
    pub line: String,
}

/// Что и в каком порядке класть в архив.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildPlan {
    /// Текст `manifest.json`. Пишется в архив ПЕРВЫМ, до [`Self::entries`].
    pub manifest_json: String,
    /// Файлы: превью, затем видео по таблице разрешений, затем дорожки в
    /// порядке командной строки.
    pub entries: Vec<ArchiveEntry>,
    /// Предупреждения на stderr, в порядке появления.
    pub warnings: Vec<String>,
}

/// Итог разбора аргументов.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildOutcome {
    /// Всё сошлось — можно паковать.
    Ready(Box<BuildPlan>),
    /// Сборка не состоится: код возврата и строка на stderr.
    Failed { code: i32, message: String },
}

/// Разобрать аргументы и собрать план архива.
///
/// Порядок проверок сохранён дословно, потому что от него зависит, КАКОЙ из
/// кодов возврата получит вызывающий, когда неверно сразу несколько вещей:
/// сначала все видео по таблице разрешений, затем «видео вообще нет», затем
/// дорожки по порядку командной строки.
pub fn plan_build<I: RxnBuildIo + ?Sized>(args: &BuildArgs, io: &mut I) -> BuildOutcome {
    let mut warnings = Vec::new();
    let mut entries = Vec::new();

    // Превью в архив кладётся первым и молча — оригинал о нём не печатал.
    if let Some(preview) = args.preview.as_deref().filter(|p| !p.is_empty()) {
        entries.push(ArchiveEntry {
            source: preview.to_string(),
            archive_path: "preview.png".to_string(),
            line: String::new(),
        });
    }

    // ── Видео ──
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
            // Путь пишется отдельным выражением, а не через
            // video_archive_path: так было и в оригинале, и совпадение с
            // манифестом проверяется голденом, а не подразумевается.
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

    // ── Звуковые дорожки ──
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
        // ⚠️ ДОБАВЛЕНО (правило #16). Язык, указанный дважды, оригинал
        // проглатывал молча: в манифест попадала последняя дорожка, а в
        // архив — обе, и лишняя оставалась мёртвым грузом. Ошибкой это не
        // делаем (архив собрать всё же можно), но молчать о таком нельзя.
        //
        // Место языка в манифесте при этом НЕ меняется: словарь Python при
        // повторном присваивании обновляет значение, оставляя ключ там, где
        // он появился впервые.
        match audio_tracks.iter_mut().find(|(l, _)| *l == parsed.lang) {
            Some(slot) => {
                let previous = slot.1.clone();
                slot.1 = archive_path.clone();
                if previous == archive_path {
                    // ⚠️ ПОЧИНЕНО (правило #16). Совпал не только язык, но и
                    // расширение — значит, и путь В АРХИВЕ. Оригинал клал под
                    // этим именем ОБА файла: архив выходил с двумя записями
                    // одного имени, распаковка оставляла последнюю, а первая
                    // просто пропадала — молчаливая потеря данных, которую
                    // автор пакета замечал уже у покупателя. Второй записи
                    // больше нет: в архив идёт та дорожка, на которую
                    // ссылается манифест, и о выпавшей мы говорим вслух.
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

/// Предупреждение о языке, указанном не один раз (дорожки легли в архив под
/// разными именами — расширения не совпали).
pub fn msg_duplicate_lang(lang: &str, previous: &str, current: &str) -> String {
    format!(
        "⚠️  язык {lang} указан не один раз: {previous} останется в архиве \
         мёртвым грузом, манифест сошлётся на {current}"
    )
}

/// Предупреждение о языке, указанном не один раз с тем же расширением: место
/// в архиве одно, и прежняя дорожка туда уже не попадёт.
pub fn msg_overwritten_lang(lang: &str, archive_path: &str) -> String {
    format!(
        "⚠️  язык {lang} указан не один раз с тем же расширением: место \
         {archive_path} в архиве одно — в пакет пойдёт ПОСЛЕДНЯЯ дорожка, \
         предыдущая не попадёт в него вовсе"
    )
}

/// Строка перед началом упаковки; `output_name` — имя файла архива без пути.
pub fn msg_start(output_name: &str) -> String {
    format!("📦 Сборка {output_name}…")
}

/// Итоговая строка: путь архива и его размер, как `{МБ:.1f}`.
pub fn msg_done(output_path: &str, size_bytes: u64) -> String {
    let mb = size_bytes as f64 / (1024.0 * 1024.0);
    format!("✅ {output_path} ({mb:.1} MB)")
}

/// `pathlib.Path(p).name`: последняя часть пути, замыкающие «/» отброшены.
fn path_name(p: &str) -> &str {
    let trimmed = p.trim_end_matches('/');
    if trimmed.is_empty() {
        return "";
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

/// `pathlib.Path(p).suffix`: ведущие точки имени отбрасываются, расширением
/// считается всё от последней оставшейся точки.
///
/// ⚠️ Правило снято с Python 3.14 (`name.lstrip('.')` + `rfind('.')`), на
/// котором и брались голдены. До 3.13 оно было другим: замыкающая точка
/// расширением не считалась, и `звук.` давал пусто вместо `.`. Расхождение
/// упирается только в имена без нормального расширения; берём поведение той
/// версии, на которой продукт работает.
fn path_suffix(p: &str) -> &str {
    let name = path_name(p).trim_start_matches('.');
    match name.rfind('.') {
        Some(i) => &name[i..],
        None => "",
    }
}
