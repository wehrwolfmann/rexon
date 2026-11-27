//! Сторож живых обоев — ОДИН на оба вида процессов, которые их рисуют:
//! рисовальщики waywallen (сцены и веб Wallpaper Engine) и собственный хост
//! Rexon `rexon-wallpaper-host` (видео, веб, Flash). Упал — поднять обои
//! заново; падает подряд — перестать и сказать человеку.
//!
//! Общее у двух видов — решение «поднимать ли ещё раз» ([`CrashBudget`]:
//! сколько подъёмов тех же обоев за окно времени) и поток сторожа
//! ([`run_forever`]). Разное — как узнать, что процесс лёг, и чем его
//! поднимать: у рисовальщика waywallen процесс чужой (ребёнок демона), и
//! падение видно по записи `systemd-coredump`, а поднимает демон; хост —
//! наш ребёнок (исполнитель `wallpaper_exec`) либо ребёнок расширения
//! оболочки GNOME/Cinnamon, и поднимается он тем же применением, что по
//! кнопке. Этому посвящена вторая половина модуля ([`HostWatch`]).
//!
//! РИСОВАЛЬЩИКИ WAYWALLEN. ЗАЧЕМ. Сцены и веб-обои рисует отдельный процесс waywallen
//! (`waywallen-<вид>-renderer`). Если он аварийно завершается — сбой
//! драйвера видеокарты, ошибка в самой сцене, — демон только убирает его из
//! своих списков: выбранные обои за ним остаются (`CurrentWallpaperId`), а на
//! рабочем столе пусто, и человеку никто ничего не говорит (живой случай
//! 10.10.2026: сброс видеокарты → рисовальщик упал → стол без обоев).
//!
//! ЧТО ДЕЛАЕТ. Пока Rexon работает (значок в лотке), сторож раз в
//! [`POLL_SECS`] смотрит на процессы рисовальщиков. Пропал процесс —
//! спрашивает журнал системы, был ли это аварийный выход (запись
//! `systemd-coredump` с номером процесса). Был — просит демона применить те
//! же обои ещё раз и говорит об этом уведомлением.
//!
//! ЧЕГО НЕ ДЕЛАЕТ.
//! * Не трогает обои, остановленные намеренно: правила автопаузы демона
//!   («сеанс заблокирован — стоп») и смена обоев человеком тоже убирают
//!   процесс, но записи об аварии при этом нет — и сторож молчит.
//! * Не перезапускает по кругу: после [`MAX_RECOVERIES`] подъёмов тех же
//!   обоев за [`RECOVERY_WINDOW_SECS`] останавливается и говорит, что обои
//!   падают сами. Каждое такое падение может стоить сброса видеокарты.
//! * Не видит падений без записи об аварии (процесс убит по нехватке памяти,
//!   вышел с кодом ошибки) и не работает там, где нет `journalctl`/`busctl`.
//!
//! Решение — чистая функция [`Watch::tick`]; всё, что ходит наружу, — в
//! «живой» части внизу.
//!
//! СОБСТВЕННЫЙ ХОСТ ОБОЕВ. Хост запускается один раз при «Применить»; лёг
//! (сброс видеокарты, сон, ошибка драйвера) — стол пустел, и поднять его
//! было некому. Теперь [`HostWatch::tick`] решает по трём признакам:
//! * процесс завершился сам (не по «Остановить» и не под замену);
//! * процесс жив, но не рисует: хост раз в [`HEARTBEAT_INTERVAL_MS`] пишет
//!   метку сердцебиения ([`heartbeat_path`]) из потока интерфейса и при этом
//!   просит кадр у каждого окна. Зависни отрисовка — встанет и поток
//!   интерфейса, метка перестанет обновляться. Проверка — после выхода из
//!   сна (сигнал logind `PrepareForSleep(false)`) и, мягче, всё время;
//! * сброс видеокарты (новая запись `devcoredump` у устройства DRM либо
//!   событие ядра `WEDGED=`): показ пересоздаётся, даже если процесс жив —
//!   после сброса картинка бывает чёрной при живом процессе.
//! Подъёмы те же, что у рисовальщика, но мягче по числу: после
//! [`HOST_MAX_RESTARTS`] подъёмов за окно следующее падение уже не
//! поднимается — на стол ставится неподвижный кадр этих же обоев (у видео —
//! кадр из ролика, у веба и Flash — превью рядом, если есть; иначе прежняя
//! картинка стола) и показывается ОДНО уведомление. Сами подъёмы молчат —
//! только журнал: человеку нечего с ними делать.
//!
//! ПОЧЕМУ НЕ СЛУЖБА systemd с `Restart=on-failure` (как у движка
//! waywallen). Хостом целиком владеет исполнитель обоев: заморозка по
//! нагрузке (SIGSTOP своему ребёнку), спутники на других мониторах, запасные
//! шаги плана, возврат стола по «Остановить». Под службой всё это пришлось
//! бы делать вторым путём через `systemctl`, а кадр после падений, одно
//! уведомление и проверку после сна служба не умеет — сторож всё равно был
//! бы нужен, и механизмов стало бы два. На GNOME и Cinnamon-Wayland хост
//! вообще ребёнок оболочки, службе его не отдать. И без systemd (Void,
//! Alpine, Gentoo с OpenRC) сторож работает так же.

use std::collections::HashMap;

use crate::i18n::tr;

/// Как часто смотреть на процессы, секунды.
pub const POLL_SECS: u64 = 10;
/// Сколько проходов ждать записи об аварии после пропажи процесса:
/// `systemd-coredump` пишет её не мгновенно (большой образ памяти — секунды).
pub const CRASH_WAIT_TICKS: u32 = 3;
/// Окно, в котором считаются подъёмы одних и тех же обоев, секунды.
pub const RECOVERY_WINDOW_SECS: u64 = 600;
/// Сколько раз за окно поднимать одни и те же обои. Одного раза хватает на
/// случайный сбой; второй подряд — значит, обои падают сами.
pub const MAX_RECOVERIES: usize = 1;

/// Сколько раз подряд поднимать одни и те же обои: подъёмы за окно
/// `window` секунд, по обоям. Общий для рисовальщиков waywallen и хоста —
/// отличаются только пределом.
#[derive(Debug, Default)]
pub struct CrashBudget {
    /// Сделанные подъёмы: (время, обои).
    recoveries: Vec<(u64, String)>,
    /// Обои, которые решено больше не поднимать (пока человек их не сменит).
    given_up: Option<String>,
}

