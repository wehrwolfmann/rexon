// Режим «Flash» хоста обоев: SWF-ролик рисует Ruffle через wgpu (крейт
// `rexon-swf`), а не QML — поэтому в `run_qml` режим не заходит и возвращает
// код сам, из той же таблицы ядра (`wallpaper_engine::host_exit`).
//
// Чьи решения где:
//   * как класть окно (слой / X11 / поток кадров) — общий `HostEnv::read`,
//     то же решение, что у видео и веба; платформу Qt под него выставляет
//     общий `main.rs` между [`plan`] и [`run`] — как у QML-режимов;
//   * контракт файла кадров (размер, частота, путь счётчика поколения, права)
//     — функции ядра `rexon-core::wallpaper_mmap` и `wallpaper::mmap_size`;
//   * слой — `layer_shell::apply_spec` с `LayerSpec::WALLPAPER`, X11 —
//     `wallpaper_host::apply_x11_desktop_layer`;
//   * отрисовка, темп, пауза, звук — `rexon-swf`; здесь только цикл событий Qt.
//
// Пауза надзора за нагрузкой — SIGSTOP/SIGCONT процесса, как у видео: после
// продолжения ролик не проматывается (шаг времени ограничен в `rexon-swf`).
//
// ⚠️ Звук (16.09.2026). До этого Flash шёл немым: громкость ехала ключом
// запуска, слежения за настройками у режима не было, а шкала Ruffle
// нелинейна — половина ползунка звучала в десять раз тише, чем та же
// половина у видео. Теперь всё как у видео и ИЗ ТЕХ ЖЕ мест:
//   * ЧТО значат настройки и что делать при такой обстановке — ядро
//     `rexon-core::wallpaper_audio` (ключи `wallpaper_audio_*` в
//     `settings.json`, один ползунок на оба вида обоев);
//   * КТО следит за файлом настроек и опрашивает окна со звуковой системой
//     — общий цикл `rexon-gui::wallpaper_audio::watch`, тот же, что питает
//     QML видео-обоев;
//   * перевод громкости в шкалу Ruffle — `rexon_swf::ruffle_volume`, и
//     больше нигде.
// Здесь только доставка решения в показ и честные жалобы, когда звука не
// будет.

use std::path::PathBuf;

use rexon_core::wallpaper_engine::{host_exit, swf_missing_message};
use rexon_gui::wallpaper_host::{parse_args, Cli, USAGE};

use crate::HostEnv;

/// Метка строк журнала режима.
const TAG: &str = "REXON-SWF";

/// Всё, что нужно показу Flash, посчитанное ДО первого вызова Qt.
pub struct SwfPlan {
    pub cli: Cli,
    pub path: PathBuf,
    pub env: HostEnv,
    /// Способ показа из общего решения — по нему `main.rs` выставляет платформу Qt.
    pub placement: &'static str,
}

/// Разбор ключей и честные отказы до всякого Qt; `Err` — готовый код возврата.
pub fn plan(argv: &[String]) -> Result<SwfPlan, i32> {
    // Отказ драйвера Vulkan или сама библиотека не должны оставлять
    // покупателю окно «программа аварийно завершилась»: ядро не зовёт
    // обработчик дампов для non-dumpable процесса.
    // SAFETY: prctl с PR_SET_DUMPABLE не трогает память процесса.
    unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) };

    let cli = match parse_args(argv) {
        Ok(cli) => cli,
        Err(message) if message == USAGE => {
            eprintln!("{message}");
            return Err(host_exit::OK);
        }
        Err(message) => {
            // Разбор общий с видео, и его «не указан файл видео» здесь неправда.
            let message = if message.starts_with("не указан файл") {
                "не указан файл Flash (.swf)".to_string()
            } else {
                message
            };
            rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR {message}");
            return Err(host_exit::NOTHING_TO_SHOW);
        }
    };
    let path = PathBuf::from(&cli.path);
    if !path.is_file() {
        rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR {}", swf_missing_message(&cli.path));
        return Err(host_exit::NOTHING_TO_SHOW);
    }
    // ⚠️ ДО первой звуковой операции: свойства потока звуковой клиент
    // читает, когда создаёт свой контекст, то есть при открытии выхода в
    // `rexon-swf`. Тот же список, что у видео (`video.rs`), — иначе обои
    // попадают в звуковую систему безымянным клиентом, и надзиратель звука
    // принимает наш собственный поток за «чужую программу» и глушит нас же.
    for (name, value) in rexon_core::wallpaper_audio::stream_env() {
        if std::env::var_os(name).is_none() {
            // SAFETY: зовётся из main до создания приложения Qt и до любых
            // потоков — читателей окружения ещё нет.
            unsafe { std::env::set_var(name, &value) };
        }
    }
    let env = HostEnv::read(cli.windowed);
    let placement = env.placement;
    Ok(SwfPlan { cli, path, env, placement })
}

