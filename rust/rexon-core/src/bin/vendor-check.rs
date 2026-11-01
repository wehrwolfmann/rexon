//! `vendor-check` — проверка свежести ЧУЖИХ кусков, которые едут в поставке.
//!
//! Зачем
//! -----
//! Забандленный KDE-плагин `org.waywallen.kde` протух молча: уехал в поставку
//! 24 июня, апстрим потом сменил протокол с v7 на v8, и никто не заметил —
//! ни ошибки, ни предупреждения, у пользователя просто пустой список
//! мониторов. Обновление бандла лечит тот случай, но не причину: любой
//! следующий вложенный кусок протухнет так же тихо. Эта проверка — сторож.
//!
//! Как звать
//! ---------
//! ```text
//! cargo run -q -p rexon-core --bin vendor-check          # с сетью
//! cargo run -q -p rexon-core --bin vendor-check -- --offline   # без сети
//! cargo run -q -p rexon-core --bin vendor-check -- --выкачать  # скачать архивы кусков
//! cargo run -q -p rexon-core --bin vendor-check -- --корень ПУТЬ
//! ```
//! Коды возврата (для CI):
//!   0 — все куски свежие, ЛИБО проверить не удалось (нет сети, неизвестное
//!       происхождение) — это НЕ повод валить сборку;
//!   1 — хотя бы один кусок отстал от апстрима;
//!   2 — беда с самими описаниями: битый `vendor.toml`, содержимое куска
//!       разошлось с записанным, сумма архива разошлась с выпуском апстрима
//!       или с рецептом пакета.
//! Без сети проверка честно пишет «проверить не смог» и выходит с 0 — она не
//! имеет права ни ломать сборку, ни врать «всё хорошо».
//!
//! ФОРМАТ ОПИСАНИЯ (один на все куски)
//! -----------------------------------
//! Каталог куска объявляет лежащий в нём `vendor.toml` (проверка просто ищет
//! эти файлы по дереву):
//!
//! ```toml
//! name        = "org.waywallen.kde"          # имя куска (короткое)
//! description = "Плагин обоев waywallen"     # что это, по-русски
//! upstream    = "https://github.com/waywallen/waywallen-display"
//! source      = "github-release"             # откуда узнавать свежесть
//! version     = "0.4.1"                      # версия куска
//! tag         = "v0.4.1"                     # тег апстрима
//! asset       = "waywallen-kde-0.4.1-x86_64-embed.zip"  # имя ассета (опц.)
//! vendored    = "2026-10-09"                 # когда приняли (ISO-дата)
//! license     = "MIT"                        # лицензия куска
//! checksum    = "sha256:<64 hex>"            # см. ниже
//! tree        = "sha256-tree:<64 hex>"       # (опц.) содержимое распакованного
//! pkgbuild_var = "_wwd_ver"                  # (опц.) переменная версии в PKGBUILD
//! note        = "..."                        # свободный текст (опц.)
//! ```
//!
//! `source` понимает:
//!   * `github-release` — свежесть берётся из релизов GitHub;
//!   * `unknown` — происхождение НЕ установлено. Это законное значение и
//!     само по себе находка: проверка о таком куске честно скажет «проверить
//!     нечем», а не промолчит. Причину писать в `note`.
//!
//! `checksum` бывает двух видов:
//!   * `sha256:<hex>` — кусок НЕ лежит в git, а скачивается архивом `asset`
//!     выпуска `tag` (пакет — при сборке, разработчик — `--выкачать`). Сумма —
//!     у самого архива; с сетью она сверяется с `digest` ассета в API выпусков
//!     GitHub, а при заданном `pkgbuild_var` — ещё и с рецептом пакета
//!     (`packaging/aur/PKGBUILD`: строка `<pkgbuild_var>=<version>` и та же
//!     сумма в `sha256sums`), чтобы две записи не разъехались. Если каталог
//!     распакован, его содержимое сверяется с `tree` — так ловится ручная
//!     правка скачанного.
//!   * `sha256-tree:<hex>` — кусок лежит в дереве; сумма — от «оглавления»
//!     каталога (см. ниже).
//!
//! «Оглавление» (`sha256-tree:`): для каждого файла каталога (кроме самого
//! `vendor.toml`), по возрастанию относительного пути, в хэш идёт
//! `<путь>\0<sha256 файла в hex>\n`. Так сумма не зависит от порядка обхода
//! и ловит любую позднейшую правку. Пересчитать после осознанной правки:
//! `--пересчитать` (переписывает `tree` у скачиваемых кусков и `checksum` у
//! лежащих в дереве).

