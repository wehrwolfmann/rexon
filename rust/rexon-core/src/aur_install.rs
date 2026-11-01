//! Установка пакетов из AUR помощником (yay/paru) — одна на весь Rexon.
//!
//! Здесь три решения, общие для КАЖДОГО места, где Rexon зовёт помощника
//! AUR (оформление, доустановка рабочих столов, починка проверки, SteamOS —
//! правило #12):
//!
//! 1. **Команда** ([`install_argv`]). Rexon запускают из меню приложений —
//!    терминала у него нет. Обычный `sudo`, которым помощник ставит
//!    собранный пакет, без терминала пароль спросить не может и падает
//!    («a terminal is required to read the password»). Поэтому права
//!    просятся графическим окном polkit (`--sudo pkexec`), как это уже
//!    делает пересборка AeroShell ([`crate::aeroshell::rebuild_command`]).
//!    Все вопросы помощника получают ответ заранее: показ изменений и чистка
//!    сборочных файлов — «нет», сборочные зависимости — убрать после
//!    установки. Замена конфликтующих пакетов (`--ask=4`) — ТОЛЬКО когда
//!    человек её явно подтвердил ([`plan_install`]): молча снести системный
//!    пакет ради темы — ровно та ловушка, от которой защищает правило #13.
//!
//! 2. **Причина отказа** ([`failure_cause`] → [`failure_text`]). Вывод
//!    помощника разбирается, и человек читает «пакет X конфликтует с Y» или
//!    «не скачался исходник», а не «подробности в журнале». Разбор понимает
//!    и английский вывод, и русский (`LANG=ru_RU`): часть мест запускает
//!    помощника в языке системы.
//!
//! 3. **Комплекты** ([`AurKit`]). Пакет, который на деле не тема, а форк
//!    Plasma (AeroThemePlasma), ставится комплектом в порядке инструкции его
//!    авторов и только после предупреждения: что заменится, что придётся
//!    пересобирать после обновлений Plasma.
//!
//! Дефект-образец (живьём 03.10.2026): «Оформление → Глобальные темы → AUR →
//! aerothemeplasma-desktop-git → Установить» собрало пять зависимостей и
//! упало на первой же установке собранного (sudo без терминала), а человек
//! прочитал только «подробности в журнале yay» — журнал при этом никуда не
//! сохранялся.

use crate::i18n::{ntr, tr, trc, trf};

/// Помощники, которые понимают `--sudo`/`--sudoflags`/`--removemake` и
/// передают `--ask` самому pacman (проверено 03-04.10.2026 на yay 13.0.1 и
/// paru подставным `--sudo`, который записывает свою командную строку:
/// оба зовут `<sudo> pacman -U … --ask 4 …`).
/// Остальные (trizen, pikaur, aurman) зовутся как раньше — их флагов мы
/// не проверяли и наугад не подставляем.
pub const FLAG_AWARE_HELPERS: &[&str] = &["yay", "paru"];

/// argv установки пакетов из AUR.
///
/// * `--needed` — уже стоящее не пересобирается;
/// * `--noconfirm` — ни одного вопроса в никуда;
/// * yay: `--answerclean None --answerdiff None --answeredit None` — меню
///   чистки/показа изменений/правки сборочных файлов отвечены «ни одного»
///   (под `--noconfirm` yay их и так пропускает, но печатает пустые
///   вопросы; явный ответ не зависит от чужого `~/.config/yay/config.json`);
/// * paru: `--skipreview` — то же для paru (иначе он ждёт просмотра
///   PKGBUILD в пейджере и стоит вечно);
/// * `--removemake` — сборочные зависимости убрать после установки, иначе
///   каждая тема оставляла бы в системе компиляторы и cmake;
/// * `--sudo pkexec --sudoflags ""` — права графическим окном polkit
///   (терминала нет, см. шапку модуля). Пустые `--sudoflags` обязательны:
///   у человека в конфиге могут стоять флаги sudo, которых pkexec не знает.
///   В окне Rexon с помощником сеанса прав вместо pkexec — шим сеанса
///   ([`push_sudo`], пункт Б10);
/// * paru: `--nosudoloop` — «держать sudo живым» paru делает вызовом
///   `<sudo> -v`, а у pkexec такого ключа нет;
/// * `replace_conflicts` → `--ask=4`: pacman обращает ответ по умолчанию на
///   вопрос «X и Y конфликтуют. Удалить Y?» (бит 4 — конфликт пакетов) и
///   ставит замену. Только после явного согласия человека.
///   ⚠️ НЕ `--useask`: у yay 13 он до `pacman -U` собранных пакетов не
///   доходит (проверено подставным sudo 03.10.2026 — флага в строке нет).
pub fn install_argv(helper: &str, packages: &[String], replace_conflicts: bool) -> Vec<String> {
    let mut argv: Vec<String> = vec![
        helper.to_string(),
        "-S".to_string(),
        "--needed".to_string(),
        "--noconfirm".to_string(),
    ];
    let mut push = |items: &[&str]| argv.extend(items.iter().map(|s| s.to_string()));
    match helper {
        "yay" => push(&[
            "--answerclean",
            "None",
            "--answerdiff",
            "None",
            "--answeredit",
            "None",
        ]),
        "paru" => push(&["--skipreview", "--nosudoloop"]),
        _ => {}
    }
    if FLAG_AWARE_HELPERS.contains(&helper) {
        push(&["--removemake"]);
        push_sudo(&mut argv, helper, packages, replace_conflicts);
        if replace_conflicts {
            argv.push("--ask=4".to_string());
        }
    }
    argv.extend(packages.iter().cloned());
    argv
}

/// `--sudo`/`--sudoflags` сборщика (пункт Б10). В окне Rexon с помощником
/// сеанса прав — шим `rexon-sudo-shim` и метка новой операции
/// ([`crate::privilege::aur::sudo_for`]): вызовы pacman сборщика идут в
/// сеанс прав (один пароль), и пропускается только то, что относится к
/// сборке `packages`. Иначе — `pkexec` с пустыми флагами, как раньше.
///
/// paru с шимом получает ещё `--nochroot --nolocalrepo`: в чистом окружении
/// и в локальном репозитории paru зовёт через sudo не pacman
/// (`mkarchroot`, `arch-nspawn`, `install -d`), а такое шим не пропустит —
/// настройка человека в `paru.conf` уронила бы сборку.
fn push_sudo(argv: &mut Vec<String>, helper: &str, packages: &[String], replace: bool) {
    match crate::privilege::aur::sudo_for(packages, replace) {
        Some(sudo) => {
            if helper == "paru" {
                argv.extend(["--nochroot".to_string(), "--nolocalrepo".to_string()]);
            }
            argv.extend(["--sudo".to_string(), sudo.program, "--sudoflags".to_string(), sudo.flags]);
        }
        None => argv.extend(["--sudo", "pkexec", "--sudoflags", ""].iter().map(|s| s.to_string())),
    }
}

/// Флаги [`install_argv`], за которыми идёт значение, — чтобы разбор
/// командной строки не принял `pkexec` или `None` за имя пакета.
pub const VALUE_FLAGS: &[&str] =
    &["--answerclean", "--answerdiff", "--answeredit", "--sudo", "--sudoflags"];

/// Пакеты из командной строки [`install_argv`] (всё после `-S`, кроме
/// флагов и их значений).
pub fn argv_packages(argv: &[String]) -> Vec<String> {
    let Some(pos) = argv.iter().position(|a| a == "-S") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut skip_value = false;
    for a in &argv[pos + 1..] {
        if skip_value {
            skip_value = false;
        } else if a.starts_with('-') {
            skip_value = VALUE_FLAGS.contains(&a.as_str());
        } else {
            out.push(a.clone());
        }
    }
    out
}

/// Команда для терминала, которую можно показать человеку («попробуйте
/// сами»): без флагов неинтерактивности — в терминале он ответит сам.
pub fn terminal_command(helper: &str, packages: &[String]) -> String {
    let mut parts = vec![helper.to_string(), "-S".to_string(), "--needed".to_string()];
    parts.extend(packages.iter().cloned());
    parts.join(" ")
}

// ════════════════════════════════════════════════════════════════════════
// Комплекты: пакет-форк ставится вместе со своими частями и с вопросом
// ════════════════════════════════════════════════════════════════════════

