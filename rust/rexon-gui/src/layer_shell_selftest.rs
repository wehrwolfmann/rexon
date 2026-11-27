// Wallpaper layer test bench: proof that a window of the Rust path really
// becomes a wlr-layer-shell background surface on a live KWin Wayland.
//
// Why the bench is built from two processes. The only honest proof of
// "the window is in the wallpaper layer" is the bytes the client sent to
// the compositor: reading back from LayerShellQt shows only OUR OWN state
// and would pass even if the library silently did nothing. So the parent
// runs itself with WAYLAND_DEBUG=1, collects the protocol trace into a file
// and gives the verdict from the trace, not from the child's self-report.
//
// What the trace proves:
//   * zwlr_layer_shell_v1.get_layer_surface with the argument layer = 0
//     (background) — the window was created AS a layer surface, not as an
//     ordinary one;
//   * set_anchor(15) — anchors on all four sides;
//   * set_exclusive_zone(-1) — the wallpaper zone;
//   * an incoming zwlr_layer_surface_v1.configure — the compositor ACCEPTED
//     the surface and gave it geometry (on a refusal there would be a
//     protocol error);
//   * the whole trace has not a single xdg_surface.get_toplevel — the window
//     was never an ordinary window even for an instant, and so it cannot
//     be in the task list either: task lists (plasma-window-management)
//     show xdg_toplevel, and layer surfaces do not get there by the design
//     of the protocol.
//
// Warning: the previous watchdog of the bench exited with code 0 on a
// timeout — "did not report" looked like "passed". Here the child's
// silence, a missing trace, an empty trace and any divergence give a
// NON-ZERO code.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The argument with which the parent runs itself as the test subject
/// window.
pub const PROBE_ARG: &str = "--layer-shell-probe";
/// The argument for running the whole bench.
pub const SELFTEST_ARG: &str = "--layer-shell-selftest";
/// The bench bite-test argument: `--layer-shell-bite=<kind>`. A run with a
/// deliberately spoiled layer MUST go red; green here is a failure.
pub const BITE_ARG: &str = "--layer-shell-bite";
/// The variable through which the parent passes the bite kind to the test
/// subject window.
pub const BITE_ENV: &str = "REXON_LS_BITE";

/// Bite kinds — each breaks the wallpaper layer in its own way. The
/// breakages themselves are analysed in qml/LayerShellProbe.qml; here there
/// is only the list for the "all in a row" run.
pub const BITE_KINDS: &[&str] = &["layer", "anchors", "zone", "keyboard", "size", "late"];

/// Surface identifier for the compositor. It is also searched for in the
/// trace — by it the bench tells its own window from others, should others
/// turn up in the trace.
pub const SCOPE: &str = "rexon-layer-shell-selftest";

/// How long to wait for the child before considering the bench failed.
const CHILD_TIMEOUT: Duration = Duration::from_secs(40);

/// Killer of the child process. It exists for one reason: the window on
/// Wehrwolfmann's LIVE desktop must disappear under any outcome — a failed
/// check, a panic, an early `return`. Closing "at the end of the function"
/// does not guarantee that, Drop does.
struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn get(&mut self) -> &mut Child {
        self.0.as_mut().expect("процесс-ребёнок уже отпущен")
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            match child.try_wait() {
                Ok(Some(_)) => {}
                _ => {
                    // Kill and MANDATORILY wait: without wait the process stays a zombie, and
                    // its window stays on screen until the compositor notices the broken
                    // socket.
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}

/// The parent half of the bench: an ordinary run, the layer must come up.
pub fn run() -> i32 {
    run_with_bite("")
}

/// Checking the bench itself on a live compositor. Each bite kind brings up
/// a window with a DELIBERATELY spoiled layer and requires the verdict to
/// be red.
///
/// Why this is separate from the `judge` mutation tests below: those test
/// the trace parser on made-up lines. Here a REAL window breaks on a REAL
/// compositor — that catches the case when the bench goes green not because
/// the layer came up but because the checks were never reached.
///
/// An empty `kind` means all kinds are run in a row.
pub fn run_bite(kind: &str) -> i32 {
    let kinds: Vec<&str> = if kind.is_empty() {
        BITE_KINDS.to_vec()
    } else {
        vec![kind]
    };
    let mut failed = 0;
    for k in &kinds {
        println!("REXON-LS-BITE укус «{k}»: прогон обязан покраснеть");
        let code = run_with_bite(k);
        // 1 — the bench found a divergence, i.e. the bite was caught. 0 — the
        // bench stayed green on a deliberately broken layer, which is its failure.
        // 2 — the run did not happen, there is nothing to prove.
        match code {
            1 => println!("REXON-LS-BITE OK укус «{k}» пойман"),
            0 => {
                println!("REXON-LS-BITE FAIL укус «{k}» остался НЕЗАМЕЧЕННЫМ — стенд врёт");
                failed += 1;
            }
            other => {
                println!("REXON-LS-BITE FAIL укус «{k}»: прогон не состоялся (код {other})");
                failed += 1;
            }
        }
    }
    println!(
        "REXON-LS-BITE-VERDICT {} проверено={} провалено={}",
        if failed == 0 { "OK" } else { "FAIL" },
        kinds.len(),
        failed
    );
    i32::from(failed != 0)
}

/// One run. An empty `bite` means an honest wallpaper layer; otherwise the
/// test subject window is handed a deliberate breakage (see `BITE_KINDS`).
///
/// Returns the process exit code: 0 — proven, 1 — a divergence, 2 — the run
/// did not happen.
pub fn run_with_bite(bite: &str) -> i32 {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        eprintln!(
            "REXON-LS-VERDICT FAIL стенд слоя обоев требует живой сессии Wayland \
             (WAYLAND_DISPLAY не задан); под offscreen/xcb проверять нечего"
        );
        return 2;
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("REXON-LS-VERDICT FAIL не удалось найти собственный бинарь: {e}");
            return 2;
        }
    };

    let trace_path = trace_file_path();
    let trace_file = match std::fs::File::create(&trace_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!(
                "REXON-LS-VERDICT FAIL не удалось создать файл следа {}: {e}",
                trace_path.display()
            );
            return 2;
        }
    };

    let mut command = Command::new(&exe);
    command.arg(PROBE_ARG);
    // The bite kind goes to the child as an environment variable, not an
    // argument: the window reads it through setInitialProperties, and the
    // product's argument parsing has nothing to do with it.
    if !bite.is_empty() {
        command.env(BITE_ENV, bite);
    }
    let spawned = command
        // The key to the whole bench: libwayland prints every protocol message to
        // stderr. It is set for the child, not for ourselves — the parent does not
        // need Wayland at all.
        .env("WAYLAND_DEBUG", "1")
        // Without this, on CachyOS QML messages go to journald when stderr is not
        // a terminal, and the child's report does not get into the trace.
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .stdout(Stdio::inherit())
        .stderr(Stdio::from(trace_file))
        .spawn();

    let mut guard = match spawned {
        Ok(child) => ChildGuard(Some(child)),
        Err(e) => {
            eprintln!("REXON-LS-VERDICT FAIL не удалось запустить подопытное окно: {e}");
            return 2;
        }
    };

    let started = Instant::now();
    let mut child_code: Option<i32> = None;
    while started.elapsed() < CHILD_TIMEOUT {
        match guard.get().try_wait() {
            Ok(Some(status)) => {
                child_code = Some(status.code().unwrap_or(-1));
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => {
                eprintln!("REXON-LS-VERDICT FAIL опрос подопытного окна сорвался: {e}");
                return 2;
            }
        }
    }
    // The guard is still alive here; if the child has not finished, the Drop
    // below will kill it — the window leaves the screen in any case.

    let trace = read_trace(&trace_path);
    // The child's trace is useful both on success and on failure: the QML
    // report lines lie in the same file as the protocol.
    for line in trace.lines().filter(|l| l.contains("REXON-LS-")) {
        println!("{}", line.trim());
    }

    let verdict = judge(child_code, &trace);
    for problem in &verdict.problems {
        println!("REXON-LS-FAIL {problem}");
    }
    println!(
        "REXON-LS-VERDICT {} problems={} след={}",
        if verdict.problems.is_empty() { "OK" } else { "FAIL" },
        verdict.problems.len(),
        trace_path.display()
    );
    if verdict.problems.is_empty() {
        // The trace is not needed: it is large (tens of thousands of protocol
        // lines).
        let _ = std::fs::remove_file(&trace_path);
        0
    } else {
        1
    }
}

fn trace_file_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!("rexon-layer-shell-trace-{}.log", std::process::id()));
    path
}

fn read_trace(path: &PathBuf) -> String {
    let mut buf = Vec::new();
    if let Ok(mut f) = std::fs::File::open(path) {
        let _ = f.read_to_end(&mut buf);
    }
    // The protocol trace is not necessarily valid UTF-8 (raw bytes of window
    // titles flash through it), hence lossy.
    String::from_utf8_lossy(&buf).into_owned()
}