/// Показ до сигнала (или до конца самопроверки). Платформа Qt уже выставлена.
pub fn run(plan: SwfPlan) -> i32 {
    #[cfg(feature = "swf")]
    {
        imp::run(plan)
    }
    #[cfg(not(feature = "swf"))]
    {
        let _ = plan;
        rexon_core::log_error!(
            "wallpaper-host",
            "{TAG}-ERROR этот хост обоев собран без Flash (фича swf крейта rexon-gui) — \
             пересоберите Rexon со стандартным набором возможностей"
        );
        host_exit::NOTHING_TO_SHOW
    }
}

#[cfg(feature = "swf")]
mod imp {
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use cxx_qt_lib::{QGuiApplication, QString};
    use raw_window_handle as rwh;
    use rexon_core::wallpaper_audio::Decision;
    use rexon_core::wallpaper_engine::host_exit;
    use rexon_core::wallpaper_mmap;
    use rexon_gui::layer_shell::{apply_spec, pin_to_window_screen, LayerSpec};
    use rexon_gui::wallpaper_host::{
        apply_x11_desktop_layer, keep_alive_without_windows, prepare_x11_desktop_layer, swf_window,
        verdict, PLACEMENT_LAYER, PLACEMENT_OFFSCREEN, PLACEMENT_X11,
    };
    use rexon_swf::{Error, GpuContext, MmapSpec, OutputTarget, Settings, Wallpaper};

    use super::{SwfPlan, TAG};

    /// Имя слоя обоев для порядка внутри фона у композитора.
    const LAYER_SCOPE: &str = "rexon-swf-wallpaper";

    /// Сколько ждать, пока композитор покажет окно.
    const EXPOSE_TIMEOUT: Duration = Duration::from_secs(10);

    /// Сколько играть, прежде чем сказать «в ролике нет звука».
    ///
    /// Звук в SWF начинается не обязательно с первого кадра, и поспешная
    /// жалоба была бы враньём наоборот. Пять секунд — заметно дольше
    /// вступления обычной Flash-заставки и всё ещё «сразу» по человеческим
    /// меркам.
    const SILENT_VERDICT_AFTER: Duration = Duration::from_secs(5);

    /// Живое состояние звука обоев в Flash-режиме.
    ///
    /// Решение целиком считает ядро, доставляет общий цикл наблюдения; здесь
    /// только перекладывание его в показ и честные жалобы.
    struct AudioLive {
        /// Последнее пришедшее решение. Цикл показа забирает его и обнуляет:
        /// доставка идёт из чужого потока, а трогать показ можно только из
        /// своего.
        inbox: Arc<Mutex<Option<(Decision, String)>>>,
        /// Пока живёт — наблюдение работает.
        _watch: rexon_gui::wallpaper_audio::AudioWatch,
        /// Громкость закреплена ключом `--volume` и за настройкой не следует
        /// (тот же уговор, что у видео: ручной прогон звучит как просили).
        fixed_volume: Option<f64>,
        verbose: bool,
        /// Ролик — для жалобы «звук просили, а его нет».
        movie: String,
        started: Instant,
        /// Про молчащий ролик говорим один раз за прогон.
        warned_silent: bool,
    }