impl CrashBudget {
    /// Человек выбрал другие обои — прежний отказ к ним не относится.
    pub fn forget_if_changed(&mut self, current: &str) {
        if self.given_up.as_deref().is_some_and(|id| id != current) {
            self.given_up = None;
        }
    }

    /// От этих обоев уже отказались.
    pub fn is_given_up(&self, id: &str) -> bool {
        self.given_up.as_deref() == Some(id)
    }

    /// Ещё одно падение обоев `id`: истина — поднимать (подъём записан),
    /// ложь — предел за окно исчерпан, обои помечены «больше не поднимать».
    pub fn spend(&mut self, now: u64, id: &str, max: usize, window: u64) -> bool {
        self.recoveries.retain(|(at, _)| now.saturating_sub(*at) <= window);
        let recent = self.recoveries.iter().filter(|(_, was)| was == id).count();
        if recent >= max {
            self.given_up = Some(id.to_string());
            return false;
        }
        self.recoveries.push((now, id.to_string()));
        true
    }
}

/// Что делать после очередного прохода.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Ничего: всё работает либо вмешиваться не нужно.
    Nothing,
    /// Рисовальщик упал — применить обои `id` заново.
    Recover { id: String },
    /// Обои `id` падают повторно — больше не поднимать, сказать человеку.
    GiveUp { id: String },
}

/// Память сторожа между проходами.
#[derive(Debug, Default)]
pub struct Watch {
    /// Живые рисовальщики: номер процесса → обои, выбранные при нём.
    seen: HashMap<u32, String>,
    /// Пропавшие процессы, по которым ещё ждём записи об аварии:
    /// (номер процесса, обои, сколько проходов осталось ждать).
    pending: Vec<(u32, String, u32)>,
    /// Сколько ещё можно поднимать.
    budget: CrashBudget,
}

impl Watch {
    /// Один проход.
    ///
    /// `now` — время, секунды; `alive` — номера живых процессов
    /// рисовальщиков; `current_id` — обои, выбранные у демона (пусто — демон
    /// молчит либо обоев нет); `crashed(pid)` — есть ли в журнале системы
    /// запись об аварийном выходе этого процесса.
    pub fn tick(
        &mut self,
        now: u64,
        alive: &[u32],
        current_id: &str,
        crashed: &dyn Fn(u32) -> bool,
    ) -> Step {
        self.budget.forget_if_changed(current_id);

        // Пропавшие процессы — в ожидание записи об аварии.
        let gone: Vec<u32> = self.seen.keys().copied().filter(|pid| !alive.contains(pid)).collect();
        for pid in gone {
            if let Some(id) = self.seen.remove(&pid) {
                self.pending.push((pid, id, CRASH_WAIT_TICKS));
            }
        }
        // Новые процессы запоминаем с обоями, выбранными сейчас.
        for pid in alive {
            self.seen.entry(*pid).or_insert_with(|| current_id.to_string());
        }

        // Кто из пропавших упал на самом деле.
        let mut crashed_id: Option<String> = None;
        let mut still_waiting = Vec::new();
        for (pid, id, left) in std::mem::take(&mut self.pending) {
            if crashed(pid) {
                crashed_id = Some(id);
            } else if left > 1 {
                still_waiting.push((pid, id, left - 1));
            }
        }
        self.pending = still_waiting;

        let Some(id) = crashed_id else { return Step::Nothing };
        // Поднимаем только то, что человек всё ещё хочет видеть: обои те же,
        // рисовальщика нет (демон не поднял его сам, человек не выбрал
        // другое), и мы от этих обоев ещё не отказались.
        if id.is_empty() || id != current_id || !alive.is_empty() {
            return Step::Nothing;
        }
        if self.budget.is_given_up(&id) {
            return Step::Nothing;
        }
        if !self.budget.spend(now, &id, MAX_RECOVERIES, RECOVERY_WINDOW_SECS) {
            return Step::GiveUp { id };
        }
        Step::Recover { id }
    }
}

// ── Собственный хост обоев: решение ───────────────────────────────────

/// Сколько раз подряд поднимать хост с теми же обоями за
/// [`RECOVERY_WINDOW_SECS`]. Следующее падение (третье подряд) — уже кадр
/// вместо обоев: у хоста сбой бывает и не по его вине (сон, сброс
/// видеокарты), поэтому предел мягче, чем у рисовальщика waywallen, но
/// круга падений всё равно нет.
pub const HOST_MAX_RESTARTS: usize = 2;
/// Как часто хост пишет метку сердцебиения, миллисекунды.
pub const HEARTBEAT_INTERVAL_MS: u64 = 2000;
/// Метка старше этого при живом процессе — хост завис, секунды. Срок
/// намеренно большой: ложный перезапуск исправных обоев хуже, чем минута
/// застывшего кадра.
pub const HEARTBEAT_STALE_SECS: u64 = 60;
/// Сколько ждать после выхода из сна, прежде чем требовать свежую метку,
/// секунды: композитору и драйверу нужно время, чтобы вернуть экраны.
pub const RESUME_GRACE_SECS: u64 = 15;
/// Начало имени файла метки сердцебиения в `$XDG_RUNTIME_DIR`.
pub const HEARTBEAT_PREFIX: &str = "rexon-wp-heartbeat-";

/// Что случилось с хостом.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostTrouble {
    /// Процесс завершился сам.
    Crashed,
    /// Процесс жив, но не рисует (метка сердцебиения не обновляется).
    Hung,
    /// Видеокарту сбросили — показ надо пересоздать.
    GpuReset,
}

/// Что делать с хостом после прохода.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostStep {
    /// Ничего.
    Nothing,
    /// Поднять те же обои заново.
    Restart { subject: String, why: HostTrouble },
    /// Обои падают подряд — поставить их неподвижный кадр и сказать человеку.
    StillFrame { subject: String },
}

/// Наблюдение за хостом на один проход.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostTick {
    /// Что сейчас должно показываться своим хостом (формат и путь); `None`
    /// — своих живых обоев нет, сторожить нечего.
    pub subject: Option<String>,
    /// Наш процесс хоста завершился сам (у исполнителя есть его статус).
    pub exited: bool,
    /// Метки живых процессов хоста: время последней (секунды
    /// [`boottime_secs`]) либо `None` — метки нет (хост прежней сборки).
    pub beats: Vec<Option<u64>>,
    /// Хост заморожен надзором за нагрузкой — молчание метки законно.
    pub paused: bool,
    /// Хост — ребёнок расширения оболочки: его статуса у нас нет, и о
    /// смерти говорит только пропажа процесса с меткой.
    pub by_heartbeat_only: bool,
}

