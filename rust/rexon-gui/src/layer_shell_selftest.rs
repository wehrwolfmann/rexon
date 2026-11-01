// Стенд слоя обоев: доказательство, что окно Rust-пути реально становится
// фоновой поверхностью wlr-layer-shell на живом KWin Wayland.
//
// Почему стенд устроен двумя процессами. Единственное честное доказательство
// «окно в слое обоев» — байты, которые клиент отправил композитору: обратное
// чтение из LayerShellQt показывает только НАШЕ же состояние и прошло бы даже
// в случае, когда библиотека молча ничего не сделала. Поэтому родитель
// запускает сам себя с WAYLAND_DEBUG=1, складывает протокольный след в файл и
// выносит приговор по следу, а не по самоотчёту ребёнка.
//
// Что доказывается следом:
//   * zwlr_layer_shell_v1.get_layer_surface с аргументом layer = 0
//     (background) — окно создано КАК поверхность слоя, а не как обычное;
//   * set_anchor(15) — якоря на все четыре стороны;
//   * set_exclusive_zone(-1) — зона обоев;
//   * входящий zwlr_layer_surface_v1.configure — композитор поверхность
//     ПРИНЯЛ и выдал ей геометрию (при отказе был бы протокольный сбой);
//   * во всём следе нет ни одного xdg_surface.get_toplevel — окно ни на
//     мгновение не было обычным окном, а значит и в списке задач его быть
//     не может: списки задач (plasma-window-management) показывают
//     xdg_toplevel, поверхности слоёв туда не попадают по устройству
//     протокола.
//
// ⚠️ Прежний вотчдог стенда выходил кодом 0 по таймауту — «не отчитался»
// выглядело как «прошло». Здесь молчание ребёнка, отсутствие следа, пустой
// след и любое расхождение дают НЕНУЛЕВОЙ код.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Аргумент, которым родитель запускает себя же в роли подопытного окна.
pub const PROBE_ARG: &str = "--layer-shell-probe";
/// Аргумент запуска стенда целиком.
pub const SELFTEST_ARG: &str = "--layer-shell-selftest";
/// Аргумент проверки стенда на укус: `--layer-shell-bite=<вид>`. Прогон с
/// заведомо испорченным слоем ОБЯЗАН покраснеть; зелёный тут — провал.
pub const BITE_ARG: &str = "--layer-shell-bite";
/// Переменная, которой родитель передаёт вид укуса подопытному окну.
pub const BITE_ENV: &str = "REXON_LS_BITE";

/// Виды укуса — каждый ломает слой обоев по-своему. Разбор самих поломок —
/// в qml/LayerShellProbe.qml, здесь только список для прогона «все подряд».
pub const BITE_KINDS: &[&str] = &["layer", "anchors", "zone", "keyboard", "size", "late"];

/// Идентификатор поверхности для композитора. Он же ищется в следе — по нему
/// стенд отличает своё окно от чужих, если в следе вдруг окажутся другие.
pub const SCOPE: &str = "rexon-layer-shell-selftest";

/// Сколько ждать ребёнка, прежде чем считать стенд проваленным.
const CHILD_TIMEOUT: Duration = Duration::from_secs(40);

/// Убийца процесса-ребёнка. Существует ради одного: окно на ЖИВОМ рабочем
/// столе Wehrwolfmann обязано исчезнуть при любом исходе — при провале
/// проверки, при панике, при раннем `return`. Закрытие «в конце функции» это
/// не гарантирует, Drop — гарантирует.
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
                    // Убиваем и ОБЯЗАТЕЛЬНО дожидаемся: без wait процесс
                    // остаётся зомби, а его окно — на экране до тех пор, пока
                    // композитор не заметит разрыв сокета.
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}

/// Родительская половина стенда: обычный прогон, слой обязан встать.
pub fn run() -> i32 {
    run_with_bite("")
}