use rexon_core::net::{NetIo, UreqNet};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Имя файла-описания. Он же — объявление куска: проверка ищет именно его.
const MANIFEST: &str = "vendor.toml";
/// Приставка суммы содержимого каталога (см. описание формата).
const SUM_PREFIX: &str = "sha256-tree:";
/// Приставка суммы скачиваемого архива.
const ARCHIVE_PREFIX: &str = "sha256:";
/// Рецепт пакета, с которым сверяется сумма архива (от корня репозитория).
const PKGBUILD_REL: &str = "packaging/aur/PKGBUILD";
const UA: &str = "rexon-vendor-check";
const HTTP_TIMEOUT: u64 = 20;

/// Каталоги, в которые обход не заходит: сборочный мусор и архивы.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "__pycache__",
    "_archived",
    "node_modules",
    ".venv",
];

// ── описание куска ──────────────────────────────────────────────────────

struct Piece {
    dir: PathBuf,
    name: String,
    description: String,
    upstream: String,
    source: String,
    version: String,
    tag: String,
    asset: String,
    vendored: String,
    checksum: String,
    tree: String,
    pkgbuild_var: String,
    note: String,
}

impl Piece {
    /// sha256 скачиваемого архива (hex), если кусок такого вида.
    fn archive_sum(&self) -> Option<&str> {
        self.checksum.strip_prefix(ARCHIVE_PREFIX)
    }
}

fn field(v: &toml::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

fn read_piece(manifest: &Path) -> Result<Piece, String> {
    let text = std::fs::read_to_string(manifest)
        .map_err(|e| format!("не читается {}: {e}", manifest.display()))?;
    let v: toml::Value = text
        .parse()
        .map_err(|e| format!("битый TOML в {}: {e}", manifest.display()))?;
    let dir = manifest.parent().unwrap_or(Path::new(".")).to_path_buf();
    let name = field(&v, "name");
    if name.is_empty() {
        return Err(format!("в {} нет поля name", manifest.display()));
    }
    Ok(Piece {
        dir,
        name,
        description: field(&v, "description"),
        upstream: field(&v, "upstream"),
        source: field(&v, "source"),
        version: field(&v, "version"),
        tag: field(&v, "tag"),
        asset: field(&v, "asset"),
        vendored: field(&v, "vendored"),
        checksum: field(&v, "checksum"),
        tree: field(&v, "tree"),
        pkgbuild_var: field(&v, "pkgbuild_var"),
        note: field(&v, "note"),
    })
}

// ── обход дерева ────────────────────────────────────────────────────────

fn find_manifests(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            let base = e.file_name();
            let base = base.to_string_lossy();
            if SKIP_DIRS.contains(&base.as_ref()) {
                continue;
            }
            find_manifests(&path, out);
        } else if e.file_name() == MANIFEST {
            out.push(path);
        }
    }
}