    impl AudioLive {
        /// Поднять наблюдение. `screen` — самый маленький экран сеанса: с ним
        /// ядро сравнивает чужие окна (разбор — `wallpaper_audio::watch`).
        fn start(
            screen: (i64, i64),
            fixed_volume: Option<f64>,
            verbose: bool,
            movie: String,
        ) -> Option<Self> {
            let inbox: Arc<Mutex<Option<(Decision, String)>>> = Arc::new(Mutex::new(None));
            let sink = inbox.clone();
            match rexon_gui::wallpaper_audio::watch(screen.0, screen.1, move |d, why| {
                *sink.lock().unwrap() = Some((d, why));
            }) {
                Ok(watch) => Some(Self {
                    inbox,
                    _watch: watch,
                    fixed_volume,
                    verbose,
                    movie,
                    started: Instant::now(),
                    warned_silent: false,
                }),
                Err(e) => {
                    // Обои обязаны играть, а не молчать: без наблюдения звук
                    // просто останется таким, каким его застали настройки.
                    rexon_core::log_warn!(
                        "wallpaper-host",
                        "{TAG}-WARN наблюдение за звуком не поднялось ({e}) — громкость \
                         останется той, что записана в настройках"
                    );
                    None
                }
            }
        }

        /// Довести решение до показа. Зовётся из цикла показа каждую итерацию.
        fn apply(&mut self, wp: &mut Wallpaper) {
            let taken = self.inbox.lock().unwrap().take();
            if let Some((decision, why)) = taken {
                if self.verbose {
                    rexon_core::log_warn!("wallpaper-host", "{TAG}-AUDIO {why}");
                }
                // Пауза — раньше звука: `set_paused` сам трогает звуковой
                // выход, и порядок «сначала пауза, потом звук» оставляет
                // последнее слово за решением ядра.
                wp.set_paused(decision.paused);
                wp.set_audio(
                    decision.audio_on,
                    self.fixed_volume.unwrap_or(decision.volume01),
                );
            }
            self.check_silent(wp);
        }

        /// «Ролик молчит» — отдельная жалоба, и она не придирка.
        ///
        /// Сам по себе молчащий SWF с виду неотличим от звучащего: ролик
        /// идёт, устройство открыто, громкость какая просили. Без этой
        /// строки человек, поставивший Flash-обои со звуком, видел бы
        /// «громкость работает» и не понимал, почему тихо.
        fn check_silent(&mut self, wp: &Wallpaper) {
            if self.warned_silent || self.started.elapsed() < SILENT_VERDICT_AFTER {
                return;
            }
            let state = wp.audio_state();
            // Спрашивать есть смысл, только когда звук ДОЛЖЕН был звучать:
            // выход открыт, громкость не ноль, устройство за сэмплами
            // приходило. Иначе тишина объясняется просьбой, а не роликом.
            if !state.available || !state.on || state.volume01 <= 0.0 || state.callbacks == 0 {
                return;
            }
            if state.peak > 0.0 {
                self.warned_silent = true;
                return;
            }
            self.warned_silent = true;
            rexon_core::log_warn!(
                "wallpaper-host",
                "{TAG}-NOAUDIO звук включён, но за {} с ролик не издал ни звука — \
                 похоже, звуковой дорожки в нём нет: {}",
                SILENT_VERDICT_AFTER.as_secs(),
                self.movie
            );
        }
    }

    /// Код возврата по ошибке библиотеки — из таблицы ядра.
    fn exit_code(e: &Error) -> i32 {
        match e {
            Error::MovieLoad(_) => host_exit::SWF_MOVIE_FAILED,
            Error::NoAdapter(_) | Error::Surface(_) | Error::Render(_) | Error::GpuLost(_) => {
                host_exit::SWF_NO_GPU
            }
            Error::Output(_) => host_exit::FRAMES_FAILED,
            Error::NoOutputs | Error::InvalidSize { .. } | Error::OutputIndex(_) => {
                host_exit::NOTHING_TO_SHOW
            }
        }
    }