/// Проверка самого стенда на живом композиторе. Каждый вид укуса поднимает
/// окно с ЗАВЕДОМО испорченным слоем и требует, чтобы приговор был красным.
///
/// Зачем это отдельно от мутационных тестов `judge` ниже: те проверяют
/// разборщик следа на выдуманных строках. Здесь ломается НАСТОЯЩЕЕ окно на
/// НАСТОЯЩЕМ композиторе — так ловится случай, когда стенд зеленеет не
/// потому, что слой встал, а потому, что до проверок дело не дошло.
///
/// `kind` пуст — прогоняются все виды подряд.
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
        // 1 — стенд нашёл расхождение, то есть укус пойман. 0 — стенд
        // остался зелёным на заведомо сломанном слое, это его провал.
        // 2 — прогон не состоялся, доказать нечего.
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

/// Один прогон. `bite` пуст — честный слой обоев; иначе подопытному окну
/// подсовывается заведомая поломка (см. `BITE_KINDS`).
///
/// Возвращает код возврата процесса: 0 — доказано, 1 — расхождение,
/// 2 — прогон не состоялся.
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
    // Вид укуса едет ребёнку переменной окружения, а не аргументом: окно
    // читает её через setInitialProperties, и продуктовый разбор аргументов
    // тут ни при чём.
    if !bite.is_empty() {
        command.env(BITE_ENV, bite);
    }
    let spawned = command
        // Ключ ко всему стенду: libwayland печатает в stderr каждое
        // сообщение протокола. Ставится ребёнку, а не себе, — родителю
        // Wayland вообще не нужен.
        .env("WAYLAND_DEBUG", "1")
        // Без этого на CachyOS сообщения QML уходят в journald, когда stderr
        // не терминал, и отчёт ребёнка в след не попадает.
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
    // Здесь guard ещё жив; если ребёнок не завершился, его убьёт Drop ниже —
    // окно уйдёт с экрана в любом случае.

    let trace = read_trace(&trace_path);
    // След ребёнка полезен и при успехе, и при провале: строки отчёта QML
    // лежат в том же файле, что и протокол.
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
        // След не нужен: он большой (десятки тысяч строк протокола).
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
    // Протокольный след — не обязательно валидный UTF-8 (в нём мелькают
    // сырые байты заголовков окон), поэтому lossy.
    String::from_utf8_lossy(&buf).into_owned()
}

/// Результат разбора: пустой список проблем = стенд зелёный.
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

/// Приговор по коду возврата ребёнка и протокольному следу. Чистая функция —
/// у неё есть свои тесты ниже, в том числе на ЛОЖНО-ЗЕЛЁНЫЙ случай.
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

    // ── Самоотчёт окна ────────────────────────────────────────────────────
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

    // ── Протокол ──────────────────────────────────────────────────────────
    v.require(
        !trace.contains(".get_toplevel("),
        "в следе есть xdg_surface.get_toplevel — окно было обычным, \
         а такое попадает в список задач",
    );
    // Протокольная ошибка обрывает соединение: всё, что «получилось» после
    // неё, ничего не значит.
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
    // Реакция на клавиатуру по умолчанию нулевая, и LayerShellQt имеет право
    // не слать запрос вовсе; проверяем только если он есть.
    if let Some(kbd) = request_arg(trace, &prefix, "set_keyboard_interactivity") {
        v.require(
            kbd == "0",
            format!("set_keyboard_interactivity {kbd} — окно обоев ловит клавиатуру"),
        );
    }

    // Композитор вправе прогнать поверхность (closed) — например, если счёл
    // запрос недопустимым. Тогда окна на экране нет, что бы ни говорили
    // остальные проверки.
    v.require(
        !trace.contains(&format!("{prefix}closed(")),
        "композитор прогнал поверхность слоя (closed)",
    );

    // Входящий configure — единственное место, где высказывается САМ
    // композитор. Без него всё вышеперечисленное осталось бы намерением.
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

/// Последняя строка следа с указанной меткой (последняя — чтобы повторный
/// отчёт не был перекрыт первым).
fn find_line<'a>(trace: &'a str, tag: &str) -> Option<&'a str> {
    trace.lines().rfind(|l| l.contains(tag))
}

/// Значение поля `ключ=значение` в строке отчёта.
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace().find_map(|token| {
        let (k, val) = token.split_once('=')?;
        (k == key).then_some(val)
    })
}