/// Память сторожа хоста между проходами.
#[derive(Debug, Default)]
pub struct HostWatch {
    budget: CrashBudget,
    /// Обои, за которыми следим сейчас.
    subject: Option<String>,
    /// У этих обоев уже была живая метка (до неё о смерти по пропаже
    /// процесса судить нельзя: расширение могло ещё не подняться).
    seen_beat: bool,
    /// Когда вышли из сна — проверка «рисует ли» назначена на потом.
    woke_at: Option<u64>,
    /// Был сброс видеокарты.
    gpu_reset: bool,
}

impl HostWatch {
    /// Система вышла из сна в момент `at` (секунды [`boottime_secs`]).
    pub fn woke(&mut self, at: u64) {
        self.woke_at = Some(at);
    }

    /// Ядро сообщило о сбросе видеокарты.
    pub fn gpu_reset(&mut self) {
        self.gpu_reset = true;
    }

    /// Один проход. `now` — секунды [`boottime_secs`].
    pub fn tick(&mut self, now: u64, t: &HostTick) -> HostStep {
        let Some(subject) = t.subject.clone() else {
            // Своих обоев нет: события, пришедшие без них, к будущим обоям
            // не относятся.
            self.subject = None;
            self.seen_beat = false;
            self.woke_at = None;
            self.gpu_reset = false;
            return HostStep::Nothing;
        };
        self.budget.forget_if_changed(&subject);
        if self.subject.as_deref() != Some(subject.as_str()) {
            self.subject = Some(subject.clone());
            self.seen_beat = false;
        }
        if self.budget.is_given_up(&subject) {
            self.woke_at = None;
            self.gpu_reset = false;
            return HostStep::Nothing;
        }
        if t.beats.iter().any(Option::is_some) {
            self.seen_beat = true;
        }
        let gone = t.exited || (t.by_heartbeat_only && self.seen_beat && t.beats.is_empty());
        let trouble = if gone {
            Some(HostTrouble::Crashed)
        } else if std::mem::take(&mut self.gpu_reset) {
            Some(HostTrouble::GpuReset)
        } else if t.paused {
            // Заморожен — молчит законно; проверку после сна не копим.
            self.woke_at = None;
            None
        } else {
            self.hung(now, &t.beats).then_some(HostTrouble::Hung)
        };
        let Some(why) = trouble else { return HostStep::Nothing };
        self.woke_at = None;
        self.gpu_reset = false;
        if !self.budget.spend(now, &subject, HOST_MAX_RESTARTS, RECOVERY_WINDOW_SECS) {
            return HostStep::StillFrame { subject };
        }
        // Новый процесс сначала обязан отметиться сам.
        self.seen_beat = false;
        HostStep::Restart { subject, why }
    }

    /// Завис ли хост по меткам. Процессы без метки (прежняя сборка) не
    /// судятся вовсе.
    fn hung(&mut self, now: u64, beats: &[Option<u64>]) -> bool {
        if let Some(woke) = self.woke_at {
            if now < woke + RESUME_GRACE_SECS {
                return false; // после сна ещё рано судить
            }
            self.woke_at = None;
            if beats.iter().flatten().any(|beat| *beat < woke) {
                return true; // со сна так и не отметился
            }
        }
        beats.iter().flatten().any(|beat| now.saturating_sub(*beat) > HEARTBEAT_STALE_SECS)
    }
}

/// Файл метки сердцебиения процесса хоста `pid` в каталоге сеанса.
pub fn heartbeat_path(runtime_dir: &str, pid: i64) -> String {
    format!("{}/{HEARTBEAT_PREFIX}{pid}", runtime_dir.trim_end_matches('/'))
}

/// Номер процесса по имени файла метки; не метка — `None`.
pub fn heartbeat_pid(file_name: &str) -> Option<i64> {
    file_name.strip_prefix(HEARTBEAT_PREFIX)?.parse().ok().filter(|pid| *pid > 0)
}

/// Время из файла метки; недописан или пуст — `None`.
pub fn parse_heartbeat(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// Секунды часов `CLOCK_BOOTTIME`: идут и во сне и не прыгают от смены
/// системного времени (часы машины с Windows рядом живут в местном времени
/// и переводятся). Одни часы на хост и сторож — метки сравнимы.
pub fn boottime_secs() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) } != 0 {
        return 0;
    }
    ts.tv_sec.max(0) as u64
}

/// Записать метку сердцебиения этого процесса. Каталога сеанса нет —
/// ничего: без него сторожу метку и не найти. Звать хосту из потока
/// интерфейса.
pub fn write_heartbeat_live() {
    let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") else { return };
    if dir.is_empty() {
        return;
    }
    let path = heartbeat_path(&dir, std::process::id() as i64);
    let _ = std::fs::write(path, boottime_secs().to_string());
}

/// Строка `gdbus monitor` о сне: `Some(true)` — уходим в сон,
/// `Some(false)` — вышли из него; другое — `None`. Вид строки:
/// `/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (false,)`.
pub fn parse_prepare_for_sleep(line: &str) -> Option<bool> {
    let (_, args) = line.split_once("org.freedesktop.login1.Manager.PrepareForSleep")?;
    let args = args.trim_start();
    if args.starts_with("(true") {
        Some(true)
    } else if args.starts_with("(false") {
        Some(false)
    } else {
        None
    }
}

/// Строка `udevadm monitor --kernel --property` о том, что устройство DRM
/// «заклинило» и было сброшено (событие ядра `WEDGED=<способ>`, ядра 6.14+).
pub fn parse_drm_wedged(line: &str) -> bool {
    line.trim_start().starts_with("WEDGED=")
}

/// Появилась ли новая запись `devcoredump` от видеокарты. `known` — имена,
/// виденные прошлым проходом (`None` — проход первый: то, что лежало до
/// запуска сторожа, событием не считается); `now` — (имя, это видеокарта).
pub fn new_gpu_dump(known: &mut Option<Vec<String>>, now: &[(String, bool)]) -> bool {
    let fresh = match known.as_ref() {
        None => false,
        Some(seen) => now.iter().any(|(name, gpu)| *gpu && !seen.contains(name)),
    };
    *known = Some(now.iter().map(|(name, _)| name.clone()).collect());
    fresh
}

