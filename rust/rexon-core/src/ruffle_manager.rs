//! Pure logic of the Ruffle manager — port of `rexon/ruffle_manager.py`.
//!
//! Ruffle is a Flash emulator (MIT + Apache 2.0) that Rexon uses to render
//! `.swf` as wallpapers. The original `RuffleManager` class mixes deterministic
//! decisions (which release asset to download for the platform, how to build
//! the URL, the argv for launching a SWF, storage paths in
//! `~/.config/rexon/ruffle/`, version choice) with side effects (actual
//! downloading/HTTP, unpacking tar/zip, writing the binary, `chmod +x`,
//! `subprocess.Popen`, `os.path` checks). Here is ONLY the pure part: input →
//! output, without the file system or the network.
//!
//! Observations of the environment (whether the binary is installed, whether
//! there is a system ruffle in PATH, whether there is a web version, the output
//! of `ruffle --version`, the contents of `.version`, `platform.machine()`) are
//! passed as PARAMETERS. Side effects (downloading, unpacking, `Popen`,
//! `os.makedirs`, `shutil.rmtree`) stay in the Python shim `ruffle_manager.py`,
//! which executes what the core decided (the asset URL, argv, paths, the ensure
//! plan).
//!
//! Byte-for-byte parity with live Python — `tests/ruffle_manager_parity.rs` (the
//! reference was captured read-only, without real downloading/launching of a
//! subprocess).
//!
//! Warning: the original has NO version comparison: `.version` stores the string
//! `nightly-<date>`, `get_version()` only reads it, and updating is decided by
//! the presence of files (`ensure_installed`), not by the version number. So the
//! building/reading of the version string and the choice of source
//! (`resolve_version`) are ported here, but there is no version comparison —
//! there is nothing to compare.
//!
//! A bug of the original was fixed (in the side-effect part, the fix was also
//! made in `ruffle_manager.py`): `RuffleManager.remove()` is declared `-> bool`
//! but always returned `None` — it does not concern the pure part, noted in the
//! .py.

// ============================================================================
// Constants (port of the module constants of ruffle_manager.py)
// ============================================================================

/// Base of Ruffle's GitHub Releases — port of `GITHUB_RELEASES_URL`.
pub const GITHUB_RELEASES_URL: &str = "https://github.com/ruffle-rs/ruffle/releases";

/// Expected binary size (MB) — the progress fallback when Content-Length is empty.
pub const EXPECTED_SIZE_MB: u64 = 25;
/// Expected size of the self-hosted web version (MB).
pub const EXPECTED_SIZE_WEB_MB: u64 = 18;

/// URL of the self-hosted web version (ruffle.js + ruffle.wasm) — port of
/// `DOWNLOAD_URL_WEB`.
///
/// Warning: a dead link (checked with a HEAD request on 15.09.2026): `latest`
/// leads to release v0.6.0, which has no `ruffle-nightly-*` files — the answer is
/// 404. Live nightly builds carry the date in their name. Rexon does not show
/// Flash wallpapers this way: the Rust executor calls the host mode
/// `rexon-wallpaper-host --mode swf` (Ruffle as a library). The function lives
/// on while the classic Python window calls it (`rexon/ruffle_manager.py`
/// through the `rexon-py` bridge).
pub fn web_download_url() -> String {
    format!("{GITHUB_RELEASES_URL}/latest/download/ruffle-nightly-web-selfhosted.zip")
}

/// Release asset URL by architecture — port of `DOWNLOAD_URLS.get(arch)`. Only
/// `x86_64` / `aarch64` are supported (as in the original); anything else → None
/// (Python: "Architecture … is not supported by Ruffle").
///
/// Warning: the same dead link `latest/download/ruffle-nightly-*` (404, see
/// [`web_download_url`]); needed only by the classic Python window.
pub fn download_url(arch: &str) -> Option<String> {
    match arch {
        "x86_64" => Some(format!(
            "{GITHUB_RELEASES_URL}/latest/download/ruffle-nightly-linux-x86_64.tar.gz"
        )),
        "aarch64" => Some(format!(
            "{GITHUB_RELEASES_URL}/latest/download/ruffle-nightly-linux-aarch64.tar.gz"
        )),
        _ => None,
    }
}

// ============================================================================
// Architecture — port of get_arch
// ============================================================================