/// Комплект пакетов AUR, который ставится одним заходом и только после
/// предупреждения.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AurKit {
    /// Имя комплекта для человека.
    pub title: &'static str,
    /// Пакеты, по которым комплект узнаётся (кнопка «Установить» на них
    /// ставит комплект целиком).
    pub triggers: &'static [&'static str],
    /// Состав в порядке инструкции авторов (INSTALL.md AeroThemePlasma,
    /// раздел «Arch Linux»): сначала форк libplasma, потом части оболочки
    /// AeroShell, потом звуки и значки, последней — сама оболочка. Помощник
    /// и сам строит порядок по зависимостям; явный список нужен, чтобы форк
    /// libplasma был выбран ИМЕННО этим пакетом, а не первым попавшимся
    /// поставщиком, и чтобы комплект был тем же, что у авторов.
    pub packages: &'static [&'static str],
    /// Что комплект ЗАМЕНЯЕТ в системе: (системный пакет, пакет форка).
    pub replaces: &'static [(&'static str, &'static str)],
    /// Ставит ли комплект отдельный сеанс на экран входа (имя сеанса).
    pub session: Option<&'static str>,
}

/// AeroThemePlasma — оболочка «как Windows 7» поверх KDE Plasma.
///
/// ⚠️ Это форк Plasma, а не тема: `aeroshell-libplasma-git` замещает
/// системную `libplasma` (Conflicts: libplasma), эффекты KWin собираются
/// против заголовков установленной Plasma и после её обновления требуют
/// пересборки (её делает [`crate::aeroshell`]).
pub const AEROTHEMEPLASMA: AurKit = AurKit {
    title: "AeroThemePlasma",
    triggers: &["aerothemeplasma-desktop-git"],
    packages: &[
        "aeroshell-libplasma-git",
        "aeroshell-workspace-git",
        "aeroshell-kwin-components-git",
        "aeroshell-smod-git",
        "aerothemeplasma-sounds-git",
        "aerothemeplasma-icons-git",
        "aerothemeplasma-desktop-git",
    ],
    replaces: &[("libplasma", "aeroshell-libplasma-git")],
    session: Some("AeroThemePlasma (Wayland)"),
};

/// AeroThemePlasma для сеанса X11: тот же комплект плюс части под
/// `kwin-x11` (их зависимости по AUR RPC 04.10.2026: desktop-x11 тянет
/// desktop, kwin-components-x11 и smodglow-x11).
pub const AEROTHEMEPLASMA_X11: AurKit = AurKit {
    title: "AeroThemePlasma (X11)",
    triggers: &["aerothemeplasma-desktop-x11-git"],
    packages: &[
        "aeroshell-libplasma-git",
        "aeroshell-workspace-git",
        "aeroshell-kwin-components-git",
        "aeroshell-smod-git",
        "aerothemeplasma-sounds-git",
        "aerothemeplasma-icons-git",
        "aerothemeplasma-desktop-git",
        "aeroshell-kwin-components-x11-git",
        "aeroshell-smodglow-x11-git",
        "aerothemeplasma-desktop-x11-git",
    ],
    replaces: &[("libplasma", "aeroshell-libplasma-git")],
    session: Some("AeroThemePlasma (X11)"),
};

/// Все комплекты. Новый — сюда (правило #12: одна таблица).
pub const KITS: &[AurKit] = &[AEROTHEMEPLASMA, AEROTHEMEPLASMA_X11];

/// Комплект, в который входит кнопка «Установить» на пакете `name`.
pub fn kit_for(name: &str) -> Option<&'static AurKit> {
    KITS.iter().find(|k| k.triggers.contains(&name))
}

/// Вопрос перед установкой: заголовок и текст.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallQuestion {
    pub title: String,
    pub text: String,
}

/// Что и как ставить по кнопке «Установить» на пакете AUR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    /// Пакеты в порядке установки.
    pub packages: Vec<String>,
    /// Разрешить pacman заменить конфликтующий системный пакет.
    pub replace_conflicts: bool,
    /// Спросить человека до начала (None — ставить сразу).
    pub question: Option<InstallQuestion>,
    /// Комплект, если это он.
    pub kit: Option<&'static AurKit>,
}

/// План установки пакета `name`. `installed` — стоит ли пакет в системе.
///
/// Обычный пакет — ставится один, без вопроса и без права заменять
/// системные пакеты. Пакет комплекта — весь комплект (уже стоящие части
/// пропускаются: помощник со `--needed` их не тронет, но и в список мы их
/// не тянем, чтобы вопрос называл правду), с вопросом; замена системного
/// пакета разрешается только если она действительно предстоит.
pub fn plan_install(name: &str, installed: &dyn Fn(&str) -> bool) -> InstallPlan {
    let Some(kit) = kit_for(name) else {
        return InstallPlan {
            packages: vec![name.to_string()],
            replace_conflicts: false,
            question: None,
            kit: None,
        };
    };
    let packages: Vec<String> =
        kit.packages.iter().filter(|p| !installed(p) || **p == name).map(|p| p.to_string()).collect();
    let replacing: Vec<&(&str, &str)> =
        kit.replaces.iter().filter(|(sys, fork)| !installed(fork) && installed(sys)).collect();
    InstallPlan {
        replace_conflicts: !replacing.is_empty(),
        question: Some(kit_question(kit, packages.len(), &replacing)),
        packages,
        kit: Some(kit),
    }
}

/// Предупреждение перед комплектом-форком.
fn kit_question(kit: &AurKit, count: usize, replacing: &[&(&str, &str)]) -> InstallQuestion {
    let mut text = trf(
        // TRANSLATORS: %1 is a theme name (AeroThemePlasma), %2 a number of packages.
        "%1 isn't an ordinary theme: it is a modified KDE Plasma (a fork). Rexon will build \
         it from source (packages: %2) — this takes 10 to 30 minutes, and the system will ask \
         for the administrator password a few times.",
        &[&kit.title, &count],
    );
    for (sys, fork) in replacing {
        text.push_str("\n\n• ");
        text.push_str(&trf(
            "The system package %1 will be replaced by %2.",
            &[sys, fork],
        ));
    }
    text.push_str("\n• ");
    text.push_str(&tr(
        "After every KDE Plasma update it has to be rebuilt — Rexon checks this at start-up \
         and offers the rebuild itself.",
    ));
    if let Some(session) = kit.session {
        text.push_str("\n• ");
        text.push_str(&trf(
            "To use it, log out and pick the “%1” session on the login screen.",
            &[&session],
        ));
    }
    text.push_str("\n\n");
    text.push_str(&tr("Continue?"));
    InstallQuestion {
        title: trf("Install %1", &[&kit.title]),
        text,
    }
}

/// Итог удачной установки комплекта: куда идти дальше.
pub fn kit_done_message(kit: &AurKit) -> String {
    match kit.session {
        Some(session) => trf(
            "“%1” is installed. Log out and pick the “%2” session on the login screen.",
            &[&kit.title, &session],
        ),
        None => trf("“%1” is installed", &[&kit.title]),
    }
}

// ════════════════════════════════════════════════════════════════════════
// Разбор вывода помощника: настоящая причина отказа
// ════════════════════════════════════════════════════════════════════════

/// Почему помощник AUR не поставил пакет.
///
/// Права различаются по тому, ЧТО человеку делать дальше (живой случай
/// 05.10.2026: три установки подряд → три окна пароля; агент KDE отказал
/// второму и третьему «Another client is already authenticating», pkexec
/// ответил «Not authorized», а Rexon написал «пароль не принят» — неправда,
/// пароля никто не вводил):
/// * [`AurFailure::WrongPassword`] — пароль введён, но неверный;
/// * [`AurFailure::PasswordDismissed`] — окно пароля закрыли (pkexec 126);
/// * [`AurFailure::NotAuthorized`] — права не выданы, а почему — система не
///   сказала (pkexec 127): окно закрыто, не дождалось или не открылось;
/// * [`AurFailure::PasswordWindowBusy`] — окно не открылось, потому что уже
///   было открыто другое (уточняется по журналу агента,
///   [`refine_with_agent_journal`]);
/// * [`AurFailure::NoPasswordAgent`] — окну пароля вообще неоткуда взяться.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AurFailure {
    /// sudo без терминала не смог спросить пароль.
    NoTerminalForPassword,
    /// Окно пароля закрыли / нажали «Отмена».
    PasswordDismissed,
    /// Права не выданы, причину система не назвала.
    NotAuthorized,
    /// Пароль введён неверно.
    WrongPassword,
    /// Окно пароля не открылось: уже было открыто другое.
    PasswordWindowBusy,
    /// Нет агента polkit — окну пароля неоткуда взяться.
    NoPasswordAgent,
    /// Собранные пакеты не установились, и pacman не назвал причину —
    /// команда повышения прав вернула ошибку молча.
    PrivilegeFailed,
    /// Пакет `new` конфликтует с установленным `old`.
    Conflict { new: String, old: String },
    /// Файл уже есть в системе (чужой или ничей).
    FileExists { path: String },
    /// Установка или удаление сломали бы чужую зависимость.
    BreaksDependency { line: String },
    /// Не скачался исходник.
    Download { what: String },
    /// Неизвестный ключ PGP у исходников.
    PgpKey,
    /// Контрольная сумма скачанного не сошлась.
    Checksum,
    /// Сборка пакета упала.
    Build { package: String },
    /// Зависимость не нашлась ни в репозиториях, ни в AUR.
    MissingDependency { name: String },
    /// Сборке `package` не хватило зависимостей `names`, которые к этому
    /// моменту должны были стоять (makepkg «Missing dependencies»): их
    /// ставила другая установка или они сами не встали.
    DependencyNotInstalled { package: String, names: Vec<String> },
    /// База пакетов занята другой установкой.
    DatabaseLocked,
    /// Кончилось место на диске.
    DiskFull,
}