/// Parse result: an empty list of problems = the bench is green.
#[derive(Debug, Default)]
pub struct Verdict {
    pub problems: Vec<String>,
}

impl Verdict {
    fn require(&mut self, ok: bool, message: impl Into<String>) {
        if !ok {
            self.problems.push(message.into());
        }
    }
}

/// Verdict by the child's exit code and the protocol trace. A pure function
/// — it has its own tests below, including for a FALSELY green case.
pub fn judge(child_code: Option<i32>, trace: &str) -> Verdict {
    let mut v = Verdict::default();

    match child_code {
        Some(0) => {}
        Some(code) => v.problems.push(format!(
            "подопытное окно вышло кодом {code} (0 — единственный успешный)"
        )),
        None => v
            .problems
            .push("подопытное окно не завершилось за отведённое время".to_string()),
    }

    if trace.trim().is_empty() {
        v.problems
            .push("протокольный след пуст — WAYLAND_DEBUG не сработал, проверять нечего".to_string());
        return v;
    }

    // ── The window's self-report ──────────────────────────────────────────
    let apply = find_line(trace, "REXON-LS-APPLY");
    match apply {
        Some(line) => {
            v.require(
                field(line, "applied") == Some("true"),
                "LayerShellQt отказался настраивать окно (applied != true)",
            );
            v.require(
                field(line, "available") == Some("true"),
                "платформа Qt не Wayland (available != true)",
            );
        }
        None => v
            .problems
            .push("окно не отчиталось строкой REXON-LS-APPLY".to_string()),
    }

    match find_line(trace, "REXON-LS-READBACK") {
        Some(line) => {
            v.require(
                field(line, "layer") == Some("0"),
                "обратное чтение: слой не LayerBackground",
            );
            v.require(
                field(line, "anchors") == Some("15"),
                "обратное чтение: якоря не на все четыре стороны",
            );
            v.require(
                field(line, "exclusionZone") == Some("-1"),
                "обратное чтение: зона вытеснения не -1",
            );
            v.require(
                field(line, "keyboard") == Some("0"),
                "обратное чтение: окно обоев ловит клавиатуру",
            );
        }
        None => v
            .problems
            .push("окно не отчиталось строкой REXON-LS-READBACK".to_string()),
    }

    match find_line(trace, "REXON-LS-GEOMETRY") {
        Some(line) => {
            let win_w = num(field(line, "windowW"));
            let win_h = num(field(line, "windowH"));
            let scr_w = num(field(line, "screenW"));
            let scr_h = num(field(line, "screenH"));
            v.require(
                scr_w.is_some_and(|x| x > 0) && scr_h.is_some_and(|x| x > 0),
                "геометрия экрана не прочиталась",
            );
            v.require(
                win_w.is_some() && win_w == scr_w && win_h.is_some() && win_h == scr_h,
                format!(
                    "окно не растянулось на весь экран: {}×{} против {}×{}",
                    show(win_w),
                    show(win_h),
                    show(scr_w),
                    show(scr_h)
                ),
            );
        }
        None => v
            .problems
            .push("окно не отчиталось строкой REXON-LS-GEOMETRY".to_string()),
    }

    // ── Protocol ──────────────────────────────────────────────────────────
    v.require(
        !trace.contains(".get_toplevel("),
        "в следе есть xdg_surface.get_toplevel — окно было обычным, \
         а такое попадает в список задач",
    );
    // A protocol error breaks the connection: anything that "worked" after it
    // means nothing.
    v.require(
        !trace.contains("wl_display#1.error(") && !trace.contains("wl_display@1.error("),
        "композитор вернул протокольную ошибку",
    );

    let Some(surface) = find_layer_surface(trace) else {
        v.problems.push(
            "в следе нет zwlr_layer_shell_v1.get_layer_surface — окно \
             поверхностью слоя не становилось"
                .to_string(),
        );
        return v;
    };

    v.require(
        surface.layer == 0,
        format!(
            "поверхность создана в слое {} вместо background(0)",
            surface.layer
        ),
    );
    v.require(
        surface.namespace == SCOPE,
        format!(
            "чужая поверхность слоя: namespace «{}» вместо «{SCOPE}»",
            surface.namespace
        ),
    );

    let prefix = format!("{}.", surface.object);
    v.require(
        request_arg(trace, &prefix, "set_anchor").as_deref() == Some("15"),
        "set_anchor не 15 — якоря не на все четыре стороны",
    );
    v.require(
        request_arg(trace, &prefix, "set_exclusive_zone").as_deref() == Some("-1"),
        "set_exclusive_zone не -1 — окно резервирует или отдаёт место",
    );
    // Keyboard interactivity is zero by default, and LayerShellQt is entitled
    // not to send the request at all; we check only if it is there.
    if let Some(kbd) = request_arg(trace, &prefix, "set_keyboard_interactivity") {
        v.require(
            kbd == "0",
            format!("set_keyboard_interactivity {kbd} — окно обоев ловит клавиатуру"),
        );
    }

    // The compositor is entitled to dismiss the surface (closed) — for
    // example, if it considered the request invalid. Then there is no window
    // on screen, whatever the other checks say.
    v.require(
        !trace.contains(&format!("{prefix}closed(")),
        "композитор прогнал поверхность слоя (closed)",
    );

    // The incoming configure is the only place where the compositor ITSELF
    // speaks. Without it everything listed above would remain an intention.
    let configure = trace.lines().find_map(|line| {
        let line = line.trim();
        if line.starts_with("->") || !line.contains(&prefix) {
            return None;
        }
        args_of(line, &format!("{prefix}configure"))
    });
    match configure {
        Some(args) => {
            let w = args.get(1).and_then(|s| s.parse::<i64>().ok());
            let h = args.get(2).and_then(|s| s.parse::<i64>().ok());
            v.require(
                w.is_some_and(|x| x > 0) && h.is_some_and(|x| x > 0),
                format!(
                    "композитор выдал вырожденную геометрию {}×{}",
                    show(w),
                    show(h)
                ),
            );
        }
        None => v.problems.push(
            "композитор не прислал zwlr_layer_surface_v1.configure — \
             поверхность слоя не принята"
                .to_string(),
        ),
    }

    v
}

fn show(v: Option<i64>) -> String {
    v.map_or_else(|| "?".to_string(), |x| x.to_string())
}

fn num(v: Option<&str>) -> Option<i64> {
    v.and_then(|s| s.parse::<i64>().ok())
}

/// The last trace line with the given tag (the last — so that a repeated
/// report is not overridden by the first).
fn find_line<'a>(trace: &'a str, tag: &str) -> Option<&'a str> {
    trace.lines().rfind(|l| l.contains(tag))
}

/// Value of a `key=value` field in a report line.
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace().find_map(|token| {
        let (k, val) = token.split_once('=')?;
        (k == key).then_some(val)
    })
}

/// A parsed get_layer_surface call.
#[derive(Debug)]
pub struct LayerSurface {
    /// What the surface is called in the trace, for example
    /// `zwlr_layer_surface_v1@23`.
    pub object: String,
    pub layer: i64,
    pub namespace: String,
}

/// Find the creation of the layer surface in the trace.
///
/// The libwayland line format:
/// `[123.456]  -> zwlr_layer_shell_v1@12.get_layer_surface(new id
/// zwlr_layer_surface_v1@23, wl_surface@22, nil, 0, "name")`
fn find_layer_surface(trace: &str) -> Option<LayerSurface> {
    for line in trace.lines() {
        if !line.contains("get_layer_surface(") {
            continue;
        }
        let args = split_args(paren_body(line)?);
        if args.len() < 5 {
            continue;
        }
        // The first argument is "new id zwlr_layer_surface_v1@23".
        let object = args[0].rsplit(' ').next()?.to_string();
        let layer = args[3].parse::<i64>().ok()?;
        let namespace = args[4].trim_matches('"').to_string();
        return Some(LayerSurface {
            object,
            layer,
            namespace,
        });
    }
    None
}

/// The only argument of an outgoing request `<object>.<name>(value)`. The
/// LAST call is taken: the client is entitled to resend the request, and
/// the final value decides.
fn request_arg(trace: &str, object_prefix: &str, request: &str) -> Option<String> {
    let needle = format!("{object_prefix}{request}(");
    trace
        .lines()
        .filter(|line| line.contains(&needle))
        .filter_map(|line| Some(split_args(paren_body(line)?).first()?.clone()))
        .next_back()
}

/// Arguments of the call `<object>.<name>(...)`, if the line is exactly
/// that.
fn args_of(line: &str, full_name: &str) -> Option<Vec<String>> {
    let needle = format!("{full_name}(");
    line.contains(&needle)
        .then(|| paren_body(line).map(split_args))
        .flatten()
}

/// Content of the outermost parentheses of the line.
fn paren_body(line: &str) -> Option<&str> {
    let start = line.find('(')?;
    let end = line.rfind(')')?;
    (end > start).then(|| &line[start + 1..end])
}