/// Normalizing `platform.machine()` → the asset key. Port of `get_arch`: the
/// string is lowercased, `x86_64/amd64` → `x86_64`, `aarch64/arm64` →
/// `aarch64`, anything else as is (in lower case).
pub fn normalize_arch(machine: &str) -> String {
    let m = machine.to_lowercase();
    match m.as_str() {
        "x86_64" | "amd64" => "x86_64".to_string(),
        "aarch64" | "arm64" => "aarch64".to_string(),
        _ => m,
    }
}

// ============================================================================
// Storage paths — port of the module RUFFLE_* (expanduser("~/.config/rexon/ruffle"))
// ============================================================================

/// Ruffle paths relative to the home directory. Port of
/// `RUFFLE_DIR`/`RUFFLE_BIN`/`RUFFLE_WEB_DIR`/`RUFFLE_VERSION_FILE`
/// (+ `web/ruffle.js`). `home` — what `~` expands to (`$HOME`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RufflePaths {
    pub dir: String,
    pub bin: String,
    pub web_dir: String,
    pub version_file: String,
    pub web_js: String,
}

/// Build the paths from the home directory.
pub fn ruffle_paths(home: &str) -> RufflePaths {
    ruffle_paths_xdg(None, home)
}

/// The same honoring `$XDG_CONFIG_HOME` (the single config root of the core).
pub fn ruffle_paths_xdg(xdg_config_home: Option<&str>, home: &str) -> RufflePaths {
    let dir = format!(
        "{}/rexon/ruffle",
        crate::utils::config_home(xdg_config_home, home)
    );
    let web_dir = format!("{dir}/web");
    RufflePaths {
        bin: format!("{dir}/ruffle"),
        version_file: format!("{dir}/.version"),
        web_js: format!("{web_dir}/ruffle.js"),
        web_dir,
        dir,
    }
}

// ============================================================================
// Presence / choice of the binary — port of is_installed / get_ruffle_path
// ============================================================================

/// Whether there is SOMETHING executable at the path — port of `is_installed`
/// (`isfile && os.access(X_OK)`). The observations are passed as parameters.
///
/// Warning: as an answer to "is Ruffle installed" this is NOT ENOUGH: the check
/// tells whether a file is present, not whether the player is usable (the
/// analysis is in the block below). Ask [`is_installed_usable`] instead; this one
/// remains only because the PyO3 bridge holds its signature.
pub fn is_installed(bin_is_file: bool, bin_is_executable: bool) -> bool {
    bin_is_file && bin_is_executable
}

// ============================================================================
// Binary usability — "installed" ≠ "something lies at the path"
// ============================================================================
//
// `is_installed` above answers only "the file is in place and executable", and
// that is NOT ENOUGH. Wehrwolfmann had a ten-byte `ruffle` file in
// `~/.config/rexon/ruffle/` — the line `#!/bin/sh` with the +x bit. Both checks
// let it through: Rexon considered the player installed, `get_ruffle_path`
// handed the stub to `run_swf`, and `ensure_decision` decided there was nothing
// to download. A buyer on such a machine cannot play a single `.swf` and does
// not even get an offer to install Ruffle.
//
// This is the same class of defect as "Popen did not raise an exception — so
// the wallpaper is running" (commit 3661e21): the check looks at PRESENCE, not
// at USABILITY. It is cured the same way — we ask the object for something
// only the real object can do.
//
// The sign is chosen so that the check can be called often (it is called by
// `is_installed`, `get_ruffle_path`, `ensure_installed`, the wallpaper tab):
// starting a process on every sneeze is not allowed, while reading 20 bytes of
// the file's head is cheaper than `os.access` itself. We look at what makes the
// file an executable player:
//
//   * the ELF signature (`\x7fELF`) — cuts off stub scripts, HTML error pages
//     instead of an archive, fragments and text "notes to self";
//   * a 64-bit little-endian ELF — Ruffle publishes no other builds for Linux;
//   * file type ET_EXEC/ET_DYN — not an object file and not a library;
//   * `e_machine` matches the machine's architecture — a binary downloaded for
//     another architecture will not run;
//   * the size is no less than [`MIN_BINARY_BYTES`] — catches an interrupted
//     download where the head is already correct but the tail is missing.
//
// The size is LAST here and auxiliary precisely because the sign is weak:
// tomorrow the stub will be not 10 bytes but 200 KB. The load-bearing check is
// the signature and `e_machine`.