/// Убрать цветовые escape-последовательности (`\x1b[1m` и т. п.) и
/// перерисовки строки прогресса (`\r`).
pub fn clean_line(raw: &str) -> String {
    let raw = raw.rsplit('\r').next().unwrap_or(raw);
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out.trim().to_string()
}

/// Значимые строки вывода (без пустых и без шума прогресса).
fn lines(log: &str) -> Vec<String> {
    log.lines().map(clean_line).filter(|l| !l.is_empty()).collect()
}

/// Текст между первой парой одинарных кавычек (`'…'`) либо None.
fn quoted(line: &str) -> Option<String> {
    let start = line.find('\'')? + 1;
    let end = line[start..].find('\'')? + start;
    Some(line[start..end].to_string())
}

/// Имя пакета без версии: `libplasma-6.7.4-1` → `libplasma`.
fn strip_version(spec: &str) -> String {
    let parts: Vec<&str> = spec.split('-').collect();
    // Версия pacman — это два последних куска «ver-rel», где ver
    // начинается с цифры (или с эпохи «N:»).
    if parts.len() >= 3 {
        let ver = parts[parts.len() - 2];
        if ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return parts[..parts.len() - 2].join("-");
        }
    }
    spec.to_string()
}

/// Найти настоящую причину отказа в выводе помощника.
///
/// Причины проверяются от первопричины к следствию: «нет пароля» важнее
/// «не удалось установить слой», конфликт важнее «не удалось подготовить
/// транзакцию». None — ничего узнаваемого (тогда показывается хвост).
pub fn failure_cause(log: &str) -> Option<AurFailure> {
    let lines = lines(log);
    let has = |needles: &[&str]| lines.iter().any(|l| needles.iter().any(|n| l.contains(n)));
    let find = |needles: &[&str]| {
        lines.iter().rev().find(|l| needles.iter().any(|n| l.contains(n))).cloned()
    };

    // ── Права ──
    if has(&["a terminal is required to read the password", "для ввода пароля требуется терминал", "no askpass program specified"]) {
        return Some(AurFailure::NoTerminalForPassword);
    }
    if has(&["No authentication agent found", "Агент аутентификации не найден"]) {
        return Some(AurFailure::NoPasswordAgent);
    }
    if has(&["Request dismissed", "Запрос отклонён", "Запрос отклонен"]) {
        return Some(AurFailure::PasswordDismissed);
    }
    // Неверный пароль называют текстовый агент polkit (pkttyagent:
    // «AUTHENTICATION FAILED», помощник PAM «Authentication failure») и
    // sudo («incorrect password attempt»). Графический агент сам в вывод
    // pkexec не пишет — его случай уточняет журнал агента.
    if has(&[
        "AUTHENTICATION FAILED",
        "ОШИБКА АУТЕНТИФИКАЦИИ",
        "pam_authenticate failed",
        "Authentication failure",
        "incorrect password attempt",
        "неверные попытки ввода пароля",
        "Sorry, try again",
    ]) {
        return Some(AurFailure::WrongPassword);
    }
    if has(&["Not authorized", "Не авторизован"]) {
        return Some(AurFailure::NotAuthorized);
    }
    // Слов pkexec нет, но код возврата его: 126 — окно закрыли, 127 —
    // права не выданы («error installing: […] - exit status 127»).
    if let Some(line) = find(&["error installing:", "ошибка установки:"]) {
        if line.ends_with("exit status 126") {
            return Some(AurFailure::PasswordDismissed);
        }
        if line.ends_with("exit status 127") {
            return Some(AurFailure::NotAuthorized);
        }
    }

    // ── Конфликты пакетов: «:: a-1-1 and b-2-1 are in conflict» ──
    // (строка pacman в русском каталоге не переведена — всегда английская).
    if let Some(line) = find(&[" are in conflict"]) {
        let body = line.trim_start_matches(':').trim();
        let body = body.split(" are in conflict").next().unwrap_or("");
        if let Some((a, b)) = body.split_once(" and ") {
            return Some(AurFailure::Conflict { new: strip_version(a.trim()), old: strip_version(b.trim()) });
        }
    }
    if let Some(line) = find(&[" exists in filesystem", " существует в файловой системе"]) {
        // «pkg: /usr/share/x exists in filesystem (owned by y)»
        let path = line
            .split_whitespace()
            .find(|w| w.trim_matches('\'').starts_with('/'))
            .map(|w| w.trim_matches('\'').to_string())
            .unwrap_or_default();
        return Some(AurFailure::FileExists { path });
    }
    if let Some(line) = find(&["breaks dependency", "нарушает зависимость"]) {
        return Some(AurFailure::BreaksDependency { line: line.trim_start_matches(':').trim().to_string() });
    }

    // ── Исходники ──
    if has(&["One or more PGP signatures could not be verified", "PGP-подписей не могут быть проверены", "unknown public key"]) {
        return Some(AurFailure::PgpKey);
    }
    if has(&["did not pass the validity check", "не прошли проверку целостности"]) {
        return Some(AurFailure::Checksum);
    }
    if let Some(line) = find(&[
        "Failure while downloading",
        "Ошибка при загрузке",
        "error downloading sources",
        "ошибка загрузки исходников",
        "Could not resolve host",
        "unable to access '",
        "failed retrieving file",
    ]) {
        let what = quoted(&line).unwrap_or_else(|| {
            line.rsplit(' ').next().unwrap_or("").trim_end_matches(['.', ':']).to_string()
        });
        return Some(AurFailure::Download { what });
    }

    // ── Зависимости ──
    // makepkg без `-s` (так его зовёт помощник): «==> Missing dependencies:»,
    // за ней строки «  -> имя», потом «==> ERROR: Could not resolve all
    // dependencies.» Так выглядела гонка 05.10: зависимость ставила
    // соседняя установка и не успела.
    if let Some(pos) = lines.iter().rposition(|l| {
        l.contains("Missing dependencies:") || l.contains("Отсутствуют зависимости:")
    }) {
        let names: Vec<String> = lines[pos + 1..]
            .iter()
            .take_while(|l| l.starts_with("->"))
            .map(|l| l.trim_start_matches("->").trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        let package = lines[..pos]
            .iter()
            .rev()
            .find(|l| l.contains("==> Making package:") || l.contains("==> Создание пакета:"))
            .and_then(|l| l.split_once(':').and_then(|(_, r)| r.split_whitespace().next()).map(String::from))
            .unwrap_or_default();
        return Some(AurFailure::DependencyNotInstalled { package, names });
    }
    if has(&["Could not resolve all dependencies", "Не удалось разрешить все зависимости"]) {
        return Some(AurFailure::DependencyNotInstalled { package: String::new(), names: Vec::new() });
    }
    if let Some(line) = find(&["unable to satisfy dependency", "не удается удовлетворить зависимость", "target not found:", "не найдена цель:", "No AUR package found for", "Не найден пакет AUR для"]) {
        let name = quoted(&line).unwrap_or_else(|| {
            line.rsplit([' ', ':']).find(|w| !w.is_empty()).unwrap_or("").to_string()
        });
        return Some(AurFailure::MissingDependency { name });
    }

    // ── Сборка ──
    if has(&["A failure occurred in", "Произошел сбой в"]) || has(&["error making:", "ошибка сборки:"]) {
        // Имя — из последнего «==> Making package: имя версия» (makepkg,
        // формат стабилен); запасной — первое слово после «error making:».
        let package = find(&["==> Making package:", "==> Создание пакета:"])
            .and_then(|l| {
                l.split_once(':').and_then(|(_, r)| r.split_whitespace().next()).map(String::from)
            })
            .or_else(|| {
                find(&["error making:", "ошибка сборки:"]).and_then(|l| {
                    l.split_once(':').and_then(|(_, r)| r.split_whitespace().next()).map(String::from)
                })
            })
            .unwrap_or_default();
        return Some(AurFailure::Build { package });
    }

    // ── Прочее ──
    if has(&["unable to lock database", "не удалось заблокировать базу"]) {
        return Some(AurFailure::DatabaseLocked);
    }
    if has(&["not enough free disk space", "No space left on device", "недостаточно места на диске"]) {
        return Some(AurFailure::DiskFull);
    }
    // Собранное не встало, а сам pacman ни на что не пожаловался — значит,
    // ему не дали прав: команда повышения прав вернула ошибку молча (так
    // выглядит и отказ подставного `--sudo /bin/false`). yay пишет
    // «error installing: [пакеты] - exit status N».
    if has(&["error installing:", "ошибка установки:"]) {
        return Some(AurFailure::PrivilegeFailed);
    }
    None
}

/// Причина отказа словами человека.
pub fn failure_text(cause: &AurFailure) -> String {
    match cause {
        AurFailure::NoTerminalForPassword => tr(
            "the system couldn't ask for the administrator password (sudo needs a terminal)",
        ),
        AurFailure::PasswordDismissed => tr("the password window was closed"),
        AurFailure::NotAuthorized => tr(
            "the system didn't grant administrator rights: the password window was closed, \
             didn't wait for the password or didn't open",
        ),
        AurFailure::WrongPassword => tr("the administrator password was wrong"),
        AurFailure::PasswordWindowBusy => tr(
            "the password window didn't open: another password window was already open",
        ),
        AurFailure::NoPasswordAgent => tr(
            "no password dialog is running (polkit agent), so the system couldn't ask for \
             the administrator password",
        ),
        AurFailure::PrivilegeFailed => tr(
            "the packages were built, but installing them needs administrator rights, and \
             they weren't granted",
        ),
        AurFailure::Conflict { new, old } => trf(
            "%1 conflicts with the installed %2 — it has to replace it",
            &[new, old],
        ),
        AurFailure::FileExists { path } if !path.is_empty() => trf(
            "the file %1 is already on the system and belongs to something else",
            &[path],
        ),
        AurFailure::FileExists { .. } => tr("some of its files are already on the system and belong to something else"),
        AurFailure::BreaksDependency { line } => trf("it would break another package: %1", &[line]),
        AurFailure::Download { what } if !what.is_empty() => trf(
            "the source code couldn't be downloaded (%1) — check the internet connection",
            &[what],
        ),
        AurFailure::Download { .. } => {
            tr("the source code couldn't be downloaded — check the internet connection")
        }
        AurFailure::PgpKey => tr(
            "the signature of the source code couldn't be checked: the author's PGP key is unknown",
        ),
        AurFailure::Checksum => tr("the downloaded source code failed the integrity check"),
        AurFailure::Build { package } if !package.is_empty() => {
            trf("building %1 from source failed", &[package])
        }
        AurFailure::Build { .. } => tr("building from source failed"),
        AurFailure::MissingDependency { name } if !name.is_empty() => trf(
            "the dependency %1 wasn't found in the repositories or the AUR",
            &[name],
        ),
        AurFailure::MissingDependency { .. } => {
            tr("a dependency wasn't found in the repositories or the AUR")
        }
        AurFailure::DependencyNotInstalled { package, names } => {
            let names = names.join(", ");
            match (package.is_empty(), names.is_empty()) {
                (false, false) => trf(
                    "%1 couldn't be built: its dependencies %2 weren't installed yet",
                    &[package, &names],
                ),
                (true, false) => trf("the dependencies %1 weren't installed yet", &[&names]),
                _ => tr("some dependencies weren't installed yet"),
            }
        }
        AurFailure::DatabaseLocked => tr("the package database is busy with another installation"),
        AurFailure::DiskFull => tr("there isn't enough free disk space"),
    }
}

/// Последние `n` значимых строк вывода — для окна хода установки.
pub fn tail_lines(log: &str, n: usize) -> Vec<String> {
    let all = lines(log);
    let from = all.len().saturating_sub(n);
    all[from..].to_vec()
}

/// Сколько строк хвоста показать при отказе.
pub const FAILURE_TAIL_LINES: usize = 8;

/// Итог отказа для человека: что не встало, почему и где полный журнал.
///
/// `log_path` — куда сохранён полный вывод помощника (пусто — не сохранён).
/// Для причин, которые лечатся только в терминале ответом «да» (конфликт,
/// файл в системе), к тексту приложена команда.
pub fn failure_message(name: &str, helper: &str, packages: &[String], log: &str, log_path: &str) -> String {
    failure_message_for(name, helper, packages, log, log_path, failure_cause(log).as_ref())
}

/// То же, что [`failure_message`], но с уже найденной (и, может быть,
/// уточнённой по журналу агента — [`refine_with_agent_journal`]) причиной.
pub fn failure_message_for(
    name: &str,
    helper: &str,
    packages: &[String],
    log: &str,
    log_path: &str,
    cause: Option<&AurFailure>,
) -> String {
    let mut msg = match cause {
        Some(c) => trf("Couldn't install “%1”: %2.", &[&name, &failure_text(c)]),
        None => match tail_lines(log, 1).first() {
            Some(last) => trf("Couldn't install “%1”. The installer's last words: %2", &[&name, last]),
            None => trf("Couldn't install “%1”.", &[&name]),
        },
    };
    if let Some(advice) = cause.and_then(failure_advice) {
        msg.push(' ');
        msg.push_str(&advice);
    }
    let needs_terminal = matches!(
        cause,
        None | Some(AurFailure::Conflict { .. })
            | Some(AurFailure::FileExists { .. })
            | Some(AurFailure::BreaksDependency { .. })
            | Some(AurFailure::PgpKey)
    );
    if needs_terminal {
        msg.push('\n');
        msg.push_str(&trf(
            "To decide it yourself, run in a terminal: %1",
            &[&terminal_command(helper, packages)],
        ));
    }
    if !log_path.is_empty() {
        msg.push('\n');
        msg.push_str(&trf("Full log: %1", &[&log_path]));
    }
    msg
}

/// Что делать человеку после отказа — одна фраза к причине (None — сказать
/// нечего сверх самой причины и команды для терминала).
pub fn failure_advice(cause: &AurFailure) -> Option<String> {
    Some(match cause {
        AurFailure::PasswordDismissed | AurFailure::NotAuthorized => {
            tr("Press “Retry” and enter your password in the window that opens.")
        }
        AurFailure::WrongPassword => {
            tr("Press “Retry” and enter the password of your user account again.")
        }
        AurFailure::PasswordWindowBusy => tr(
            "Finish or close the other password window (another program asked for it), \
             then press “Retry”.",
        ),
        AurFailure::NoPasswordAgent => tr(
            "Log out and back in; if that doesn't help, install the password dialog \
             (polkit agent) for your desktop.",
        ),
        AurFailure::NoTerminalForPassword | AurFailure::PrivilegeFailed => tr("Press “Retry”."),
        AurFailure::DependencyNotInstalled { .. } => tr(
            "Press “Retry”: Rexon installs one package at a time, so the dependencies \
             will be in place.",
        ),
        AurFailure::MissingDependency { .. } => tr(
            "The package may be outdated in the AUR — check its page for comments.",
        ),
        AurFailure::Build { .. } => tr(
            "This is usually a problem of the package itself — check its page in the AUR \
             for comments, or try again later.",
        ),
        AurFailure::Download { .. } => tr("Check the internet connection and press “Retry”."),
        AurFailure::Checksum => tr("Press “Retry” — the download may have been damaged."),
        AurFailure::DatabaseLocked => {
            tr("Wait until the other installation finishes and press “Retry”.")
        }
        AurFailure::DiskFull => tr("Free up some disk space and press “Retry”."),
        AurFailure::Conflict { .. }
        | AurFailure::FileExists { .. }
        | AurFailure::BreaksDependency { .. }
        | AurFailure::PgpKey => return None,
    })
}

/// Короткая причина для подписи на плитке (без имени пакета и советов).
/// Строчной буквой: на плитке она идёт после «Ошибка:», а первым словом
/// причины бывает имя пакета, которое заглавной искажалось бы.
pub fn failure_short(cause: Option<&AurFailure>) -> String {
    match cause {
        Some(c) => failure_text(c),
        None => tr("The installation failed — see the log"),
    }
}

/// Уточнить причину «права не выданы» по журналу системы за время
/// установки (`journalctl -o cat --since …`).
///
/// pkexec говорит одно и то же «Not authorized», когда окно пароля закрыли
/// и когда агент polkit его даже не открыл, потому что уже показывал
/// другое. Графический агент пишет настоящую причину только в свой журнал:
/// агент KDE — «Another client is already authenticating, please try again
/// later.», а неверный пароль виден по записи PAM «authentication failure»
/// службы `polkit-1` (замер 05.10.2026, KDE Plasma 6.7).
pub fn refine_with_agent_journal(cause: Option<AurFailure>, journal: &str) -> Option<AurFailure> {
    match cause {
        Some(AurFailure::NotAuthorized) | Some(AurFailure::PrivilegeFailed) => {
            if journal.contains("Another client is already authenticating") {
                Some(AurFailure::PasswordWindowBusy)
            } else if journal.lines().any(|l| {
                l.contains("polkit-1") && l.contains("authentication failure")
            }) {
                Some(AurFailure::WrongPassword)
            } else {
                cause
            }
        }
        other => other,
    }
}

// ════════════════════════════════════════════════════════════════════════
// Стадии установки: что сейчас делает помощник
// ════════════════════════════════════════════════════════════════════════

/// Где установка пакета AUR сейчас — для подписи на плитке и строки сверху.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AurStage {
    /// Ждёт своей очереди: в Rexon установки AUR идут строго по одной.
    Queued,
    /// Ждёт, пока чужая установка отпустит базу пакетов.
    WaitingLock,
    /// Скачивает сборочные файлы и исходники (и пакеты из репозиториев).
    Downloading,
    /// Собирает (makepkg).
    Building,
    /// Ставит собранное (pacman).
    Installing,
    /// Встало.
    Done,
    /// Не встало.
    Failed,
}