    pub fn run(plan: SwfPlan) -> i32 {
        let SwfPlan { cli, path, env, .. } = plan;
        let cli = &cli;
        let path: &Path = &path;
        let mut settings = Settings::new(path);
        // Звук у Flash-обоев есть по умолчанию, как у видео. Ключ `--no-audio`
        // остаётся правом вето: он приходит от того, кто запустил обои, а
        // настройка — от человека, и ни одно не отменяет другое молча.
        // Дальше звук живёт сам: громкость и «играть/молчать» приходят из
        // настроек через наблюдение (`AudioLive`).
        let audio_allowed = cli.audio != Some(false);
        let audio_settings = rexon_gui::wallpaper_audio::settings_now();
        settings.audio_volume = audio_allowed
            .then(|| cli.volume.unwrap_or_else(|| audio_settings.volume01()) as f32);
        let selftest = (cli.selftest_ms > 0).then(|| Duration::from_millis(cli.selftest_ms as u64));
        if cli.verbose {
            rexon_core::log_info!(
                "wallpaper-host",
                "{TAG}-START файл={} способ={} окно={} звук={:?} закреплённая_громкость={:?}",
                path.display(),
                env.placement,
                cli.windowed,
                settings.audio_volume,
                cli.volume
            );
        }
        // Наблюдение поднимается только там, где звук вообще возможен: без
        // него незачем каждые полсекунды опрашивать окна и звуковую систему.
        let live = |screen: (i64, i64)| {
            audio_allowed.then(|| {
                AudioLive::start(screen, cli.volume, cli.verbose, path.display().to_string())
            })
            .flatten()
        };

        if env.placement == PLACEMENT_OFFSCREEN {
            // GNOME/Cinnamon-Wayland: окна нет вовсе, Qt не нужен.
            let Some(frame) = env.mmap_output.clone() else {
                rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR офскрин-режим без REXON_WP_OUTPUT: кадры некуда писать");
                return verdict(host_exit::NOTHING_TO_SHOW);
            };
            settings.max_fps = 1000.0 / wallpaper_mmap::headless_tick_ms(env.fps) as f64;
            let spec = MmapSpec {
                generation_path: Some(wallpaper_mmap::frame_generation_path(&frame).into()),
                frame_path: frame.into(),
                width: env.mmap_w.clamp(1, i64::from(u32::MAX)) as u32,
                height: env.mmap_h.clamp(1, i64::from(u32::MAX)) as u32,
                file_mode: wallpaper_mmap::FRAME_FILE_MODE,
            };
            // SAFETY: у mmap-вывода нет внешних хэндлов.
            let wallpaper = unsafe { Wallpaper::new(GpuContext::new(), &settings, vec![OutputTarget::Mmap(spec)]) };
            // Экранов офскрин-режим не знает — с чужими окнами ядро сравнит
            // размер кадра, ровно как это делает видео (`audioProbeSize`).
            let audio = live((env.mmap_w, env.mmap_h));
            return verdict(drive(wallpaper, selftest, audio, |_| true));
        }

        settings.max_fps = 60.0;
        // VkInstance создаётся до окна: так поверхность и адаптер берутся из одного инстанса.
        let gpu = GpuContext::new();
        let mut app = QGuiApplication::new();
        if app.as_mut().is_none() {
            rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR приложение Qt не создалось");
            return verdict(host_exit::NOTHING_TO_SHOW);
        }
        // Уход последнего монитора не должен убивать процесс обоев.
        keep_alive_without_windows();

        // Окон столько, сколько мониторов: обои полагаются КАЖДОМУ экрану,
        // а не только основному. Отладочное окно — одно, у него экрана нет.
        // Звук при этом не задваивается: `rexon-swf` заводит звуковой выход
        // только первому выводу, остальные немые.
        let screens = swf_window::screen_count();
        let windows_wanted = if cli.windowed { 1 } else { screens.max(0) };
        if windows_wanted <= 0 {
            rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR мониторов в сеансе нет — обои класть некуда");
            return verdict(host_exit::NOTHING_TO_SHOW);
        }
        if cli.verbose {
            rexon_core::log_info!("wallpaper-host", "{TAG}-SCREENS экранов={screens} окон={windows_wanted}");
        }

        let (w, h) = if cli.windowed { (1280, 720) } else { (0, 0) };
        let mut windows = Vec::with_capacity(windows_wanted as usize);
        for i in 0..windows_wanted {
            // Отладочному окну экран не назначаем: оно одно и живёт там, куда
            // его положит композитор.
            let screen_index = if cli.windowed { -1 } else { i };
            // SAFETY: главный поток, приложение Qt живо.
            let window = unsafe { swf_window::create(w, h, screen_index) };
            if window.is_null() {
                rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR окно не создалось (экран {i})");
                return verdict(host_exit::NOTHING_TO_SHOW);
            }
            if env.placement == PLACEMENT_LAYER && !cli.windowed {
                // SAFETY: окно живо и ещё не показано — слой ставится до показа.
                let ok = unsafe { apply_spec(window.cast(), &LayerSpec::WALLPAPER, &QString::from(LAYER_SCOPE)) };
                if !ok {
                    rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN слой обоев не поставился: композитор без wlr-layer-shell?");
                }
                // ⚠️ И только теперь — выход поверхности. Без этого все окна
                // легли бы туда, куда решит композитор, то есть скорее всего
                // на один и тот же монитор.
                // SAFETY: окно живо, слой ему уже поставлен, показа ещё не было.
                else if !unsafe { pin_to_window_screen(window.cast()) } {
                    rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN поверхность не прицелилась на экран {i}");
                }
            }
            if env.placement == PLACEMENT_X11 && !cli.windowed {
                // Тип «рабочий стол» и состояние — ДО показа (EWMH): тип
                // оконный менеджер читает при показе.
                // SAFETY: окно из swf_window::create, ещё не показано.
                let id = unsafe { swf_window::x11_window_id(window) };
                if id != 0 && !prepare_x11_desktop_layer(id) {
                    rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN окно X11 не подготовлено к роли рабочего стола");
                }
            }
            // SAFETY: окно из swf_window::create.
            unsafe { swf_window::show(window) };
            windows.push(window);
        }

        let shown = Instant::now();
        // SAFETY: окна из swf_window::create.
        while windows.iter().any(|w| !unsafe { swf_window::exposed(*w) })
            && shown.elapsed() < EXPOSE_TIMEOUT
        {
            swf_window::process_events();
            std::thread::sleep(Duration::from_millis(5));
        }
        if env.placement == PLACEMENT_X11 && !cli.windowed {
            for window in &windows {
                // SAFETY: окно показано.
                let id = unsafe { swf_window::x11_window_id(*window) };
                if id != 0 && !apply_x11_desktop_layer(id) {
                    rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN окно X11 не стало рабочим столом (EWMH)");
                }
            }
        }

        let mut targets = Vec::with_capacity(windows.len());
        let mut sizes = Vec::with_capacity(windows.len());
        for window in &windows {
            let Some((display, window_handle)) = native_handles(*window) else {
                return verdict(host_exit::SWF_NO_GPU);
            };
            // SAFETY: окно показано.
            let (width, height) = unsafe { swf_window::size(*window) };
            let size = (width.max(1) as u32, height.max(1) as u32);
            sizes.push(size);
            targets.push(OutputTarget::Window {
                display,
                window: window_handle,
                width: size.0,
                height: size.1,
            });
        }

        // SAFETY: окна и соединение живут до выхода из процесса.
        let wallpaper = unsafe { Wallpaper::new(gpu, &settings, targets) };
        // ⚠️ Экраны, в которые видеокарта рисовать отказалась, уносим вместе с
        // их окнами: пустое окно обоев — это чёрный прямоугольник на весь
        // монитор, и он хуже, чем честно оставленный прежний фон. Обои
        // остаются на тех мониторах, где рисовать МОЖНО: «всё или ничего»
        // для человека с двумя мониторами — тот же пустой рабочий стол.
        if let Ok(wp) = &wallpaper {
            for index in wp.refused_targets().iter().rev() {
                rexon_core::log_warn!(
                    "wallpaper-host",
                    "{TAG}-WARN на экране {index} обои показать нечем: видеокарта не берётся \
                     рисовать в это окно. Обои остаются на остальных экранах."
                );
                // SAFETY: окно наше, поверхность его уже отпущена библиотекой.
                unsafe { swf_window::close(windows.remove(*index)) };
                sizes.remove(*index);
            }
        }
        // Самый маленький экран — тот же выбор, что у видео: по большому
        // кино, развёрнутое на маленьком мониторе, не считалось бы
        // полноэкранным, и обои играли бы поверх него.
        let screen = sizes.iter().fold((i64::MAX, i64::MAX), |acc, (w, h)| {
            (acc.0.min(i64::from(*w)), acc.1.min(i64::from(*h)))
        });
        let audio = live(screen);
        // Перезапускаться ли при смене набора мониторов. На прогоне
        // самопроверки — нет: она меряет то, что застала.
        let may_restart = selftest.is_none() && !cli.windowed;
        verdict(drive(wallpaper, selftest, audio, move |wp| {
            swf_window::process_events();
            // ⚠️ ДО отрисовки, сразу после разбора событий. Когда монитор
            // отключают, композитор снимает поверхность слоя, и первый же
            // кадр в снятую поверхность — протокольная ошибка: клиента
            // отключают, обои исчезают со ВСЕХ мониторов разом (замер
            // 16.09: код возврата 255, «error in client communication»).
            // Здесь набор окон задан при создании выводов и на ходу не
            // меняется (это устройство крейта `rexon-swf`), поэтому
            // единственный честный ответ на смену конфигурации — начать
            // заново. Перезапуск на месте, `execv`: номер процесса
            // сохраняется, и для исполнителя обоев ничего не меняется.
            if may_restart && swf_window::screen_count() != windows_wanted {
                restart_in_place();
            }
            for (index, window) in windows.iter().enumerate() {
                // SAFETY: окно живо.
                let (w, h) = unsafe { swf_window::size(*window) };
                let size = (w.max(1) as u32, h.max(1) as u32);
                if size != sizes[index] {
                    sizes[index] = size;
                    if let Err(e) = wp.resize(index, size.0, size.1) {
                        rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN новый размер окна не принят: {e}");
                    }
                }
            }
            true
        }))
    }