/// How many bytes of the file's head must be read to judge usability. Exactly
/// the ELF header up to and including `e_machine`.
pub const BINARY_HEAD_BYTES: usize = 20;

/// Lower bound of a plausible Ruffle binary size (bytes).
///
/// Ruffle nightly for Linux is tens of megabytes (it contains wgpu, its own
/// renderer and the whole ActionScript machine). A 4 MB threshold does not cut
/// off any real build and still catches an interrupted download.
pub const MIN_BINARY_BYTES: u64 = 4 * 1024 * 1024;

/// Architecture code in the `e_machine` field of the ELF header.
///
/// The input is the already normalized string from [`normalize_arch`].
/// Architectures for which Ruffle publishes no builds (see [`download_url`])
/// have no code: a usable binary cannot come from anywhere there.
pub fn elf_machine(arch: &str) -> Option<u16> {
    match arch {
        "x86_64" => Some(0x3E),
        "aarch64" => Some(0xB7),
        _ => None,
    }
}

/// Whether the file looks like an executable Ruffle binary for this machine.
///
/// `head` — the first [`BINARY_HEAD_BYTES`] bytes of the file (fewer — no right
/// away), `size` — its size, `arch` — the machine's normalized architecture.
/// Reading the file and `platform.machine()` are the executor's observations.
pub fn binary_is_runnable(head: &[u8], size: u64, arch: &str) -> bool {
    if head.len() < BINARY_HEAD_BYTES || size < MIN_BINARY_BYTES {
        return false;
    }
    if &head[0..4] != b"\x7fELF" {
        return false;
    }
    // EI_CLASS = ELFCLASS64, EI_DATA = ELFDATA2LSB.
    if head[4] != 2 || head[5] != 1 {
        return false;
    }
    // e_type: ET_EXEC (2) or ET_DYN (3) — Ruffle is built as PIE, i.e. ET_DYN;
    // ET_EXEC is kept in case of a build without PIE.
    let e_type = u16::from_le_bytes([head[16], head[17]]);
    if e_type != 2 && e_type != 3 {
        return false;
    }
    let e_machine = u16::from_le_bytes([head[18], head[19]]);
    elf_machine(arch) == Some(e_machine)
}

/// Whether a USABLE binary is installed — a stricter [`is_installed`].
///
/// A separate function rather than a third parameter of [`is_installed`],
/// because the latter's signature is held by the PyO3 bridge; when the bridge is
/// switched to this one, [`is_installed`] will go away together with its call.
pub fn is_installed_usable(
    bin_is_file: bool,
    bin_is_executable: bool,
    bin_is_runnable: bool,
) -> bool {
    is_installed(bin_is_file, bin_is_executable) && bin_is_runnable
}

/// Whether the unpacked web version (`ruffle.js` + `.wasm`) is usable.
///
/// The same defect here: `is_web_installed` in the shim checks only
/// `isfile(ruffle.js)`, and the nine-byte `// ruffle` from the same folder of
/// Wehrwolfmann's passes it. `ruffle.js` by itself is a loader: without a
/// `.wasm` next to it, it will play nothing, so usability = a non-empty loader
/// AND at least one `.wasm` module in the directory.
pub const MIN_WEB_JS_BYTES: u64 = 1024;

/// Decision on the web version's usability from observations of the directory.
pub fn web_is_usable(js_is_file: bool, js_size: u64, wasm_count: usize) -> bool {
    js_is_file && js_size >= MIN_WEB_JS_BYTES && wasm_count > 0
}

/// Names of the Ruffle binary in the packages of different distributions — port
/// of `RuffleManager.SYSTEM_BINS`. The order matters: `system_bin` takes the
/// FIRST name found in `PATH`.
pub const SYSTEM_BINS: &[&str] = &["ruffle", "ruffle-nightly", "ruffle_desktop"];

/// System Ruffle from the distribution's package — port of `system_bin`: going
/// through [`SYSTEM_BINS`] until the first one found. The `PATH` lookup
/// (`shutil.which`) is a side effect, passed as the `which` closure that returns
/// the found path.
pub fn select_system_bin<F>(which: F) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    SYSTEM_BINS.iter().find_map(|name| which(name))
}