/// Кандидаты в неподвижный кадр веб- и Flash-обоев: превью рядом с обоями.
/// `path` — путь обоев (каталог либо файл, адрес `file://` тоже);
/// `project_json` — текст `project.json` их каталога, если есть. Адрес
/// сайта — пусто: кадра сайта у нас нет.
pub fn still_frame_candidates(path: &str, project_json: Option<&str>) -> Vec<String> {
    let local = path.strip_prefix("file://").unwrap_or(path);
    if !local.starts_with('/') {
        return Vec::new();
    }
    let lower = local.to_lowercase();
    let dir = if [".html", ".htm", ".swf"].iter().any(|ext| lower.ends_with(ext)) {
        local.rsplit_once('/').map(|(d, _)| d).unwrap_or(local)
    } else {
        local
    };
    let dir = dir.trim_end_matches('/');
    let mut out = Vec::new();
    if let Some(name) = project_json
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| v["preview"].as_str().map(str::to_string))
        .filter(|n| !n.is_empty() && !n.contains('/'))
    {
        out.push(format!("{dir}/{name}"));
    }
    for ext in ["jpg", "png", "gif", "webp"] {
        let cand = format!("{dir}/preview.{ext}");
        if !out.contains(&cand) {
            out.push(cand);
        }
    }
    out
}

/// Каталог обоев, где искать `project.json` для [`still_frame_candidates`].
pub fn still_frame_dir(path: &str) -> Option<String> {
    still_frame_candidates(path, None)
        .first()
        .and_then(|c| c.rsplit_once('/').map(|(d, _)| d.to_string()))
}

// ── Подписи ────────────────────────────────────────────────────────────

/// Заголовок уведомлений сторожа.
pub fn notice_title() -> String {
    tr("Live wallpaper")
}

/// Обои подняты после падения рисовальщика.
pub fn recovered_text() -> String {
    tr("The program that draws your wallpaper stopped unexpectedly. Rexon has started the wallpaper again.")
}

/// Обои упали повторно — больше не поднимаем.
pub fn gave_up_text() -> String {
    tr("Your wallpaper keeps stopping unexpectedly, so Rexon no longer restarts it. \
        Choose another wallpaper in the Wallpaper Engine tab.")
}

/// Поднять обои не получилось (демон отказал).
pub fn recover_failed_text() -> String {
    tr("The program that draws your wallpaper stopped unexpectedly, and Rexon couldn't start it again. \
        Apply the wallpaper again in the Wallpaper Engine tab.")
}

/// Свои живые обои падали подряд — вместо них стоит их неподвижный кадр.
pub fn host_still_frame_text() -> String {
    tr("Your live wallpaper stopped unexpectedly several times in a row, so Rexon has put a still picture of it on the desktop instead. \
        To try the live wallpaper again, apply it once more.")
}

/// Свои живые обои падали подряд, кадра нет — вернулась прежняя картинка.
pub fn host_previous_picture_text() -> String {
    tr("Your live wallpaper stopped unexpectedly several times in a row, so Rexon has put back your previous desktop picture. \
        To try the live wallpaper again, apply it once more.")
}

// ── Разбор ответов системы ─────────────────────────────────────────────

/// Процесс рисовальщика waywallen по первому слову его командной строки:
/// файл `waywallen-<вид>-renderer`.
pub fn is_renderer_exe(argv0: &str) -> bool {
    let name = argv0.rsplit('/').next().unwrap_or(argv0);
    name.starts_with("waywallen-") && name.ends_with("-renderer")
}

/// Значение строкового свойства из ответа `busctl get-property`:
/// `s "16352"` → `16352`. Не строка либо пусто — пустая строка.
pub fn busctl_string(output: &str) -> String {
    let line = output.trim();
    let Some(rest) = line.strip_prefix("s ") else { return String::new() };
    rest.trim().trim_matches('"').to_string()
}

/// Говорит ли ответ журнала (`journalctl -o json`, записи об авариях по
/// номеру процесса), что упал именно рисовальщик: в записи есть путь
/// программы `COREDUMP_EXE`, и это рисовальщик.
pub fn journal_names_renderer_crash(journal_json: &str) -> bool {
    journal_json.lines().any(|line| {
        serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|v| v["COREDUMP_EXE"].as_str().map(is_renderer_exe))
            .unwrap_or(false)
    })
}

// ── Живая часть ────────────────────────────────────────────────────────

/// Номер сообщения `systemd-coredump` в журнале («процесс сбросил образ
/// памяти») — постоянный идентификатор из каталога сообщений systemd.
const COREDUMP_MESSAGE_ID: &str = "fc2e22bc6ee647b6b90729ab34a250b1";
const DAEMON_BUS: &str = "org.waywallen.waywallen.Daemon";
const DAEMON_PATH: &str = "/org/waywallen/waywallen/Daemon";
const DAEMON_IFACE: &str = "org.waywallen.waywallen.Daemon1";

fn run_capture(argv: &[&str]) -> Option<(i32, String)> {
    let spec = crate::utils::RunSpec {
        argv: argv.iter().map(|s| s.to_string()).collect(),
        output: crate::utils::RunOutput::Capture,
        timeout_secs: Some(15),
        ..crate::utils::RunSpec::default()
    };
    let report = crate::utils::run(&spec, std::env::var("PATH").ok().as_deref());
    let code = report.returncode?;
    Some((code, String::from_utf8_lossy(&report.stdout).into_owned()))
}

/// Номера живых процессов рисовальщиков этого пользователя.
fn live_renderer_pids() -> Vec<u32> {
    let Ok(dir) = std::fs::read_dir("/proc") else { return Vec::new() };
    let mut pids = Vec::new();
    for entry in dir.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else { continue };
        let argv0 = cmdline.split(|b| *b == 0).next().unwrap_or_default();
        if is_renderer_exe(&String::from_utf8_lossy(argv0)) {
            pids.push(pid);
        }
    }
    pids
}

/// Живут ли рисовальщики WE-обоев этого пользователя (плейлист Rexon по
/// этому узнаёт, что экран заняли обои Wallpaper Engine).
pub fn renderers_running() -> bool {
    !live_renderer_pids().is_empty()
}

/// Обои, выбранные у демона; пусто — демон не отвечает либо обоев нет.
fn live_current_id() -> String {
    run_capture(&["busctl", "--user", "get-property", DAEMON_BUS, DAEMON_PATH, DAEMON_IFACE, "CurrentWallpaperId"])
        .filter(|(code, _)| *code == 0)
        .map(|(_, out)| busctl_string(&out))
        .unwrap_or_default()
}

/// Есть ли в журнале запись об аварийном выходе процесса `pid`.
fn live_crashed(pid: u32) -> bool {
    let by_id = format!("MESSAGE_ID={COREDUMP_MESSAGE_ID}");
    let by_pid = format!("COREDUMP_PID={pid}");
    run_capture(&["journalctl", "--user", "--no-pager", "-o", "json", "--since", "-15min", &by_id, &by_pid])
        .map(|(_, out)| journal_names_renderer_crash(&out))
        .unwrap_or(false)
}