/// Parsing the argument list. Quoted string values may contain a comma, so
/// a plain split(',') is not enough.
fn split_args(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in body.chars() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ',' if !in_quotes => {
                out.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trace "as it should be": a known-green sample from which the other
    /// cases start.
    fn good_trace() -> String {
        format!(
            "qml: REXON-LS-APPLY applied=true available=true\n\
             qml: REXON-LS-READBACK layer=0 anchors=15 exclusionZone=-1 keyboard=0\n\
             qml: REXON-LS-GEOMETRY windowW=1920 windowH=1080 screenW=1920 screenH=1080\n\
             [1.000]  -> wl_compositor@5.create_surface(new id wl_surface@22)\n\
             [1.001]  -> zwlr_layer_shell_v1@12.get_layer_surface(new id \
             zwlr_layer_surface_v1@23, wl_surface@22, nil, 0, \"{SCOPE}\")\n\
             [1.002]  -> zwlr_layer_surface_v1@23.set_anchor(15)\n\
             [1.003]  -> zwlr_layer_surface_v1@23.set_exclusive_zone(-1)\n\
             [1.004]  -> zwlr_layer_surface_v1@23.set_keyboard_interactivity(0)\n\
             [1.010] zwlr_layer_surface_v1@23.configure(7, 1920, 1080)\n"
        )
    }

    #[test]
    fn зелёный_след_проходит() {
        let v = judge(Some(0), &good_trace());
        assert!(v.problems.is_empty(), "{:?}", v.problems);
    }

    /// The main check of the bench itself: it must GO RED. Each mutation is a
    /// real way to break the wallpaper layer.
    #[test]
    fn каждая_мутация_валит_стенд() {
        let cases: [(&str, String); 9] = [
            // The window stayed ordinary — it will get into the task list and go on
            // top of other windows.
            (
                "обычное окно",
                good_trace().replace(
                    "zwlr_layer_shell_v1@12.get_layer_surface",
                    "xdg_surface@23.get_toplevel",
                ),
            ),
            // Layer Top instead of Background — the wallpaper on top of everything.
            ("слой top", good_trace().replace("nil, 0, ", "nil, 2, ")),
            // Anchor only at the top — the wallpaper does not cover the screen.
            (
                "один якорь",
                good_trace().replace("set_anchor(15)", "set_anchor(1)"),
            ),
            // Zone 0 instead of -1 — panels will start pushing the wallpaper aside.
            (
                "зона 0",
                good_trace().replace("set_exclusive_zone(-1)", "set_exclusive_zone(0)"),
            ),
            // The compositor did not accept the surface.
            (
                "нет configure",
                good_trace().replace("zwlr_layer_surface_v1@23.configure(7, 1920, 1080)", ""),
            ),
            // The window is smaller than the screen.
            (
                "окно не во весь экран",
                good_trace().replace("windowW=1920 windowH=1080", "windowW=1280 windowH=720"),
            ),
            // The wallpaper grabs the keyboard.
            (
                "клавиатура",
                good_trace().replace("keyboard=0", "keyboard=2"),
            ),
            // The compositor dismissed the surface — there is no window on screen.
            (
                "closed",
                good_trace() + "[1.020] zwlr_layer_surface_v1@23.closed()\n",
            ),
            // A protocol error breaks the connection.
            (
                "ошибка протокола",
                good_trace() + "[1.020] wl_display@1.error(zwlr_layer_surface_v1@23, 0, \"плохо\")\n",
            ),
        ];
        for (name, trace) in cases {
            let v = judge(Some(0), &trace);
            assert!(!v.problems.is_empty(), "мутация «{name}» осталась зелёной");
        }
    }

    #[test]
    fn молчание_ребёнка_не_успех() {
        // Exactly the trap that was closed: a timeout must be a failure.
        assert!(!judge(None, &good_trace()).problems.is_empty());
        assert!(!judge(Some(0), "").problems.is_empty());
        assert!(!judge(Some(2), &good_trace()).problems.is_empty());
    }

    #[test]
    fn разбор_аргументов_щадит_кавычки() {
        let args = split_args("new id foo@1, bar@2, nil, 0, \"а, б\"");
        assert_eq!(args.len(), 5);
        assert_eq!(args[4], "\"а, б\"");
    }
}

// ============================================================================
// The idle screensaver overlay bench
// ============================================================================

/// The idle screensaver overlay bench: proof that the screensaver window
/// (`LayerSpec::SCREENSAVER`) really lands ABOVE everything on EVERY output,
/// gets the keyboard, and goes away on a key and on a pointer move.
///
/// Same two-process design as the wallpaper bench above: the child
/// (`qml/OverlayProbe.qml`) runs under `WAYLAND_DEBUG=1`, and the verdict is
/// read from the protocol trace — what the client sent and, above all, what
/// the compositor answered:
///   * one `get_layer_surface` per screen, layer 3 (overlay), each aimed at
///     its own `wl_output` (not nil, all different);
///   * `set_anchor(15)`, `set_exclusive_zone(-1)` and an explicit
///     `set_keyboard_interactivity(1)` (exclusive — the default is 0);
///   * an incoming `configure` with non-zero size for every surface, and a
///     buffer attached after it (the first frame — otherwise it is the
///     "invisible wall" of plan section 5, item 2);
///   * an incoming `wl_keyboard.enter` on one of OUR surfaces — the
///     compositor really gave us the keyboard;
///   * after the bench injected a key / a pointer move through a virtual
///     device of the stand, the child left with exactly that reason.
///
/// ⛔ The probe covers everything and takes the keyboard. It runs only on a
/// nested compositor in an isolated environment: `--overlay-stand=<name>`
/// brings one up (own `XDG_RUNTIME_DIR`, own D-Bus session, no `DISPLAY`)
/// and kills it by its own process group. On the person's own session the
/// bench refuses to start.
///
/// The result is one machine-readable line and the exit code:
///
/// ```text
/// REXON-OV-VERDICT OK|FAIL|PARTIAL compositor=<comm> outputs=<n> surfaces=<n>
///     configure=ok|fail focus=ok|fail key=ok|fail|untested motion=ok|fail|untested problems=<n>
/// ```
///
/// `0` — everything proven; `1` — a divergence; `2` — the bench could not run
/// (no Wayland, no compositor, refused live session); `3` — nothing red, but
/// some input kind could not be injected on this compositor (`untested`, with
/// the reason in a `REXON-OV-UNTESTED` line).
pub mod overlay {
    use super::{args_of, field, find_line, num, paren_body, request_arg, show, split_args};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    /// The child half: the probe window itself.
    pub const PROBE_ARG: &str = "--overlay-probe";
    /// The bench: `--overlay-selftest` (key and motion), or
    /// `--overlay-selftest=key|motion|none`.
    pub const SELFTEST_ARG: &str = "--overlay-selftest";
    /// The bite run: `--overlay-bite` (all kinds) or `--overlay-bite=<kind>`.
    pub const BITE_ARG: &str = "--overlay-bite";
    /// Bring up a nested compositor and run the bench inside it:
    /// `--overlay-stand=labwc|sway|kwin` followed by the bench arguments
    /// (default `--overlay-selftest`).
    pub const STAND_ARG: &str = "--overlay-stand";
    /// The bite kind for the child (see `qml/OverlayProbe.qml`).
    pub const BITE_ENV: &str = "REXON_OV_BITE";
    /// How long the child waits for input after READY, ms.
    pub const INPUT_WAIT_ENV: &str = "REXON_OV_INPUT_WAIT_MS";
    /// Explicit consent to run the probe on a non-isolated session (a test
    /// VM where the session IS the stand). Never set it on a real desktop.
    pub const ALLOW_LIVE_ENV: &str = "REXON_OV_ALLOW_LIVE";
    /// Surface namespace of the probe.
    pub const SCOPE: &str = "rexon-overlay-selftest";
    /// Bite kinds — each must turn the bench red.
    pub const BITE_KINDS: &[&str] = &["layer", "keyboard", "screens", "pin", "deaf"];
    /// Compositors the stand knows how to bring up.
    pub const STANDS: &[&str] = &["labwc", "sway", "kwin"];

    const CHILD_TIMEOUT: Duration = Duration::from_secs(40);

    /// Input the bench injects after the probe is READY.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Input {
        Key,
        Motion,
    }

    impl Input {
        pub fn name(self) -> &'static str {
            match self {
                Input::Key => "key",
                Input::Motion => "motion",
            }
        }
    }

    /// What a single probe run proved.
    #[derive(Debug, Default)]
    pub struct OverlayVerdict {
        pub problems: Vec<String>,
        /// Screens Qt saw (READY line).
        pub outputs: usize,
        /// Our layer surfaces in the trace.
        pub surfaces: usize,
        /// Surfaces that got a non-empty configure.
        pub configured: usize,
        /// The compositor sent `wl_keyboard.enter` to one of our surfaces.
        pub focus: bool,
        /// `REXON-OV-EXIT reason=…`.
        pub exit_reason: Option<String>,
    }

    impl OverlayVerdict {
        fn require(&mut self, ok: bool, message: impl Into<String>) {
            if !ok {
                self.problems.push(message.into());
            }
        }
    }

    /// One of our layer surfaces, as created in the trace.
    #[derive(Debug, Clone)]
    struct ProbeSurface {
        object: String,
        wl_surface: String,
        output: String,
        layer: i64,
    }

    fn outgoing(line: &str) -> bool {
        line.contains(" -> ")
    }

    /// Every `get_layer_surface` with our namespace.
    fn probe_surfaces(trace: &str) -> Vec<ProbeSurface> {
        let mut out = Vec::new();
        for line in trace.lines() {
            if !line.contains("get_layer_surface(") || !outgoing(line) {
                continue;
            }
            let Some(body) = paren_body(line) else { continue };
            let args = split_args(body);
            if args.len() < 5 || args[4].trim_matches('"') != SCOPE {
                continue;
            }
            let Some(object) = args[0].rsplit(' ').next() else { continue };
            out.push(ProbeSurface {
                object: object.to_string(),
                wl_surface: args[1].clone(),
                output: args[2].clone(),
                layer: args[3].parse().unwrap_or(-1),
            });
        }
        out
    }

    /// Verdict of one probe run. `injected` is the input the bench actually
    /// delivered (None — nothing was injected, the exit reason is not judged).
    /// A pure function with its own tests, including falsely green cases.
    pub fn judge(child_code: Option<i32>, trace: &str, injected: Option<Input>) -> OverlayVerdict {
        let mut v = OverlayVerdict::default();
        match child_code {
            Some(0) => {}
            Some(code) => v
                .problems
                .push(format!("подопытное окно заставки вышло кодом {code} (0 — единственный успешный)")),
            None => v
                .problems
                .push("подопытное окно заставки не завершилось за отведённое время".to_string()),
        }
        if trace.trim().is_empty() {
            v.problems
                .push("протокольный след пуст — WAYLAND_DEBUG не сработал, проверять нечего".to_string());
            return v;
        }

        // ── The child's own report ────────────────────────────────────────
        let mut windows = 0usize;
        match find_line(trace, "REXON-OV-READY") {
            Some(line) => {
                v.outputs = num(field(line, "screens")).unwrap_or(0).max(0) as usize;
                windows = num(field(line, "windows")).unwrap_or(0).max(0) as usize;
                v.require(v.outputs > 0, "Qt не увидел ни одного экрана");
                v.require(
                    windows == v.outputs,
                    format!("окон заставки {windows}, экранов {} — не на каждом мониторе", v.outputs),
                );
            }
            None => v.problems.push("окно не отчиталось строкой REXON-OV-READY".to_string()),
        }
        let reports: Vec<&str> = trace.lines().filter(|l| l.contains("REXON-OV-WINDOW")).collect();
        v.require(
            reports.len() == windows && windows > 0,
            format!("отчётов окон {} при {windows} окнах", reports.len()),
        );
        for line in &reports {
            let screen = field(line, "screen").unwrap_or("?");
            let expect = |key: &str, want: &str| field(line, key) == Some(want);
            v.require(expect("applied", "true"), format!("{screen}: LayerShellQt отказался (applied != true)"));
            v.require(expect("available", "true"), format!("{screen}: платформа Qt не Wayland"));
            v.require(expect("pinned", "true"), format!("{screen}: поверхность не прицелена на свой экран"));
            v.require(expect("layer", "3"), format!("{screen}: обратное чтение: слой не Overlay"));
            v.require(expect("anchors", "15"), format!("{screen}: обратное чтение: якоря не на все стороны"));
            v.require(expect("exclusionZone", "-1"), format!("{screen}: обратное чтение: зона не -1"));
            v.require(expect("keyboard", "1"), format!("{screen}: обратное чтение: клавиатура не exclusive"));
            let (ww, wh) = (num(field(line, "windowW")), num(field(line, "windowH")));
            let (sw, sh) = (num(field(line, "screenW")), num(field(line, "screenH")));
            v.require(
                ww.is_some() && ww == sw && wh.is_some() && wh == sh && sw.is_some_and(|x| x > 0),
                format!(
                    "{screen}: окно не на весь экран: {}×{} против {}×{}",
                    show(ww),
                    show(wh),
                    show(sw),
                    show(sh)
                ),
            );
        }

        // ── Protocol ──────────────────────────────────────────────────────
        v.require(
            !trace.contains(".get_toplevel("),
            "в следе есть xdg_surface.get_toplevel — заставка была обычным окном",
        );
        v.require(
            !trace.contains("wl_display#1.error(") && !trace.contains("wl_display@1.error("),
            "композитор вернул протокольную ошибку",
        );

        let surfaces = probe_surfaces(trace);
        v.surfaces = surfaces.len();
        if surfaces.is_empty() {
            v.problems
                .push("в следе нет get_layer_surface пробы — окно поверхностью слоя не становилось".to_string());
            return v;
        }
        v.require(
            surfaces.len() == v.outputs,
            format!("поверхностей слоя {} при {} экранах", surfaces.len(), v.outputs),
        );
        let mut outputs: Vec<&str> = surfaces.iter().map(|s| s.output.as_str()).collect();
        outputs.sort_unstable();
        outputs.dedup();
        v.require(
            outputs.len() == surfaces.len(),
            "две поверхности заставки на одном выводе — второй монитор остался открыт",
        );

        for s in &surfaces {
            let prefix = format!("{}.", s.object);
            let name = &s.object;
            v.require(s.layer == 3, format!("{name}: слой {} вместо overlay(3)", s.layer));
            v.require(
                s.output != "nil",
                format!("{name}: вывод не назван (nil) — композитор кладёт куда хочет"),
            );
            v.require(
                request_arg(trace, &prefix, "set_anchor").as_deref() == Some("15"),
                format!("{name}: set_anchor не 15"),
            );
            v.require(
                request_arg(trace, &prefix, "set_exclusive_zone").as_deref() == Some("-1"),
                format!("{name}: set_exclusive_zone не -1 — панели оттеснят заставку"),
            );
            // Unlike wallpapers the request is mandatory: the default is 0 (none).
            v.require(
                request_arg(trace, &prefix, "set_keyboard_interactivity").as_deref() == Some("1"),
                format!("{name}: нет set_keyboard_interactivity(1) — клавиша уйдёт программе под заставкой"),
            );
            v.require(
                !trace.contains(&format!("{prefix}closed(")),
                format!("{name}: композитор прогнал поверхность (closed)"),
            );

            // The incoming configure, then a buffer on the same wl_surface.
            let mut configured_at = None;
            for (i, line) in trace.lines().enumerate() {
                if outgoing(line) {
                    continue;
                }
                if let Some(args) = args_of(line, &format!("{prefix}configure")) {
                    let w = args.get(1).and_then(|x| x.parse::<i64>().ok());
                    let h = args.get(2).and_then(|x| x.parse::<i64>().ok());
                    if w.is_some_and(|x| x > 0) && h.is_some_and(|x| x > 0) {
                        configured_at = Some(i);
                        break;
                    }
                }
            }
            match configured_at {
                Some(at) => {
                    v.configured += 1;
                    let attach = format!("{}.attach(", s.wl_surface);
                    let drawn = trace
                        .lines()
                        .skip(at)
                        .any(|l| outgoing(l) && l.contains(&attach) && !l.contains("attach(nil"));
                    v.require(
                        drawn,
                        format!("{name}: после configure не приложен ни один буфер — невидимая стена"),
                    );
                }
                None => v.problems.push(format!(
                    "{name}: композитор не прислал configure с ненулевым размером — поверхность не принята"
                )),
            }
        }

        // Keyboard focus: the compositor's own word, not Qt's `active`.
        let ours: Vec<&str> = surfaces.iter().map(|s| s.wl_surface.as_str()).collect();
        v.focus = trace.lines().any(|line| {
            if outgoing(line) || !line.contains("wl_keyboard") || !line.contains(".enter(") {
                return false;
            }
            paren_body(line)
                .map(split_args)
                .and_then(|a| a.get(1).cloned())
                .is_some_and(|s| ours.contains(&s.as_str()))
        });
        v.require(
            v.focus,
            "композитор не отдал клавиатуру ни одной поверхности заставки (нет wl_keyboard.enter)",
        );

        // ── Leaving on input ──────────────────────────────────────────────
        v.exit_reason = find_line(trace, "REXON-OV-EXIT").and_then(|l| field(l, "reason")).map(str::to_string);
        if let Some(input) = injected {
            v.require(
                v.exit_reason.as_deref() == Some(input.name()),
                format!(
                    "после поданного ввода «{}» окно вышло по причине «{}»",
                    input.name(),
                    v.exit_reason.as_deref().unwrap_or("нет отчёта")
                ),
            );
        }
        v
    }

    // ── Input injection (inside the stand) ───────────────────────────────

    /// Deliver `input` to the compositor of `WAYLAND_DISPLAY`. `Err` — this
    /// compositor offers no virtual device the bench can drive (the reason
    /// goes to the report as "untested").
    pub fn inject(input: Input) -> Result<&'static str, String> {
        let globals = inject_impl::globals().map_err(|e| format!("нет подключения к композитору: {e}"))?;
        let has = |name: &str| globals.iter().any(|(_, iface, _)| iface == name);
        match input {
            Input::Motion if has("zwlr_virtual_pointer_manager_v1") => {
                inject_impl::wlr_motion().map(|_| "zwlr_virtual_pointer_v1")
            }
            Input::Key if has("zwp_virtual_keyboard_manager_v1") => {
                if which("wtype").is_none() {
                    return Err("есть zwp_virtual_keyboard_v1, но нет программы wtype".to_string());
                }
                // The pause comes BEFORE the key: a stand seat has no keyboard until
                // wtype creates its virtual one, and a press sent at once reaches the
                // compositor before the probe has bound its new wl_keyboard — the
                // trace then shows only the release (measured on labwc 0.20).
                let status = Command::new("wtype")
                    .args(["-s", "600", "-k", "space"])
                    .stdin(Stdio::null())
                    .status()
                    .map_err(|e| format!("wtype не запустился: {e}"))?;
                if status.success() {
                    Ok("zwp_virtual_keyboard_v1 (wtype)")
                } else {
                    Err(format!("wtype вышел с {status}"))
                }
            }
            _ if has("org_kde_kwin_fake_input") => {
                let (name, _, version) = globals
                    .iter()
                    .find(|(_, iface, _)| iface == "org_kde_kwin_fake_input")
                    .cloned()
                    .unwrap_or_default();
                inject_impl::kwin_fake_input(name, version, input).map(|_| "org_kde_kwin_fake_input")
            }
            _ => Err(format!(
                "композитор не даёт виртуального устройства для «{}» (нет zwlr_virtual_pointer_v1 / \
                 zwp_virtual_keyboard_v1 / org_kde_kwin_fake_input)",
                input.name()
            )),
        }
    }

    mod inject_impl {
        use super::Input;
        use std::ffi::CString;
        use std::os::fd::OwnedFd;
        use std::sync::Arc;
        use std::time::Duration;
        use wayland_client::backend::protocol::{
            AllowNull, Argument, ArgumentType, Interface, Message, MessageDesc,
        };
        use wayland_client::backend::smallvec::smallvec;
        use wayland_client::backend::{Backend, ObjectData, ObjectId};
        use wayland_client::globals::{registry_queue_init, GlobalListContents};
        use wayland_client::protocol::wl_registry;
        use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle};
        use wayland_protocols_wlr::virtual_pointer::v1::client::{
            zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
        };

        struct State;

        impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
            fn event(
                _: &mut Self,
                _: &wl_registry::WlRegistry,
                _: wl_registry::Event,
                _: &GlobalListContents,
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
            }
        }
        delegate_noop!(State: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1);
        delegate_noop!(State: zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1);

        /// `(name, interface, version)` of every global.
        pub fn globals() -> Result<Vec<(u32, String, u32)>, String> {
            let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
            let (globals, _queue) = registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
            Ok(globals
                .contents()
                .clone_list()
                .into_iter()
                .map(|g| (g.name, g.interface, g.version))
                .collect())
        }

        /// Steps of the injected pointer move: the first one only places the
        /// pointer on the probe (the anchor), the rest travel well past
        /// `WAKE_MOTION_PX`.
        const MOTION_STEPS: [(f64, f64); 5] = [(8.0, 6.0); 5];

        pub fn wlr_motion() -> Result<(), String> {
            let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
            let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
            let qh = queue.handle();
            let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 =
                globals.bind(&qh, 1..=2, ()).map_err(|e| e.to_string())?;
            let pointer = manager.create_virtual_pointer(None, &qh, ());
            queue.roundtrip(&mut State).map_err(|e| e.to_string())?;
            for (i, (dx, dy)) in MOTION_STEPS.iter().enumerate() {
                pointer.motion(i as u32 * 16, *dx, *dy);
                pointer.frame();
                queue.roundtrip(&mut State).map_err(|e| e.to_string())?;
                std::thread::sleep(Duration::from_millis(60));
            }
            pointer.destroy();
            let _ = queue.roundtrip(&mut State);
            Ok(())
        }

        // org_kde_kwin_fake_input has no Rust binding in our dependencies, and
        // the bench needs three requests of it. They are sent as raw messages
        // through the backend; the request table repeats the order of the
        // protocol description (plasma-wayland-protocols, fake-input.xml,
        // version 6) — the opcode is the position in it.
        const STR: ArgumentType = ArgumentType::Str(AllowNull::No);
        const fn req(
            name: &'static str,
            signature: &'static [ArgumentType],
            since: u32,
            is_destructor: bool,
        ) -> MessageDesc {
            MessageDesc { name, signature, since, is_destructor, child_interface: None, arg_interfaces: &[] }
        }
        static FAKE_INPUT_REQUESTS: [MessageDesc; 13] = [
            req("authenticate", &[STR, STR], 1, false),
            req("pointer_motion", &[ArgumentType::Fixed, ArgumentType::Fixed], 1, false),
            req("button", &[ArgumentType::Uint, ArgumentType::Uint], 1, false),
            req("axis", &[ArgumentType::Uint, ArgumentType::Fixed], 1, false),
            req("touch_down", &[ArgumentType::Uint, ArgumentType::Fixed, ArgumentType::Fixed], 2, false),
            req("touch_motion", &[ArgumentType::Uint, ArgumentType::Fixed, ArgumentType::Fixed], 2, false),
            req("touch_up", &[ArgumentType::Uint], 2, false),
            req("touch_cancel", &[], 2, false),
            req("touch_frame", &[], 2, false),
            req("pointer_motion_absolute", &[ArgumentType::Fixed, ArgumentType::Fixed], 3, false),
            req("keyboard_key", &[ArgumentType::Uint, ArgumentType::Uint], 4, false),
            req("destroy", &[], 5, true),
            req("keyboard_keysym", &[ArgumentType::Uint, ArgumentType::Uint], 6, false),
        ];
        static FAKE_INPUT: Interface = Interface {
            name: "org_kde_kwin_fake_input",
            version: 6,
            requests: &FAKE_INPUT_REQUESTS,
            events: &[],
            c_ptr: None,
        };
        const OP_AUTHENTICATE: u16 = 0;
        const OP_POINTER_MOTION: u16 = 1;
        const OP_KEYBOARD_KEY: u16 = 10;
        const OP_DESTROY: u16 = 11;
        /// evdev KEY_SPACE.
        const KEY_SPACE: u32 = 57;

        struct Ignore;
        impl ObjectData for Ignore {
            fn event(self: Arc<Self>, _: &Backend, _: Message<ObjectId, OwnedFd>) -> Option<Arc<dyn ObjectData>> {
                None
            }
            fn destroyed(&self, _: ObjectId) {}
        }

        fn fixed(v: f64) -> i32 {
            (v * 256.0) as i32
        }

        fn cstr(s: &str) -> Argument<ObjectId, std::os::fd::RawFd> {
            Argument::Str(Some(Box::new(CString::new(s).unwrap_or_default())))
        }

        pub fn kwin_fake_input(name: u32, advertised: u32, input: Input) -> Result<(), String> {
            let version = advertised.min(FAKE_INPUT.version);
            if input == Input::Key && version < 4 {
                return Err(format!("org_kde_kwin_fake_input версии {version}: keyboard_key нет"));
            }
            let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
            let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
            let backend = conn.backend();
            let bind = Message {
                sender_id: globals.registry().id(),
                opcode: 0, // wl_registry.bind
                args: smallvec![
                    Argument::Uint(name),
                    cstr(FAKE_INPUT.name),
                    Argument::Uint(version),
                    Argument::NewId(ObjectId::null()),
                ],
            };
            let fake = backend
                .send_request(bind, Some(Arc::new(Ignore)), Some((&FAKE_INPUT, version)))
                .map_err(|_| "bind org_kde_kwin_fake_input не прошёл".to_string())?;
            let send = |opcode: u16, args| {
                backend
                    .send_request(Message { sender_id: fake.clone(), opcode, args }, None, None)
                    .map(|_| ())
                    .map_err(|_| format!("запрос {opcode} fake_input не ушёл"))
            };
            send(OP_AUTHENTICATE, smallvec![cstr("Rexon"), cstr("overlay self-test")])?;
            queue.roundtrip(&mut State).map_err(|e| e.to_string())?;
            // Same reason as the wtype pause: give the probe time to bind the
            // keyboard/pointer the fake device may have just added to the seat.
            std::thread::sleep(Duration::from_millis(600));
            match input {
                Input::Motion => {
                    for (dx, dy) in MOTION_STEPS {
                        send(
                            OP_POINTER_MOTION,
                            smallvec![Argument::Fixed(fixed(dx)), Argument::Fixed(fixed(dy))],
                        )?;
                        queue.roundtrip(&mut State).map_err(|e| e.to_string())?;
                        std::thread::sleep(Duration::from_millis(60));
                    }
                }
                Input::Key => {
                    for state in [1u32, 0] {
                        send(OP_KEYBOARD_KEY, smallvec![Argument::Uint(KEY_SPACE), Argument::Uint(state)])?;
                        queue.roundtrip(&mut State).map_err(|e| e.to_string())?;
                        std::thread::sleep(Duration::from_millis(40));
                    }
                }
            }
            if version >= 5 {
                let _ = send(OP_DESTROY, smallvec![]);
            }
            let _ = queue.roundtrip(&mut State);
            Ok(())
        }
    }

    fn which(program: &str) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path).map(|d| d.join(program)).find(|p| p.is_file())
    }

    // ── The parent half ──────────────────────────────────────────────────

    /// Killer of the probe: the exclusive-keyboard window must disappear under
    /// any outcome.
    struct ChildGuard(Option<Child>);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                if !matches!(child.try_wait(), Ok(Some(_))) {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }

    /// Why the probe may not run here; `None` — it may.
    ///
    /// The person's own session is recognised by its runtime directory: a
    /// nested stand always has its own. A test VM whose session is the stand
    /// passes with [`ALLOW_LIVE_ENV`]`=1`.
    pub fn refusal(
        wayland_display: Option<&str>,
        runtime_dir: Option<&str>,
        uid: u32,
        allow_live: bool,
    ) -> Option<String> {
        if wayland_display.is_none_or(str::is_empty) {
            return Some("нужен Wayland (WAYLAND_DISPLAY не задан); под X11/offscreen проверять нечего".to_string());
        }
        let own = format!("/run/user/{uid}");
        let runtime = runtime_dir.unwrap_or("");
        if !allow_live && (runtime.is_empty() || Path::new(runtime) == Path::new(&own)) {
            return Some(format!(
                "XDG_RUNTIME_DIR={own} — это живой сеанс человека; окно поверх всего с захватом \
                 клавиатуры здесь не показываем. Стенд: {STAND_ARG}=labwc|sway|kwin"
            ));
        }
        None
    }

    /// Outcome of one probe run.
    struct RunOutcome {
        verdict: OverlayVerdict,
        /// `Some(Ok(device))` — injected; `Some(Err(reason))` — could not.
        injection: Option<Result<&'static str, String>>,
        trace_path: PathBuf,
    }

    fn run_once(bite: &str, input: Option<Input>) -> Result<RunOutcome, String> {
        let exe = std::env::current_exe().map_err(|e| format!("не найден собственный бинарь: {e}"))?;
        let trace_path = std::env::temp_dir().join(format!(
            "rexon-overlay-trace-{}-{}.log",
            std::process::id(),
            if bite.is_empty() { input.map_or("none", Input::name) } else { bite }
        ));
        let trace_file = std::fs::File::create(&trace_path)
            .map_err(|e| format!("файл следа {}: {e}", trace_path.display()))?;
        let wait_ms = if input.is_some() { 8000 } else { 1500 };
        let mut cmd = Command::new(&exe);
        cmd.arg(PROBE_ARG)
            .env("WAYLAND_DEBUG", "1")
            .env("QT_FORCE_STDERR_LOGGING", "1")
            // Wayland or nothing: a fallback to xcb would put the probe on an X
            // display, i.e. on a live desktop.
            .env("QT_QPA_PLATFORM", "wayland")
            .env_remove("DISPLAY")
            .env(INPUT_WAIT_ENV, wait_ms.to_string())
            .stdout(Stdio::inherit())
            .stderr(Stdio::from(trace_file));
        if !bite.is_empty() {
            cmd.env(BITE_ENV, bite);
        }
        let mut guard = ChildGuard(Some(cmd.spawn().map_err(|e| format!("проба не запустилась: {e}"))?));

        let started = Instant::now();
        let mut code = None;
        let mut injection = None;
        while started.elapsed() < CHILD_TIMEOUT {
            let child = guard.0.as_mut().expect("child");
            match child.try_wait() {
                Ok(Some(status)) => {
                    code = Some(status.code().unwrap_or(-1));
                    break;
                }
                Ok(None) => {}
                Err(e) => return Err(format!("опрос пробы сорвался: {e}")),
            }
            if let (Some(kind), None) = (input, &injection) {
                let ready = std::fs::read(&trace_path)
                    .map(|b| String::from_utf8_lossy(&b).contains("REXON-OV-READY"))
                    .unwrap_or(false);
                if ready {
                    injection = Some(inject(kind));
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        drop(guard);

        let trace = std::fs::read(&trace_path)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        for line in trace.lines().filter(|l| l.contains("REXON-OV-")) {
            println!("{}", line.trim());
        }
        let delivered = match &injection {
            Some(Ok(_)) => input,
            _ => None,
        };
        let verdict = judge(code, &trace, delivered);
        Ok(RunOutcome { verdict, injection, trace_path })
    }

    fn compositor_name() -> String {
        crate::screensaver_bridge::wayland_idle::probe()
            .map(|f| f.compositor)
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "?".to_string())
    }

    fn preflight() -> Result<(), String> {
        let uid = unsafe { libc::getuid() };
        let display = std::env::var("WAYLAND_DISPLAY").ok();
        let runtime = std::env::var("XDG_RUNTIME_DIR").ok();
        let allow = std::env::var(ALLOW_LIVE_ENV).as_deref() == Ok("1");
        match refusal(display.as_deref(), runtime.as_deref(), uid, allow) {
            Some(why) => Err(why),
            None => Ok(()),
        }
    }

    /// `--overlay-selftest[=key|motion|none]`.
    pub fn run(spec: &str) -> i32 {
        if let Err(why) = preflight() {
            println!("REXON-OV-VERDICT FAIL reason=refused {why}");
            return 2;
        }
        let inputs: Vec<Option<Input>> = match spec {
            "" => vec![Some(Input::Key), Some(Input::Motion)],
            "key" => vec![Some(Input::Key)],
            "motion" => vec![Some(Input::Motion)],
            "none" => vec![None],
            other => {
                println!("REXON-OV-VERDICT FAIL reason=usage неизвестный ввод «{other}» (key|motion|none)");
                return 2;
            }
        };
        let compositor = compositor_name();
        let mut problems: Vec<String> = Vec::new();
        let mut untested: Vec<String> = Vec::new();
        let mut results: Vec<(Input, &'static str)> = Vec::new();
        let mut last = OverlayVerdict::default();
        let mut configure_ok = true;
        let mut focus_ok = true;
        let mut traces = Vec::new();
        for input in &inputs {
            let tag = input.map_or("none", Input::name);
            let out = match run_once("", *input) {
                Ok(o) => o,
                Err(why) => {
                    println!("REXON-OV-VERDICT FAIL reason=not-run {why}");
                    return 2;
                }
            };
            let v = &out.verdict;
            configure_ok &= v.configured == v.surfaces && v.surfaces > 0 && v.surfaces == v.outputs;
            focus_ok &= v.focus;
            problems.extend(v.problems.iter().map(|p| format!("[{tag}] {p}")));
            if let Some(kind) = input {
                match &out.injection {
                    Some(Ok(device)) => {
                        println!("REXON-OV-INJECT {} через {device}", kind.name());
                        let ok = v.exit_reason.as_deref() == Some(kind.name());
                        results.push((*kind, if ok { "ok" } else { "fail" }));
                    }
                    Some(Err(why)) => {
                        untested.push(format!("{}: {why}", kind.name()));
                        results.push((*kind, "untested"));
                    }
                    None => {
                        problems.push(format!("[{tag}] проба так и не дошла до READY — ввод не подан"));
                        results.push((*kind, "fail"));
                    }
                }
            }
            if v.problems.is_empty() {
                let _ = std::fs::remove_file(&out.trace_path);
            } else {
                traces.push(out.trace_path.display().to_string());
            }
            last = out.verdict;
        }
        let state = |k: Input| results.iter().find(|(i, _)| *i == k).map_or("untested", |(_, s)| *s);
        for p in &problems {
            println!("REXON-OV-FAIL {p}");
        }
        for u in &untested {
            println!("REXON-OV-UNTESTED {u}");
        }
        if !traces.is_empty() {
            println!("REXON-OV-TRACE {}", traces.join(" "));
        }
        let verdict = if !problems.is_empty() {
            "FAIL"
        } else if !untested.is_empty() || inputs.contains(&None) {
            "PARTIAL"
        } else {
            "OK"
        };
        println!(
            "REXON-OV-VERDICT {verdict} compositor={compositor} outputs={} surfaces={} configure={} \
             focus={} key={} motion={} problems={}",
            last.outputs,
            last.surfaces,
            if configure_ok { "ok" } else { "fail" },
            if focus_ok { "ok" } else { "fail" },
            state(Input::Key),
            state(Input::Motion),
            problems.len()
        );
        match verdict {
            "OK" => 0,
            "FAIL" => 1,
            _ => 3,
        }
    }

    /// `--overlay-bite[=kind]`: every bite MUST turn the bench red.
    pub fn run_bite(kind: &str) -> i32 {
        if let Err(why) = preflight() {
            println!("REXON-OV-BITE-VERDICT FAIL reason=refused {why}");
            return 2;
        }
        let kinds: Vec<&str> = if kind.is_empty() { BITE_KINDS.to_vec() } else { vec![kind] };
        let mut failed = 0;
        let mut skipped = 0;
        for k in &kinds {
            // "deaf" is caught only by input that should have closed the window.
            let input = (*k == "deaf").then_some(Input::Key);
            let out = match run_once(k, input) {
                Ok(o) => o,
                Err(why) => {
                    println!("REXON-OV-BITE FAIL укус «{k}»: прогон не состоялся: {why}");
                    failed += 1;
                    continue;
                }
            };
            if let Some(Err(why)) = &out.injection {
                println!("REXON-OV-BITE UNTESTED укус «{k}»: ввод не подать: {why}");
                skipped += 1;
                continue;
            }
            if *k == "screens" && out.verdict.outputs < 2 {
                println!("REXON-OV-BITE UNTESTED укус «{k}»: у стенда один вывод");
                skipped += 1;
                continue;
            }
            if out.verdict.problems.is_empty() {
                println!("REXON-OV-BITE FAIL укус «{k}» остался НЕЗАМЕЧЕННЫМ — стенд врёт");
                failed += 1;
            } else {
                println!("REXON-OV-BITE OK укус «{k}» пойман: {}", out.verdict.problems[0]);
            }
            let _ = std::fs::remove_file(&out.trace_path);
        }
        println!(
            "REXON-OV-BITE-VERDICT {} проверено={} провалено={failed} не_проверено={skipped}",
            if failed > 0 {
                "FAIL"
            } else if skipped > 0 {
                "PARTIAL"
            } else {
                "OK"
            },
            kinds.len()
        );
        if failed > 0 {
            1
        } else if skipped > 0 {
            3
        } else {
            0
        }
    }

    // ── The stand: a nested compositor in an isolated environment ───────

    /// How to start a compositor of the stand.
    #[derive(Debug)]
    pub struct StandCommand {
        pub program: &'static str,
        pub args: Vec<String>,
        pub env: Vec<(&'static str, &'static str)>,
        /// The socket name; `None` — the compositor picks `wayland-N` itself.
        pub socket: Option<&'static str>,
    }

    /// The stand socket of KWin.
    const KWIN_SOCKET: &str = "rexon-overlay-stand";

    /// How to bring a compositor up headless with two outputs and no input
    /// devices of the machine. `config_dir` — the stand's own empty config.
    pub fn stand_command(name: &str, config_dir: &Path) -> Option<StandCommand> {
        const WLR: [(&str, &str); 3] =
            [("WLR_BACKENDS", "headless"), ("WLR_HEADLESS_OUTPUTS", "2"), ("WLR_LIBINPUT_NO_DEVICES", "1")];
        let cfg = config_dir.display().to_string();
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        Some(match name {
            "labwc" => StandCommand { program: "labwc", args: s(&["-C", &cfg]), env: WLR.to_vec(), socket: None },
            // An empty config: no bar, no key bindings, nothing of the person's.
            "sway" => StandCommand {
                program: "sway",
                args: s(&["-c", &format!("{cfg}/sway.conf")]),
                env: WLR.to_vec(),
                socket: None,
            },
            "kwin" => StandCommand {
                program: "kwin_wayland",
                args: s(&[
                    "--virtual",
                    "--socket",
                    KWIN_SOCKET,
                    "--width",
                    "1280",
                    "--height",
                    "720",
                    "--output-count",
                    "2",
                    "--no-lockscreen",
                    "--no-global-shortcuts",
                    "--no-kactivities",
                ]),
                env: vec![],
                socket: Some(KWIN_SOCKET),
            },
            _ => return None,
        })
    }

    /// `--overlay-stand=<name> [bench args]`.
    pub fn run_stand(name: &str, bench_args: &[String]) -> i32 {
        let Some(probe_cmd) = stand_command(name, Path::new("/")) else {
            println!("REXON-OV-VERDICT FAIL reason=usage неизвестный стенд «{name}» ({})", STANDS.join("|"));
            return 2;
        };
        if which(probe_cmd.program).is_none() {
            println!(
                "REXON-OV-VERDICT UNTESTED compositor={name} reason=НЕ ПРОВЕРЕНО: {} не установлен",
                probe_cmd.program
            );
            return 2;
        }
        if which("dbus-run-session").is_none() {
            println!("REXON-OV-VERDICT UNTESTED compositor={name} reason=нет dbus-run-session для своей шины");
            return 2;
        }
        let root = std::env::temp_dir().join(format!("rexon-overlay-stand-{}", std::process::id()));
        let code = stand_in(name, &root, bench_args);
        let _ = std::fs::remove_dir_all(&root);
        code
    }

    fn stand_in(name: &str, root: &Path, bench_args: &[String]) -> i32 {
        use std::os::unix::fs::PermissionsExt;
        use std::os::unix::process::CommandExt;

        for d in ["run", "config", "data", "cache", "compositor"] {
            if let Err(e) = std::fs::create_dir_all(root.join(d)) {
                println!("REXON-OV-VERDICT FAIL reason=not-run каталог стенда: {e}");
                return 2;
            }
        }
        let run_dir = root.join("run");
        let _ = std::fs::set_permissions(&run_dir, std::fs::Permissions::from_mode(0o700));
        let _ = std::fs::write(root.join("compositor/sway.conf"), "");
        let stand = stand_command(name, &root.join("compositor")).expect("checked by run_stand");

        // The environment is built from scratch: nothing of the live session
        // (DISPLAY, WAYLAND_DISPLAY, the session bus, XDG_* dirs) leaks in.
        let keep = ["HOME", "PATH", "USER", "LOGNAME", "LANG", "LC_ALL", "TERM"];
        let mut env: Vec<(String, String)> =
            keep.iter().filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v))).collect();
        let p = |d: &str| root.join(d).display().to_string();
        env.push(("XDG_RUNTIME_DIR".into(), p("run")));
        env.push(("XDG_CONFIG_HOME".into(), p("config")));
        env.push(("XDG_DATA_HOME".into(), p("data")));
        env.push(("XDG_CACHE_HOME".into(), p("cache")));

        let log_path = root.join("compositor.log");
        let (log_out, log_err) = match std::fs::File::create(&log_path).and_then(|f| Ok((f.try_clone()?, f))) {
            Ok(pair) => pair,
            Err(e) => {
                println!("REXON-OV-VERDICT FAIL reason=not-run журнал композитора: {e}");
                return 2;
            }
        };
        let mut cmd = Command::new("dbus-run-session");
        cmd.arg("--")
            .arg(stand.program)
            .args(&stand.args)
            .env_clear()
            .envs(env.iter().cloned())
            .envs(stand.env.iter().copied())
            // Own process group: the stand is put down by ITS group id, never by
            // a process name (a name would hit the person's own compositor).
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_out))
            .stderr(Stdio::from(log_err));
        let mut comp = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                println!("REXON-OV-VERDICT FAIL reason=not-run {} не запустился: {e}", stand.program);
                return 2;
            }
        };
        let pgid = comp.id() as i32;
        println!("REXON-OV-STAND {name} pgid={pgid} runtime={}", run_dir.display());

        let code = (|| {
            // Wait for the compositor's socket.
            let started = Instant::now();
            let display = loop {
                if let Ok(Some(status)) = comp.try_wait() {
                    println!(
                        "REXON-OV-VERDICT FAIL reason=not-run {} вышел сразу ({status}); журнал {}",
                        stand.program,
                        log_path.display()
                    );
                    return 2;
                }
                let found = match stand.socket {
                    Some(s) => run_dir.join(s).exists().then(|| s.to_string()),
                    None => std::fs::read_dir(&run_dir).ok().and_then(|rd| {
                        rd.filter_map(|e| e.ok())
                            .map(|e| e.file_name().to_string_lossy().into_owned())
                            .find(|n| n.starts_with("wayland-") && !n.ends_with(".lock"))
                    }),
                };
                if let Some(d) = found {
                    break d;
                }
                if started.elapsed() > Duration::from_secs(20) {
                    println!("REXON-OV-VERDICT FAIL reason=not-run сокет {} не появился за 20 с", stand.program);
                    return 2;
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            // Outputs and seats come up a moment after the socket.
            std::thread::sleep(Duration::from_millis(1500));

            let exe = match std::env::current_exe() {
                Ok(e) => e,
                Err(e) => {
                    println!("REXON-OV-VERDICT FAIL reason=not-run {e}");
                    return 2;
                }
            };
            let args: Vec<String> =
                if bench_args.is_empty() { vec![SELFTEST_ARG.to_string()] } else { bench_args.to_vec() };
            let mut bench = match Command::new(exe)
                .args(&args)
                .env_clear()
                .envs(env.iter().cloned())
                .env("WAYLAND_DISPLAY", &display)
                .stdin(Stdio::null())
                .spawn()
            {
                Ok(b) => ChildGuard(Some(b)),
                Err(e) => {
                    println!("REXON-OV-VERDICT FAIL reason=not-run стенд не запустил проверку: {e}");
                    return 2;
                }
            };
            let started = Instant::now();
            loop {
                let child = bench.0.as_mut().expect("bench");
                if let Ok(Some(status)) = child.try_wait() {
                    bench.0 = None;
                    return status.code().unwrap_or(2);
                }
                if started.elapsed() > Duration::from_secs(300) {
                    println!("REXON-OV-VERDICT FAIL reason=not-run проверка не закончилась за 300 с");
                    return 2;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        })();

        // Put the stand down by its process group: TERM, a grace period, KILL.
        unsafe { libc::killpg(pgid, libc::SIGTERM) };
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && !matches!(comp.try_wait(), Ok(Some(_))) {
            std::thread::sleep(Duration::from_millis(100));
        }
        unsafe { libc::killpg(pgid, libc::SIGKILL) };
        let _ = comp.wait();
        code
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A trace "as it should be" on two outputs: the known-green sample the
        /// mutations start from.
        fn good_trace() -> String {
            format!(
                "qml: REXON-OV-WINDOW index=0 screen=HEADLESS-1 applied=true available=true pinned=true target=HEADLESS-1 layer=3 anchors=15 exclusionZone=-1 keyboard=1 windowW=1920 windowH=1080 screenW=1920 screenH=1080\n\
                 qml: REXON-OV-WINDOW index=1 screen=HEADLESS-2 applied=true available=true pinned=true target=HEADLESS-2 layer=3 anchors=15 exclusionZone=-1 keyboard=1 windowW=1280 windowH=720 screenW=1280 screenH=720\n\
                 [1.000] {{Default Queue}}  -> zwlr_layer_shell_v1@12.get_layer_surface(new id zwlr_layer_surface_v1@23, wl_surface@22, wl_output@7, 3, \"{SCOPE}\")\n\
                 [1.001] {{Default Queue}}  -> zwlr_layer_surface_v1@23.set_anchor(15)\n\
                 [1.002] {{Default Queue}}  -> zwlr_layer_surface_v1@23.set_exclusive_zone(-1)\n\
                 [1.003] {{Default Queue}}  -> zwlr_layer_surface_v1@23.set_keyboard_interactivity(1)\n\
                 [1.004] {{Default Queue}}  -> zwlr_layer_shell_v1@12.get_layer_surface(new id zwlr_layer_surface_v1@25, wl_surface@24, wl_output@8, 3, \"{SCOPE}\")\n\
                 [1.005] {{Default Queue}}  -> zwlr_layer_surface_v1@25.set_anchor(15)\n\
                 [1.006] {{Default Queue}}  -> zwlr_layer_surface_v1@25.set_exclusive_zone(-1)\n\
                 [1.007] {{Default Queue}}  -> zwlr_layer_surface_v1@25.set_keyboard_interactivity(1)\n\
                 [1.010] {{Default Queue}} zwlr_layer_surface_v1@23.configure(7, 1920, 1080)\n\
                 [1.011] {{Default Queue}} zwlr_layer_surface_v1@25.configure(8, 1280, 720)\n\
                 [1.012] {{Default Queue}}  -> zwlr_layer_surface_v1@23.ack_configure(7)\n\
                 [1.020] {{Default Queue}}  -> wl_surface@22.attach(wl_buffer@40, 0, 0)\n\
                 [1.021] {{Default Queue}}  -> wl_surface@24.attach(wl_buffer@41, 0, 0)\n\
                 [1.030] {{Default Queue}} wl_keyboard@15.enter(9, wl_surface@22, array[0])\n\
                 qml: REXON-OV-FOCUS screen=HEADLESS-1\n\
                 qml: REXON-OV-READY windows=2 screens=2 focus=true\n\
                 qml: REXON-OV-EXIT reason=key\n"
            )
        }

        #[test]
        fn зелёный_след_заставки_проходит() {
            let v = judge(Some(0), &good_trace(), Some(Input::Key));
            assert!(v.problems.is_empty(), "{:?}", v.problems);
            assert_eq!((v.outputs, v.surfaces, v.configured), (2, 2, 2));
            assert!(v.focus);
            // Nothing injected — the exit reason is not judged.
            assert!(judge(Some(0), &good_trace(), None).problems.is_empty());
            // libwayland 1.23+ prints objects as `iface#N` (seen on the labwc
            // stand); the verdict must not depend on the separator.
            let hashed = good_trace().replace('@', "#");
            let v = judge(Some(0), &hashed, Some(Input::Key));
            assert!(v.problems.is_empty(), "{:?}", v.problems);
            assert!(!judge(Some(0), &hashed.replace("wl_output#8, 3,", "nil, 3,"), None).problems.is_empty());
        }

        /// Each mutation is a real way to break the screensaver; every one must
        /// turn the bench red.
        #[test]
        fn каждая_мутация_валит_стенд_заставки() {
            let g = good_trace();
            let cases: Vec<(&str, String, Option<Input>)> = vec![
                ("слой top", g.replace("wl_output@7, 3,", "wl_output@7, 2,"), Some(Input::Key)),
                ("вывод nil", g.replace("wl_output@8, 3,", "nil, 3,"), Some(Input::Key)),
                ("один вывод на двоих", g.replace("wl_output@8, 3,", "wl_output@7, 3,"), Some(Input::Key)),
                (
                    "нет exclusive",
                    g.replace("@25.set_keyboard_interactivity(1)", "@25.set_keyboard_interactivity(0)"),
                    Some(Input::Key),
                ),
                (
                    "нет запроса клавиатуры",
                    g.replace(" -> zwlr_layer_surface_v1@23.set_keyboard_interactivity(1)", ""),
                    Some(Input::Key),
                ),
                (
                    "нет фокуса",
                    g.replace("wl_keyboard@15.enter(9, wl_surface@22", "wl_keyboard@15.enter(9, wl_surface@99"),
                    None,
                ),
                ("нет configure", g.replace("@25.configure(8, 1280, 720)", ""), None),
                ("нулевой configure", g.replace("configure(8, 1280, 720)", "configure(8, 0, 0)"), None),
                ("нет кадра", g.replace(" -> wl_surface@24.attach(wl_buffer@41, 0, 0)", ""), None),
                ("одно окно на два экрана", g.replace("windows=2 screens=2", "windows=1 screens=2"), None),
                (
                    "обычное окно",
                    g.clone() + "[1.040]  -> xdg_surface@50.get_toplevel(new id xdg_toplevel@51)\n",
                    None,
                ),
                ("closed", g.clone() + "[1.040] zwlr_layer_surface_v1@25.closed()\n", None),
                ("ошибка протокола", g.clone() + "[1.040] wl_display@1.error(wl_surface@22, 2, \"x\")\n", None),
                ("не прицелена", g.replace("pinned=true target=HEADLESS-2", "pinned=false target="), None),
                ("ушла по таймеру", g.replace("reason=key", "reason=timeout"), Some(Input::Key)),
                ("ушла не от того", g.clone(), Some(Input::Motion)),
            ];
            for (name, trace, input) in cases {
                let v = judge(Some(0), &trace, input);
                assert!(!v.problems.is_empty(), "мутация «{name}» осталась зелёной");
            }
        }

        #[test]
        fn молчание_пробы_не_успех() {
            assert!(!judge(None, &good_trace(), None).problems.is_empty());
            assert!(!judge(Some(3), &good_trace(), None).problems.is_empty());
            assert!(!judge(Some(0), "", None).problems.is_empty());
        }

        #[test]
        fn живой_сеанс_отказ() {
            // The person's own session: refused.
            assert!(refusal(Some("wayland-0"), Some("/run/user/1000"), 1000, false).is_some());
            assert!(refusal(Some("wayland-0"), None, 1000, false).is_some());
            // No Wayland at all: refused even with consent.
            assert!(refusal(None, Some("/tmp/stand/run"), 1000, true).is_some());
            assert!(refusal(Some(""), Some("/tmp/stand/run"), 1000, false).is_some());
            // An isolated stand, or a VM with explicit consent: allowed.
            assert!(refusal(Some("wayland-0"), Some("/tmp/stand/run"), 1000, false).is_none());
            assert!(refusal(Some("wayland-0"), Some("/run/user/1000"), 1000, true).is_none());
        }

        #[test]
        fn стенды_известны() {
            for s in STANDS {
                assert!(stand_command(s, Path::new("/x")).is_some(), "{s}");
            }
            assert!(stand_command("mutter", Path::new("/x")).is_none());
            // wlroots stands are headless with two outputs and no real input
            // devices — nothing of the machine's own keyboard reaches them.
            let labwc = stand_command("labwc", Path::new("/x")).unwrap();
            assert!(labwc.env.contains(&("WLR_BACKENDS", "headless")));
            assert!(labwc.env.contains(&("WLR_LIBINPUT_NO_DEVICES", "1")));
            // Never `--replace`: the stand must not take over a running compositor.
            for s in STANDS {
                assert!(!stand_command(s, Path::new("/x")).unwrap().args.iter().any(|a| a == "--replace"));
            }
        }
    }
}