/// Whether to copy a file from the archive next to the binary — port of the
/// predicate `f.startswith("LICENSE") or f == "README.md"` in `download()`.
/// Ruffle's licenses (MIT + Apache 2.0) must lie next to the binary: we do not
/// distribute it ourselves, but they must be available to the user.
pub fn is_extra_asset(name: &str) -> bool {
    name.starts_with("LICENSE") || name == "README.md"
}

/// Path to the binary: ours → the system one (from PATH, literally `"ruffle"`)
/// → none. Port of `get_ruffle_path`.
pub fn ruffle_path(
    is_installed: bool,
    is_system_installed: bool,
    bin_path: &str,
) -> Option<String> {
    // Wehrwolfmann's decision of 2026-07-29: first the package from the
    // distribution's repository, our downloaded copy is only a fallback. The
    // package is updated and fixed together with the system, signed with its
    // key, and does not require pulling 25 MB past the package manager.
    if is_system_installed {
        Some("ruffle".to_string())
    } else if is_installed {
        Some(bin_path.to_string())
    } else {
        None
    }
}

// ============================================================================
// Version — port of get_version (choice of source; the original has no version comparison)
// ============================================================================

/// Choice of the version string — port of the `get_version` logic:
///   * no path to ruffle → None (the `.version` file is NOT read — as in Python);
///   * `ruffle --version` finished successfully (rc==0) → its stdout, trimmed
///     at the edges (`cmd_output = Some(stdout)` when rc==0);
///   * otherwise — the contents of `.version`, trimmed (`version_file`);
///   * otherwise None.
pub fn resolve_version(
    ruffle_path: Option<&str>,
    cmd_output: Option<&str>,
    version_file: Option<&str>,
) -> Option<String> {
    ruffle_path?;
    if let Some(s) = cmd_output {
        return Some(s.trim().to_string());
    }
    if let Some(c) = version_file {
        return Some(c.trim().to_string());
    }
    None
}

/// Contents of the `.version` file written by `download()` — port of
/// `f"nightly-{time.strftime('%Y-%m-%d')}"`. The date is passed as a parameter
/// (computing it is the executor's side effect).
pub fn version_file_content(date: &str) -> String {
    format!("nightly-{date}")
}

/// Reading the version string from the `.version` file — port of `f.read().strip()`.
pub fn parse_version_file(content: &str) -> String {
    content.trim().to_string()
}

// ============================================================================
// Launching a SWF — port of run_swf / run_swf_as_wallpaper (argv + wrapper script)
// ============================================================================

/// argv for launching a SWF directly — port of `run_swf`. `swf_abspath` is an
/// already absolute path (`os.path.abspath` is a side effect that depends on the
/// cwd — the shim does it).
pub fn run_swf_argv(ruffle: &str, swf_abspath: &str, fullscreen: bool) -> Vec<String> {
    let mut cmd = vec![ruffle.to_string()];
    if fullscreen {
        cmd.push("--fullscreen".to_string());
    }
    cmd.push("--graphics".to_string());
    cmd.push("default".to_string());
    cmd.push(swf_abspath.to_string());
    cmd
}

/// Base name without the extension — port of
/// `os.path.splitext(os.path.basename(p))[0]` (posix semantics: the last `/` →
/// basename, the last dot → the extension, leading dots do NOT count as an
/// extension).
pub fn swf_stem(swf_path: &str) -> String {
    splitext_root(basename(swf_path)).to_string()
}

fn basename(p: &str) -> &str {
    match p.rfind('/') {
        Some(i) => &p[i + 1..],
        None => p,
    }
}

/// Root of the name (the part before the extension) for a basename without `/`
/// — port of `posixpath.splitext` (leading dots are not split off).
fn splitext_root(name: &str) -> &str {
    let Some(dot) = name.rfind('.') else {
        return name;
    };
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < dot {
        if bytes[i] != b'.' {
            return &name[..dot];
        }
        i += 1;
    }
    // Everything before the dot is dots themselves (e.g. ".hidden"): there is no
    // extension.
    name
}

/// Path of the wrapper script — port of `wrapper_dir/f"run_{swf_name}.sh"`.
pub fn wrapper_path(dir: &str, swf_stem: &str) -> String {
    format!("{dir}/wrappers/run_{swf_stem}.sh")
}