    /// Начать показ заново тем же процессом: мониторов стало другое число,
    /// а набор выводов Flash-режима задаётся при создании.
    ///
    /// Возвращается только если запуск не удался — тогда показ продолжается
    /// на прежних окнах: испорченные обои лучше, чем никаких.
    fn restart_in_place() {
        use std::os::unix::process::CommandExt;

        let argv: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
        rexon_core::log_info!("wallpaper-host", "{TAG} набор мониторов сменился — начинаю показ заново");
        let error = std::process::Command::new("/proc/self/exe").args(&argv).exec();
        rexon_core::log_warn!("wallpaper-host", "{TAG}-WARN перезапуск не удался ({error}) — обои остаются на прежних экранах");
    }

    /// Хэндлы окна для wgpu: поверхность Wayland либо окно X11. `None` —
    /// у окна нет ни того, ни другого (жаловаться уже некому).
    fn native_handles(
        window: *mut rexon_gui::wallpaper_host::qobject::QObject,
    ) -> Option<(rwh::RawDisplayHandle, rwh::RawWindowHandle)> {
        let display = swf_window::wayland_display();
        if display != 0 {
            // SAFETY: окно показано — поверхность у платформы есть.
            let surface = unsafe { swf_window::wayland_surface(window) };
            let (Some(d), Some(s)) = (
                std::ptr::NonNull::new(display as *mut std::ffi::c_void),
                std::ptr::NonNull::new(surface as *mut std::ffi::c_void),
            ) else {
                rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR у окна нет поверхности Wayland");
                return None;
            };
            return Some((
                rwh::RawDisplayHandle::Wayland(rwh::WaylandDisplayHandle::new(d)),
                rwh::RawWindowHandle::Wayland(rwh::WaylandWindowHandle::new(s)),
            ));
        }
        // SAFETY: окно показано.
        let id = unsafe { swf_window::x11_window_id(window) };
        let connection = std::ptr::NonNull::new(swf_window::xcb_connection() as *mut std::ffi::c_void);
        let Some(id) = std::num::NonZeroU32::new(id as u32).filter(|_| connection.is_some()) else {
            rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR у окна нет ни поверхности Wayland, ни окна X11");
            return None;
        };
        Some((
            rwh::RawDisplayHandle::Xcb(rwh::XcbDisplayHandle::new(connection, 0)),
            rwh::RawWindowHandle::Xcb(rwh::XcbWindowHandle::new(id)),
        ))
    }