/// Разобранный вызов get_layer_surface.
#[derive(Debug)]
pub struct LayerSurface {
    /// Как поверхность зовут в следе, например `zwlr_layer_surface_v1@23`.
    pub object: String,
    pub layer: i64,
    pub namespace: String,
}

/// Найти создание поверхности слоя в следе.
///
/// Формат строки libwayland:
/// `[123.456]  -> zwlr_layer_shell_v1@12.get_layer_surface(new id
/// zwlr_layer_surface_v1@23, wl_surface@22, nil, 0, "имя")`
fn find_layer_surface(trace: &str) -> Option<LayerSurface> {
    for line in trace.lines() {
        if !line.contains("get_layer_surface(") {
            continue;
        }
        let args = split_args(paren_body(line)?);
        if args.len() < 5 {
            continue;
        }
        // Первый аргумент — «new id zwlr_layer_surface_v1@23».
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

/// Единственный аргумент исходящего запроса `<объект>.<имя>(значение)`.
/// Берётся ПОСЛЕДНИЙ вызов: клиент вправе переслать запрос, и решает
/// итоговое значение.
fn request_arg(trace: &str, object_prefix: &str, request: &str) -> Option<String> {
    let needle = format!("{object_prefix}{request}(");
    trace
        .lines()
        .filter(|line| line.contains(&needle))
        .filter_map(|line| Some(split_args(paren_body(line)?).first()?.clone()))
        .next_back()
}

/// Аргументы вызова `<объект>.<имя>(...)`, если строка — именно он.
fn args_of(line: &str, full_name: &str) -> Option<Vec<String>> {
    let needle = format!("{full_name}(");
    line.contains(&needle)
        .then(|| paren_body(line).map(split_args))
        .flatten()
}

/// Содержимое самых внешних скобок строки.
fn paren_body(line: &str) -> Option<&str> {
    let start = line.find('(')?;
    let end = line.rfind(')')?;
    (end > start).then(|| &line[start + 1..end])
}

/// Разбор списка аргументов. Строковые значения в кавычках могут содержать
/// запятую, поэтому просто split(',') недостаточно.
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

    /// След «как надо»: заведомо зелёный образец, от которого пляшут
    /// остальные случаи.
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

    /// Главная проверка самого стенда: он обязан КРАСНЕТЬ. Каждая мутация —
    /// это реальный способ сломать слой обоев.
    #[test]
    fn каждая_мутация_валит_стенд() {
        let cases: [(&str, String); 9] = [
            // Окно осталось обычным — попадёт в список задач и встанет
            // поверх других окон.
            (
                "обычное окно",
                good_trace().replace(
                    "zwlr_layer_shell_v1@12.get_layer_surface",
                    "xdg_surface@23.get_toplevel",
                ),
            ),
            // Слой Top вместо Background — обои поверх всего.
            ("слой top", good_trace().replace("nil, 0, ", "nil, 2, ")),
            // Якорь только сверху — обои не на весь экран.
            (
                "один якорь",
                good_trace().replace("set_anchor(15)", "set_anchor(1)"),
            ),
            // Зона 0 вместо -1 — панели начнут отодвигать обои.
            (
                "зона 0",
                good_trace().replace("set_exclusive_zone(-1)", "set_exclusive_zone(0)"),
            ),
            // Композитор поверхность не принял.
            (
                "нет configure",
                good_trace().replace("zwlr_layer_surface_v1@23.configure(7, 1920, 1080)", ""),
            ),
            // Окно меньше экрана.
            (
                "окно не во весь экран",
                good_trace().replace("windowW=1920 windowH=1080", "windowW=1280 windowH=720"),
            ),
            // Обои перехватывают клавиатуру.
            (
                "клавиатура",
                good_trace().replace("keyboard=0", "keyboard=2"),
            ),
            // Композитор прогнал поверхность — окна на экране нет.
            (
                "closed",
                good_trace() + "[1.020] zwlr_layer_surface_v1@23.closed()\n",
            ),
            // Протокольная ошибка обрывает соединение.
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
        // Ровно та грабля, которую закрыли: таймаут обязан быть провалом.
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
