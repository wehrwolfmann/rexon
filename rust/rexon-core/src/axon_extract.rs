//! Подготовка скачанных обоев Razer Axon к показу.
//!
//! Каталог обоев (`WallpaperSource/{Id}{H}{W}`): `ResourceConfig.txt` (JSON)
//! и `Resource/<файл>`. Файл — ZIP со стандартным шифрованием ZipCrypto и
//! одним файлом внутри; пароль — `hex(HMAC-SHA256(ключ, сырые байты
//! ResourceConfig.txt))`, ключ — в [`crate::axon_key`]. У старых обоев
//! (без `SourceEncryptedTypes`) файл лежит как есть.
//!
//! Каталог Axon только читается; готовый файл пишется атомарно в выданный
//! каталог кэша, рядом — метка (сумма конфига и размер), по которой повторный
//! вызов отдаёт готовое без работы. Снаружи модуль вызывается через
//! [`crate::axon_library::BuiltinExtractor`].

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

pub use crate::axon_key::RAZER_HMAC_KEY;

const CONFIG_NAME: &str = "ResourceConfig.txt";
const STAMP_SUFFIX: &str = ".rexon-axon";

#[derive(Debug)]
pub enum ExtractError {
    /// Missing or unparseable ResourceConfig.txt (exit code 2).
    Config(String),
    /// Wrong password, bad or unsupported container (exit code 3).
    Format(String),
    /// Cannot write the output (exit code 4).
    Write(String),
}