/// Попросить демона применить обои `id` заново.
fn live_apply(id: &str) -> bool {
    run_capture(&["busctl", "--user", "call", DAEMON_BUS, DAEMON_PATH, DAEMON_IFACE, "ApplyById", "s", id])
        .is_some_and(|(code, _)| code == 0)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Кнопка уведомления «Открыть Wallpaper Engine».
pub const ACTION_OPEN: &str = "open";

/// Кнопки уведомления, которое просит выбрать или применить обои заново.
pub fn open_actions() -> Vec<(String, String)> {
    vec![(ACTION_OPEN.to_string(), tr("Open Wallpaper Engine"))]
}

/// Уведомление `text` с кнопкой «Открыть Wallpaper Engine»: текст просит
/// человека выбрать или применить обои во вкладке — кнопка ведёт прямо туда.
/// Ответа ждём отдельным потоком: сторож тем временем сторожит дальше.
fn ask_open_wallpaper_engine(text: String) {
    std::thread::spawn(move || {
        let answer = crate::utils::notify_ask_live(&notice_title(), &text, &open_actions(), "", 24 * 3600);
        if answer.as_deref() == Some(ACTION_OPEN) {
            let section = format!("{}wallpaper_engine", crate::cli::SECTION_FLAG);
            if let Err(e) = crate::installer::open_rexon_live(&[&section]) {
                crate::log_warn!("wallpaper", "окно Wallpaper Engine не открылось: {e}");
            }
        }
    });
}

/// Поднять сторожа в этом процессе, если он ещё не поднят. Зовут значок в
/// лотке и каждое применение живых обоев (`wallpaper_exec::run_in_slot`):
/// без значка (лотка в сеансе нет) свой хост тоже должен быть под присмотром.
pub fn ensure_running() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("renderer-watch".into())
            .spawn(run_forever);
        if let Err(e) = spawned {
            crate::log_warn!("wallpaper", "сторож живых обоев не запущен: {e}");
        }
    });
}

/// События системы, на которые сторож отвечает сразу, не дожидаясь прохода.
enum SystemEvent {
    /// Вышли из сна.
    Woke,
    /// Ядро сообщило о сбросе видеокарты.
    GpuReset,
}

/// Читать строки долгоживущей команды и отдавать события сторожу. Команда
/// умирает вместе с процессом Rexon (`PR_SET_PDEATHSIG`). Нет команды или
/// она вышла — одна строка в журнал; сторож живёт и без неё.
fn spawn_line_monitor(
    argv: &'static [&'static str],
    parse: fn(&str) -> Option<SystemEvent>,
    tx: std::sync::mpsc::Sender<SystemEvent>,
) {
    use std::io::BufRead;
    use std::os::unix::process::CommandExt;
    let Some(bin) = crate::utils::which(argv[0], std::env::var("PATH").ok().as_deref()) else {
        crate::log_info!("wallpaper", "сторож живых обоев: нет {}, это событие не ловится", argv[0]);
        return;
    };
    let _ = std::thread::Builder::new()
        .name(format!("watch-{}", argv[0]))
        .spawn(move || {
            let mut cmd = std::process::Command::new(bin);
            cmd.args(&argv[1..])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            unsafe {
                cmd.pre_exec(|| {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                    Ok(())
                });
            }
            let Ok(mut child) = cmd.spawn() else {
                crate::log_warn!("wallpaper", "сторож живых обоев: {} не запустился", argv[0]);
                return;
            };
            if let Some(out) = child.stdout.take() {
                for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                    if let Some(event) = parse(&line) {
                        if tx.send(event).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            crate::log_info!("wallpaper", "сторож живых обоев: {} завершился, событие больше не ловится", argv[0]);
        });
}

/// Сигнал logind о выходе из сна — системная шина, без root.
const SLEEP_MONITOR: &[&str] = &[
    "gdbus", "monitor", "--system", "--dest", "org.freedesktop.login1", "--object-path", "/org/freedesktop/login1",
];
/// События ядра от устройств DRM (`WEDGED=` после сброса) — сокет netlink
/// читается без root.
const DRM_MONITOR: &[&str] = &["udevadm", "monitor", "--kernel", "--property", "--subsystem-match=drm"];
/// Записи образов памяти устройств: у видеокарты после сброса здесь
/// появляется `devcdN` (живёт ~5 минут, читается без root).
const DEVCOREDUMP_DIR: &str = "/sys/class/devcoredump";

/// Записи `devcoredump` сейчас: (имя, это видеокарта — у устройства есть
/// каталог `drm`).
fn live_devcoredumps() -> Vec<(String, bool)> {
    let Ok(dir) = std::fs::read_dir(DEVCOREDUMP_DIR) else { return Vec::new() };
    dir.flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            name.starts_with("devcd").then(|| {
                let gpu = e.path().join("failing_device/drm").is_dir();
                (name, gpu)
            })
        })
        .collect()
}

/// Совпадает ли исполняемый файл процесса с нашим хостом (сверка по пути,
/// никогда по имени).
fn pid_runs(pid: i64, binary: &str) -> bool {
    let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) else { return false };
    let want = std::fs::canonicalize(binary).unwrap_or_else(|_| binary.into());
    exe == want
}

/// Метка процесса из каталога сеанса.
fn live_beat(runtime: &str, pid: i64) -> Option<u64> {
    std::fs::read_to_string(heartbeat_path(runtime, pid)).ok().and_then(|t| parse_heartbeat(&t))
}

/// Процессы хоста, поднятые расширением оболочки: метка в каталоге сеанса,
/// процесс жив и это наш бинарь. Метки умерших процессов убираются.
fn live_extension_hosts(runtime: &str, binary: &str) -> Vec<i64> {
    let Ok(dir) = std::fs::read_dir(runtime) else { return Vec::new() };
    let mut pids = Vec::new();
    for entry in dir.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(heartbeat_pid) else { continue };
        if pid_runs(pid, binary) {
            pids.push(pid);
        } else if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    pids
}

