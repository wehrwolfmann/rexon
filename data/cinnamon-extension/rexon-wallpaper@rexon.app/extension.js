// Rexon Wallpaper Engine — Cinnamon extension wrapper.
//
// Wehrwolfmann: аналог GNOME-extension. Cinnamon использует Muffin (fork
// Mutter), wlr-layer-shell не поддерживает. На Cinnamon-X11 работает
// xprop fallback (`rexon-gui::wallpaper_host::apply_x11_desktop_layer`); на
// Cinnamon-Wayland (experimental с 6.0) — нужен этот extension.
//
// Архитектура:
// 1. Хост обоев рендерит каждый кадр в /run/user/$UID/rexon-wp-frame.rgba
//    (RGBA8888, под flock LOCK_EX). Строку запуска ПРИСЫЛАЕТ Rexon в ключе
//    настроек `daemon-argv`: расширение — код оболочки, оно не знает, где
//    лежит бинарь хоста обоев. Запасного пути нет: не прислали строку —
//    расширение говорит об этом в журнал и обои не запускает.
// 2. Этот extension ставит по актёру на каждый монитор между фоном и
//    окнами (где именно и почему — у `_installActors`) и каждый tick
//    (~30 FPS) кладёт в них кадр как Clutter.Image.
//
// Cinnamon JS API: classic imports (НЕ ESM как GNOME 45+).

const GLib = imports.gi.GLib;
const Gio = imports.gi.Gio;
const Clutter = imports.gi.Clutter;
const Cogl = imports.gi.Cogl;
const Meta = imports.gi.Meta;
const Main = imports.ui.main;
const Lang = imports.lang;

const FRAME_PATH = GLib.get_user_runtime_dir() + '/rexon-wp-frame.rgba';
const DAEMON_TICK_MS = 33;  // ~30 FPS

// Module-level state (Cinnamon extension lifecycle: init/enable/disable
// вызываются как обычные функции, не методы класса).
let _settings = null;
let _daemon_pid = null;
let _daemon_stopping = false;
let _tick_source = 0;
let _actors = [];          // [{actor, metaWindow, sizeId}] — куда кладётся кадр
let _fallback = null;      // Meta.BackgroundGroup, когда фон csd не найден
let _bottomAddedId = 0;    // подписка на новые окна нижней группы
let _monitorsChangedId = 0;
let _fallbackIdle = 0;
let _frame_w = 0;
let _frame_h = 0;
let _meta = null;
let _gio_settings = null;
let _settings_changed_id = 0;

function init(metadata) {
    _meta = metadata;
}

function enable() {
    _reload();
    // Rexon меняет обои, записывая настройки расширения. Без подписки новые
    // обои появлялись бы только после перезахода в сеанс.
    if (_gio_settings && !_settings_changed_id) {
        _settings_changed_id = _gio_settings.connect('changed', function() {
            _reload();
        });
    }
    global.log('[rexon-wp] enabled, daemon pid=' + _daemon_pid);
}

// Перечитать настройки и перезапустить обои под них.
// Актёры ставятся ТОЛЬКО когда обои реально запущены: иначе чёрный актёр
// перекрыл бы обычный фон и рабочий стол стал бы чёрным.
function _reload() {
    _stopTick();
    _removeActors();
    _killDaemon();

    _settings = _loadSettings();
    if (!_settings || !_settings.engine || !_settings.source) {
        global.log('[rexon-wp] обои не заданы — рабочий стол не трогаем');
        return;
    }

    _spawnDaemon(_settings);
    if (!_daemon_pid) return;
    _installActors();
    _startTick();
}

function disable() {
    if (_gio_settings && _settings_changed_id) {
        _gio_settings.disconnect(_settings_changed_id);
        _settings_changed_id = 0;
    }
    _stopTick();
    _removeActors();
    _killDaemon();
    _settings = null;
    global.log('[rexon-wp] disabled');
}

// ── Settings (gsettings, не GSettings binding из cinnamon-settings) ────

