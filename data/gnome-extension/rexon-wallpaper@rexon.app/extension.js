// Rexon Wallpaper Engine — GNOME Shell extension wrapper.
//
// Зачем это вообще нужно: у Mutter нет wlr-layer-shell, поэтому обои сюда
// нельзя положить окном, как на KDE/sway/Hyprland. Движок waywallen тут
// бессилен — единственный путь показать живые обои на GNOME-Wayland — это
// расширение оболочки, читающее готовые кадры.
//
// Архитектура:
// 1. Хост обоев запускается фоновым процессом через GLib.spawn_async и
//    рисует обои в невидимое окно. Строку запуска ПРИСЫЛАЕТ Rexon в ключе
//    настроек `daemon-argv`: расширение — код оболочки, оно не знает, где
//    лежит бинарь хоста обоев. Запасного пути нет: не прислали строку —
//    расширение говорит об этом в журнал и обои не запускает.
// 2. Демон пишет каждый кадр в shared-mmap-файл /run/user/$UID/rexon-wp-frame.rgba
//    (RGBA8888, без заголовка, под flock LOCK_EX).
// 3. Этот extension кладёт St.ImageContent на свои актёры поверх фона
//    GNOME Shell (Main.layoutManager._bgManagers) и обновляет текстуру
//    каждый tick. Clutter.Image в mutter 48 удалён — см. makeFrameUploader.
//
// GNOME Shell 45+ требует ESM (import) синтаксис.

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import St from 'gi://St';
import Cogl from 'gi://Cogl';
import * as Config from 'resource:///org/gnome/shell/misc/config.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';


const FRAME_PATH = GLib.get_user_runtime_dir() + '/rexon-wp-frame.rgba';
const DAEMON_TICK_MS = 33;  // ~30 FPS

// Переходник под версии оболочки: как залить кадр в St.ImageContent.
//
// GNOME 45–47: St.ImageContent — наследник Clutter.Image, своего set_data
// у него нет, работает унаследованный set_data(data, format, w, h, stride).
// GNOME 48+: Clutter.Image из mutter удалён (в Clutter-51.gir класса
// Image нет), St.ImageContent стал самостоятельным и требует первым
// аргументом контекст Cogl — set_data(cogl, data, format, w, h, stride).
// Контекст берём так же, как сама оболочка (screenshot.js 48, qrCode.js 51):
// global.stage.context.get_backend().get_cogl_context().
// Прежде здесь стоял `new Clutter.Image()`: на 48+ он бросал исключение
// каждый tick, а чёрная подложка актёра закрывала стол.
function makeFrameUploader() {
    const major = parseInt(Config.PACKAGE_VERSION, 10);
    if (major >= 48) {
        const cogl = global.stage.context.get_backend().get_cogl_context();
        return (content, data, w, h) => content.set_data(
            cogl, data, Cogl.PixelFormat.RGBA_8888, w, h, w * 4);
    }
    return (content, data, w, h) => content.set_data(
        data, Cogl.PixelFormat.RGBA_8888, w, h, w * 4);
}

// Строка запуска демона обоев — ТОЛЬКО та, что прислал Rexon.
//
// Запасной сборки `python3 -m rexon.video_wallpaper` здесь больше нет:
// Python-хост обоев снят с дерева, подменять бинарь нечем. Пустой массив в
// настройках = «Rexon ничего не присылал»; разбирается этот случай не
// здесь, а в общем блоке ниже — вместе с остальными негодными строками
// запуска, чтобы объяснение человеку было одно на все случаи.
function _daemonArgv(settings) {
    // ⚠️ Наличие ключа спрашиваем У СХЕМЫ, а не через try/catch вокруг
    // чтения. `get_strv` на ключе, которого в схеме нет, не бросает
    // перехватываемое исключение: GLib печатает
    // «does not contain a key named …» и ГАСИТ ПРОЦЕСС (проверено —
    // SIGABRT, код выхода 134). Здесь процесс — сама оболочка: вместо
    // «обои не появились» покупатель получил бы вылет сеанса.
    const schema = settings.settings_schema;
    return (schema && schema.has_key('daemon-argv'))
        ? settings.get_strv('daemon-argv')
        : [];
}