impl AurStage {
    /// Имя для обмена со страницей.
    pub fn as_str(self) -> &'static str {
        match self {
            AurStage::Queued => "queued",
            AurStage::WaitingLock => "waiting",
            AurStage::Downloading => "downloading",
            AurStage::Building => "building",
            AurStage::Installing => "installing",
            AurStage::Done => "done",
            AurStage::Failed => "failed",
        }
    }

    /// Обратно из [`AurStage::as_str`].
    pub fn parse(s: &str) -> Option<AurStage> {
        [
            AurStage::Queued,
            AurStage::WaitingLock,
            AurStage::Downloading,
            AurStage::Building,
            AurStage::Installing,
            AurStage::Done,
            AurStage::Failed,
        ]
        .into_iter()
        .find(|st| st.as_str() == s)
    }

    /// Ещё идёт (в очереди или в работе).
    pub fn active(self) -> bool {
        !matches!(self, AurStage::Done | AurStage::Failed)
    }

    /// Подпись на кнопке плитки.
    pub fn label(self) -> String {
        match self {
            AurStage::Queued => trc("AUR install stage", "In queue"),
            AurStage::WaitingLock => trc("AUR install stage", "Waiting…"),
            AurStage::Downloading => trc("AUR install stage", "Downloading…"),
            AurStage::Building => trc("AUR install stage", "Building…"),
            AurStage::Installing => trc("AUR install stage", "Installing…"),
            AurStage::Done => trc("AUR install stage", "Done"),
            AurStage::Failed => trc("AUR install stage", "Error"),
        }
    }

    /// Та же стадия строчными — для строки «пакет: сборка…».
    fn status_word(self) -> String {
        match self {
            AurStage::Queued => trc("AUR install status", "in queue"),
            AurStage::WaitingLock => {
                trc("AUR install status", "waiting for another installation to finish…")
            }
            AurStage::Downloading => trc("AUR install status", "downloading…"),
            AurStage::Building => trc("AUR install status", "building…"),
            AurStage::Installing => trc("AUR install status", "installing…"),
            AurStage::Done => trc("AUR install status", "installed"),
            AurStage::Failed => trc("AUR install status", "failed"),
        }
    }
}

