//! Чистая логика менеджера Ruffle — порт `rexon/ruffle_manager.py`.
//!
//! Ruffle — Flash-эмулятор (MIT + Apache 2.0), которым Rexon рендерит `.swf`
//! как обои. Оригинальный класс `RuffleManager` смешивает детерминированные
//! решения (какой ассет релиза скачать под платформу, как построить URL,
//! argv запуска SWF, пути хранения в `~/.config/rexon/ruffle/`, выбор версии)
//! с побочными эффектами (реальное скачивание/HTTP, распаковка tar/zip,
//! запись бинаря, `chmod +x`, `subprocess.Popen`, проверки `os.path`). Здесь —
//! ТОЛЬКО чистая часть: вход → выход, без файловой системы и сети.
//!
//! Наблюдения за средой (установлен ли бинарь, есть ли системный ruffle в
//! PATH, есть ли web-версия, вывод `ruffle --version`, содержимое `.version`,
//! `platform.machine()`) подаются ПАРАМЕТРАМИ. Побочные эффекты (скачивание,
//! распаковка, `Popen`, `os.makedirs`, `shutil.rmtree`) остаются в Python-шиме
//! `ruffle_manager.py`, который исполняет то, что решило ядро (URL ассета,
//! argv, пути, план ensure).
//!
//! Паритет байт-в-байт с живым Python — `tests/ruffle_manager_parity.rs`
//! (эталон снят read-only, без реального скачивания/запуска subprocess).
//!
//! ⚠️ В оригинале НЕТ сравнения версий: `.version` хранит строку
//! `nightly-<дата>`, `get_version()` её лишь читает, обновление решается по
//! наличию файлов (`ensure_installed`), а не по номеру версии. Поэтому здесь
//! портированы построение/чтение строки версии и выбор источника
//! (`resolve_version`), но сравнения версий нет — сравнивать нечего.
//!
//! Починен баг оригинала (в side-effect-части, правка внесена и в
//! `ruffle_manager.py`): `RuffleManager.remove()` объявлен `-> bool`, но
//! всегда возвращал `None` — чистой части не касается, отмечено в .py.

// ============================================================================
// Константы (порт модульных констант ruffle_manager.py)
// ============================================================================

/// База GitHub Releases Ruffle — порт `GITHUB_RELEASES_URL`.
pub const GITHUB_RELEASES_URL: &str = "https://github.com/ruffle-rs/ruffle/releases";

/// Ожидаемый размер бинаря (МБ) — фолбэк прогресса при пустом Content-Length.
pub const EXPECTED_SIZE_MB: u64 = 25;
/// Ожидаемый размер web self-hosted (МБ).
pub const EXPECTED_SIZE_WEB_MB: u64 = 18;

/// URL web self-hosted (ruffle.js + ruffle.wasm) — порт `DOWNLOAD_URL_WEB`.
///
/// ⚠️ Мёртвая ссылка (проверено HEAD-запросом 15.09.2026): `latest` ведёт на
/// выпуск v0.6.0, где файлов `ruffle-nightly-*` нет, — ответ 404. Живые
/// ночные сборки носят дату в имени. Rexon Flash-обои так не показывает:
/// Rust-исполнитель зовёт режим хоста `rexon-wallpaper-host --mode swf`
/// (Ruffle как библиотека). Функция живёт, пока её зовёт классическое окно
/// на Python (`rexon/ruffle_manager.py` через мост `rexon-py`).
pub fn web_download_url() -> String {
    format!("{GITHUB_RELEASES_URL}/latest/download/ruffle-nightly-web-selfhosted.zip")
}

/// URL ассета релиза по архитектуре — порт `DOWNLOAD_URLS.get(arch)`.
/// Поддержаны только `x86_64` / `aarch64` (как в оригинале); прочее → None
/// (Python: «Архитектура … не поддерживается Ruffle»).
///
/// ⚠️ Та же мёртвая ссылка `latest/download/ruffle-nightly-*` (404, см.
/// [`web_download_url`]); нужна только классическому окну на Python.
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
// Архитектура — порт get_arch
// ============================================================================

