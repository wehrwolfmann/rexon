//! Thumbnails for wallpaper cards without a ready preview (`we_thumbnail`):
//! source choice, place in the shared thumbnail cache, freshness, the `ffmpeg`
//! command, labels inside the PNG and the full path «not cached → made → cached».

use std::path::PathBuf;

use rexon_core::we_thumbnail as th;

/// A directory of its own per test — tests run in parallel.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rexon-we-thumbnail-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn s(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

/// PNG labels: (key, value) from `tEXt` and `iTXt`.
fn png_texts(path: &str) -> Vec<(String, String)> {
    let file = std::io::BufReader::new(std::fs::File::open(path).unwrap());
    let mut reader = png::Decoder::new(file).read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    let info = reader.info();
    let mut out: Vec<(String, String)> = info
        .uncompressed_latin1_text
        .iter()
        .map(|c| (c.keyword.clone(), c.text.clone()))
        .collect();
    out.extend(info.utf8_text.iter().map(|c| (c.keyword.clone(), c.get_text().unwrap())));
    out
}

fn write_test_image(path: &std::path::Path, width: u32, height: u32) {
    let img = image::RgbaImage::from_fn(width, height, |x, y| {
        image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
    });
    img.save(path).unwrap();
}

#[test]
fn states_follow_the_page_enum_order() {
    // The page reads `W.ThumbnailRequest.Ready` as a number from the enum
    // Idle, Loading, Ready, Failed.
    assert_eq!(
        [th::STATE_IDLE, th::STATE_LOADING, th::STATE_READY, th::STATE_FAILED],
        [0, 1, 2, 3]
    );
}

#[test]
fn job_is_taken_from_preview_then_from_the_wallpaper_file() {
    // A ready preview beats the wallpaper file and is always an image.
    assert_eq!(
        th::resolve_job("/p/preview.gif", "/p/clip.mp4", "video"),
        Ok(th::Job { path: "/p/preview.gif".into(), is_video: false })
    );
    assert_eq!(
        th::resolve_job("", "/p/clip.mp4", "video"),
        Ok(th::Job { path: "/p/clip.mp4".into(), is_video: true })
    );
    assert_eq!(
        th::resolve_job("", "/p/photo.jpg", "image"),
        Ok(th::Job { path: "/p/photo.jpg".into(), is_video: false })
    );
    // A scene or a web page cannot become an image without a renderer.
    for wp_type in ["scene", "web", "application", ""] {
        assert_eq!(th::resolve_job("", "/p/scene.pkg", wp_type), Err(th::ERR_NO_SOURCE), "{wp_type}");
    }
    assert_eq!(th::resolve_job("", "", "video"), Err(th::ERR_NO_SOURCE));
}

#[test]
fn cache_lives_in_the_shared_thumbnail_directory() {
    assert_eq!(th::thumb_root(None, Some("/x/cache"), "/home/u"), "/x/cache/thumbnails");
    assert_eq!(th::thumb_root(None, None, "/home/u"), "/home/u/.cache/thumbnails");
    assert_eq!(th::thumb_root(Some(""), None, "/home/u"), "/home/u/.cache/thumbnails");
    assert_eq!(th::thumb_root(Some("/own/thumbs/"), Some("/x/cache"), "/home/u"), "/own/thumbs");
    // The example from the freedesktop.org thumbnail spec.
    assert_eq!(
        th::cache_file("/home/jens/.cache/thumbnails", "/home/jens/photos/me.png"),
        "/home/jens/.cache/thumbnails/x-large/c6ee772d9e49320e97ec29a7eb5b1697.png"
    );
    // The key is computed from the address as is, without percent-encoding.
    assert_eq!(th::thumb_uri("/видео/мой ролик.mp4"), "file:///видео/мой ролик.mp4");
}

#[test]
fn size_fits_inside_the_square_without_upscaling() {
    assert_eq!(th::fit_inside(1920, 1080, 512), Some((512, 288)));
    assert_eq!(th::fit_inside(1080, 1920, 512), Some((288, 512)));
    assert_eq!(th::fit_inside(640, 360, 512), Some((512, 288)));
    assert_eq!(th::fit_inside(300, 200, 512), Some((300, 200)));
    assert_eq!(th::fit_inside(512, 512, 512), Some((512, 512)));
    assert_eq!(th::fit_inside(100_000, 1, 512), Some((512, 1)));
    assert_eq!(th::fit_inside(0, 10, 512), None);
    assert_eq!(th::fit_inside(10, 0, 512), None);
}

#[test]
fn stale_thumbnail_is_not_reused() {
    assert!(th::cache_is_fresh(100, 100));
    assert!(th::cache_is_fresh(100, 200));
    assert!(!th::cache_is_fresh(200, 100));
}