// ─── ОБЩИЙ БЛОК ДИАГНОСТИКИ ЗАПУСКА ────────────────────────────────
//
// Дословно одинаков в расширениях GNOME и Cinnamon: дефект был один на оба
// файла, значит и лечение одно (правило #12). Сторож от расхождения — тест
// `rust/rexon-core/tests/wallpaper_mmap_parity.rs`,
// `shell_extensions_share_launch_diagnostics`: он вырезает этот блок из
// обоих файлов и сравнивает посимвольно, поэтому правку сюда надо вносить
// В ОБА ФАЙЛА. Вывод в журнал и показ сообщения сюда НЕ входят: у GNOME
// это `console.*`, у Cinnamon — `global.log*`, и это единственная разница,
// которую блок терпит.
// [rexon-wp:launch-diagnostics:begin]
const LAUNCH_FAIL_TITLE = 'Rexon: живые обои не запустились';

// Куда идти человеку, когда строка запуска негодна. Чинится это ровно в
// одном месте: строку пишет сам Rexon, когда передаёт обои расширению.
const REINSTALL_HINT = 'Откройте в Rexon «Оформление», раздел расширений ' +
    'оболочки, и установите расширение обоев заново — строку запуска пишет туда сам Rexon.';

// Что не так со строкой запуска, ДО того как звать оболочку.
// `null` — всё в порядке.
//
// Зачем проверять заранее. `GLib.spawn_async` на негодной строке отвечает
// голым «Failed to execute child process … (No such file or directory)»,
// и по этому тексту покупатель не поймёт ни ЧТО не нашлось, ни ГДЕ искали.
// Поэтому каждый случай называем своими словами и ВСЕГДА показываем сам
// путь: он и есть ответ на «где искали».
function launchProblem(argv) {
    if (!argv || argv.length === 0 || !argv[0]) {
        return 'Rexon не сказал, чем показывать обои: ключ настроек ' +
            '«daemon-argv» пуст. Так выглядит расширение, оставшееся от ' +
            'прежней версии Rexon. ' + REINSTALL_HINT;
    }
    const exe = argv[0];
    if (exe.charAt(0) !== '/') {
        return 'Rexon прислал имя программы, а не путь к ней: «' + exe +
            '». Расширение НЕ ищет программы в PATH, и это намеренно: в ' +
            'PATH оболочки лежит не тот Rexon, что установлен у вас, если ' +
            'он стоит в venv, в переносном каталоге или во Flatpak. Нужен ' +
            'полный путь от корня. ' + REINSTALL_HINT;
    }
    if (!GLib.file_test(exe, GLib.FileTest.EXISTS)) {
        return 'Программа показа обоев не найдена. Искали ровно один файл, ' +
            'путь к нему прислал сам Rexon: ' + exe + ' (в PATH расширение ' +
            'не смотрит). Похоже, Rexon переставили или перенесли в другой ' +
            'каталог. ' + REINSTALL_HINT;
    }
    if (!GLib.file_test(exe, GLib.FileTest.IS_EXECUTABLE)) {
        return 'Программа показа обоев на месте, но запускать её нечем: у ' +
            'файла ' + exe + ' нет права на выполнение. Так бывает после ' +
            'распаковки архива или копирования на диск, где прав Unix нет. ' +
            REINSTALL_HINT;
    }
    return null;
}

// Отказ, случившийся уже внутри оболочки: строка была годной, а запуск не
// вышел. `detail` — текст исключения либо пусто, если причины не назвали.
function spawnFailureText(argv, detail) {
    return 'Оболочка не смогла запустить показ обоев: ' +
        (detail ? detail : 'причину она не назвала') + '. Запускали: ' +
        argv.join(' ');
}