/// Нормализация `platform.machine()` → ключ ассета. Порт `get_arch`:
/// строку приводим к нижнему регистру, `x86_64/amd64` → `x86_64`,
/// `aarch64/arm64` → `aarch64`, остальное — как есть (в нижнем регистре).
pub fn normalize_arch(machine: &str) -> String {
    let m = machine.to_lowercase();
    match m.as_str() {
        "x86_64" | "amd64" => "x86_64".to_string(),
        "aarch64" | "arm64" => "aarch64".to_string(),
        _ => m,
    }
}

// ============================================================================
// Пути хранения — порт модульных RUFFLE_* (expanduser("~/.config/rexon/ruffle"))
// ============================================================================

/// Пути Ruffle относительно домашнего каталога. Порт `RUFFLE_DIR`/`RUFFLE_BIN`/
/// `RUFFLE_WEB_DIR`/`RUFFLE_VERSION_FILE` (+ `web/ruffle.js`). `home` — то, во
/// что раскрывается `~` (`$HOME`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RufflePaths {
    pub dir: String,
    pub bin: String,
    pub web_dir: String,
    pub version_file: String,
    pub web_js: String,
}

/// Построить пути от домашнего каталога.
pub fn ruffle_paths(home: &str) -> RufflePaths {
    ruffle_paths_xdg(None, home)
}

/// То же с учётом `$XDG_CONFIG_HOME` (единый корень конфигов ядра).
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
// Наличие / выбор бинаря — порт is_installed / get_ruffle_path
// ============================================================================

/// Есть ли по пути НЕЧТО исполнимое — порт `is_installed`
/// (`isfile && os.access(X_OK)`). Наблюдения подаются параметрами.
///
/// ⚠️ Как ответ на вопрос «установлен ли Ruffle» этого НЕ ХВАТАЕТ: проверка
/// говорит о наличии файла, а не о пригодности проигрывателя (разбор — в
/// блоке ниже). Спрашивать надо [`is_installed_usable`]; эта остаётся только
/// потому, что её сигнатуру держит PyO3-мост.
pub fn is_installed(bin_is_file: bool, bin_is_executable: bool) -> bool {
    bin_is_file && bin_is_executable
}

// ============================================================================
// Пригодность бинаря — «установлен» ≠ «по пути что-то лежит»
// ============================================================================
//
// `is_installed` выше отвечает лишь на вопрос «файл на месте и исполним»,
// и этого НЕ ХВАТАЕТ. У Wehrwolfmann в `~/.config/rexon/ruffle/` лежал файл
// `ruffle` из десяти байт — строка `#!/bin/sh` с битом +x. Обе проверки его
// пропускали: Rexon считал проигрыватель установленным, `get_ruffle_path`
// отдавал заглушку в `run_swf`, а `ensure_decision` решал, что скачивать
// нечего. Покупатель в такой машине не может проиграть ни один `.swf` и не
// получает даже предложения доустановить Ruffle.
//
// Это тот же класс дефекта, что и «Popen не бросил исключение — значит
// обои запущены» (коммит 3661e21): проверка смотрит на НАЛИЧИЕ, а не на
// ПРИГОДНОСТЬ. Лечится он одинаково — спрашиваем у объекта то, что умеет
// только настоящий объект.
//
// Признак выбран так, чтобы проверку можно было звать часто (её зовут
// `is_installed`, `get_ruffle_path`, `ensure_installed`, вкладка обоев):
// запускать процесс на каждый чих нельзя, а прочитать 20 байт головы файла
// — дешевле, чем сам `os.access`. Смотрим на то, что делает файл
// исполняемым проигрывателем:
//
//   * сигнатура ELF (`\x7fELF`) — отсекает скрипты-заглушки, HTML-страницы
//     ошибок вместо архива, обрывки и текстовые «заметки на память»;
//   * 64-разрядный little-endian ELF — Ruffle других сборок под Linux не
//     публикует;
//   * тип файла ET_EXEC/ET_DYN — не объектник и не библиотека;
//   * `e_machine` совпадает с архитектурой машины — бинарь, скачанный под
//     чужую архитектуру, не запустится;
//   * размер не меньше [`MIN_BINARY_BYTES`] — ловит оборванную загрузку,
//     когда голова уже правильная, а хвоста нет.
//
// Размер здесь ПОСЛЕДНИЙ и вспомогательный именно потому, что признак
// слабый: завтра заглушка будет не 10 байт, а 200 КБ. Несущая проверка —
// сигнатура и `e_machine`.