/// Проход по своему хосту: наблюдение у исполнителя, решение у
/// [`HostWatch`], исполнение — снова у исполнителя.
fn live_host_pass(watch: &mut HostWatch) {
    use crate::wallpaper_exec as exec;
    let slot = exec::live_slot();
    // Исполнитель занят (идёт применение или замер надзора) — в следующий раз.
    let Some(snap) = exec::try_with_slot(&slot, |e| e.host_snapshot()) else { return };
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    // Обход меток идёт на каждом проходе: заодно убираются метки умерших
    // процессов (и наших детей тоже — они сами за собой не убирают).
    let marked = if runtime.is_empty() {
        Vec::new()
    } else {
        live_extension_hosts(&runtime, snap.host_binary.as_deref().unwrap_or(""))
    };
    let pids = if snap.via_extension { marked } else { snap.pids.clone() };
    let tick = HostTick {
        subject: snap.subject.clone(),
        exited: snap.exited,
        beats: pids.iter().map(|pid| if runtime.is_empty() { None } else { live_beat(&runtime, *pid) }).collect(),
        paused: snap.paused,
        by_heartbeat_only: snap.via_extension,
    };
    match watch.tick(boottime_secs(), &tick) {
        HostStep::Nothing => {}
        HostStep::Restart { subject, why } => {
            crate::log_warn!("wallpaper", "хост обоев {subject}: {why:?} — поднимаю обои заново");
            if snap.via_extension {
                // Зависший ребёнок оболочки: снять его самим — сверено, что
                // это наш бинарь, по пути. Своих детей снимает применение.
                if let Some(bin) = &snap.host_binary {
                    for pid in pids.iter().filter(|pid| pid_runs(**pid, bin)) {
                        unsafe { libc::kill(*pid as libc::pid_t, libc::SIGTERM) };
                    }
                }
            }
            let res = exec::run_in_slot(&slot, |e| e.restart_host());
            crate::log_info!("wallpaper", "хост обоев {subject}: подъём {}", if res.ok { "удался" } else { "не удался" });
            if !res.ok {
                // Подняться не вышло (файла нет, хост отказал) — дальше
                // так же, как после падений подряд.
                give_up_host(&slot, &subject);
            }
        }
        HostStep::StillFrame { subject } => give_up_host(&slot, &subject),
    }
}

/// Перестать поднимать свои обои: кадр вместо них и ОДНО уведомление.
fn give_up_host(slot: &crate::wallpaper_exec::LiveSlot, subject: &str) {
    let still = crate::wallpaper_exec::with_slot(slot, |e| e.show_still_frame());
    crate::log_warn!(
        "wallpaper",
        "хост обоев {subject} падает подряд — больше не поднимаю; {}",
        if still { "поставлен неподвижный кадр" } else { "возвращена прежняя картинка" }
    );
    let text = if still { host_still_frame_text() } else { host_previous_picture_text() };
    crate::utils::notify_live(&notice_title(), &text);
}