#[test]
fn frame_command_pipes_one_png_frame() {
    let video = th::frame_argv("/p/мой ролик.mp4", true);
    assert_eq!(video[0], "ffmpeg");
    // The path is a separate argument right after `-i`: spaces and non-Latin stay intact.
    let i = video.iter().position(|a| a == "-i").unwrap();
    assert_eq!(video[i + 1], "/p/мой ролик.mp4");
    let vf = &video[video.iter().position(|a| a == "-vf").unwrap() + 1];
    assert_eq!(
        vf,
        "thumbnail=n=30,scale=w='min(512,iw)':h='min(512,ih)':force_original_aspect_ratio=decrease"
    );
    assert!(video.contains(&"-nostdin".to_string()));
    assert_eq!(&video[video.len() - 7..], ["-frames:v", "1", "-f", "image2pipe", "-c:v", "png", "-"]);

    let image = th::frame_argv("/p/photo.avif", false);
    let vf = &image[image.iter().position(|a| a == "-vf").unwrap() + 1];
    assert!(vf.starts_with("scale="), "{vf}");
}

#[test]
fn png_carries_the_three_standard_labels() {
    let dir = scratch("labels");
    for (name, uri) in [("ascii.png", "file:///p/clip.mp4"), ("utf8.png", "file:///видео/мой ролик.mp4")] {
        let rgba = vec![200u8; 4 * 3 * 4];
        let png = th::encode_thumb_png(&rgba, 4, 3, uri, 1_700_000_000, 443_012).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, &png).unwrap();
        let texts = png_texts(&s(&path));
        assert!(texts.contains(&("Thumb::URI".into(), uri.into())), "{texts:?}");
        assert!(texts.contains(&("Thumb::MTime".into(), "1700000000".into())), "{texts:?}");
        assert!(texts.contains(&("Thumb::Size".into(), "443012".into())), "{texts:?}");
        let back = image::open(&path).unwrap();
        assert_eq!((back.width(), back.height()), (4, 3));
    }
    assert!(th::encode_thumb_png(&[0u8; 5], 4, 3, "file:///x", 0, 0).is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn lookup_reports_missing_source_and_missing_type() {
    let dir = scratch("lookup");
    let root = s(&dir.join("thumbs"));
    assert_eq!(
        th::lookup_in(&root, "", &s(&dir.join("nope.mp4")), "video"),
        th::Lookup::Failed(th::ERR_NOT_FOUND.into())
    );
    // A directory is not a wallpaper file.
    assert_eq!(th::lookup_in(&root, "", &s(&dir), "video"), th::Lookup::Failed(th::ERR_NOT_FOUND.into()));
    assert_eq!(
        th::lookup_in(&root, "", &s(&dir.join("nope.pkg")), "scene"),
        th::Lookup::Failed(th::ERR_NO_SOURCE.into())
    );
    // The check creates nothing: the cache directory appears only on write.
    assert!(!dir.join("thumbs").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn image_goes_miss_then_generated_then_ready() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("image");
    let root = s(&dir.join("thumbs"));
    let photo = dir.join("фото обои.png");
    write_test_image(&photo, 1024, 256);

    let th::Lookup::Miss(job) = th::lookup_in(&root, "", &s(&photo), "image") else {
        panic!("ждали промах кэша");
    };
    assert_eq!(job.path, s(&photo));
    assert!(!job.is_video);
    assert_eq!(job.cache_path, th::cache_file(&root, &s(&photo)));
    assert_eq!(job.source_size, std::fs::metadata(&photo).unwrap().len());

    let made = th::live_generate(&job).unwrap();
    assert_eq!(made, job.cache_path);
    let thumb = image::open(&made).unwrap();
    assert_eq!((thumb.width(), thumb.height()), (512, 128));
    let texts = png_texts(&made);
    assert!(texts.contains(&("Thumb::URI".into(), th::thumb_uri(&s(&photo)))), "{texts:?}");
    assert!(texts.contains(&("Thumb::MTime".into(), job.source_mtime.to_string())), "{texts:?}");
    // Spec: directory 700, file 600; no temporary files left.
    let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dir.join("thumbs/x-large")), 0o700);
    assert_eq!(mode(std::path::Path::new(&made)), 0o600);
    assert_eq!(std::fs::read_dir(dir.join("thumbs/x-large")).unwrap().count(), 1);

    assert_eq!(th::lookup_in(&root, "", &s(&photo), "image"), th::Lookup::Ready(made.clone()));
    // The same preview given as `source` lands in the same thumbnail.
    assert_eq!(th::lookup_in(&root, &s(&photo), "", ""), th::Lookup::Ready(made.clone()));

    // The source was replaced later — the old thumbnail is no longer valid.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
    std::fs::File::options().write(true).open(&photo).unwrap().set_modified(later).unwrap();
    assert!(matches!(th::lookup_in(&root, "", &s(&photo), "image"), th::Lookup::Miss(_)));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn broken_image_fails_with_a_reason() {
    let dir = scratch("broken");
    let root = s(&dir.join("thumbs"));
    let junk = dir.join("junk.png");
    std::fs::write(&junk, b"not a picture at all").unwrap();
    let th::Lookup::Miss(job) = th::lookup_in(&root, "", &s(&junk), "image") else {
        panic!("ждали промах кэша");
    };
    let err = th::live_generate(&job).unwrap_err();
    assert!(!err.is_empty());
    assert!(!std::path::Path::new(&job.cache_path).exists());
    let _ = std::fs::remove_dir_all(dir);
}

/// Live check: a frame from a real clip. Needs `ffmpeg`; without it the test
/// reports that it was not checked instead of silently going green.
#[test]
fn video_frame_is_extracted_by_ffmpeg() {
    let has_ffmpeg = std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !has_ffmpeg {
        eprintln!("НЕ ПРОВЕРЕНО: нет ffmpeg — кадр из ролика не снимался");
        return;
    }
    let dir = scratch("video");
    let root = s(&dir.join("thumbs"));
    let clip = dir.join("мой ролик.mkv");
    let made = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=10:duration=1"])
        .args(["-c:v", "mjpeg"])
        .arg(&clip)
        .status()
        .unwrap();
    if !made.success() {
        eprintln!("НЕ ПРОВЕРЕНО: ffmpeg не собрал пробный ролик");
        return;
    }
    let th::Lookup::Miss(job) = th::lookup_in(&root, "", &s(&clip), "video") else {
        panic!("ждали промах кэша");
    };
    assert!(job.is_video);
    let png = th::live_generate(&job).unwrap();
    let thumb = image::open(&png).unwrap().to_rgba8();
    assert_eq!((thumb.width(), thumb.height()), (512, 288));
    // The frame is not blank: the test picture has many different colours.
    let distinct: std::collections::HashSet<[u8; 4]> = thumb.pixels().map(|p| p.0).collect();
    assert!(distinct.len() > 50, "цветов: {}", distinct.len());
    assert_eq!(th::lookup_in(&root, "", &s(&clip), "video"), th::Lookup::Ready(png));

    // Not a clip disguised as a clip — refusal with a reason, the cache is empty.
    let fake = dir.join("fake.mp4");
    std::fs::write(&fake, b"definitely not a video").unwrap();
    let th::Lookup::Miss(job) = th::lookup_in(&root, "", &s(&fake), "video") else {
        panic!("ждали промах кэша");
    };
    let err = th::live_generate(&job).unwrap_err();
    assert!(err.starts_with("ffmpeg: "), "{err}");
    assert!(!std::path::Path::new(&job.cache_path).exists());
    let _ = std::fs::remove_dir_all(dir);
}

// ── waywallen 0.6.0: the `ThumbnailRequest::submit` decision ─────────────────

#[test]
fn plan_follows_vendor_0_6_0() {
    use th::Plan;
    let cwd = "/home/u";
    // Disabled or empty — idle.
    assert_eq!(th::plan("/p.gif", "", "scene", false, false, cwd), Plan::Idle);
    assert_eq!(th::plan("", "", "video", false, true, cwd), Plan::Idle);
    // A ready preview — by address as is (animation is kept).
    assert_eq!(
        th::plan("/w/preview.gif", "/w/scene.pkg", "scene", false, true, cwd),
        Plan::Direct("file:///w/preview.gif".into())
    );
    assert_eq!(
        th::plan("https://x.invalid/p.jpg", "", "scene", false, true, cwd),
        Plan::Direct("https://x.invalid/p.jpg".into())
    );
    // An image without a preview — the image itself.
    assert_eq!(
        th::plan("", "img/a.png", "image", false, true, cwd),
        Plan::Direct("file:///home/u/img/a.png".into())
    );
    // A scene without a preview — refusal; a clip without a preview — a frame from the file.
    assert_eq!(
        th::plan("", "/w/s.pkg", "scene", false, true, cwd),
        Plan::Failed(th::ERR_NO_PREVIEW.into())
    );
    assert_eq!(th::plan("", "/v/a.mp4", "video", false, true, cwd), Plan::Generate);
    assert_eq!(
        th::plan("", "https://x.invalid/a.mp4", "video", false, true, cwd),
        Plan::Failed(th::ERR_NOT_LOCAL.into())
    );
    // Timeline: preview present — use it, absent — an honest refusal.
    assert_eq!(
        th::plan("/t/day.jpg", "/t/meta.json", "image", true, true, cwd),
        Plan::Direct("file:///t/day.jpg".into())
    );
    assert_eq!(
        th::plan("", "/t/meta.json", "image", true, true, cwd),
        Plan::Failed(th::ERR_TIMELINE.into())
    );
    // Qt resource and path cleanup.
    assert_eq!(th::input_url(":/img/x.png", cwd), "qrc:/img/x.png");
    assert_eq!(th::input_url("/a/./b/../c//d.png", cwd), "file:///a/c/d.png");
}