/// Сколько байт головы файла нужно прочитать для суждения о пригодности.
/// Ровно заголовок ELF до `e_machine` включительно.
pub const BINARY_HEAD_BYTES: usize = 20;

/// Нижняя граница правдоподобного размера бинаря Ruffle (байты).
///
/// Ruffle-nightly под Linux — десятки мегабайт (в нём wgpu, свой рендерер и
/// вся ActionScript-машина). Порог в 4 МБ не отсекает ни одной реальной
/// сборки и при этом ловит оборванную загрузку.
pub const MIN_BINARY_BYTES: u64 = 4 * 1024 * 1024;

/// Код архитектуры в поле `e_machine` заголовка ELF.
///
/// Вход — уже нормализованная строка из [`normalize_arch`]. Архитектуры, под
/// которые Ruffle сборок не публикует (см. [`download_url`]), кода не имеют:
/// пригодного бинаря там взяться неоткуда.
pub fn elf_machine(arch: &str) -> Option<u16> {
    match arch {
        "x86_64" => Some(0x3E),
        "aarch64" => Some(0xB7),
        _ => None,
    }
}

/// Похож ли файл на исполняемый бинарь Ruffle для этой машины.
///
/// `head` — первые [`BINARY_HEAD_BYTES`] байт файла (меньше — сразу нет),
/// `size` — его размер, `arch` — нормализованная архитектура машины.
/// Чтение файла и `platform.machine()` — наблюдения исполнителя.
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
    // e_type: ET_EXEC (2) либо ET_DYN (3) — Ruffle собирается PIE, то есть
    // ET_DYN; ET_EXEC оставлен на случай сборки без PIE.
    let e_type = u16::from_le_bytes([head[16], head[17]]);
    if e_type != 2 && e_type != 3 {
        return false;
    }
    let e_machine = u16::from_le_bytes([head[18], head[19]]);
    elf_machine(arch) == Some(e_machine)
}

/// Установлен ли ПРИГОДНЫЙ бинарь — ужесточение [`is_installed`].
///
/// Отдельная функция, а не третий параметр у [`is_installed`], потому что
/// сигнатуру последней держит PyO3-мост; когда мост переведут на эту,
/// [`is_installed`] уйдёт вместе со своим вызовом.
pub fn is_installed_usable(
    bin_is_file: bool,
    bin_is_executable: bool,
    bin_is_runnable: bool,
) -> bool {
    is_installed(bin_is_file, bin_is_executable) && bin_is_runnable
}

/// Пригодна ли распакованная web-версия (`ruffle.js` + `.wasm`).
///
/// Тот же дефект и здесь: `is_web_installed` в шиме проверяет только
/// `isfile(ruffle.js)`, и девятибайтовый `// ruffle` из той же папки
/// Wehrwolfmann'а её проходит. Сам по себе `ruffle.js` — загрузчик: без
/// `.wasm` рядом он не проиграет ничего, поэтому пригодность = непустой
/// загрузчик И хотя бы один модуль `.wasm` в каталоге.
pub const MIN_WEB_JS_BYTES: u64 = 1024;

/// Решение о пригодности web-версии по наблюдениям каталога.
pub fn web_is_usable(js_is_file: bool, js_size: u64, wasm_count: usize) -> bool {
    js_is_file && js_size >= MIN_WEB_JS_BYTES && wasm_count > 0
}

