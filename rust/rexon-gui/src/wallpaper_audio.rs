// Надзиратель звука обоев: живое применение настроек и приглушение по
// обстановке.
//
// Что здесь появилось и почему. До этого звук обоев задавался ОДИН раз,
// ключами командной строки при запуске хоста, и менялся только вместе с
// перезапуском всего процесса: человек двигал настройку — на экране не
// происходило ничего, пока обои не переставишь заново. Приглушения по
// обстановке не было вовсе, поэтому обои играли поверх игры и поверх кино.
//
// Разделение обязанностей то же, что во всём хосте: ЧТО значат настройки и
// ЧТО делать при такой обстановке — решает ядро
// (`rexon-core::wallpaper_audio`), здесь только ввод-вывод: чтение файла
// настроек, опрос оконного менеджера и звуковой системы, доставка решения
// в QML. Своих правил и своих чисел в этом файле нет намеренно — иначе
// «громкость обоев» опять начала бы значить в двух местах разное.
//
// Живое применение сделано БЕЗ отдельного канала: хост следит за тем же
// `~/.config/rexon/settings.json`, который пишет окно настроек. Ни сокета,
// ни файла состояния, ни второго формата — «сохранил» и «услышал» не могут
// разойтись, потому что источник один (правило #12).
//
// ⚠️ Наблюдение поднимают ДВА режима хоста, и цикл у них ОДИН (16.09.2026).
// Видео живёт в QML и получает решение свойствами cxx-qt-объекта ниже;
// Flash (`--mode swf`) рисует Ruffle через wgpu, QML у него нет вовсе, и
// решение он забирает обычным замыканием — [`watch`]. Общим сделан именно
// цикл ([`watch_loop`]): своя копия «перечитать настройки, опросить окна,
// спросить ядро» у второго режима означала бы, что «приглушить, когда
// поверх играет полноэкранное окно» у видео и у Flash однажды разойдётся.

use cxx_qt::{CxxQtType, Threading};

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use rexon_core::wallpaper_audio::{
    other_audio_playing, parse_screensaver_active, AudioSettings, Conditions, Decision,
    // Приметы, по которым в звуковой системе виден НАШ поток: он не должен
    // сходить за «чужой звук» и глушить сам себя. Своего списка здесь нет
    // намеренно — таблица одна на продукт и живёт в ядре (правило #12),
    // иначе Flash и видео узнавали бы себя по разным признакам.
    OWN_STREAM_MARKERS as OWN_STREAM_NAMES,
};

/// Мост cxx-qt: состояние звука обоев для QML.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        /// Живое состояние звука обоев.
        ///
        /// QML держит на этих свойствах привязки, поэтому смена настройки
        /// или обстановки доходит до проигрывателя сама, без перезапуска.
        #[qobject]
        #[qml_element]
        // Держать ли звуковой поток открытым. ⚠️ Именно «поток есть /
        // потока нет», а не «громкость 0»: молчащий поток весь сеанс висит
        // в микшере отдельным ползунком и не даёт звуковой карте уснуть, а
        // главное — по нему невозможно отличить «играет тихо» от «не
        // играет вовсе».
        #[qproperty(bool, audio_on, cxx_name = "audioOn")]
        #[qproperty(bool, paused)]
        #[qproperty(f64, volume)]
        // Почему сейчас так, словами. Нужно не для красоты: в самопроверке
        // это единственный способ отличить «тишина, потому что просили» от
        // «тишина, потому что не завелось».
        #[qproperty(QString, reason)]
        type WallpaperAudio = super::WallpaperAudioRust;
    }

    extern "RustQt" {
        /// Запустить наблюдение. Размер экрана нужен, чтобы отличить окно
        /// во весь экран от обычного, — окно обоев и есть весь экран.
        #[qinvokable]
        fn start(self: Pin<&mut WallpaperAudio>, screen_w: i32, screen_h: i32);
    }

    impl cxx_qt::Threading for WallpaperAudio {}
    impl cxx_qt::Constructor<()> for WallpaperAudio {}
}