function _loadSettings() {
    // Wehrwolfmann: читаем напрямую через Gio.Settings — Cinnamon
    // умеет автоматически разрешать schema по metadata.json.
    try {
        const schemaSrc = Gio.SettingsSchemaSource.new_from_directory(
            _meta.path + '/schemas',
            Gio.SettingsSchemaSource.get_default(),
            false,
        );
        const schema = schemaSrc.lookup(
            'org.cinnamon.extensions.rexon-wallpaper', false,
        );
        if (!schema) {
            global.logError('[rexon-wp] schema не найдена');
            return null;
        }
        const s = new Gio.Settings({ settings_schema: schema });
        _gio_settings = s;   // нужен живым для подписки на 'changed'
        // Ключ появился позже самого расширения: у оставшегося от прежней
        // версии его нет, и падать из-за этого нельзя — иначе обои
        // перестанут работать вовсе. Пусто = «Rexon ничего не присылал».
        //
        // ⚠️ Спрашиваем СХЕМУ, а не оборачиваем чтение в try/catch:
        // `get_strv` на неизвестном ключе не бросает перехватываемое
        // исключение, а печатает «does not contain a key named …» и ГАСИТ
        // ПРОЦЕСС (проверено — SIGABRT, код выхода 134). Процесс здесь —
        // сама оболочка, то есть ценой была бы не обоина, а сеанс.
        const daemon_argv = schema.has_key('daemon-argv')
            ? s.get_strv('daemon-argv')
            : [];
        return {
            engine: s.get_string('engine'),
            source: s.get_string('source'),
            fps: s.get_int('fps') || 30,
            daemon_argv: daemon_argv,
        };
    } catch (e) {
        global.logError('[rexon-wp] settings load: ' + e);
        return null;
    }
}

// ── Daemon control ──────────────────────────────────────────────────

// Строка запуска демона обоев — ТОЛЬКО та, что прислал Rexon.
//
// Запасной сборки `python3 -m rexon.video_wallpaper` здесь больше нет:
// Python-хост обоев снят с дерева, подменять бинарь нечем. Пустой массив в
// настройках = «Rexon ничего не присылал»; разбирается этот случай не
// здесь, а в общем блоке ниже — вместе с остальными негодными строками
// запуска, чтобы объяснение человеку было одно на все случаи.

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

// Сказать об отказе ОБА раза: в журнал (там подробности для разбора) и
// всплывающим сообщением оболочки — иначе покупатель видит только, что
// «обои не работают», и причину узнать неоткуда.
function _reportFailure(text, cfg) {
    global.logError('[rexon-wp] ' + text + ' [обои: движок «' +
        (cfg ? cfg.engine : '?') + '», источник «' +
        (cfg ? cfg.source : '?') + '»]');
    try {
        Main.notifyError(LAUNCH_FAIL_TITLE, text);
    } catch (e) {
        global.logError('[rexon-wp] показать сообщение не вышло: ' + e);
    }
}

function _spawnDaemon(cfg) {
    const argv = cfg.daemon_argv;
    const problem = launchProblem(argv);
    if (problem) {
        _reportFailure(problem, cfg);
        return;
    }

    // Размер первого монитора (Cinnamon обычно один primary)
    const monitor = Main.layoutManager.primaryMonitor;
    const w = monitor ? monitor.width : 1920;
    const h = monitor ? monitor.height : 1080;
    _frame_w = w;
    _frame_h = h;

    const env = GLib.get_environ();
    env.push('REXON_WP_OUTPUT=' + FRAME_PATH);
    env.push('REXON_WP_WIDTH=' + w);
    env.push('REXON_WP_HEIGHT=' + h);
    env.push('REXON_WP_FPS=' + cfg.fps);

    // Новый запуск — новая жизнь: пометка «снимаем сами» могла остаться с
    // прошлого раза, если тот демон так и не отчитался о выходе.
    _daemon_stopping = false;

    // ⚠️ БЕЗ `SpawnFlags.SEARCH_PATH`, и это главная часть починки: с ним
    // оболочка молча взяла бы ПЕРВУЮ подходящую программу из своего PATH, а
    // у покупателя с venv, переносным каталогом или Flatpak это не тот
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
            _reportFailure(spawnFailureText(argv, null), cfg);
            return;
        }
        _daemon_pid = pid;
        GLib.child_watch_add(GLib.PRIORITY_DEFAULT, pid, function(_p, status) {
            global.log('[rexon-wp] daemon exited, status=' + status);
            _daemon_pid = null;
            GLib.spawn_close_pid(_p);
            // Сами сняли демона (смена обоев, выключение) — это не отказ.
            const stopping = _daemon_stopping;
            _daemon_stopping = false;
            if (stopping || status === 0) return;
            // Демон лёг сам: снимаем чёрные актёры, иначе рабочий стол так
            // и останется чёрным прямоугольником без обоев.
            _stopTick();
            _removeActors();
            _reportFailure(earlyExitText(argv, status), cfg);
        });
    } catch (e) {
        _reportFailure(spawnFailureText(argv, e), cfg);
    }
}

function _killDaemon() {
    if (_daemon_pid) {
        // Пометка для child-watch: следующий выход демона — наших рук дело,
        // ругаться на него не надо.
        _daemon_stopping = true;
        try {
            GLib.spawn_command_line_async('kill ' + _daemon_pid);
        } catch (e) {
            global.logError('[rexon-wp] kill failed: ' + e);
        }
        _daemon_pid = null;
    }
}