/// Имена бинаря Ruffle в пакетах разных дистрибутивов — порт
/// `RuffleManager.SYSTEM_BINS`. Порядок значим: `system_bin` берёт ПЕРВОЕ
/// найденное в `PATH` имя.
pub const SYSTEM_BINS: &[&str] = &["ruffle", "ruffle-nightly", "ruffle_desktop"];

/// Системный Ruffle из пакета дистрибутива — порт `system_bin`: перебор
/// [`SYSTEM_BINS`] до первого найденного. Поиск в `PATH` (`shutil.which`) —
/// побочный эффект, подаётся замыканием `which`, возвращающим найденный путь.
pub fn select_system_bin<F>(which: F) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    SYSTEM_BINS.iter().find_map(|name| which(name))
}

/// Копировать ли файл из архива рядом с бинарём — порт предиката
/// `f.startswith("LICENSE") or f == "README.md"` в `download()`.
/// Лицензии Ruffle (MIT + Apache 2.0) обязаны лежать рядом с бинарём: мы его
/// распространяем не сами, но пользователю они должны быть доступны.
pub fn is_extra_asset(name: &str) -> bool {
    name.starts_with("LICENSE") || name == "README.md"
}

/// Путь к бинарю: наш → системный (из PATH, буквально `"ruffle"`) → нет.
/// Порт `get_ruffle_path`.
pub fn ruffle_path(
    is_installed: bool,
    is_system_installed: bool,
    bin_path: &str,
) -> Option<String> {
    // Решение Wehrwolfmann 2026-07-29: сначала пакет из репозитория
    // дистрибутива, наша скачанная копия — только фолбэк. Пакет
    // обновляется и чинится вместе с системой, подписан её ключом и не
    // требует тянуть 25 МБ мимо пакетного менеджера.
    if is_system_installed {
        Some("ruffle".to_string())
    } else if is_installed {
        Some(bin_path.to_string())
    } else {
        None
    }
}

// ============================================================================
// Версия — порт get_version (выбор источника; сравнения версий в оригинале нет)
// ============================================================================

/// Выбор строки версии — порт логики `get_version`:
///   * нет пути к ruffle → None (файл `.version` НЕ читается — как в Python);
///   * `ruffle --version` завершился успешно (rc==0) → его stdout,
///     обрезанный по краям (`cmd_output = Some(stdout)` при rc==0);
///   * иначе — содержимое `.version`, обрезанное (`version_file`);
///   * иначе None.
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

/// Содержимое файла `.version`, которое пишет `download()` —
/// порт `f"nightly-{time.strftime('%Y-%m-%d')}"`. Дата подаётся параметром
/// (её вычисление — побочный эффект исполнителя).
pub fn version_file_content(date: &str) -> String {
    format!("nightly-{date}")
}

/// Чтение строки версии из файла `.version` — порт `f.read().strip()`.
pub fn parse_version_file(content: &str) -> String {
    content.trim().to_string()
}

// ============================================================================
// Запуск SWF — порт run_swf / run_swf_as_wallpaper (argv + скрипт-обёртка)
// ============================================================================

/// argv прямого запуска SWF — порт `run_swf`. `swf_abspath` — уже абсолютный
/// путь (`os.path.abspath` — побочный эффект, зависит от cwd — делает шим).
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

/// Базовое имя без расширения — порт `os.path.splitext(os.path.basename(p))[0]`
/// (posix-семантика: последний `/` → basename, последняя точка → расширение,
/// ведущие точки расширением НЕ считаются).
pub fn swf_stem(swf_path: &str) -> String {
    splitext_root(basename(swf_path)).to_string()
}

fn basename(p: &str) -> &str {
    match p.rfind('/') {
        Some(i) => &p[i + 1..],
        None => p,
    }
}

/// Корень имени (часть до расширения) для basename без `/` — порт
/// `posixpath.splitext` (ведущие точки не отделяются).
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
    // Всё до точки — сами точки (например ".hidden"): расширения нет.
    name
}

/// Путь скрипта-обёртки — порт `wrapper_dir/f"run_{swf_name}.sh"`.
pub fn wrapper_path(dir: &str, swf_stem: &str) -> String {
    format!("{dir}/wrappers/run_{swf_stem}.sh")
}