// Хост поднялся и тут же лёг. Молчать нельзя: актёры уже стоят, и на
// экране остался бы чёрный прямоугольник поверх обычного фона — ровно то
// самое «просто не работает», из-за которого баг и завели.
function earlyExitText(argv, status) {
    return 'Показ обоев запустился и сразу завершился (код ' + status +
        '). Запускали: ' + argv.join(' ') + '. Причину называет сама ' +
        'программа показа — её видно в журнале сеанса.';
}
// [rexon-wp:launch-diagnostics:end]


// ─── ОБЩИЙ БЛОК ЧТЕНИЯ КАДРА ───────────────────────────────────────
//
// Дословно одинаков в расширениях GNOME и Cinnamon, сторож —
// `shell_extensions_share_frame_read` в
// `rust/rexon-core/tests/wallpaper_mmap_parity.rs`. Правку — В ОБА ФАЙЛА.
// [rexon-wp:frame-read:begin]
// Счётчик поколения кадра лежит рядом с файлом кадра (договор —
// `rexon-core::wallpaper_mmap::frame_generation_path`): u64 little-endian,
// нечётный — хост сейчас пишет кадр, чётный — кадр записан целиком.
//
// Зачем. Хост пишет кадр под flock(LOCK_EX), а взять замок отсюда нечем: у
// GJS нет ни flock, ни fcntl. Без замка копия попадала на середину записи —
// замер на 1920×1080 и 30 кадрах: 12 рваных кадров из 180 тиков (верх от
// одного кадра, низ от другого).
const FRAME_GEN_SUFFIX = '.gen';

// Значение счётчика; null — счётчика нет (хост прежней сборки).
function frameGeneration(framePath) {
    let data;
    try {
        data = GLib.file_get_contents(framePath + FRAME_GEN_SUFFIX)[1];
    } catch (e) {
        return null;
    }
    if (!data || data.length < 8) return null;
    return new DataView(data.buffer, data.byteOffset, 8).getBigUint64(0, true);
}

// Целый кадр (Uint8Array длиной не меньше expected) либо null, если кадра
// ещё нет, он недописан или хост писал во время копии — тогда тик
// пропускается и на экране остаётся прежний кадр. Ошибки отображения файла
// пробрасываются вызывающему: у него свой журнал.
function readWholeFrame(framePath, expected) {
    const before = frameGeneration(framePath);
    if (before !== null && (before & 1n) === 1n) return null;
    const bytes = GLib.MappedFile.new(framePath, false).get_bytes();
    if (bytes.get_size() < expected) return null;
    // get_data() КОПИРУЕТ (проверено: после перезаписи файла массив прежний),
    // поэтому сверка счётчика после копии и ограждает именно эту копию.
    const data = bytes.get_data();
    if (frameGeneration(framePath) !== before) return null;
    return data;
}
// [rexon-wp:frame-read:end]

export default class RexonWallpaperExtension extends Extension {

    enable() {
        this._settings = this.getSettings();
        this._daemon_pid = null;
        this._daemon_stopping = false;
        this._tick_source = 0;
        this._actors = [];
        this._content = null;

        // Rexon меняет обои, записывая настройки расширения. Без подписки
        // новые обои появлялись бы только после перезахода в сеанс.
        this._settings_changed_id = this._settings.connect(
            'changed', () => this._reload());

        this._reload();
        console.log('[rexon-wp] enabled, daemon pid=' + this._daemon_pid);
    }

    disable() {
        if (this._settings_changed_id) {
            this._settings.disconnect(this._settings_changed_id);
            this._settings_changed_id = 0;
        }
        this._stopTick();
        this._removeActors();
        this._killDaemon();
        this._settings = null;
        console.log('[rexon-wp] disabled');
    }

    // Перечитать настройки и перезапустить обои под них.
    // Актёры ставятся ТОЛЬКО когда обои реально запущены: иначе чёрный
    // актёр перекрыл бы обычный фон и рабочий стол стал бы чёрным.
    _reload() {
        this._stopTick();
        this._removeActors();
        this._killDaemon();

        const config = this._loadConfig();
        if (!config) {
            console.log('[rexon-wp] обои не заданы — рабочий стол не трогаем');
            return;
        }

        this._spawnDaemon(config);
        if (!this._daemon_pid) return;
        this._installActors();
        this._startTick();
    }