/// Что строка вывода помощника говорит о ходе установки.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineStage {
    /// Новая стадия (None — стадия прежняя).
    pub stage: Option<AurStage>,
    /// Пакет, за который помощник взялся (None — прежний).
    pub package: Option<String>,
}

/// Стадия по строке вывода помощника (yay/paru → makepkg → pacman) и имя
/// пакета, если строка его называет. None — строка о ходе ничего не говорит.
///
/// Строки makepkg и pacman в русском каталоге частично переведены, поэтому
/// узнаются обе формы; помощник Rexon запускает под `LANG=C`.
pub fn stage_of_line(raw: &str) -> Option<LineStage> {
    let line = clean_line(raw);
    let after_colon = |l: &str| {
        l.split_once(':').and_then(|(_, r)| r.split_whitespace().next()).map(String::from)
    };
    // «==> Making package: имя версия (дата)» — makepkg взялся за очередной
    // пакет. Стадию строка не меняет: yay зовёт makepkg на один пакет до
    // трёх раз (скачать, собрать, упаковать), и что именно сейчас — скажут
    // следующие строки.
    if line.contains("==> Making package:") || line.contains("==> Создание пакета:") {
        return Some(LineStage { stage: None, package: after_colon(&line) });
    }
    const DOWNLOADING: &[&str] = &[
        "==> Retrieving sources",
        "==> Получение исходных",
        "Downloaded PKGBUILD",
        "Downloading PKGBUILDs",
        ":: Retrieving packages",
        ":: Получение пакетов",
        "-> Downloading",
        "-> Cloning",
        "-> Updating",
    ];
    const BUILDING: &[&str] = &[
        "==> Extracting sources",
        "==> Извлечение исходных",
        "==> Starting prepare()",
        "==> Starting build()",
        "==> Starting check()",
        "==> Starting package()",
        "==> Entering fakeroot",
        "==> Запуск prepare()",
        "==> Запуск build()",
        "==> Запуск package()",
    ];
    const INSTALLING: &[&str] = &[
        "loading packages...",
        "загрузка пакетов...",
        "resolving dependencies...",
        "разрешение зависимостей...",
        "checking keyring...",
        "проверка связки ключей...",
        ":: Processing package changes",
        ":: Обработка изменений пакета",
        ":: Running pre-transaction hooks",
        ":: Запуск хуков перед транзакцией",
    ];
    let stage = |st| Some(LineStage { stage: Some(st), package: None });
    if DOWNLOADING.iter().any(|m| line.contains(m)) {
        return stage(AurStage::Downloading);
    }
    if BUILDING.iter().any(|m| line.contains(m)) {
        return stage(AurStage::Building);
    }
    if INSTALLING.iter().any(|m| line.contains(m)) {
        return stage(AurStage::Installing);
    }
    // «installing имя...» — pacman ставит конкретный пакет.
    if let Some(rest) = line.strip_prefix("installing ").or_else(|| line.strip_prefix("установка ")) {
        let name = rest.trim_end_matches("...").trim().to_string();
        return Some(LineStage {
            stage: Some(AurStage::Installing),
            package: (!name.is_empty()).then_some(name),
        });
    }
    None
}

/// Строка состояния сверху: «aerothemeplasma-desktop-git: сборка…».
///
/// `current` — пакет, над которым помощник работает сейчас (у комплекта —
/// его часть), `queued` — сколько установок ждут после этой.
pub fn stage_status_line(name: &str, current: &str, stage: AurStage, queued: usize) -> String {
    let who = if current.is_empty() || current == name {
        name.to_string()
    } else {
        // TRANSLATORS: %1 is the package being built right now, %2 the
        // package (or theme kit) the person asked to install.
        trf("%1 (for %2)", &[&current, &name])
    };
    let mut line = format!("{who}: {}", stage.status_word());
    if queued > 0 {
        line.push(' ');
        line.push_str(&ntr("(%n more in queue)", "(%n more in queue)", queued as u64));
    }
    line
}

/// Плитка пакета AUR, у которого есть установка в очереди этой сессии.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardJobState {
    /// Подпись кнопки.
    pub button_text: String,
    /// Кнопку можно нажать («Повторить» — можно, «Сборка…» — нет).
    pub button_enabled: bool,
    /// Вид кнопки для `appearance_page::card_button_colors`: «true» —
    /// контур состояния, «false» — залитая кнопка-действие.
    pub button_state: &'static str,
    /// Подсказка к кнопке.
    pub button_tooltip: String,
    /// Строка под кнопками (причина отказа), пусто — строки нет.
    pub note: String,
    /// Подсказка к строке — полный итог с советом и путём журнала.
    pub note_tooltip: String,
    /// Подпись ссылки «Журнал» (пусто — ссылки нет).
    pub log_text: String,
}