/// Содержимое bash-обёртки, запускающей Ruffle — порт heredoc из
/// `run_swf_as_wallpaper` (всегда `--fullscreen`). `swf_abspath` — абсолютный.
pub fn wrapper_script(ruffle: &str, swf_abspath: &str) -> String {
    format!("#!/bin/bash\nexec \"{ruffle}\" --fullscreen \"{swf_abspath}\"\n")
}

/// argv запуска SWF как обои — `rexon-wallpaper-host --mode swf <ролик>`.
///
/// `host` — путь к собранному хосту обоев
/// ([`crate::wallpaper_engine::find_wallpaper_host_binary`]), `swf_abspath` —
/// абсолютный путь к самому ролику. Внешний Ruffle и bash-обёртка
/// ([`wrapper_script`]) этой дорогой не нужны: Flash играет НАТИВНО, Ruffle
/// подключён библиотекой (крейт `rexon-swf`), кадры пишет сам хост.
///
/// ⭐ Починка 16.09.2026 (блок H). Здесь стоял мёртвый путь
/// `<python> -m rexon.app_wallpaper <обёртка> --mode subprocess`: модуль
/// `rexon.app_wallpaper` снят с дерева коммитом `2091eac`, процесс падал
/// сразу, и SWF-обои не работали с 23.08.2026 — молча, с зелёным рапортом
/// «обои применены».
///
/// ⚠️ ХВОСТ ДЛЯ МОСТА. Подпись сохранена (два `&str` → `Vec<String>`), но
/// СМЫСЛ обоих аргументов другой. Мост `rexon-py`
/// (`lib.rs: ruffle_wallpaper_argv`) и `rexon/ruffle_manager.py:492` всё ещё
/// передают сюда интерпретатор и путь обёртки — их надо перевести на хост и
/// ролик. Регрессии это не даёт: прежняя дорога и так была мёртвой.
/// Rust-исполнитель этой функцией не пользуется вовсе — он строит план
/// [`crate::wallpaper_engine::swf_host_plan`].
pub fn wallpaper_argv(host: &str, swf_abspath: &str) -> Vec<String> {
    use crate::wallpaper_engine::{wallpaper_host_argv, HostMode};
    wallpaper_host_argv(host, HostMode::Swf, swf_abspath)
}

// ============================================================================
// ensure_installed — решение, что доскачать
// ============================================================================

/// Что нужно доустановить — порт первой части `ensure_installed`.
/// Итоговый успех (`return success`) определяет ИСПОЛНИТЕЛЬ по результату
/// реальных `download()`/`download_web()`; web некритичен (лишь warning).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureDecision {
    pub need_bin: bool,
    pub need_web: bool,
}

/// Решение ensure по наблюдениям среды.
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
    /// Всё на месте — ранний `return True` в Python (скачивать нечего).
    pub fn already_ok(&self) -> bool {
        !self.need_bin && !self.need_web
    }
}

// ============================================================================
// Прогресс скачивания — чистая арифметика _download_file / CLI progress-bar
// ============================================================================

/// Процент скачивания — порт `min(75, int(downloaded/total*75))`
/// (фаза скачивания занимает 0-75%; распаковка/установка — 80/90/100 отдельно).
/// `total` предполагается > 0 (исполнитель подставляет
/// `EXPECTED_SIZE_MB * 1024 * 1024` при пустом Content-Length).
pub fn download_progress_pct(downloaded: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    let pct = ((downloaded as f64 / total as f64) * 75.0) as u64;
    pct.min(75)
}

/// Ширина CLI-прогрессбара — порт `bar_w = 30` из CLI `progress()`.
pub const PROGRESS_BAR_WIDTH: usize = 30;