/// Contents of the bash wrapper that launches Ruffle — port of the heredoc from
/// `run_swf_as_wallpaper` (always `--fullscreen`). `swf_abspath` is absolute.
pub fn wrapper_script(ruffle: &str, swf_abspath: &str) -> String {
    format!("#!/bin/bash\nexec \"{ruffle}\" --fullscreen \"{swf_abspath}\"\n")
}

/// argv for launching a SWF as a wallpaper — `rexon-wallpaper-host --mode swf <movie>`.
///
/// `host` — the path to the built wallpaper host
/// ([`crate::wallpaper_engine::find_wallpaper_host_binary`]), `swf_abspath` —
/// the absolute path to the movie itself. An external Ruffle and the bash
/// wrapper ([`wrapper_script`]) are not needed on this route: Flash plays
/// NATIVELY, Ruffle is linked as a library (the `rexon-swf` crate), and the host
/// writes the frames itself.
///
/// ⭐ Fix of 16.09.2026 (block H). A dead path stood here:
/// `<python> -m rexon.app_wallpaper <wrapper> --mode subprocess`: the module
/// `rexon.app_wallpaper` was removed from the tree by commit `2091eac`, the
/// process crashed immediately, and SWF wallpapers had not worked since
/// 23.08.2026 — silently, with a green report "wallpaper applied".
///
/// Warning: A TAIL FOR THE BRIDGE. The signature is kept (two `&str` →
/// `Vec<String>`), but the MEANING of both arguments is different. The
/// `rexon-py` bridge (`lib.rs: ruffle_wallpaper_argv`) and
/// `rexon/ruffle_manager.py:492` still pass the interpreter and the wrapper path
/// here — they need to be switched to the host and the movie. This causes no
/// regression: the old route was dead anyway. The Rust executor does not use
/// this function at all — it builds the plan
/// [`crate::wallpaper_engine::swf_host_plan`].
pub fn wallpaper_argv(host: &str, swf_abspath: &str) -> Vec<String> {
    use crate::wallpaper_engine::{wallpaper_host_argv, HostMode};
    wallpaper_host_argv(host, HostMode::Swf, swf_abspath)
}

// ============================================================================
// ensure_installed — deciding what to download
// ============================================================================

/// What needs to be installed — port of the first part of `ensure_installed`.
/// The final success (`return success`) is determined by the EXECUTOR from the
/// result of the real `download()`/`download_web()`; the web version is not
/// critical (only a warning).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureDecision {
    pub need_bin: bool,
    pub need_web: bool,
}

/// The ensure decision from observations of the environment.
pub fn ensure_decision(
    is_installed: bool,
    is_system_installed: bool,
    is_web_installed: bool,
) -> EnsureDecision {
    EnsureDecision {
        need_bin: !is_installed && !is_system_installed,
        need_web: !is_web_installed,
    }
}

impl EnsureDecision {
    /// Everything is in place — the early `return True` in Python (nothing to download).
    pub fn already_ok(&self) -> bool {
        !self.need_bin && !self.need_web
    }
}

// ============================================================================
// Download progress — pure arithmetic of _download_file / the CLI progress bar
// ============================================================================

/// Download percentage — port of `min(75, int(downloaded/total*75))` (the
/// download phase takes 0-75%; unpacking/installation — 80/90/100 separately).
/// `total` is assumed to be > 0 (the executor substitutes
/// `EXPECTED_SIZE_MB * 1024 * 1024` when Content-Length is empty).
pub fn download_progress_pct(downloaded: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    let pct = ((downloaded as f64 / total as f64) * 75.0) as u64;
    pct.min(75)
}

/// Width of the CLI progress bar — port of `bar_w = 30` from the CLI `progress()`.
pub const PROGRESS_BAR_WIDTH: usize = 30;