// ── Background actors ────────────────────────────────────────────────
//
// Куда ставить картинку. Разобрано по исходникам Cinnamon 6.6 / Muffin 6.6:
//
// * `Main.layoutManager._bgManagers` у Cinnamon НЕТ — это поле GNOME Shell.
//   Прежний код шёл циклом по пустому массиву, и обои не появлялись вовсе.
// * Стопка Muffin (`compositor.c`, `sync_actor_stacking`): внутри
//   `global.window_group` снизу вверх — фон X11 (`global.background_actor`,
//   только на X11), `global.bottom_window_group` с окнами типа DESKTOP,
//   контейнер десклетов, обычные окна. Любой «чужой» актёр в группе окон при
//   перестановке всплывает НАД окнами; на своём месте остаются только
//   Meta.BackgroundActor / Meta.BackgroundGroup — их Muffin опускает в самый
//   низ.
// * На Wayland фон рисует csd-background: на каждый монитор окно XWayland
//   типа DESKTOP с keep-below (`monitor-background.c`). Muffin кладёт его в
//   `bottom_window_group` и опускает ниже окна стола Nemo. Сплошное окно фона
//   закрывает всё, что ниже, поэтому актёр под ним не виден, а над ним в той
//   же группе — всплывает над Nemo и прячет значки.
//
// Отсюда два места:
// 1. Окно фона csd есть — актёр становится ДОЧЕРНИМ актёром его
//    MetaWindowActor: рисуется поверх картинки csd и ездит в стопке вместе с
//    окном, то есть всегда под Nemo и под окнами. Монитор — тот же, что у
//    окна: csd держит по окну на монитор.
// 2. Окна фона csd нет (X11, либо фон csd выключен) — Meta.BackgroundGroup
//    внизу группы окон (над фоном X11, если он есть) с актёром на каждый
//    монитор. Muffin считает группу фоном и держит её под всеми окнами.
const CSD_BACKGROUND_CLASS = 'csd-background';

function _isCsdBackground(windowActor) {
    if (!(windowActor instanceof Meta.WindowActor)) return false;
    const mw = windowActor.get_meta_window();
    if (!mw || mw.get_window_type() !== Meta.WindowType.DESKTOP) return false;
    const names = [mw.get_wm_class_instance(), mw.get_wm_class()];
    for (let i = 0; i < names.length; i++) {
        if (names[i] && names[i].toLowerCase() === CSD_BACKGROUND_CLASS) return true;
    }
    return false;
}

function _trackActor(actor, metaWindow, sizeId) {
    const entry = { actor: actor, metaWindow: metaWindow, sizeId: sizeId };
    _actors.push(entry);
    // Окно фона уходит (смена мониторов — csd пересоздаёт окна) и уносит
    // дочерний актёр с собой: забываем его, а если окон фона не осталось —
    // встаёт запасная группа.
    actor.connect('destroy', function() {
        const i = _actors.indexOf(entry);
        if (i >= 0) _actors.splice(i, 1);
        _disconnectSize(entry);
        if (metaWindow) _queueFallbackCheck();
    });
}

function _disconnectSize(entry) {
    if (entry.metaWindow && entry.sizeId) {
        try {
            entry.metaWindow.disconnect(entry.sizeId);
        } catch (e) {
            // окно уже разобрано — отписываться не от чего
        }
    }
    entry.sizeId = 0;
}

// Кадр — ровно на рамку окна: координаты дочернего актёра считаются от
// буфера окна, а он может быть шире рамки.
function _fitToWindow(actor, mw) {
    const buf = mw.get_buffer_rect();
    const frame = mw.get_frame_rect();
    actor.set_position(frame.x - buf.x, frame.y - buf.y);
    actor.set_size(frame.width, frame.height);
}

function _attachToCsd(windowActor) {
    for (let i = 0; i < _actors.length; i++) {
        if (_actors[i].actor.get_parent() === windowActor) return;
    }
    const mw = windowActor.get_meta_window();
    // Без цвета фона: до первого кадра виден обычный фон csd, а не чёрный
    // прямоугольник.
    const actor = new Clutter.Actor({ reactive: false });
    _fitToWindow(actor, mw);
    windowActor.add_child(actor);
    const sizeId = mw.connect('size-changed', function() {
        _fitToWindow(actor, mw);
    });
    _trackActor(actor, mw, sizeId);
    _removeFallback();
}

function _csdBackgrounds() {
    const found = [];
    const group = global.bottom_window_group;
    if (!group) return found;
    const children = group.get_children();
    for (let i = 0; i < children.length; i++) {
        if (_isCsdBackground(children[i])) found.push(children[i]);
    }
    return found;
}