    // ── Daemon control ─────────────────────────────────────────────

    _loadConfig() {
        // gsettings: org.gnome.shell.extensions.rexon-wallpaper
        // ключи: 'engine' (string: video), 'source' (string: путь),
        //         'fps' (int)
        const engine = this._settings.get_string('engine');
        const source = this._settings.get_string('source');
        if (!engine || !source) return null;
        return {
            engine: engine,
            source: source,
            fps: this._settings.get_int('fps') || 30,
        };
    }

    // Сказать об отказе ОБА раза: в журнал (там подробности для разбора) и
    // всплывающим сообщением оболочки — иначе покупатель видит только, что
    // «обои не работают», и причину узнать неоткуда.
    _reportFailure(text, cfg) {
        console.error('[rexon-wp] ' + text + ' [обои: движок «' +
            (cfg ? cfg.engine : '?') + '», источник «' +
            (cfg ? cfg.source : '?') + '»]');
        try {
            Main.notifyError(LAUNCH_FAIL_TITLE, text);
        } catch (e) {
            console.error('[rexon-wp] показать сообщение не вышло: ' + e.message);
        }
    }

    _spawnDaemon(cfg) {
        const argv = _daemonArgv(this._settings);
        const problem = launchProblem(argv);
        if (problem) {
            this._reportFailure(problem, cfg);
            return;
        }

        const monitor = Main.layoutManager.primaryMonitor;
        const w = monitor ? monitor.width : 1920;
        const h = monitor ? monitor.height : 1080;
        this._frame_w = w;
        this._frame_h = h;

        // ENV: REXON_WP_OUTPUT — путь к mmap-файлу кадров (разбор —
        // rexon-core::wallpaper_mmap),
        // REXON_WP_WIDTH/HEIGHT — размер кадра (равен размеру primary monitor),
        // REXON_WP_FPS — частота обновления.
        const env = GLib.get_environ();
        env.push('REXON_WP_OUTPUT=' + FRAME_PATH);
        env.push('REXON_WP_WIDTH=' + w);
        env.push('REXON_WP_HEIGHT=' + h);
        env.push('REXON_WP_FPS=' + cfg.fps);

        // Новый запуск — новая жизнь: пометка «снимаем сами» могла остаться
        // с прошлого раза, если тот демон так и не отчитался о выходе.
        this._daemon_stopping = false;

        // ⚠️ БЕЗ `SpawnFlags.SEARCH_PATH`, и это главная часть починки: с ним
        // оболочка молча взяла бы ПЕРВУЮ подходящую программу из своего PATH,
        // а у покупателя с venv, переносным каталогом или Flatpak это не тот
        // Rexon, что установлен. Запускаем ровно тот файл, путь к которому
        // прислал сам Rexon, — а годность пути проверена выше.
        try {
            const [success, pid] = GLib.spawn_async(
                null, argv, env,
                GLib.SpawnFlags.DO_NOT_REAP_CHILD,
                null,
            );
            if (!success) {
                // Ветка «вернула false, но не бросила»: раньше здесь не
                // происходило вообще ничего — ни строки в журнале.
                this._reportFailure(spawnFailureText(argv, null), cfg);
                return;
            }
            this._daemon_pid = pid;
            GLib.child_watch_add(GLib.PRIORITY_DEFAULT, pid, (_pid, status) => {
                console.log('[rexon-wp] daemon exited, status=' + status);
                this._daemon_pid = null;
                GLib.spawn_close_pid(_pid);
                // Сами сняли демона (смена обоев, выключение) — это не отказ.
                const stopping = this._daemon_stopping;
                this._daemon_stopping = false;
                if (stopping || status === 0) return;
                // Демон лёг сам: снимаем чёрные актёры, иначе рабочий стол
                // так и останется чёрным прямоугольником без обоев.
                this._stopTick();
                this._removeActors();
                this._reportFailure(earlyExitText(argv, status), cfg);
            });
        } catch (e) {
            this._reportFailure(spawnFailureText(argv, e.message), cfg);
        }
    }