/// Оглавление каталога: относительный путь → sha256 файла. `vendor.toml`
/// в сумму не входит — иначе запись суммы меняла бы саму сумму.
fn tree_files(dir: &Path, base: &Path, acc: &mut BTreeMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            tree_files(&path, base, acc);
        } else if e.file_name() != MANIFEST {
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let data = std::fs::read(&path).unwrap_or_default();
            acc.insert(rel, hex(&Sha256::digest(&data)));
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Сумма оглавления и число файлов в нём (без `vendor.toml`).
fn tree_checksum(dir: &Path) -> (String, usize) {
    let mut files = BTreeMap::new();
    tree_files(dir, dir, &mut files);
    let mut h = Sha256::new();
    for (rel, sum) in &files {
        h.update(rel.as_bytes());
        h.update([0u8]);
        h.update(sum.as_bytes());
        h.update(b"\n");
    }
    (format!("{SUM_PREFIX}{}", hex(&h.finalize())), files.len())
}

// ── свежесть у апстрима ─────────────────────────────────────────────────

/// Релиз апстрима: тег, дата публикации (для счёта «на сколько отстали») и
/// суммы ассетов (`имя` → `sha256:<hex>` из поля `digest` API).
struct Release {
    tag: String,
    published: String,
    digests: BTreeMap<String, String>,
}

fn github_repo(upstream: &str) -> Option<String> {
    let rest = upstream
        .trim_end_matches('/')
        .strip_prefix("https://github.com/")?;
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

fn github_releases(net: &mut UreqNet, repo: &str) -> Result<Vec<Release>, String> {
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=100");
    let body = net
        .get(&url, UA, HTTP_TIMEOUT)
        .map_err(|e| format!("{e}"))?;
    let v: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| format!("непонятный ответ GitHub: {e}"))?;
    let arr = v
        .as_array()
        .ok_or_else(|| "GitHub ответил не списком релизов".to_string())?;
    Ok(arr
        .iter()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .map(|r| Release {
            tag: r["tag_name"].as_str().unwrap_or("").to_string(),
            published: r["published_at"].as_str().unwrap_or("").to_string(),
            digests: r["assets"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| {
                            Some((x["name"].as_str()?.to_string(), x["digest"].as_str()?.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
        .filter(|r| !r.tag.is_empty())
        .collect())
}

/// Итог по одному куску.
enum Verdict {
    /// Свежий: тег совпал с последним релизом.
    Fresh { latest: String, date: String },
    /// Отстали: сколько релизов вышло после нашего и какой сейчас последний.
    Behind {
        count: usize,
        latest: String,
        date: String,
    },
    /// Проверить не смогли — с причиной (нет сети, нет происхождения, …).
    Unknown { why: String },
}

/// Релизы апстрима куска либо причина, по которой их не узнать.
fn upstream_releases(piece: &Piece, net: Option<&mut UreqNet>) -> Result<Vec<Release>, String> {
    if piece.source != "github-release" {
        return Err(if piece.source == "unknown" || piece.source.is_empty() {
            "происхождение не установлено — сверять не с чем".to_string()
        } else {
            format!("способ проверки «{}» пока не поддержан", piece.source)
        });
    }
    let Some(net) = net else {
        return Err("запрошен режим без сети (--offline)".to_string());
    };
    let Some(repo) = github_repo(&piece.upstream) else {
        return Err(format!("не разобрать адрес апстрима: {}", piece.upstream));
    };
    let releases =
        github_releases(net, &repo).map_err(|e| format!("не удалось спросить GitHub: {e}"))?;
    if releases.is_empty() {
        return Err("у апстрима нет ни одного релиза".to_string());
    }
    Ok(releases)
}

fn verdict(piece: &Piece, releases: &[Release]) -> Verdict {
    let latest = &releases[0];
    let Some(ours) = releases.iter().position(|r| r.tag == piece.tag) else {
        return Verdict::Unknown {
            why: format!(
                "тега {} нет среди {} последних релизов апстрима — проверьте tag в {MANIFEST}",
                piece.tag,
                releases.len()
            ),
        };
    };
    if ours == 0 {
        Verdict::Fresh {
            latest: latest.tag.clone(),
            date: date_only(&latest.published),
        }
    } else {
        Verdict::Behind {
            count: ours,
            latest: latest.tag.clone(),
            date: date_only(&latest.published),
        }
    }
}

fn date_only(iso: &str) -> String {
    iso.split('T').next().unwrap_or(iso).to_string()
}

/// «релиз / релиза / релизов» — иначе вывод читается как машинный.
fn releases_word(n: usize) -> &'static str {
    let (n10, n100) = (n % 10, n % 100);
    if n10 == 1 && n100 != 11 {
        "релиз"
    } else if (2..=4).contains(&n10) && !(12..=14).contains(&n100) {
        "релиза"
    } else {
        "релизов"
    }
}

// ── скачиваемый архив ───────────────────────────────────────────────────

/// Сверка с рецептом пакета: версия в `<pkgbuild_var>=…` и сумма архива в
/// `sha256sums`. `Ok(None)` — сверять не с чем (поле не задано).
fn check_pkgbuild(root: &Path, piece: &Piece, sum: &str) -> Result<Option<String>, String> {
    if piece.pkgbuild_var.is_empty() {
        return Ok(None);
    }
    let path = root.join(PKGBUILD_REL);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("не читается {}: {e}", path.display()))?;
    let prefix = format!("{}=", piece.pkgbuild_var);
    let values: Vec<&str> = text
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix(&prefix))
        .map(|v| v.split('#').next().unwrap_or("").trim().trim_matches(['"', '\'']))
        .collect();
    match values.as_slice() {
        [] => return Err(format!("в {PKGBUILD_REL} нет строки {prefix}…")),
        [v] if *v == piece.version => {}
        [v] => {
            return Err(format!(
                "в {PKGBUILD_REL} {prefix}{v}, а в {MANIFEST} version = {}",
                piece.version
            ))
        }
        _ => return Err(format!("в {PKGBUILD_REL} строка {prefix}… не одна")),
    }
    let quoted = [format!("'{sum}'"), format!("\"{sum}\"")];
    if !quoted.iter().any(|q| text.contains(q.as_str())) {
        return Err(format!(
            "суммы архива {}… нет в sha256sums {PKGBUILD_REL}",
            &sum[..sum.len().min(16)]
        ));
    }
    Ok(Some(format!("{prefix}{} и сумма архива совпадают", piece.version)))
}

/// Скачать архив куска, сверить сумму и распаковать в его каталог (всё
/// прежнее, кроме `vendor.toml`, сносится). Корень архива обязан быть
/// каталогом с тем же именем, что у куска: его содержимое и ложится в
/// каталог куска.
fn fetch_archive(net: &mut UreqNet, piece: &Piece, sum: &str) -> Result<usize, String> {
    let repo = github_repo(&piece.upstream)
        .ok_or_else(|| format!("не разобрать адрес апстрима: {}", piece.upstream))?;
    if piece.asset.is_empty() || piece.tag.is_empty() {
        return Err(format!("в {MANIFEST} нет asset или tag — качать нечего"));
    }
    let url = format!(
        "https://github.com/{repo}/releases/download/{}/{}",
        piece.tag, piece.asset
    );
    let body = net
        .get(&url, UA, 120)
        .map_err(|e| format!("не скачался {url}: {e}"))?;
    let got = hex(&Sha256::digest(&body));
    if got != sum {
        return Err(format!(
            "сумма скачанного {} не совпала с записанной {}",
            &got[..16],
            &sum[..sum.len().min(16)]
        ));
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(body))
        .map_err(|e| format!("архив не читается: {e}"))?;
    let top = piece
        .dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Сначала убедиться, что архив целиком наш по раскладке, потом сносить.
    let mut plan = Vec::new();
    for i in 0..zip.len() {
        let f = zip.by_index_raw(i).map_err(|e| format!("архив не читается: {e}"))?;
        let Some(rel) = f.enclosed_name() else {
            return Err(format!("в архиве небезопасный путь: {}", f.name()));
        };
        let Ok(inner) = rel.strip_prefix(&top) else {
            return Err(format!("в архиве путь вне {top}/: {}", f.name()));
        };
        if inner.as_os_str().is_empty() || f.is_dir() {
            continue;
        }
        if inner == Path::new(MANIFEST) {
            return Err(format!("архив несёт свой {MANIFEST} — не затираю наш"));
        }
        plan.push((i, inner.to_path_buf(), f.unix_mode()));
    }
    for e in std::fs::read_dir(&piece.dir).map_err(|e| e.to_string())?.flatten() {
        if e.file_name() == MANIFEST {
            continue;
        }
        let p = e.path();
        let r = if p.is_dir() && !p.is_symlink() {
            std::fs::remove_dir_all(&p)
        } else {
            std::fs::remove_file(&p)
        };
        r.map_err(|err| format!("не удалось убрать {}: {err}", p.display()))?;
    }
    for (i, inner, mode) in &plan {
        let mut f = zip.by_index(*i).map_err(|e| format!("архив не читается: {e}"))?;
        let dest = piece.dir.join(inner);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&dest)
            .map_err(|e| format!("не пишется {}: {e}", dest.display()))?;
        std::io::copy(&mut f, &mut out).map_err(|e| format!("не пишется {}: {e}", dest.display()))?;
        if let Some(m) = mode {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(m & 0o777));
        }
    }
    Ok(plan.len())
}

// ── точка входа ─────────────────────────────────────────────────────────

fn usage() {
    println!(
        "vendor-check — свежесть чужих кусков в поставке Rexon\n\
         \n\
         Ключи:\n\
         \x20 --offline        не ходить в сеть (для сборки без интернета)\n\
         \x20 --корень ПУТЬ    корень репозитория (по умолчанию — текущий каталог)\n\
         \x20 --выкачать       скачать архивы кусков (checksum = sha256:…), сверить\n\
         \x20                  сумму и распаковать рядом с vendor.toml — нужно,\n\
         \x20                  чтобы запускать Rexon и тесты из дерева\n\
         \x20 --пересчитать    переписать сумму содержимого в vendor.toml по нынешнему\n\
         \x20                  каталогу (после осознанной правки или приёма версии)\n\
         \x20 --помощь         эта справка\n\
         \n\
         Возврат: 0 — свежо либо проверить не смогли, 1 — отстали,\n\
         2 — беда с описаниями (битый vendor.toml, изменившееся содержимое,\n\
         сумма архива разошлась с выпуском или с PKGBUILD)."
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut offline = false;
    let mut rewrite = false;
    let mut fetch = false;
    let mut root = PathBuf::from(".");
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--offline" => offline = true,
            "--пересчитать" => rewrite = true,
            "--выкачать" | "--fetch" => fetch = true,
            "--помощь" | "--help" | "-h" => {
                usage();
                return;
            }
            "--корень" | "--root" => {
                i += 1;
                match args.get(i) {
                    Some(p) => root = PathBuf::from(p),
                    None => {
                        eprintln!("после --корень нужен путь");
                        std::process::exit(2);
                    }
                }
            }
            other => {
                eprintln!("неизвестный ключ: {other}");
                usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }
    if fetch && offline {
        eprintln!("--выкачать без сети невозможен");
        std::process::exit(2);
    }

    let mut manifests = Vec::new();
    find_manifests(&root, &mut manifests);
    manifests.sort();

    println!("Свежесть чужих кусков в поставке Rexon");
    println!("Корень: {}\n", root.display());
    if manifests.is_empty() {
        println!("Ни одного {MANIFEST} не найдено — описывать нечего.");
        return;
    }

    let mut net = if offline { None } else { Some(UreqNet::new()) };
    let (mut fresh, mut behind, mut unknown, mut broken) = (0, 0, 0, 0);

    for path in &manifests {
        let piece = match read_piece(path) {
            Ok(p) => p,
            Err(e) => {
                println!("  ❌ {e}\n");
                broken += 1;
                continue;
            }
        };
        println!("  {} — {}", piece.name, piece.description);
        println!(
            "    версия       : {} (тег {}, принято {})",
            if piece.version.is_empty() {
                "неизвестна"
            } else {
                &piece.version
            },
            if piece.tag.is_empty() { "—" } else { &piece.tag },
            if piece.vendored.is_empty() {
                "дата неизвестна"
            } else {
                &piece.vendored
            }
        );
        println!(
            "    апстрим      : {}",
            if piece.upstream.is_empty() {
                "неизвестен"
            } else {
                &piece.upstream
            }
        );

        if let Some(sum) = piece.archive_sum() {
            // ── кусок скачивается архивом ──
            println!("    архив        : {} (sha256 {}…)", piece.asset, &sum[..sum.len().min(16)]);
            match check_pkgbuild(&root, &piece, sum) {
                Ok(Some(ok)) => println!("    PKGBUILD     : {ok}"),
                Ok(None) => println!("    PKGBUILD     : pkgbuild_var не задан — не сверялось"),
                Err(e) => {
                    println!("    PKGBUILD     : ❌ {e}");
                    broken += 1;
                }
            }
            if fetch {
                match fetch_archive(net.as_mut().expect("сеть есть"), &piece, sum) {
                    Ok(n) => println!("    выкачано     : {n} файлов в {}", piece.dir.display()),
                    Err(e) => {
                        println!("    выкачано     : ❌ {e}");
                        broken += 1;
                    }
                }
            }
            let (now, files) = tree_checksum(&piece.dir);
            if files == 0 {
                println!("    содержимое   : не распаковано (для запуска из дерева: --выкачать)");
            } else if rewrite {
                match rewrite_field(path, "tree", &now) {
                    Ok(()) => println!("    содержимое   : сумма переписана ({})", short(&now)),
                    Err(e) => {
                        println!("    ❌ не удалось переписать сумму: {e}");
                        broken += 1;
                    }
                }
            } else if piece.tree.is_empty() {
                println!("    содержимое   : сумма распакованного не записана — нечего сверять");
            } else if piece.tree != now {
                println!(
                    "    содержимое   : ❌ распакованное РАЗОШЛОСЬ с архивом выпуска\n\
                     \x20                  записано {}\n\
                     \x20                  сейчас   {}\n\
                     \x20                  вернуть как было — `--выкачать`",
                    short(&piece.tree),
                    short(&now)
                );
                broken += 1;
            } else {
                println!("    содержимое   : распакованное совпадает с архивом выпуска");
            }
        } else {
            // ── кусок лежит в дереве ──
            // Содержимое не должно меняться незаметно: сумма записана при
            // вкладывании и пересчитывается здесь.
            let (now, _) = tree_checksum(&piece.dir);
            if rewrite {
                if let Err(e) = rewrite_field(path, "checksum", &now) {
                    println!("    ❌ не удалось переписать сумму: {e}");
                    broken += 1;
                } else {
                    println!("    содержимое   : сумма переписана ({})", short(&now));
                }
            } else if piece.checksum.is_empty() {
                println!("    содержимое   : сумма не записана — нечего сверять");
            } else if piece.checksum != now {
                println!(
                    "    содержимое   : ❌ РАЗОШЛОСЬ с записанным при вкладывании\n\
                     \x20                  записано {}\n\
                     \x20                  сейчас   {}\n\
                     \x20                  если правка осознанная — `--пересчитать`",
                    short(&piece.checksum),
                    short(&now)
                );
                broken += 1;
            } else {
                println!("    содержимое   : совпадает с записанным при вкладывании");
            }
        }

        let v = match upstream_releases(&piece, net.as_mut()) {
            Ok(releases) => {
                if let Some(sum) = piece.archive_sum() {
                    let digest = releases
                        .iter()
                        .find(|r| r.tag == piece.tag)
                        .and_then(|r| r.digests.get(&piece.asset));
                    match digest {
                        Some(d) if d.strip_prefix(ARCHIVE_PREFIX) == Some(sum) => {
                            println!("    выпуск       : сумма архива = digest выпуска");
                        }
                        Some(d) => {
                            println!(
                                "    выпуск       : ❌ сумма архива НЕ совпадает с digest выпуска ({d})"
                            );
                            broken += 1;
                        }
                        None => println!(
                            "    выпуск       : digest ассета {} у тега {} не найден — не сверено",
                            piece.asset, piece.tag
                        ),
                    }
                }
                verdict(&piece, &releases)
            }
            Err(why) => Verdict::Unknown { why },
        };
        match v {
            Verdict::Fresh { latest, date } => {
                println!("    у апстрима   : {latest} от {date}");
                println!("    состояние    : СВЕЖИЙ");
                fresh += 1;
            }
            Verdict::Behind {
                count,
                latest,
                date,
            } => {
                println!("    у апстрима   : {latest} от {date}");
                println!(
                    "    состояние    : ОТСТАЛИ на {count} {} — у нас {}, у апстрима {latest}",
                    releases_word(count),
                    if piece.tag.is_empty() { "—" } else { &piece.tag }
                );
                behind += 1;
            }
            Verdict::Unknown { why } => {
                println!("    состояние    : ПРОВЕРИТЬ НЕ СМОГ — {why}");
                unknown += 1;
            }
        }
        if !piece.note.is_empty() {
            println!("    примечание   : {}", piece.note);
        }
        println!();
    }

    println!(
        "Итог: кусков {}, свежих {fresh}, отставших {behind}, не проверено {unknown}, \
         с бедой в описании {broken}",
        manifests.len()
    );
    if behind > 0 {
        println!(
            "\nОтставшие куски надо обновить: у пользователя они молча ломаются, \n\
             без единой ошибки в журнале."
        );
    }
    if unknown > 0 && offline {
        println!("\nБез сети свежесть не проверяется — это не «всё хорошо», а «не смотрел».");
    }

    if broken > 0 {
        std::process::exit(2);
    }
    if behind > 0 {
        std::process::exit(1);
    }
}

fn short(sum: &str) -> String {
    let body = sum.strip_prefix(SUM_PREFIX).unwrap_or(sum);
    format!("{SUM_PREFIX}{}…", &body[..body.len().min(16)])
}

/// Переписать строку `key = "…"` в существующем описании, не трогая
/// остального (нет строки — дописать в конец).
fn rewrite_field(manifest: &Path, key: &str, sum: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(manifest).map_err(|e| e.to_string())?;
    let mut out = String::with_capacity(text.len());
    let mut seen = false;
    let line_for = |k: &str| format!("{k:<11} = \"{sum}\"\n");
    for line in text.lines() {
        let t = line.trim_start();
        let is_key = t
            .strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with('='));
        if is_key && !seen {
            out.push_str(&line_for(key));
            seen = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !seen {
        out.push_str(&line_for(key));
    }
    std::fs::write(manifest, out).map_err(|e| e.to_string())
}