    /// Цикл показа до сигнала (или до конца самопроверки) и приговор.
    fn drive(
        wallpaper: Result<Wallpaper, Error>,
        selftest: Option<Duration>,
        mut audio: Option<AudioLive>,
        mut between: impl FnMut(&mut Wallpaper) -> bool,
    ) -> i32 {
        let mut wallpaper = match wallpaper {
            Ok(w) => w,
            Err(e) => {
                rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR {e}");
                return exit_code(&e);
            }
        };
        for w in wallpaper.warnings() {
            rexon_core::log_info!("wallpaper-host", "{TAG} {w}");
        }
        // Звук просили, а звуковая система его дать не может. Ровно та
        // неразличимость, на которой самопроверка обоев уже однажды зеленела:
        // «громкость выставлена» и «звук идёт» — разные вещи.
        if audio.is_some() && !wallpaper.audio_state().available {
            rexon_core::log_warn!(
                "wallpaper-host",
                "{TAG}-NOAUDIO звук обоев включён, но звуковая система его не дала — \
                 Flash-обои идут немыми, картинка не пострадала"
            );
        }
        let started = Instant::now();
        let between = move |wp: &mut Wallpaper| {
            if let Some(live) = audio.as_mut() {
                live.apply(wp);
            }
            between(wp)
        };
        match wallpaper.run(selftest, between) {
            Ok(stats) => {
                if selftest.is_none() {
                    return host_exit::OK;
                }
                let secs = started.elapsed().as_secs_f64().max(1e-3);
                let avg = |v: &[f32]| if v.is_empty() { 0.0 } else { v.iter().map(|x| *x as f64).sum::<f64>() / v.len() as f64 };
                // ⚠️ Звук в отчёте — отдельными величинами, а не одним
                // «звук=да». «Выход есть», «поток открыт», «громкость» и
                // «уровень дошёл до устройства» — четыре разных факта, и
                // зелёный рапорт при тишине получался ровно из их смешения.
                let a = wallpaper.audio_state();
                eprintln!(
                    "{TAG}-SELFTEST кадров={} к/с={:.2} время_ролика={:.2}с разрывов={} рендер_мс={:.3} запись_кадра_мс={:.3} \
                     звук_выход={} звук_играет={} громкость={:.2} пик={:.4} обращений_устройства={}",
                    stats.frames,
                    stats.frames as f64 / secs,
                    stats.movie_time.as_secs_f64(),
                    stats.gaps,
                    avg(&stats.render_ms),
                    avg(&stats.mmap_ms),
                    a.available,
                    a.on,
                    a.volume01,
                    a.peak,
                    a.callbacks
                );
                if stats.frames == 0 {
                    host_exit::FRAMES_FAILED
                } else {
                    host_exit::OK
                }
            }
            Err(e) => {
                rexon_core::log_error!("wallpaper-host", "{TAG}-ERROR {e}");
                exit_code(&e)
            }
        }
    }
}