/// Сторожить, пока жив процесс. Звать в отдельном потоке (через
/// [`ensure_running`]).
///
/// Рисовальщикам waywallen нужны `journalctl` и `busctl`: без них эта
/// половина молчит (одна строка в журнал), а свой хост сторожится всё равно.
pub fn run_forever() {
    let path = std::env::var("PATH").ok();
    let renderers = ["journalctl", "busctl"].iter().all(|tool| {
        let found = crate::utils::which(tool, path.as_deref()).is_some();
        if !found {
            crate::log_info!("wallpaper", "сторож рисовальщика waywallen не работает: нет {tool}");
        }
        found
    });
    let (tx, rx) = std::sync::mpsc::channel();
    spawn_line_monitor(SLEEP_MONITOR, |l| (parse_prepare_for_sleep(l) == Some(false)).then_some(SystemEvent::Woke), tx.clone());
    spawn_line_monitor(DRM_MONITOR, |l| parse_drm_wedged(l).then_some(SystemEvent::GpuReset), tx);
    crate::log_info!("wallpaper", "сторож живых обоев запущен: проход раз в {POLL_SECS} с");
    let mut watch = Watch::default();
    let mut host = HostWatch::default();
    let mut dumps: Option<Vec<String>> = None;
    loop {
        match rx.recv_timeout(std::time::Duration::from_secs(POLL_SECS)) {
            Ok(SystemEvent::Woke) => {
                crate::log_info!("wallpaper", "система вышла из сна — проверю, рисуют ли обои");
                host.woke(boottime_secs());
                continue; // проверка — через RESUME_GRACE_SECS, обычным проходом
            }
            Ok(SystemEvent::GpuReset) => {
                crate::log_warn!("wallpaper", "ядро сообщило о сбросе видеокарты");
                host.gpu_reset();
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            // Оба наблюдателя ушли — дальше только обычные проходы.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                std::thread::sleep(std::time::Duration::from_secs(POLL_SECS));
            }
        }
        if new_gpu_dump(&mut dumps, &live_devcoredumps()) {
            crate::log_warn!("wallpaper", "видеокарта сброшена (новая запись devcoredump)");
            host.gpu_reset();
        }
        live_host_pass(&mut host);
        if !renderers {
            continue;
        }
        let alive = live_renderer_pids();
        // Демона спрашиваем, только когда есть о чём: рисовальщики живы
        // либо недавно пропали. В простое сторож не будит никого.
        if alive.is_empty() && watch.seen.is_empty() && watch.pending.is_empty() {
            continue;
        }
        let current = live_current_id();
        match watch.tick(now_secs(), &alive, &current, &live_crashed) {
            Step::Nothing => {}
            Step::Recover { id } => {
                crate::log_warn!("wallpaper", "рисовальщик обоев {id} аварийно завершился — применяю обои заново");
                let ok = live_apply(&id);
                crate::log_info!("wallpaper", "повторное применение обоев {id}: {}", if ok { "принято демоном" } else { "демон отказал" });
                if ok {
                    crate::utils::notify_live(&notice_title(), &recovered_text());
                } else {
                    ask_open_wallpaper_engine(recover_failed_text());
                }
            }
            Step::GiveUp { id } => {
                crate::log_warn!("wallpaper", "рисовальщик обоев {id} упал повторно — больше не перезапускаю");
                ask_open_wallpaper_engine(gave_up_text());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crashed_set(pids: &'static [u32]) -> impl Fn(u32) -> bool {
        move |pid| pids.contains(&pid)
    }

    #[test]
    fn crash_of_the_chosen_wallpaper_is_recovered_once() {
        let mut w = Watch::default();
        let none = crashed_set(&[]);
        assert_eq!(w.tick(0, &[100], "16352", &none), Step::Nothing);
        // Процесс пропал, записи об аварии ещё нет — ждём.
        assert_eq!(w.tick(10, &[], "16352", &none), Step::Nothing);
        // Запись появилась на следующем проходе — поднимаем.
        assert_eq!(w.tick(20, &[], "16352", &crashed_set(&[100])), Step::Recover { id: "16352".into() });
        // Демон поднял новый процесс; он упал снова в том же окне — отказ.
        assert_eq!(w.tick(30, &[200], "16352", &none), Step::Nothing);
        assert_eq!(w.tick(40, &[], "16352", &crashed_set(&[200])), Step::GiveUp { id: "16352".into() });
        // После отказа те же обои больше не трогаем, даже если их применили
        // руками и они упали ещё раз.
        assert_eq!(w.tick(50, &[300], "16352", &none), Step::Nothing);
        assert_eq!(w.tick(60, &[], "16352", &crashed_set(&[300])), Step::Nothing);
        // Другие обои — счёт заново.
        assert_eq!(w.tick(70, &[400], "777", &none), Step::Nothing);
        assert_eq!(w.tick(80, &[], "777", &crashed_set(&[400])), Step::Recover { id: "777".into() });
    }

    #[test]
    fn deliberate_stop_and_wallpaper_change_are_left_alone() {
        // Процесс пропал без записи об аварии (автопауза, выход демона).
        let mut w = Watch::default();
        let none = crashed_set(&[]);
        w.tick(0, &[100], "1", &none);
        for at in 1..=5 {
            assert_eq!(w.tick(at * 10, &[], "1", &none), Step::Nothing);
        }
        assert!(w.pending.is_empty(), "ожидание записи об аварии не кончилось");

        // Упал, но человек уже выбрал другие обои — не возвращаем старые.
        let mut w = Watch::default();
        w.tick(0, &[100], "1", &none);
        assert_eq!(w.tick(10, &[], "2", &crashed_set(&[100])), Step::Nothing);

        // Упал, но другой рисовальщик уже работает — демон справился сам.
        let mut w = Watch::default();
        w.tick(0, &[100], "1", &none);
        assert_eq!(w.tick(10, &[101], "1", &crashed_set(&[100])), Step::Nothing);

        // Демон молчит (обои неизвестны) — применять нечего.
        let mut w = Watch::default();
        w.tick(0, &[100], "", &none);
        assert_eq!(w.tick(10, &[], "", &crashed_set(&[100])), Step::Nothing);
    }

    #[test]
    fn old_recoveries_fall_out_of_the_window() {
        let mut w = Watch::default();
        let none = crashed_set(&[]);
        w.tick(0, &[1], "9", &none);
        assert_eq!(w.tick(10, &[], "9", &crashed_set(&[1])), Step::Recover { id: "9".into() });
        // Следующее падение — через час: это уже не «подряд».
        let later = 10 + RECOVERY_WINDOW_SECS + 3000;
        w.tick(later, &[2], "9", &none);
        assert_eq!(w.tick(later + 10, &[], "9", &crashed_set(&[2])), Step::Recover { id: "9".into() });
    }

    fn alive(subject: &str, beats: &[Option<u64>]) -> HostTick {
        HostTick { subject: Some(subject.into()), beats: beats.to_vec(), ..HostTick::default() }
    }

    fn exited(subject: &str) -> HostTick {
        HostTick { subject: Some(subject.into()), exited: true, ..HostTick::default() }
    }

    #[test]
    fn host_crashes_are_restarted_twice_then_a_still_frame() {
        let mut w = HostWatch::default();
        let s = "video:/v.mp4";
        assert_eq!(w.tick(100, &alive(s, &[Some(99)])), HostStep::Nothing);
        assert_eq!(w.tick(110, &exited(s)), HostStep::Restart { subject: s.into(), why: HostTrouble::Crashed });
        assert_eq!(w.tick(120, &alive(s, &[Some(119)])), HostStep::Nothing);
        assert_eq!(w.tick(130, &exited(s)), HostStep::Restart { subject: s.into(), why: HostTrouble::Crashed });
        // Третье падение подряд — кадр, а не круг падений.
        assert_eq!(w.tick(140, &exited(s)), HostStep::StillFrame { subject: s.into() });
        // От этих обоев отказались: дальше молчим, пока человек не сменит их.
        assert_eq!(w.tick(150, &exited(s)), HostStep::Nothing);
        // Другие обои — счёт заново.
        let other = "web:/page/index.html";
        assert_eq!(w.tick(160, &exited(other)), HostStep::Restart { subject: other.into(), why: HostTrouble::Crashed });
    }

    #[test]
    fn host_crashes_far_apart_are_not_in_a_row() {
        let mut w = HostWatch::default();
        let s = "swf:/a.swf";
        for k in 0..6u64 {
            let at = 100 + k * (RECOVERY_WINDOW_SECS + 1);
            assert_eq!(w.tick(at, &exited(s)), HostStep::Restart { subject: s.into(), why: HostTrouble::Crashed }, "падение {k}");
        }
    }

    #[test]
    fn nothing_of_ours_means_nothing_to_do() {
        let mut w = HostWatch::default();
        w.gpu_reset();
        w.woke(10);
        assert_eq!(w.tick(100, &HostTick::default()), HostStep::Nothing);
        // События без своих обоев к следующим обоям не относятся.
        assert_eq!(w.tick(110, &alive("video:/v.mp4", &[Some(109)])), HostStep::Nothing);
    }

    #[test]
    fn host_that_does_not_beat_after_sleep_is_hung() {
        let mut w = HostWatch::default();
        let s = "video:/v.mp4";
        assert_eq!(w.tick(100, &alive(s, &[Some(100)])), HostStep::Nothing);
        w.woke(1000);
        // Рано судить: экраны ещё возвращаются.
        assert_eq!(w.tick(1005, &alive(s, &[Some(100)])), HostStep::Nothing);
        // Срок вышел, метка всё ещё со времён до сна — завис.
        assert_eq!(
            w.tick(1000 + RESUME_GRACE_SECS, &alive(s, &[Some(100)])),
            HostStep::Restart { subject: s.into(), why: HostTrouble::Hung }
        );

        // Отметился после сна — жив, ничего не делаем.
        let mut w = HostWatch::default();
        w.tick(100, &alive(s, &[Some(100)]));
        w.woke(1000);
        assert_eq!(w.tick(1000 + RESUME_GRACE_SECS, &alive(s, &[Some(1003)])), HostStep::Nothing);
    }

    #[test]
    fn hang_is_not_judged_without_a_beat_or_while_paused() {
        let s = "video:/v.mp4";
        // Хост прежней сборки (метки нет) — не судим вовсе.
        let mut w = HostWatch::default();
        w.woke(10);
        assert_eq!(w.tick(10_000, &alive(s, &[None])), HostStep::Nothing);
        // Заморожен надзором за нагрузкой — молчание законно.
        let mut w = HostWatch::default();
        let mut t = alive(s, &[Some(1)]);
        t.paused = true;
        assert_eq!(w.tick(10_000, &t), HostStep::Nothing);
        // Без сна метка устаревает только после большого срока.
        let mut w = HostWatch::default();
        assert_eq!(w.tick(100 + HEARTBEAT_STALE_SECS, &alive(s, &[Some(100)])), HostStep::Nothing);
        assert_eq!(
            w.tick(101 + HEARTBEAT_STALE_SECS, &alive(s, &[Some(100)])),
            HostStep::Restart { subject: s.into(), why: HostTrouble::Hung }
        );
    }

    #[test]
    fn gpu_reset_recreates_a_living_host() {
        let mut w = HostWatch::default();
        let s = "web:https://example.com";
        w.tick(100, &alive(s, &[Some(100)]));
        w.gpu_reset();
        assert_eq!(w.tick(101, &alive(s, &[Some(101)])), HostStep::Restart { subject: s.into(), why: HostTrouble::GpuReset });
        // Сброс отработан один раз.
        assert_eq!(w.tick(102, &alive(s, &[Some(102)])), HostStep::Nothing);
    }

    #[test]
    fn extension_host_is_dead_only_after_it_was_seen_alive() {
        let s = "video:/v.mp4";
        let ext = |beats: &[Option<u64>]| HostTick {
            subject: Some(s.into()),
            beats: beats.to_vec(),
            by_heartbeat_only: true,
            ..HostTick::default()
        };
        let mut w = HostWatch::default();
        // Расширение ещё не подняло хост (или нужен перезаход) — это не падение.
        assert_eq!(w.tick(100, &ext(&[])), HostStep::Nothing);
        assert_eq!(w.tick(110, &ext(&[])), HostStep::Nothing);
        assert_eq!(w.tick(120, &ext(&[Some(120)])), HostStep::Nothing);
        // Был живой — пропал.
        assert_eq!(w.tick(130, &ext(&[])), HostStep::Restart { subject: s.into(), why: HostTrouble::Crashed });
        // Новый хост ещё не отметился — снова ждём, а не поднимаем по кругу.
        assert_eq!(w.tick(140, &ext(&[])), HostStep::Nothing);
    }

    #[test]
    fn crash_budget_is_shared_by_both_watchers() {
        let mut b = CrashBudget::default();
        assert!(b.spend(0, "a", 1, 600));
        assert!(!b.spend(10, "a", 1, 600));
        assert!(b.is_given_up("a"));
        b.forget_if_changed("a");
        assert!(b.is_given_up("a"));
        b.forget_if_changed("b");
        assert!(!b.is_given_up("a"));
    }

    #[test]
    fn host_signals_are_parsed() {
        let woke = "/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (false,)";
        let sleeping = "/org/freedesktop/login1: org.freedesktop.login1.Manager.PrepareForSleep (true,)";
        assert_eq!(parse_prepare_for_sleep(woke), Some(false));
        assert_eq!(parse_prepare_for_sleep(sleeping), Some(true));
        assert_eq!(parse_prepare_for_sleep("/org/freedesktop/login1: org.freedesktop.login1.Manager.SessionNew ('3',)"), None);
        assert!(parse_drm_wedged("WEDGED=none"));
        assert!(parse_drm_wedged("  WEDGED=rebind,bus-reset"));
        assert!(!parse_drm_wedged("ACTION=change"));
        assert!(!parse_drm_wedged("KERNEL[12.3] change /devices/x/drm/card1 (drm)"));

        assert_eq!(heartbeat_path("/run/user/1000/", 42), "/run/user/1000/rexon-wp-heartbeat-42");
        assert_eq!(heartbeat_pid("rexon-wp-heartbeat-42"), Some(42));
        assert_eq!(heartbeat_pid("rexon-wp-heartbeat-"), None);
        assert_eq!(heartbeat_pid("rexon-wp-frame.rgba"), None);
        assert_eq!(parse_heartbeat("123\n"), Some(123));
        assert_eq!(parse_heartbeat(""), None);

        let mut known = None;
        // Что лежало до запуска сторожа — не событие.
        assert!(!new_gpu_dump(&mut known, &[("devcd1".into(), true)]));
        assert!(!new_gpu_dump(&mut known, &[("devcd1".into(), true)]));
        // Новая запись не от видеокарты — не наш случай.
        assert!(!new_gpu_dump(&mut known, &[("devcd1".into(), true), ("devcd2".into(), false)]));
        assert!(new_gpu_dump(&mut known, &[("devcd3".into(), true)]));
    }

    #[test]
    fn still_frame_is_looked_for_next_to_the_wallpaper() {
        let project = r#"{"title":"x","preview":"preview.gif","type":"web"}"#;
        let c = still_frame_candidates("/lib/123/index.html", Some(project));
        assert_eq!(c[0], "/lib/123/preview.gif");
        assert!(c.contains(&"/lib/123/preview.jpg".to_string()));
        assert_eq!(still_frame_candidates("file:///lib/7/Page.HTML", None)[0], "/lib/7/preview.jpg");
        assert_eq!(still_frame_candidates("/flash/a.swf", None)[0], "/flash/preview.jpg");
        assert_eq!(still_frame_candidates("/lib/123/", None)[0], "/lib/123/preview.jpg");
        assert!(still_frame_candidates("https://example.com", None).is_empty());
        // Чужой путь в превью (выход из каталога) не берём.
        let evil = r#"{"preview":"../../etc/x.png"}"#;
        assert_eq!(still_frame_candidates("/lib/1/index.html", Some(evil))[0], "/lib/1/preview.jpg");
        assert_eq!(still_frame_dir("/lib/1/index.html").as_deref(), Some("/lib/1"));
    }

    #[test]
    fn system_answers_are_parsed() {
        assert!(is_renderer_exe("/home/u/.local/share/waywallen/plugins/x/bin/waywallen-wescene-renderer"));
        assert!(is_renderer_exe("waywallen-weweb-renderer"));
        assert!(!is_renderer_exe("/tmp/.mount_x/usr/bin/waywallen"));
        assert!(!is_renderer_exe("rexon-wallpaper-host"));
        assert_eq!(busctl_string("s \"16352\"\n"), "16352");
        assert_eq!(busctl_string("s \"\"\n"), "");
        assert_eq!(busctl_string("u 5\n"), "");
        let entry = r#"{"COREDUMP_PID":"729862","COREDUMP_EXE":"/home/u/p/bin/waywallen-wescene-renderer","COREDUMP_SIGNAL_NAME":"SIGABRT"}"#;
        assert!(journal_names_renderer_crash(entry));
        assert!(!journal_names_renderer_crash(r#"{"COREDUMP_PID":"5","COREDUMP_EXE":"/usr/bin/firefox"}"#));
        assert!(!journal_names_renderer_crash(""));
    }
}