/// CLI-прогрессбар — порт `"█"*filled + "░"*(bar_w-filled)`,
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

    /// Голова 64-разрядного little-endian ELF: тип `e_type`, машина `e_machine`.
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

    /// Ровно тот файл, что лежал у Wehrwolfmann: `#!/bin/sh` с битом +x.
    #[test]
    fn shell_stub_is_not_a_player() {
        let stub = b"#!/bin/sh\n";
        assert!(!binary_is_runnable(stub, stub.len() as u64, "x86_64"));
        // И весь вывод продукта на нём: «установлено» больше не выходит.
        assert!(is_installed(true, true), "старая проверка заглушку пропускала");
        assert!(!is_installed_usable(true, true, false));
    }

    /// Настоящая сборка Ruffle (PIE под x86_64) — пригодна.
    #[test]
    fn real_ruffle_binary_passes() {
        let head = elf_head(3, 0x3E);
        assert!(binary_is_runnable(&head, BIG, "x86_64"));
        assert!(is_installed_usable(true, true, true));
        // Сборка без PIE (ET_EXEC) тоже.
        assert!(binary_is_runnable(&elf_head(2, 0x3E), BIG, "x86_64"));
        // И то же на aarch64.
        assert!(binary_is_runnable(&elf_head(3, 0xB7), BIG, "aarch64"));
    }

    /// Бинарь под чужую архитектуру не запустится — значит не установлен.
    #[test]
    fn foreign_architecture_rejected() {
        assert!(!binary_is_runnable(&elf_head(3, 0xB7), BIG, "x86_64"));
        assert!(!binary_is_runnable(&elf_head(3, 0x3E), BIG, "aarch64"));
        // Архитектура, под которую Ruffle сборок не выпускает.
        assert_eq!(elf_machine("riscv64"), None);
        assert!(!binary_is_runnable(&elf_head(3, 0xF3), BIG, "riscv64"));
    }

    /// Не-ELF, 32-разрядный, big-endian и не-исполняемый ELF — мимо.
    #[test]
    fn wrong_shape_rejected() {
        // HTML-страница ошибки вместо архива.
        let html = b"<!DOCTYPE html><html><head><ti";
        assert!(!binary_is_runnable(html, BIG, "x86_64"));
        let mut h32 = elf_head(3, 0x3E);
        h32[4] = 1; // ELFCLASS32
        assert!(!binary_is_runnable(&h32, BIG, "x86_64"));
        let mut hbe = elf_head(3, 0x3E);
        hbe[5] = 2; // ELFDATA2MSB
        assert!(!binary_is_runnable(&hbe, BIG, "x86_64"));
        // ET_REL (объектник) и ET_CORE — не проигрыватели.
        assert!(!binary_is_runnable(&elf_head(1, 0x3E), BIG, "x86_64"));
        assert!(!binary_is_runnable(&elf_head(4, 0x3E), BIG, "x86_64"));
    }

    /// Оборванная загрузка: голова правильная, хвоста нет.
    #[test]
    fn truncated_download_rejected() {
        let head = elf_head(3, 0x3E);
        assert!(!binary_is_runnable(&head, 1024 * 1024, "x86_64"));
        assert!(binary_is_runnable(&head, MIN_BINARY_BYTES, "x86_64"));
        // Пустой файл и слишком короткая голова.
        assert!(!binary_is_runnable(&[], 0, "x86_64"));
        assert!(!binary_is_runnable(&head[..8], BIG, "x86_64"));
    }

    /// Бит +x и наличие файла остаются обязательными.
    #[test]
    fn usable_needs_all_three() {
        assert!(!is_installed_usable(false, true, true));
        assert!(!is_installed_usable(true, false, true));
        assert!(!is_installed_usable(true, true, false));
        assert!(is_installed_usable(true, true, true));
    }

    /// Web-версия: девятибайтовый `// ruffle` и загрузчик без `.wasm` — не в счёт.
    #[test]
    fn web_stub_is_not_installed() {
        assert!(!web_is_usable(true, 9, 0));
        assert!(!web_is_usable(true, 9, 1));
        assert!(!web_is_usable(true, 512 * 1024, 0));
        assert!(!web_is_usable(false, 512 * 1024, 1));
        assert!(web_is_usable(true, 512 * 1024, 1));
    }
}
