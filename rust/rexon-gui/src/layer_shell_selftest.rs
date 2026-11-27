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