/// Как часто перечитывать обстановку.
///
/// Полсекунды — компромисс, который видно на глаз: правка ползунка
/// громкости отзывается «сразу», а опрос оконного менеджера и звуковой
/// системы стоит два коротких процесса в секунду. Меньше делать незачем,
/// больше — и приглушение при запуске игры начинает опаздывать.
const TICK: Duration = Duration::from_millis(500);

/// Сколько ждать внешнюю команду опроса.
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct WallpaperAudioRust {
    audio_on: bool,
    paused: bool,
    volume: f64,
    reason: cxx_qt_lib::QString,
    started: bool,
    stop: Option<Arc<AtomicBool>>,
}

impl Drop for WallpaperAudioRust {
    fn drop(&mut self) {
        // Поток наблюдения переживать окно не должен: он держит handle на
        // объект и на мёртвом объекте `queue` уже не проходит, а вот
        // процессы опроса продолжал бы плодить.
        if let Some(flag) = &self.stop {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

impl cxx_qt::Initialize for qobject::WallpaperAudio {
    fn initialize(self: Pin<&mut Self>) {
        // Начальное состояние — «звук как записано в настройках, ничем не
        // приглушён». Иначе первые полсекунды обои молчали бы, а на коротком
        // прогоне самопроверки — весь прогон.
        let settings = load_settings();
        let decision = settings.decide(&Conditions::default());
        let mut this = self;
        this.as_mut().apply(decision, "начальные настройки");
    }
}

impl qobject::WallpaperAudio {
    /// Разложить решение по свойствам. Сеттеры сгенерированы cxx-qt и сами
    /// молчат, когда значение не изменилось.
    fn apply(mut self: Pin<&mut Self>, d: Decision, why: &str) {
        self.as_mut().set_audio_on(d.audio_on);
        self.as_mut().set_paused(d.paused);
        self.as_mut().set_volume(d.volume01);
        self.as_mut().set_reason(cxx_qt_lib::QString::from(why));
    }

    /// Поднять наблюдение. Повторный вызов ничего не делает.
    pub fn start(mut self: Pin<&mut Self>, screen_w: i32, screen_h: i32) {
        if self.started {
            return;
        }
        self.as_mut().rust_mut().started = true;
        let stop = Arc::new(AtomicBool::new(false));
        self.as_mut().rust_mut().stop = Some(stop.clone());
        let handle = self.as_ref().get_ref().qt_thread();
        let (w, h) = (i64::from(screen_w), i64::from(screen_h));
        // Обычный поток, а не таймер Qt: опрос ходит во внешние программы,
        // и делать это в потоке отрисовки — значит дёргать видео на каждом
        // тике.
        let spawned = std::thread::Builder::new()
            .name("rexon-wp-audio".into())
            .spawn(move || {
                watch_loop(stop, w, h, |decision, why| {
                    // `Err` — объект умер (окно закрывают): наблюдать больше
                    // не за чем, цикл выходит сам.
                    handle
                        .queue(move |object| object.apply(decision, &why))
                        .is_ok()
                })
            });
        if let Err(e) = spawned {
            rexon_core::log_warn!(
                "wallpaper-host",
                "REXON-WPH-AUDIO наблюдение не поднялось ({e}) — звук обоев \
                 останется таким, каким его застали настройки"
            );
        }
    }
}

/// Цикл наблюдения. Живёт в своём потоке; наружу уходит только готовое
/// решение, и только когда оно изменилось.
///
/// `deliver` возвращает `false`, когда доставлять больше некому, — тогда
/// цикл выходит. Ни Qt, ни QML здесь не упоминаются намеренно: тем же
/// циклом живёт Flash-режим, у которого QML нет (см. [`watch`]).
fn watch_loop(
    stop: Arc<AtomicBool>,
    screen_w: i64,
    screen_h: i64,
    mut deliver: impl FnMut(Decision, String) -> bool,
) {
    let path = rexon_core::settings::settings_file();
    let backend = window_backend();
    let mut settings = load_settings();
    let mut stamp = mtime(&path);
    let mut last: Option<Decision> = None;
    while !stop.load(Ordering::SeqCst) {
        // Настройки перечитываются только когда файл действительно
        // изменился: разбор JSON каждые полсекунды впустую никому не нужен.
        let now = mtime(&path);
        if now != stamp {
            stamp = now;
            settings = load_settings();
        }
        let conditions = probe(&settings, backend, screen_w, screen_h);
        let decision = settings.decide(&conditions);
        if last != Some(decision) {
            last = Some(decision);
            let why = explain(&settings, &conditions, &decision);
            if !deliver(decision, why) {
                return;
            }
        }
        std::thread::sleep(TICK);
    }
}

/// Живая подписка на решения о звуке обоев для режима БЕЗ QML.
///
/// Тот же цикл, те же настройки и то же решение ядра, что у видео, — иначе
/// «громкость обоев» опять начала бы значить у двух режимов разное
/// (правило #12). `on_change` зовётся из отдельного потока и только при
/// СМЕНЕ решения; первое решение приходит сразу, ждать полсекунды не надо.
///
/// Размер экрана — тот же, что у видео: САМЫЙ МАЛЕНЬКИЙ из подключённых.
/// Почему именно маленький, разобрано в `WallpaperVideo.qml`
/// (`audioProbeSize`): приглушиться зря — мелочь, играть поверх кино — беда.
///
/// Наблюдение живёт, пока жив возвращённый [`AudioWatch`].
pub fn watch(
    screen_w: i64,
    screen_h: i64,
    mut on_change: impl FnMut(Decision, String) + Send + 'static,
) -> Result<AudioWatch, std::io::Error> {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::Builder::new()
        .name("rexon-wp-audio".into())
        .spawn(move || {
            watch_loop(flag, screen_w, screen_h, |decision, why| {
                on_change(decision, why);
                true
            })
        })?;
    Ok(AudioWatch { stop })
}

/// Живое наблюдение за звуком обоев. Уронили — наблюдение остановлено.
pub struct AudioWatch {
    stop: Arc<AtomicBool>,
}

impl Drop for AudioWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Настройки звука обоев прямо сейчас — чтобы режим без QML начинал с того
/// же, что записано у человека, а не с умолчаний.
pub fn settings_now() -> AudioSettings {
    load_settings()
}

/// Настройки из общего файла. Файла нет — умолчания ядра, а не тишина.
fn load_settings() -> AudioSettings {
    AudioSettings::from_map(&rexon_core::settings::load_settings(
        &rexon_core::settings::settings_file(),
    ))
}

fn mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Чем спрашивать про окна в этом сеансе (решает ядро — таблица одна на
/// продукт).
fn window_backend() -> &'static str {
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    // Окружение — тем же ответом, что у движка обоев (правило #12): здесь
    // была своя цепочка подстрок, и порядок в ней отдавал `Budgie:GNOME`
    // как gnome.
    let de = rexon_core::wallpaper_engine::detect_de(
        &env("XDG_CURRENT_DESKTOP"),
        &env("DESKTOP_SESSION"),
    );
    let session = env("XDG_SESSION_TYPE");
    rexon_core::wallpaper_engine::fullscreen_backend(&de, &session)
}

/// Опрос обстановки. Спрашивается только то, что реально влияет на решение:
/// если человек не просил глушить при чужом звуке, звуковую систему никто
/// не дёргает.
fn probe(
    settings: &AudioSettings,
    backend: &str,
    screen_w: i64,
    screen_h: i64,
) -> Conditions {
    let mut c = Conditions::default();
    let window_matters = settings
        .actions
        .list()
        .iter()
        .any(|a| *a != rexon_core::wallpaper_audio::Action::None);
    if window_matters {
        c = probe_windows(backend, screen_w, screen_h);
        if settings.actions.session_locked != rexon_core::wallpaper_audio::Action::None {
            c.session_locked = run(&rexon_core::wallpaper_audio::screensaver_argv())
                .map(|out| parse_screensaver_active(&out))
                .unwrap_or(false);
        }
    }
    if settings.mute_when_other_audio {
        c.other_audio = run(&rexon_core::wallpaper_audio::sink_inputs_argv())
            .map(|out| other_audio_playing(&out, &OWN_STREAM_NAMES))
            .unwrap_or(false);
    }
    c
}

/// Состояние окон. Разбор во всех случаях ядерный, здесь только запуск.
fn probe_windows(backend: &str, screen_w: i64, screen_h: i64) -> Conditions {
    use rexon_core::wallpaper_engine as we;
    match backend {
        "kdotool" => {
            let Some(id) = run(&we::kdotool_active_argv()) else {
                return Conditions::default();
            };
            let id = id.trim();
            if id.is_empty() {
                return Conditions::default();
            }
            let mut argv: Vec<String> =
                vec!["qdbus6".into(), we::KWIN_WINDOW_INFO[0].into(),
                     we::KWIN_WINDOW_INFO[1].into(), we::KWIN_WINDOW_INFO[2].into()];
            argv.push(id.to_string());
            let Some(info) = run(&argv) else {
                return Conditions::default();
            };
            rexon_core::wallpaper_audio::kwin_window_conditions(
                &info, screen_w, screen_h, &own_pids(),
            )
        }
        "hyprctl" => {
            let Some(out) = run(&we::hyprctl_active_argv()) else {
                return Conditions::default();
            };
            // Своё же окно обоев «окном поверх обоев» не считается —
            // тот же отсев, что на X11 и на KWin (правило #12).
            if we::hyprctl_window_is_wallpaper_layer(&out, &own_pids()) {
                return Conditions::default();
            }
            let full = we::parse_hyprctl_fullscreen(&out);
            Conditions {
                any_window: !out.trim().is_empty(),
                focused: !out.trim().is_empty(),
                fullscreen: full,
                maximized: full,
                ..Conditions::default()
            }
        }
        "gdbus" => {
            let full = run(&we::gdbus_fullscreen_argv())
                .map(|o| we::parse_gdbus_fullscreen(&o))
                .unwrap_or(false);
            Conditions {
                any_window: full,
                focused: full,
                fullscreen: full,
                maximized: full,
                ..Conditions::default()
            }
        }
        "xdotool" => {
            // ⚠️ Одной геометрии мало: активным окном оконный менеджер
            // называет и САМ рабочий стол, а наши обои и есть рабочий стол
            // во весь экран, — иначе «поверх играет полноэкранное окно»
            // было бы выполнено всегда. Что значит эта геометрия, решает
            // ядро; здесь только запуск команд опроса.
            let Some(out) = run(&we::xdotool_geometry_argv()) else {
                return Conditions::default();
            };
            rexon_core::wallpaper_audio::x11_window_conditions(
                &out,
                &format!("{screen_w}x{screen_h}"),
                &own_pids(),
                |id| run(&we::xprop_window_info_argv(id)),
            )
        }
        // Узнать нечем — обои играют, как их просили. Молчащие «на всякий
        // случай» обои человек чинить не пойдёт, он решит, что звука нет.
        _ => Conditions::default(),
    }
}

/// Процессы, чьи окна и есть обои.
///
/// Наблюдение живёт ВНУТРИ хоста обоев, поэтому свой процесс — он и есть:
/// окно с этим `_NET_WM_PID` рисует обои, а не закрывает их. Это
/// подстраховка на случай, когда тип окна спросить нечем; основной отсев
/// идёт по `_NET_WM_WINDOW_TYPE_DESKTOP`.
fn own_pids() -> Vec<i64> {
    vec![i64::from(std::process::id())]
}

/// Запустить команду опроса и забрать её вывод.
///
/// ⚠️ C-локаль (`utils::parsed_output_locale`): под русской локалью `pactl`
/// переводит имена полей, и разбор «играет ли кто-то ещё» перестаёт их
/// находить.
fn run(argv: &[String]) -> Option<String> {
    let (program, args) = argv.split_first()?;
    let mut child = rexon_core::utils::parsed_output_locale(&mut std::process::Command::new(program))
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let deadline = std::time::Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Почему сейчас так — одной строкой, по-человечески.
fn explain(settings: &AudioSettings, c: &Conditions, d: &Decision) -> String {
    if !settings.enabled {
        return "звук обоев выключен в настройках".into();
    }
    if d.paused {
        return if c.fullscreen {
            "полноэкранное окно — обои на паузе".into()
        } else {
            "обои на паузе по настройке".into()
        };
    }
    if !d.audio_on {
        if settings.mute_when_other_audio && c.other_audio {
            return "звук играет другая программа".into();
        }
        if c.maximized {
            return "окно закрывает обои".into();
        }
        return "заглушено по настройке".into();
    }
    format!("играет, громкость {}%", (d.volume01 * 100.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn свой_поток_не_считается_чужим_звуком() {
        // Приметы должны покрывать то, чем наш поток подписан в звуковой
        // системе: иначе обои глушат сами себя и это выглядит как «звук
        // просто не работает».
        let ours = "Sink Input #1\n\tCorked: no\n\t\tnode.name = \"RexonVideoWallpaper\"\n";
        assert!(!other_audio_playing(ours, &OWN_STREAM_NAMES));
    }

    #[test]
    fn поток_flash_обоев_тоже_свой() {
        // ⭐ Живой замер 16.09.2026: Flash ходит в звук клиентом ALSA, и
        // плагин ALSA у PipeWire подписывает поток своими словами. Ни одно
        // прежнее имя точного совпадения не давало — Flash-обои глушили
        // сами себя, а в журнале стояло «звук играет другая программа» при
        // пустом столе. Строки ниже сняты с `pactl list sink-inputs`
        // дословно.
        let flash = "Sink Input #1\n\tCorked: no\n\t\t                     application.name = \"PipeWire ALSA [rexon-wallpaper-host]\"\n\t\t                     node.name = \"alsa_playback.rexon-wallpaper-host\"\n\t\t                     media.name = \"ALSA Playback\"\n\t\t                     application.id = \"com.rexon.wallpaper\"\n";
        assert!(!other_audio_playing(flash, &OWN_STREAM_NAMES));

        // И наоборот: чужой проигрыватель обязан остаться чужим.
        let alien = "Sink Input #1\n\tCorked: no\n\t\t                     application.name = \"PipeWire ALSA [mpv]\"\n\t\t                     node.name = \"alsa_playback.mpv\"\n";
        assert!(other_audio_playing(alien, &OWN_STREAM_NAMES));
    }

    #[test]
    fn бэкенд_опроса_берётся_из_ядра() {
        // Своей таблицы «чем спрашивать» здесь нет — только перевод имени
        // окружения в то, что ядро уже знает.
        assert_eq!(
            rexon_core::wallpaper_engine::fullscreen_backend("kde", "wayland"),
            "kdotool"
        );
        assert_eq!(
            rexon_core::wallpaper_engine::fullscreen_backend("kde", "x11"),
            "xdotool"
        );
    }

    #[test]
    fn неизвестное_окружение_не_оставляет_без_звука() {
        // Узнать про окна нечем — обои обязаны играть, а не молчать.
        assert_eq!(probe_windows("", 1920, 1080), Conditions::default());
    }

    #[test]
    fn объяснение_различает_тишину_по_просьбе_и_по_обстановке() {
        let off = AudioSettings {
            enabled: false,
            ..AudioSettings::default()
        };
        let c = Conditions::default();
        assert!(explain(&off, &c, &off.decide(&c)).contains("выключен"));

        let on = AudioSettings::default();
        let loud = Conditions {
            other_audio: true,
            ..Conditions::default()
        };
        assert!(explain(&on, &loud, &on.decide(&loud)).contains("другая программа"));

        let full = Conditions {
            fullscreen: true,
            ..Conditions::default()
        };
        assert!(explain(&on, &full, &on.decide(&full)).contains("полноэкранное"));
        assert!(explain(&on, &c, &on.decide(&c)).contains("50%"));
    }
}