/// Что показать на плитке по стадии установки. `short` — короткая причина
/// отказа, `message` — полный итог.
pub fn card_job_state(stage: AurStage, short: &str, message: &str) -> CardJobState {
    match stage {
        AurStage::Failed => CardJobState {
            button_text: trc("AUR install", "Retry"),
            button_enabled: true,
            button_state: "false",
            button_tooltip: message.to_string(),
            note: if short.is_empty() {
                stage.label()
            } else {
                format!("{}: {short}", stage.label())
            },
            note_tooltip: message.to_string(),
            log_text: trc("AUR install", "Log"),
        },
        AurStage::Done => CardJobState {
            button_text: format!("✅ {}", stage.label()),
            button_enabled: false,
            button_state: "true",
            button_tooltip: message.to_string(),
            note: String::new(),
            note_tooltip: String::new(),
            log_text: String::new(),
        },
        _ => CardJobState {
            button_text: stage.label(),
            button_enabled: false,
            button_state: "true",
            button_tooltip: if stage == AurStage::Queued {
                tr("Rexon installs AUR packages one at a time — this one starts when the previous one finishes.")
            } else {
                String::new()
            },
            note: String::new(),
            note_tooltip: String::new(),
            log_text: String::new(),
        },
    }
}

// ════════════════════════════════════════════════════════════════════════
// Очередь: установки AUR строго по одной на всё приложение
// ════════════════════════════════════════════════════════════════════════
//
// Живой случай 05.10.2026: три «Установить» подряд запустили три помощника
// разом. Окон пароля стало три, агент polkit два из них отклонил, а сборка
// одного комплекта шла раньше, чем соседний процесс поставил её
// зависимость. Поэтому — одна очередь, и в ней по одной установке.
//
// ⚠️ Подряд нажатые НЕ сливаются в один вызов помощника. Пароль это не
// сократило бы до одного: yay просит права на каждый `pacman -U` слоя
// сборки и ещё раз на `pacman -D --asexplicit` (видно в журнале 05.10 —
// пять окон на один комплект). А отказ одного пакета в общем вызове
// обрывал бы и все остальные, и стадию на плитках было бы не разделить.

/// Одна установка в очереди.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AurJob {
    /// Пакет, на котором нажали «Установить».
    pub name: String,
    /// Что ставить (у комплекта — весь комплект).
    pub plan: InstallPlan,
    /// Каким помощником.
    pub helper: String,
    pub stage: AurStage,
    /// Пакет, над которым помощник работает сейчас.
    pub current: String,
    /// Итог для человека (после конца).
    pub message: String,
    /// Короткая причина отказа — для подписи на плитке.
    pub short: String,
    /// Полный журнал помощника.
    pub log_path: String,
}

/// Очередь установок AUR. Чистое состояние: запуск и потоки — у
/// вызывающего; здесь только решения «что дальше» и «надо ли будить
/// исполнителя».
#[derive(Debug, Default, Clone)]
pub struct AurQueue {
    jobs: Vec<AurJob>,
    /// Исполнитель работает (поток жив и сам заберёт следующую).
    worker_busy: bool,
}

/// Ответ [`AurQueue::enqueue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Enqueued {
    /// Установка встала в очередь (false — она уже там или идёт).
    pub added: bool,
    /// Исполнителя нет — его надо запустить.
    pub start_worker: bool,
}

impl AurQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Поставить установку в очередь. Пакет, который уже ждёт или
    /// ставится, второй раз не встаёт; закончившийся (в том числе с
    /// ошибкой — «Повторить») встаёт заново в конец.
    pub fn enqueue(&mut self, name: &str, plan: InstallPlan, helper: &str, log_path: &str) -> Enqueued {
        if self.jobs.iter().any(|j| j.name == name && j.stage.active()) {
            return Enqueued { added: false, start_worker: false };
        }
        self.jobs.retain(|j| j.name != name);
        self.jobs.push(AurJob {
            name: name.to_string(),
            plan,
            helper: helper.to_string(),
            stage: AurStage::Queued,
            current: String::new(),
            message: String::new(),
            short: String::new(),
            log_path: log_path.to_string(),
        });
        let start_worker = !self.worker_busy;
        self.worker_busy = true;
        Enqueued { added: true, start_worker }
    }

    /// Исполнитель берёт следующую. None — очередь пуста, и исполнитель
    /// обязан завершиться (флаг снимается здесь же, под тем же замком, —
    /// иначе установка, вставшая между «пусто» и выходом, осталась бы
    /// ждать вечно).
    pub fn take_next(&mut self) -> Option<AurJob> {
        match self.jobs.iter_mut().find(|j| j.stage == AurStage::Queued) {
            Some(job) => {
                job.stage = AurStage::WaitingLock;
                Some(job.clone())
            }
            None => {
                self.worker_busy = false;
                None
            }
        }
    }

    /// Новая стадия идущей установки. true — что-то изменилось.
    pub fn set_stage(&mut self, name: &str, stage: Option<AurStage>, current: Option<&str>) -> bool {
        let Some(job) = self.jobs.iter_mut().find(|j| j.name == name && j.stage.active()) else {
            return false;
        };
        let mut changed = false;
        if let Some(stage) = stage.filter(|st| st.active() && *st != job.stage) {
            job.stage = stage;
            changed = true;
        }
        if let Some(cur) = current {
            if job.current != cur {
                job.current = cur.to_string();
                changed = true;
            }
        }
        changed
    }

    /// Установка кончилась.
    pub fn finish(&mut self, name: &str, ok: bool, message: &str, short: &str) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.name == name && j.stage.active()) {
            job.stage = if ok { AurStage::Done } else { AurStage::Failed };
            job.current.clear();
            job.message = message.to_string();
            job.short = short.to_string();
        }
    }

    /// Все установки — идущие, ждущие и закончившиеся за сессию.
    pub fn jobs(&self) -> &[AurJob] {
        &self.jobs
    }

    /// Строка состояния сверху: идущая установка с именем пакета; пусто —
    /// ничего не идёт.
    pub fn status_line(&self) -> String {
        let Some(job) = self.jobs.iter().find(|j| j.stage.active() && j.stage != AurStage::Queued) else {
            return String::new();
        };
        let queued = self.jobs.iter().filter(|j| j.stage == AurStage::Queued).count();
        stage_status_line(&job.name, &job.current, job.stage, queued)
    }

    /// Состояние для страницы: `{"jobs": [...], "status": "..."}`.
    ///
    /// У идущей установки комплекта в `packages` — весь комплект: плитка
    /// его части показывает ту же стадию.
    pub fn snapshot_json(&self) -> String {
        let jobs: Vec<serde_json::Value> = self
            .jobs
            .iter()
            .map(|j| {
                serde_json::json!({
                    "name": j.name,
                    "packages": j.plan.packages,
                    "stage": j.stage.as_str(),
                    "label": j.stage.label(),
                    "current": j.current,
                    "message": j.message,
                    "short": j.short,
                    "logPath": j.log_path,
                })
            })
            .collect();
        serde_json::json!({ "jobs": jobs, "status": self.status_line() }).to_string()
    }
}