    _killDaemon() {
        if (this._daemon_pid) {
            // Пометка для child-watch: следующий выход демона — наших рук
            // дело, ругаться на него не надо.
            this._daemon_stopping = true;
            try {
                GLib.spawn_command_line_async(`kill ${this._daemon_pid}`);
            } catch (e) {
                console.error('[rexon-wp] kill failed: ' + e.message);
            }
            this._daemon_pid = null;
        }
    }

    // ── Background actors ──────────────────────────────────────────

    _installActors() {
        // GNOME Shell имеет один bg actor на каждый монитор:
        // Main.layoutManager._bgManagers[i].backgroundActor.
        // Добавляем наш актёр (St.Widget) поверх дефолтного wallpaper, под окнами.
        // Размер экрана пишем в ENV хосту обоев, чтобы кадр совпадал.
        const bgManagers = Main.layoutManager._bgManagers || [];
        for (const bgm of bgManagers) {
            const monitor = bgm._monitorIndex !== undefined
                ? Main.layoutManager.monitors[bgm._monitorIndex]
                : Main.layoutManager.primaryMonitor;
            if (!monitor) continue;

            const actor = new St.Widget({
                width: monitor.width,
                height: monitor.height,
                x: monitor.x,
                y: monitor.y,
            });
            actor._monitor_w = monitor.width;
            actor._monitor_h = monitor.height;
            const parent = bgm.backgroundActor.get_parent();
            if (parent) {
                parent.insert_child_above(actor, bgm.backgroundActor);
            }
            this._actors.push(actor);
        }
    }

    _removeActors() {
        for (const a of this._actors) {
            const parent = a.get_parent();
            if (parent) parent.remove_child(a);
            a.destroy();
        }
        this._actors = [];
        // Содержимое привязано к размеру кадра — при перезапуске обоев
        // размер мог смениться, заводим заново на первом кадре.
        this._content = null;
    }

    // ── Frame tick (заглушка) ──────────────────────────────────────

    _startTick() {
        this._tick_source = GLib.timeout_add(
            GLib.PRIORITY_DEFAULT, DAEMON_TICK_MS,
            () => this._tick(),
        );
    }

    _stopTick() {
        if (this._tick_source) {
            GLib.source_remove(this._tick_source);
            this._tick_source = 0;
        }
    }

    _tick() {
        // Читаем mmap-файл → St.ImageContent → actor.content на каждом мониторе.
        // GLib.MappedFile берёт zero-copy view на файл; при следующем вызове
        // create new mapping чтобы получить свежий снимок (хост обоев пишет под
        // flock, mmap отражает актуальное состояние файла).
        if (!this._frame_w || !this._frame_h) {
            return GLib.SOURCE_CONTINUE;
        }
        if (!GLib.file_test(FRAME_PATH, GLib.FileTest.EXISTS)) {
            return GLib.SOURCE_CONTINUE;  // демон ещё не запустил рендер
        }

        let data;
        try {
            data = readWholeFrame(FRAME_PATH, this._frame_w * this._frame_h * 4);
        } catch (e) {
            console.warn('[rexon-wp] mmap read failed: ' + e.message);
            return GLib.SOURCE_CONTINUE;
        }
        if (!data) {
            return GLib.SOURCE_CONTINUE;  // кадра нет, недописан или пишется — ждём
        }

        // Одна копия на все мониторы: прежде каждый актёр копировал файл
        // заново, и мониторы могли получить разные кадры. Содержимое тоже
        // одно — актёры разных мониторов показывают один и тот же кадр.
        try {
            if (!this._content) {
                this._content = St.ImageContent.new_with_preferred_size(
                    this._frame_w, this._frame_h);
                this._upload = makeFrameUploader();
            }
            this._upload(this._content, data, this._frame_w, this._frame_h);
        } catch (e) {
            console.warn('[rexon-wp] St.ImageContent set failed: ' + e.message);
            return GLib.SOURCE_CONTINUE;
        }
        for (const actor of this._actors) {
            if (actor.get_content() !== this._content)
                actor.set_content(this._content);
        }

        return GLib.SOURCE_CONTINUE;
    }
}