function _installFallback() {
    _removeFallback();
    const group = new Meta.BackgroundGroup();
    const monitors = Main.layoutManager.monitors;
    for (let i = 0; i < monitors.length; i++) {
        const m = monitors[i];
        const actor = new Clutter.Actor({
            x: m.x, y: m.y, width: m.width, height: m.height, reactive: false,
        });
        group.add_child(actor);
        _trackActor(actor, null, 0);
    }
    const parent = global.window_group;
    const x11bg = global.background_actor;
    if (x11bg && x11bg.get_parent() === parent) {
        parent.insert_child_above(group, x11bg);
    } else {
        parent.insert_child_at_index(group, 0);
    }
    _fallback = group;
}

function _removeFallback() {
    if (!_fallback) return;
    const group = _fallback;
    _fallback = null;
    group.destroy();   // дочерние актёры уходят из _actors сами, по 'destroy'
}

function _queueFallbackCheck() {
    if (_fallbackIdle || !_bottomAddedId) return;
    _fallbackIdle = GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, function() {
        _fallbackIdle = 0;
        if (_bottomAddedId && !_fallback && _csdBackgrounds().length === 0) {
            _installFallback();
        }
        return GLib.SOURCE_REMOVE;
    });
}

function _installActors() {
    const csd = _csdBackgrounds();
    for (let i = 0; i < csd.length; i++) _attachToCsd(csd[i]);
    if (csd.length === 0) _installFallback();

    // csd часто поднимает окна фона позже расширения и пересоздаёт их при
    // смене мониторов — встаём на каждое новое.
    if (global.bottom_window_group && !_bottomAddedId) {
        _bottomAddedId = global.bottom_window_group.connect('actor-added',
            function(_group, child) {
                if (_isCsdBackground(child)) _attachToCsd(child);
            });
    }
    if (!_monitorsChangedId) {
        _monitorsChangedId = Main.layoutManager.connect('monitors-changed',
            function() {
                if (_fallback) _installFallback();
            });
    }
    global.log('[rexon-wp] актёры обоев: ' + _actors.length + ' (' +
        (csd.length ? 'поверх фона csd-background' : 'своя группа фона') + ')');
}

function _removeActors() {
    if (_bottomAddedId) {
        global.bottom_window_group.disconnect(_bottomAddedId);
        _bottomAddedId = 0;
    }
    if (_monitorsChangedId) {
        Main.layoutManager.disconnect(_monitorsChangedId);
        _monitorsChangedId = 0;
    }
    if (_fallbackIdle) {
        GLib.source_remove(_fallbackIdle);
        _fallbackIdle = 0;
    }
    _removeFallback();
    const left = _actors.slice();
    _actors = [];
    for (let i = 0; i < left.length; i++) {
        _disconnectSize(left[i]);
        left[i].actor.destroy();
    }
}

// ── Frame tick ───────────────────────────────────────────────────────

function _startTick() {
    _tick_source = GLib.timeout_add(
        GLib.PRIORITY_DEFAULT, DAEMON_TICK_MS, _tick,
    );
}

function _stopTick() {
    if (_tick_source) {
        GLib.source_remove(_tick_source);
        _tick_source = 0;
    }
}

function _tick() {
    if (!_frame_w || !_frame_h) return GLib.SOURCE_CONTINUE;
    if (!GLib.file_test(FRAME_PATH, GLib.FileTest.EXISTS)) {
        return GLib.SOURCE_CONTINUE;  // демон ещё не стартовал
    }

    let data;
    try {
        data = readWholeFrame(FRAME_PATH, _frame_w * _frame_h * 4);
    } catch (e) {
        global.logError('[rexon-wp] mmap read failed: ' + e);
        return GLib.SOURCE_CONTINUE;
    }
    if (!data) {
        return GLib.SOURCE_CONTINUE;  // кадра нет, недописан или пишется — ждём
    }

    // Одна копия и одна картинка на все мониторы: прежде каждый актёр
    // копировал файл заново, и мониторы могли получить разные кадры. Кадр
    // размером с основной монитор, на остальных растягивается.
    if (_actors.length === 0) return GLib.SOURCE_CONTINUE;
    let image;
    try {
        image = new Clutter.Image();
        image.set_data(
            data,
            Cogl.PixelFormat.RGBA_8888,
            _frame_w, _frame_h,
            _frame_w * 4,
        );
    } catch (e) {
        global.logError('[rexon-wp] Clutter.Image set failed: ' + e);
        return GLib.SOURCE_CONTINUE;
    }
    for (let i = 0; i < _actors.length; i++) {
        _actors[i].actor.set_content(image);
    }

    return GLib.SOURCE_CONTINUE;
}