/// CLI progress bar — port of `"█"*filled + "░"*(bar_w-filled)`,
/// `filled = int(pct/100*bar_w)`.
pub fn progress_bar(pct: u64) -> String {
    let filled = (((pct as f64) / 100.0) * PROGRESS_BAR_WIDTH as f64) as usize;
    let filled = filled.min(PROGRESS_BAR_WIDTH);
    let empty = PROGRESS_BAR_WIDTH - filled;
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Head of a 64-bit little-endian ELF: type `e_type`, machine `e_machine`.
    fn elf_head(e_type: u16, e_machine: u16) -> Vec<u8> {
        let mut h = vec![0u8; BINARY_HEAD_BYTES];
        h[0..4].copy_from_slice(b"\x7fELF");
        h[4] = 2; // ELFCLASS64
        h[5] = 1; // ELFDATA2LSB
        h[16..18].copy_from_slice(&e_type.to_le_bytes());
        h[18..20].copy_from_slice(&e_machine.to_le_bytes());
        h
    }

    const BIG: u64 = 60 * 1024 * 1024;

    /// Exactly the file Wehrwolfmann had: `#!/bin/sh` with the +x bit.
    #[test]
    fn shell_stub_is_not_a_player() {
        let stub = b"#!/bin/sh\n";
        assert!(!binary_is_runnable(stub, stub.len() as u64, "x86_64"));
        // And all of the product's output on it: "installed" no longer comes out.
        assert!(is_installed(true, true), "старая проверка заглушку пропускала");
        assert!(!is_installed_usable(true, true, false));
    }

    /// A real Ruffle build (PIE for x86_64) is usable.
    #[test]
    fn real_ruffle_binary_passes() {
        let head = elf_head(3, 0x3E);
        assert!(binary_is_runnable(&head, BIG, "x86_64"));
        assert!(is_installed_usable(true, true, true));
        // A build without PIE (ET_EXEC) too.
        assert!(binary_is_runnable(&elf_head(2, 0x3E), BIG, "x86_64"));
        // And the same on aarch64.
        assert!(binary_is_runnable(&elf_head(3, 0xB7), BIG, "aarch64"));
    }

    /// A binary for another architecture will not run — so it is not installed.
    #[test]
    fn foreign_architecture_rejected() {
        assert!(!binary_is_runnable(&elf_head(3, 0xB7), BIG, "x86_64"));
        assert!(!binary_is_runnable(&elf_head(3, 0x3E), BIG, "aarch64"));
        // An architecture for which Ruffle releases no builds.
        assert_eq!(elf_machine("riscv64"), None);
        assert!(!binary_is_runnable(&elf_head(3, 0xF3), BIG, "riscv64"));
    }

    /// Non-ELF, 32-bit, big-endian and non-executable ELF — rejected.
    #[test]
    fn wrong_shape_rejected() {
        // An HTML error page instead of an archive.
        let html = b"<!DOCTYPE html><html><head><ti";
        assert!(!binary_is_runnable(html, BIG, "x86_64"));
        let mut h32 = elf_head(3, 0x3E);
        h32[4] = 1; // ELFCLASS32
        assert!(!binary_is_runnable(&h32, BIG, "x86_64"));
        let mut hbe = elf_head(3, 0x3E);
        hbe[5] = 2; // ELFDATA2MSB
        assert!(!binary_is_runnable(&hbe, BIG, "x86_64"));
        // ET_REL (an object file) and ET_CORE are not players.
        assert!(!binary_is_runnable(&elf_head(1, 0x3E), BIG, "x86_64"));
        assert!(!binary_is_runnable(&elf_head(4, 0x3E), BIG, "x86_64"));
    }

    /// An interrupted download: the head is correct, the tail is missing.
    #[test]
    fn truncated_download_rejected() {
        let head = elf_head(3, 0x3E);
        assert!(!binary_is_runnable(&head, 1024 * 1024, "x86_64"));
        assert!(binary_is_runnable(&head, MIN_BINARY_BYTES, "x86_64"));
        // An empty file and a head that is too short.
        assert!(!binary_is_runnable(&[], 0, "x86_64"));
        assert!(!binary_is_runnable(&head[..8], BIG, "x86_64"));
    }

    /// The +x bit and the presence of the file remain mandatory.
    #[test]
    fn usable_needs_all_three() {
        assert!(!is_installed_usable(false, true, true));
        assert!(!is_installed_usable(true, false, true));
        assert!(!is_installed_usable(true, true, false));
        assert!(is_installed_usable(true, true, true));
    }

    /// Web version: a nine-byte `// ruffle` and a loader without `.wasm` do not count.
    #[test]
    fn web_stub_is_not_installed() {
        assert!(!web_is_usable(true, 9, 0));
        assert!(!web_is_usable(true, 9, 1));
        assert!(!web_is_usable(true, 512 * 1024, 0));
        assert!(!web_is_usable(false, 512 * 1024, 1));
        assert!(web_is_usable(true, 512 * 1024, 1));
    }
}