impl ExtractError {
    pub fn exit_code(&self) -> i32 {
        match self {
            ExtractError::Config(_) => 2,
            ExtractError::Format(_) => 3,
            ExtractError::Write(_) => 4,
        }
    }
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExtractError::Config(m) | ExtractError::Format(m) | ExtractError::Write(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ExtractError {}

/// ZIP password for a wallpaper: lowercase hex HMAC-SHA256 of the raw config bytes.
pub fn derive_password_with_key(key: &[u8], config: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(config);
    to_hex(&mac.finalize().into_bytes())
}

fn to_hex(bytes: &[u8]) -> String {
    use fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub fn derive_password(config: &[u8]) -> String {
    derive_password_with_key(RAZER_HMAC_KEY, config)
}

/// What ResourceConfig.txt says about the resource.
#[derive(Debug, PartialEq, Eq)]
pub struct ResourceInfo {
    /// Relative path of the resource inside the wallpaper directory.
    pub source: PathBuf,
    /// File name of the resource (also the output file name).
    pub file_name: String,
    /// `Some(true)` = ZIP, `Some(false)` = other declared scheme, `None` = not declared.
    pub zip_encrypted: Option<bool>,
    /// Declared scheme if it is not ZIP.
    pub scheme: Option<String>,
}

pub fn parse_config(raw: &[u8]) -> Result<ResourceInfo, ExtractError> {
    let text = raw.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(raw);
    let json: serde_json::Value = serde_json::from_slice(text)
        .map_err(|e| ExtractError::Config(format!("bad {CONFIG_NAME}: {e}")))?;
    let source = json
        .get("Source")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ExtractError::Config(format!("{CONFIG_NAME}: no Source")))?;

    let mut rel = PathBuf::new();
    for part in source.split(['\\', '/']).filter(|p| !p.is_empty() && *p != ".") {
        if part == ".." || part.contains(':') {
            return Err(ExtractError::Config(format!("{CONFIG_NAME}: unsafe Source {source:?}")));
        }
        rel.push(part);
    }
    let file_name = rel
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ExtractError::Config(format!("{CONFIG_NAME}: bad Source {source:?}")))?
        .to_owned();

    let scheme = json
        .get("SourceEncryptedTypes")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let zip_encrypted = scheme.map(|s| s.eq_ignore_ascii_case("ZIP"));
    Ok(ResourceInfo {
        source: rel,
        file_name,
        zip_encrypted,
        scheme: scheme.filter(|s| !s.eq_ignore_ascii_case("ZIP")).map(str::to_owned),
    })
}

/// Extracts the wallpaper using the Razer key. Returns the absolute path of the ready file.
pub fn extract(resource_dir: &Path, out_dir: &Path) -> Result<PathBuf, ExtractError> {
    extract_with_key(RAZER_HMAC_KEY, resource_dir, out_dir)
}

pub fn extract_with_key(key: &[u8], resource_dir: &Path, out_dir: &Path) -> Result<PathBuf, ExtractError> {
    let config_path = resource_dir.join(CONFIG_NAME);
    let raw = fs::read(&config_path)
        .map_err(|e| ExtractError::Config(format!("cannot read {}: {e}", config_path.display())))?;
    let info = parse_config(&raw)?;
    let config_sum = to_hex(&Sha256::digest(&raw));

    let archive_path = resource_dir.join(&info.source);
    let mut src = File::open(&archive_path)
        .map_err(|e| ExtractError::Format(format!("cannot open {}: {e}", archive_path.display())))?;

    fs::create_dir_all(out_dir)
        .map_err(|e| ExtractError::Write(format!("cannot create {}: {e}", out_dir.display())))?;
    let out_dir = fs::canonicalize(out_dir)
        .map_err(|e| ExtractError::Write(format!("cannot resolve {}: {e}", out_dir.display())))?;
    let target = out_dir.join(&info.file_name);
    let stamp = out_dir.join(format!(".{}{STAMP_SUFFIX}", info.file_name));

    if is_cached(&target, &stamp, &config_sum) {
        return Ok(target);
    }

    let is_zip = match info.zip_encrypted {
        Some(true) => true,
        Some(false) => {
            return Err(ExtractError::Format(format!(
                "unsupported encryption {:?}",
                info.scheme.as_deref().unwrap_or("")
            )));
        }
        // Not declared: old unencrypted layout, unless the file is a ZIP anyway.
        None => starts_with_zip_magic(&mut src)?,
    };

    let size = if is_zip {
        decrypt_zip(key, &raw, &info.file_name, src, &target)?
    } else {
        write_atomically(&mut src, &target)?
    };

    let stamp_body = format!("{config_sum}\n{size}\n");
    write_atomically(&mut stamp_body.as_bytes(), &stamp)?;
    Ok(target)
}

/// Decrypts the ZIP container `archive` described by `config_raw` straight
/// into `target`: temp file next to `target`, fsync, rename over it. No cache
/// stamp. `target` may be `archive` itself (in-place preparation): the open
/// handle keeps reading the old file until the rename. Returns bytes written.
pub fn decrypt_resource_with_key(
    key: &[u8],
    config_raw: &[u8],
    archive: &Path,
    target: &Path,
) -> Result<u64, ExtractError> {
    let info = parse_config(config_raw)?;
    if info.zip_encrypted != Some(true) {
        return Err(ExtractError::Format(format!(
            "unsupported encryption {:?}",
            info.scheme.as_deref().unwrap_or("")
        )));
    }
    let src = File::open(archive)
        .map_err(|e| ExtractError::Format(format!("cannot open {}: {e}", archive.display())))?;
    decrypt_zip(key, config_raw, &info.file_name, src, target)
}

/// True if the file starts with a ZIP local header (`PK\x03\x04`).
pub fn is_zip_file(path: &Path) -> io::Result<bool> {
    let mut f = File::open(path)?;
    let mut magic = [0u8; 4];
    Ok(read_full(&mut f, &mut magic)? == 4 && magic == *b"PK\x03\x04")
}

fn decrypt_zip(
    key: &[u8],
    config_raw: &[u8],
    file_name: &str,
    src: File,
    target: &Path,
) -> Result<u64, ExtractError> {
    let password = derive_password_with_key(key, config_raw);
    let mut archive = zip::ZipArchive::new(src)
        .map_err(|e| ExtractError::Format(format!("not a valid ZIP: {e}")))?;
    let index = pick_entry(&mut archive, file_name)?;
    let mut entry = archive.by_index_decrypt(index, password.as_bytes()).map_err(|e| match e {
        zip::result::ZipError::InvalidPassword => ExtractError::Format("wrong password".into()),
        e => ExtractError::Format(format!("cannot open ZIP entry: {e}")),
    })?;
    write_atomically(&mut entry, target)
}

fn starts_with_zip_magic(src: &mut File) -> Result<bool, ExtractError> {
    use std::io::{Seek, SeekFrom};
    let mut magic = [0u8; 4];
    let n = read_full(src, &mut magic)
        .map_err(|e| ExtractError::Format(format!("cannot read resource: {e}")))?;
    src.seek(SeekFrom::Start(0))
        .map_err(|e| ExtractError::Format(format!("cannot read resource: {e}")))?;
    Ok(n == 4 && magic == *b"PK\x03\x04")
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

fn pick_entry<R: Read + io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    wanted: &str,
) -> Result<usize, ExtractError> {
    let mut files = Vec::new();
    for i in 0..archive.len() {
        let e = archive
            .by_index_raw(i)
            .map_err(|e| ExtractError::Format(format!("bad ZIP entry: {e}")))?;
        if e.is_dir() {
            continue;
        }
        if e.name().rsplit(['/', '\\']).next() == Some(wanted) {
            return Ok(i);
        }
        files.push(i);
    }
    match files.as_slice() {
        [only] => Ok(*only),
        [] => Err(ExtractError::Format("ZIP has no files".into())),
        _ => Err(ExtractError::Format(format!("ZIP has no entry {wanted:?}"))),
    }
}

fn is_cached(target: &Path, stamp: &Path, config_sum: &str) -> bool {
    let Ok(body) = fs::read_to_string(stamp) else { return false };
    let mut lines = body.lines();
    let (Some(sum), Some(size)) = (lines.next(), lines.next()) else { return false };
    let Ok(size) = size.parse::<u64>() else { return false };
    sum == config_sum && fs::metadata(target).is_ok_and(|m| m.is_file() && m.len() == size)
}

/// Copies `src` into a temp file next to `target`, fsyncs and renames it into place.
/// Read errors are format errors (bad password shows up as a CRC mismatch here);
/// write errors are write errors. Returns the number of bytes written.
fn write_atomically(src: &mut impl Read, target: &Path) -> Result<u64, ExtractError> {
    let dir = target.parent().unwrap_or(Path::new("."));
    let name = target.file_name().and_then(|n| n.to_str()).unwrap_or("out");
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let werr = |e: io::Error| ExtractError::Write(format!("cannot write {}: {e}", tmp.display()));

    let result = (|| {
        let file = File::create(&tmp).map_err(werr)?;
        let mut w = BufWriter::with_capacity(1 << 20, file);
        let mut buf = vec![0u8; 1 << 20];
        let mut total = 0u64;
        loop {
            let n = match src.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(ExtractError::Format(format!("cannot decrypt/read resource: {e}"))),
            };
            w.write_all(&buf[..n]).map_err(werr)?;
            total += n as u64;
        }
        let file = w.into_inner().map_err(|e| werr(e.into_error()))?;
        file.sync_all().map_err(werr)?;
        drop(file);
        fs::rename(&tmp, target).map_err(|e| {
            ExtractError::Write(format!("cannot rename to {}: {e}", target.display()))
        })?;
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
        Ok(total)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::unstable::write::FileOptionsExt;
    use zip::write::SimpleFileOptions;

    const TEST_KEY: &[u8] = b"test-key-not-razer";

    /// Временный каталог теста (убирается после).
    struct TempDir(PathBuf);
    impl TempDir {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir() -> TempDir {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!(
            "rexon-axon-extract-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        TempDir(d)
    }

    fn config(name: &str) -> String {
        format!(
            "{{\"WallPaperType\":\"VIDEO\",\"Source\":\"Resource\\\\{name}\",\
             \"SourceEncryptedTypes\":\"ZIP\",\"SourceEncryptedLevel\":\"LOW\"}}"
        )
    }

    fn make_wallpaper(dir: &Path, cfg: &str, name: &str, payload: &[u8], password: &str) {
        fs::create_dir_all(dir.join("Resource")).unwrap();
        fs::write(dir.join(CONFIG_NAME), cfg).unwrap();
        let f = File::create(dir.join("Resource").join(name)).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opts = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .with_deprecated_encryption(password.as_bytes());
        z.start_file(name, opts).unwrap();
        z.write_all(payload).unwrap();
        z.finish().unwrap();
    }

    fn payload() -> Vec<u8> {
        (0..300_000u32).flat_map(|i| (i.wrapping_mul(2654435761)).to_le_bytes()).collect()
    }

    #[test]
    fn hmac_matches_python_reference() {
        // python3 -c 'import hmac,hashlib;print(hmac.new(b"test-key-not-razer",
        //   b"{\"Source\":\"x\"}",hashlib.sha256).hexdigest())'
        assert_eq!(
            derive_password_with_key(TEST_KEY, br#"{"Source":"x"}"#),
            "ba51f994761f7c4973504cd391136d5bf8596535d9be6725be3c45c852e2ff6a"
        );
    }

    #[test]
    fn extracts_and_caches() {
        let t = tempdir();
        let wp = t.path().join("wp");
        let out = t.path().join("out");
        let cfg = config("3840x2160.mp4");
        let pw = derive_password_with_key(TEST_KEY, cfg.as_bytes());
        let data = payload();
        make_wallpaper(&wp, &cfg, "3840x2160.mp4", &data, &pw);

        let path = extract_with_key(TEST_KEY, &wp, &out).unwrap();
        assert!(path.is_absolute());
        assert_eq!(path.file_name().unwrap(), "3840x2160.mp4");
        assert_eq!(fs::read(&path).unwrap(), data);
        let leftovers: Vec<_> = fs::read_dir(&out)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");

        // Second call is served from cache: mtime unchanged.
        let m1 = fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(extract_with_key(TEST_KEY, &wp, &out).unwrap(), path);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), m1);

        // Truncated output is re-extracted.
        fs::write(&path, b"x").unwrap();
        extract_with_key(TEST_KEY, &wp, &out).unwrap();
        assert_eq!(fs::read(&path).unwrap(), data);
    }

    #[test]
    fn changed_config_means_new_password() {
        let t = tempdir();
        let wp = t.path().join("wp");
        let cfg = config("a.jpg");
        let pw = derive_password_with_key(TEST_KEY, cfg.as_bytes());
        make_wallpaper(&wp, &cfg, "a.jpg", b"jpeg-bytes", &pw);
        // Raw bytes matter: a trailing newline changes the password.
        fs::write(wp.join(CONFIG_NAME), format!("{cfg}\n")).unwrap();
        let err = extract_with_key(TEST_KEY, &wp, &t.path().join("out")).unwrap_err();
        assert_eq!(err.exit_code(), 3, "{err}");
    }

    #[test]
    fn wrong_key_is_code_3() {
        let t = tempdir();
        let wp = t.path().join("wp");
        let cfg = config("a.mp4");
        make_wallpaper(&wp, &cfg, "a.mp4", &payload(), "some-other-password");
        let err = extract_with_key(TEST_KEY, &wp, &t.path().join("out")).unwrap_err();
        assert_eq!(err.exit_code(), 3, "{err}");
        let out = t.path().join("out");
        assert!(!out.join("a.mp4").exists());
        assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    }

    #[test]
    fn missing_or_bad_config_is_code_2() {
        let t = tempdir();
        let out = t.path().join("out");
        assert_eq!(extract_with_key(TEST_KEY, t.path(), &out).unwrap_err().exit_code(), 2);
        fs::write(t.path().join(CONFIG_NAME), "{not json").unwrap();
        assert_eq!(extract_with_key(TEST_KEY, t.path(), &out).unwrap_err().exit_code(), 2);
        fs::write(t.path().join(CONFIG_NAME), "{\"Source\":\"..\\\\x.mp4\"}").unwrap();
        assert_eq!(extract_with_key(TEST_KEY, t.path(), &out).unwrap_err().exit_code(), 2);
    }

    #[test]
    fn not_a_zip_is_code_3() {
        let t = tempdir();
        let wp = t.path();
        fs::create_dir_all(wp.join("Resource")).unwrap();
        fs::write(wp.join(CONFIG_NAME), config("a.mp4")).unwrap();
        fs::write(wp.join("Resource/a.mp4"), b"plain mp4, not zip").unwrap();
        let err = extract_with_key(TEST_KEY, wp, &wp.join("out")).unwrap_err();
        assert_eq!(err.exit_code(), 3, "{err}");
    }

    #[test]
    fn old_unencrypted_layout_with_bom_and_crlf() {
        let t = tempdir();
        let wp = t.path().join("wp");
        fs::create_dir_all(wp.join("Resource")).unwrap();
        fs::write(
            wp.join(CONFIG_NAME),
            "\u{FEFF}{\r\n  \"Type\": \"VIDEO\",\r\n  \"Source\": \"Resource\\\\old.mp4\"\r\n}",
        )
        .unwrap();
        fs::write(wp.join("Resource/old.mp4"), b"plain video").unwrap();
        let path = extract_with_key(TEST_KEY, &wp, &t.path().join("out")).unwrap();
        assert_eq!(fs::read(path).unwrap(), b"plain video");
    }

    #[test]
    fn unknown_scheme_is_code_3() {
        let t = tempdir();
        let wp = t.path();
        fs::create_dir_all(wp.join("Resource")).unwrap();
        fs::write(
            wp.join(CONFIG_NAME),
            "{\"Source\":\"Resource\\\\a.mp4\",\"SourceEncryptedTypes\":\"AES\"}",
        )
        .unwrap();
        fs::write(wp.join("Resource/a.mp4"), b"x").unwrap();
        assert_eq!(extract_with_key(TEST_KEY, wp, &wp.join("out")).unwrap_err().exit_code(), 3);
    }

    #[test]
    fn unwritable_out_is_code_4() {
        let t = tempdir();
        let wp = t.path().join("wp");
        let cfg = config("a.mp4");
        let pw = derive_password_with_key(TEST_KEY, cfg.as_bytes());
        make_wallpaper(&wp, &cfg, "a.mp4", b"data", &pw);
        let blocker = t.path().join("file");
        fs::write(&blocker, b"").unwrap();
        let err = extract_with_key(TEST_KEY, &wp, &blocker.join("out")).unwrap_err();
        assert_eq!(err.exit_code(), 4, "{err}");
    }
}