/// Куда сохранить полный вывод помощника: `aur/<пакет>.log` в кеше
/// «Оформления» (`~/.cache/rexon`, [`crate::appearance_cache::cache_dir`]).
pub fn log_path(home: &str, name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_.+@".contains(c) { c } else { '_' })
        .collect();
    format!("{}/aur/{safe}.log", crate::appearance_cache::cache_dir(home.trim_end_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn yay_argv_asks_password_graphically_and_answers_everything() {
        let argv = install_argv("yay", &s(&["foo"]), false);
        assert_eq!(
            argv,
            s(&[
                "yay", "-S", "--needed", "--noconfirm", "--answerclean", "None", "--answerdiff",
                "None", "--answeredit", "None", "--removemake", "--sudo", "pkexec", "--sudoflags",
                "", "foo"
            ])
        );
        assert!(!argv.iter().any(|a| a.starts_with("--ask")));
    }

    #[test]
    fn paru_argv_skips_review_and_sudoloop() {
        let argv = install_argv("paru", &s(&["a", "b"]), true);
        assert_eq!(
            argv,
            s(&[
                "paru", "-S", "--needed", "--noconfirm", "--skipreview", "--nosudoloop",
                "--removemake", "--sudo", "pkexec", "--sudoflags", "", "--ask=4", "a", "b"
            ])
        );
    }

    #[test]
    fn argv_packages_skips_flag_values() {
        let argv = install_argv("yay", &s(&["a", "b"]), true);
        assert_eq!(argv_packages(&argv), s(&["a", "b"]));
        let argv = install_argv("paru", &s(&["c"]), false);
        assert_eq!(argv_packages(&argv), s(&["c"]));
    }

    #[test]
    fn unknown_helper_keeps_plain_argv() {
        assert_eq!(
            install_argv("pikaur", &s(&["x"]), true),
            s(&["pikaur", "-S", "--needed", "--noconfirm", "x"])
        );
    }

    #[test]
    fn plain_package_has_no_question_and_no_replacement() {
        let plan = plan_install("bibata-cursor-theme", &|_| false);
        assert_eq!(plan.packages, s(&["bibata-cursor-theme"]));
        assert!(plan.question.is_none() && !plan.replace_conflicts && plan.kit.is_none());
    }

    #[test]
    fn aero_kit_in_authors_order_with_warning() {
        // Чистая Plasma: libplasma системная, форка нет.
        let plan = plan_install("aerothemeplasma-desktop-git", &|p| p == "libplasma");
        assert_eq!(plan.packages, AEROTHEMEPLASMA.packages.iter().map(|p| p.to_string()).collect::<Vec<_>>());
        assert_eq!(plan.packages.first().unwrap(), "aeroshell-libplasma-git");
        assert_eq!(plan.packages.last().unwrap(), "aerothemeplasma-desktop-git");
        assert!(plan.replace_conflicts);
        let q = plan.question.unwrap();
        assert!(q.text.contains("libplasma") && q.text.contains("aeroshell-libplasma-git"), "{}", q.text);
        assert!(q.text.contains("AeroThemePlasma"));
    }

    #[test]
    fn aero_kit_on_machine_with_fork_already_installed() {
        // Как у владельца 03.10: форк libplasma уже стоит — замены не будет.
        let plan = plan_install("aerothemeplasma-desktop-git", &|p| p == "aeroshell-libplasma-git");
        assert!(!plan.packages.contains(&"aeroshell-libplasma-git".to_string()));
        assert_eq!(plan.packages.len(), 6);
        assert!(!plan.replace_conflicts);
        assert!(!plan.question.unwrap().text.contains("will be replaced"));
    }

    #[test]
    fn x11_variant_is_a_kit_too() {
        let plan = plan_install("aerothemeplasma-desktop-x11-git", &|p| p == "libplasma");
        assert_eq!(plan.packages.last().unwrap(), "aerothemeplasma-desktop-x11-git");
        assert!(plan.packages.contains(&"aerothemeplasma-desktop-git".to_string()));
        assert!(plan.replace_conflicts && plan.question.is_some());
    }

    #[test]
    fn icons_alone_are_not_a_kit() {
        assert!(kit_for("aerothemeplasma-icons-git").is_none());
        assert!(kit_for("aerothemeplasma-sounds-git").is_none());
    }

    #[test]
    fn cause_sudo_without_terminal() {
        let log = "==> Finished making: aeroshell-smod-git\n\
                   sudo: a terminal is required to read the password; either use the -S option to read from standard input or configure an askpass helper\n\
                   sudo: a password is required\n -> error installing: [/usr/bin/sudo pacman -U ...] - exit status 1\n";
        assert_eq!(failure_cause(log), Some(AurFailure::NoTerminalForPassword));
    }

    #[test]
    fn cause_sudo_without_terminal_russian() {
        let log = "sudo: для ввода пароля требуется терминал; добавьте параметр -S\n";
        assert_eq!(failure_cause(log), Some(AurFailure::NoTerminalForPassword));
    }

    /// Хвост настоящего сухого прогона 03.10.2026 (`yay -S --sudo /bin/false
    /// aerothemeplasma-desktop-git`): пять зависимостей собраны, установка
    /// слоя отказала — ровно то, что было у владельца.
    #[test]
    fn cause_dry_run_install_layer_refused() {
        let log = "==> Sources are ready.\n -> error installing: [/b/aeroshell-smod-git/aeroshell-smod-git-6.7.4_66.rf58780d-1-x86_64.pkg.tar.zst /b/aerothemeplasma-icons-git/aerothemeplasma-icons-git-13.r0100536-1-any.pkg.tar.zst] - exit status 1\n\
                   error installing: [/b/x.pkg.tar.zst] - exit status 1\n";
        assert_eq!(failure_cause(log), Some(AurFailure::PrivilegeFailed));
    }

    #[test]
    fn cause_pkexec_dismissed() {
        let log = "Error executing command as another user: Request dismissed\n";
        assert_eq!(failure_cause(log), Some(AurFailure::PasswordDismissed));
    }

    #[test]
    fn cause_conflict_names_both_packages() {
        let log = "\x1b[1;33m:: \x1b[0maeroshell-libplasma-git-6.7.4_18049.rda45ff7-1 and libplasma-6.7.4-1 are in conflict\n\
                   error: failed to prepare transaction (conflicting dependencies)\n";
        assert_eq!(
            failure_cause(log),
            Some(AurFailure::Conflict { new: "aeroshell-libplasma-git".into(), old: "libplasma".into() })
        );
        let msg = failure_message("aerothemeplasma-desktop-git", "yay", &s(&["aerothemeplasma-desktop-git"]), log, "/h/.cache/rexon/aur/x.log");
        assert!(msg.contains("aeroshell-libplasma-git conflicts with the installed libplasma"), "{msg}");
        assert!(msg.contains("yay -S --needed aerothemeplasma-desktop-git"), "{msg}");
        assert!(msg.contains("/h/.cache/rexon/aur/x.log"), "{msg}");
    }

    #[test]
    fn cause_download() {
        let log = "==> ERROR: Failure while downloading foo-git git repo\nAborting...\n";
        assert!(matches!(failure_cause(log), Some(AurFailure::Download { .. })));
        let log = "fatal: unable to access 'https://gitgud.io/x.git/': Could not resolve host: gitgud.io\n";
        assert_eq!(failure_cause(log), Some(AurFailure::Download { what: "https://gitgud.io/x.git/".into() }));
    }

    #[test]
    fn cause_build_failure_names_package() {
        let log = "==> Making package: aeroshell-smod-git 6.7.4-1 (Sat)\nmake: *** [all] Error 2\n\
                   ==> ERROR: A failure occurred in build().\n    Aborting...\n -> error making: aeroshell-smod-git\n";
        assert_eq!(failure_cause(log), Some(AurFailure::Build { package: "aeroshell-smod-git".into() }));
    }

    #[test]
    fn cause_missing_dependency() {
        let log = "error: target not found: qt5-foo\n";
        assert_eq!(failure_cause(log), Some(AurFailure::MissingDependency { name: "qt5-foo".into() }));
    }

    #[test]
    fn unknown_failure_quotes_last_line() {
        let msg = failure_message("x", "yay", &s(&["x"]), "something odd\nweird end\n", "");
        assert!(msg.contains("weird end"), "{msg}");
        assert!(msg.contains("yay -S --needed x"), "{msg}");
    }

    #[test]
    fn clean_line_strips_colors_and_progress() {
        assert_eq!(clean_line("\x1b[1m(1/6) Downloaded\x1b[0m"), "(1/6) Downloaded");
        assert_eq!(clean_line("Updating 1%\rUpdating 100%, done."), "Updating 100%, done.");
    }

    /// Хвост журнала владельца 05.10.2026 (aerothemeplasma-icons-git):
    /// агент KDE уже показывал окно другой установке, pkexec ответил «Not
    /// authorized» с кодом 127. Пароля никто не вводил — «пароль не принят»
    /// было неправдой.
    const OWNER_ICONS_TAIL: &str = "==> Sources are ready.\n -> /var/lib/pacman/db.lck is present.\n\
         -> There may be another Pacman instance running. Waiting...\n\
        Error executing command as another user: Not authorized\n\nThis incident has been reported.\n\
         -> error installing: [/home/user/.cache/yay/aerothemeplasma-icons-git/aerothemeplasma-icons-git-13.r0100536-1-any.pkg.tar.zst] - exit status 127\n";

    /// Тот же отказ в журнале системы за время установки (journalctl -o cat).
    const OWNER_AGENT_JOURNAL: &str = "Initiating authentication\n\
        Another client is already authenticating, please try again later.\n\
        Operator of unix-session:2 FAILED to authenticate to gain authorization for action org.freedesktop.policykit.exec\n";

    #[test]
    fn owner_case_not_authorized_is_not_wrong_password() {
        let cause = failure_cause(OWNER_ICONS_TAIL);
        assert_eq!(cause, Some(AurFailure::NotAuthorized));
        assert!(!failure_text(&AurFailure::NotAuthorized).contains("not accepted"));
        // По журналу агента — точная причина: окно было занято другим.
        let refined = refine_with_agent_journal(cause, OWNER_AGENT_JOURNAL);
        assert_eq!(refined, Some(AurFailure::PasswordWindowBusy));
        let msg = failure_message_for("aerothemeplasma-icons-git", "yay", &s(&["aerothemeplasma-icons-git"]), OWNER_ICONS_TAIL, "/l", refined.as_ref());
        assert!(msg.contains("another password window was already open"), "{msg}");
        assert!(msg.contains("“Retry”"), "{msg}");
    }

    #[test]
    fn journal_refines_wrong_password_only_for_unclear_refusals() {
        let journal = "pam_unix(polkit-1:auth): authentication failure; logname= uid=1000\n";
        assert_eq!(refine_with_agent_journal(Some(AurFailure::NotAuthorized), journal), Some(AurFailure::WrongPassword));
        // Ясная причина журналом не перебивается.
        assert_eq!(
            refine_with_agent_journal(Some(AurFailure::PasswordDismissed), OWNER_AGENT_JOURNAL),
            Some(AurFailure::PasswordDismissed)
        );
        // Пустой журнал — причина та же.
        assert_eq!(refine_with_agent_journal(Some(AurFailure::NotAuthorized), ""), Some(AurFailure::NotAuthorized));
    }

    #[test]
    fn pkexec_exit_codes_without_words() {
        let log = " -> error installing: [/b/x.pkg.tar.zst] - exit status 126\n";
        assert_eq!(failure_cause(log), Some(AurFailure::PasswordDismissed));
        let log = " -> error installing: [/b/x.pkg.tar.zst] - exit status 127\n";
        assert_eq!(failure_cause(log), Some(AurFailure::NotAuthorized));
        // Иной код — права отказали молча, как раньше.
        let log = " -> error installing: [/b/x.pkg.tar.zst] - exit status 1\n";
        assert_eq!(failure_cause(log), Some(AurFailure::PrivilegeFailed));
    }

    #[test]
    fn wrong_password_from_text_agent() {
        let log = "polkit-agent-helper-1: pam_authenticate failed: Authentication failure\n==== AUTHENTICATION FAILED ====\nError executing command as another user: Not authorized\n";
        assert_eq!(failure_cause(log), Some(AurFailure::WrongPassword));
    }

    #[test]
    fn no_polkit_agent() {
        let log = "Error executing command as another user: No authentication agent found.\n";
        assert_eq!(failure_cause(log), Some(AurFailure::NoPasswordAgent));
        assert!(failure_advice(&AurFailure::NoPasswordAgent).is_some());
    }

    /// Гонка 05.10: часть комплекта собиралась раньше, чем соседний процесс
    /// поставил форк libplasma.
    #[test]
    fn missing_dependencies_race() {
        let log = "==> Making package: aeroshell-workspace-git 6.7.5_22.rad919f4-1 (Mon Oct  5 02:10:01 2026)\n\
                   ==> Checking runtime dependencies...\n==> Missing dependencies:\n  -> aeroshell-libplasma-git\n\
                   ==> Checking buildtime dependencies...\n==> ERROR: Could not resolve all dependencies.\n\
                    -> error making: aeroshell-workspace-git-exit status 8\n";
        let cause = failure_cause(log);
        assert_eq!(
            cause,
            Some(AurFailure::DependencyNotInstalled {
                package: "aeroshell-workspace-git".into(),
                names: vec!["aeroshell-libplasma-git".into()]
            })
        );
        let text = failure_text(cause.as_ref().unwrap());
        assert!(text.contains("aeroshell-workspace-git") && text.contains("aeroshell-libplasma-git"), "{text}");
        assert!(failure_advice(cause.as_ref().unwrap()).unwrap().contains("one package at a time"));
    }

    #[test]
    fn conflict_and_build_keep_their_causes() {
        let log = ":: a-1-1 and b-2-1 are in conflict\n";
        assert!(matches!(failure_cause(log), Some(AurFailure::Conflict { .. })));
        assert!(failure_advice(&AurFailure::Conflict { new: "a".into(), old: "b".into() }).is_none());
        let short = failure_short(Some(&AurFailure::PasswordDismissed));
        assert_eq!(short, "the password window was closed");
        assert_eq!(failure_short(None), "The installation failed — see the log");
    }

    #[test]
    fn stages_from_real_output() {
        let st = |l: &str| stage_of_line(l).and_then(|x| x.stage);
        let pkg = |l: &str| stage_of_line(l).and_then(|x| x.package);
        assert_eq!(pkg("==> Making package: aerothemeplasma-icons-git 8.rb8d5ce1-1 (Mon Oct  5 02:08:52 2026)").as_deref(), Some("aerothemeplasma-icons-git"));
        assert_eq!(st("==> Making package: x 1-1"), None);
        assert_eq!(st("==> Retrieving sources..."), Some(AurStage::Downloading));
        assert_eq!(st("  -> Updating aerothemeplasma-icons-git git repo..."), Some(AurStage::Downloading));
        assert_eq!(st("\x1b[1m:: (1/3) Downloaded PKGBUILD: x\x1b[0m"), Some(AurStage::Downloading));
        assert_eq!(st("==> Extracting sources..."), Some(AurStage::Building));
        assert_eq!(st("==> Starting build()..."), Some(AurStage::Building));
        assert_eq!(st("loading packages..."), Some(AurStage::Installing));
        assert_eq!(st(":: Running pre-transaction hooks..."), Some(AurStage::Installing));
        assert_eq!(st(":: Processing package changes..."), Some(AurStage::Installing));
        assert_eq!(pkg("installing aerothemeplasma-sounds-git...").as_deref(), Some("aerothemeplasma-sounds-git"));
        assert_eq!(stage_of_line("Updating files:  43% (3522/8167)"), None);
        assert_eq!(stage_of_line("cc1plus: warning: x"), None);
    }

    #[test]
    fn status_line_names_package() {
        let line = stage_status_line("aerothemeplasma-desktop-git", "", AurStage::Building, 0);
        assert_eq!(line, "aerothemeplasma-desktop-git: building…");
        let line = stage_status_line("aerothemeplasma-desktop-git", "aeroshell-workspace-git", AurStage::Installing, 2);
        assert_eq!(line, "aeroshell-workspace-git (for aerothemeplasma-desktop-git): installing… (2 more in queue)");
    }

    fn plain(name: &str) -> InstallPlan {
        plan_install(name, &|_| false)
    }

    /// Три «Установить» подряд: исполнитель один, остальные ждут.
    #[test]
    fn queue_runs_one_at_a_time() {
        let mut q = AurQueue::new();
        let a = q.enqueue("a", plain("a"), "yay", "/la");
        assert_eq!(a, Enqueued { added: true, start_worker: true });
        let b = q.enqueue("b", plain("b"), "yay", "/lb");
        assert_eq!(b, Enqueued { added: true, start_worker: false });
        // Повторное нажатие на ждущий пакет ничего не добавляет.
        assert!(!q.enqueue("b", plain("b"), "yay", "/lb").added);
        let first = q.take_next().unwrap();
        assert_eq!(first.name, "a");
        assert_eq!(q.jobs()[1].stage, AurStage::Queued);
        assert!(q.set_stage("a", Some(AurStage::Building), Some("a")));
        assert!(!q.set_stage("a", Some(AurStage::Building), Some("a")));
        assert_eq!(q.status_line(), "a: building… (1 more in queue)");
        q.finish("a", false, "msg", "short");
        assert_eq!(q.jobs()[0].stage, AurStage::Failed);
        // Нажатие во время работы исполнителя его второй раз не запускает.
        let c = q.enqueue("c", plain("c"), "yay", "/lc");
        assert!(c.added && !c.start_worker);
        assert_eq!(q.take_next().unwrap().name, "b");
        q.finish("b", true, "ok", "");
        assert_eq!(q.take_next().unwrap().name, "c");
        q.finish("c", true, "ok", "");
        assert!(q.take_next().is_none());
        // Исполнитель вышел — следующее нажатие обязано его запустить.
        let again = q.enqueue("a", plain("a"), "yay", "/la");
        assert_eq!(again, Enqueued { added: true, start_worker: true });
        assert_eq!(q.jobs().iter().filter(|j| j.name == "a").count(), 1);
        let snap: serde_json::Value = serde_json::from_str(&q.snapshot_json()).unwrap();
        assert_eq!(snap["jobs"].as_array().unwrap().len(), 3);
        assert_eq!(snap["jobs"][2]["stage"], "queued");
    }

    #[test]
    fn log_path_is_in_cache() {
        assert_eq!(log_path("/home/a/", "aero-git"), "/home/a/.cache/rexon/aur/aero-git.log");
        assert_eq!(log_path("/home/a", "../x"), "/home/a/.cache/rexon/aur/.._x.log");
    }
}
