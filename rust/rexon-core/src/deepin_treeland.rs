//! «Deepin на Treeland (экспериментально)» — ЕДИНСТВЕННЫЙ Deepin, который
//! ставит Rexon (решение Wehrwolfmann 26.10.2026), на Arch и производных.
//!
//! ЗАЧЕМ. Обычный набор Deepin тянет сеанс X11: `deepin-session` жёстко
//! зависит от `deepin-kwin` — форка KWin, который объявляет
//! `Conflicts With: kwin kwin-x11` (сотни общих файлов) и рядом с KDE
//! Plasma не встаёт. Но `deepin-kwin` нужен ТОЛЬКО сеансу X11: его
//! запускает лишь `dde-session@x11.service` (условие
//! `XDG_SESSION_TYPE=x11`). В сеансе Treeland (Wayland-композитор Deepin)
//! он не запускается вовсе. Поэтому Rexon ставит Deepin одним путём — с
//! Plasma на машине и без неё: собирает `deepin-session` из официального
//! рецепта Arch под своим именем [`LOCAL_PACKAGE`] — с `deepin-kwin` в
//! необязательных зависимостях и без пункта X11 в экране входа — и ставит
//! набор Deepin без `deepin-kwin`. Обычный Deepin (X11) Rexon не ставит ни
//! при каких условиях. Вход — пункт «Treeland User»
//! (`treeland-user.desktop`) в экране входа, который уже стоит (SDDM,
//! Plasma Login Manager): свой экран входа Deepin (`ddm`) ставится, потому
//! что без него не ставится `treeland`, но НЕ включается.
//!
//! УЖЕ СТОИТ ОБЫЧНЫЙ DEEPIN (`deepin-session` из репозитория). Rexon его
//! сам не удаляет и «Починить» его не переустанавливает (отказ до пароля,
//! [`install_verdict`]); на карточке — предложение перейти на вариант
//! Treeland ([`Offer::Install`]): та же сборка, которая ставится заменой
//! установленного `deepin-session` (`pacman -U --ask=4`), и снятие
//! `deepin-kwin` той же командой.
//!
//! ОБНОВЛЕНИЯ — решение и почему. Пакет под именем Arch (`deepin-session`
//! той же версии) первое же обновление Arch с новой версией «обновило» бы
//! обратно на рецепт Arch, а тот снова потребовал бы `deepin-kwin`: pacman
//! спросил бы «удалить kwin?» (ответ по умолчанию «нет» обрывает ВСЁ
//! обновление системы, ответ «да» сносит KDE Plasma). `IgnorePkg` в
//! `pacman.conf` спасает от этого, но правит чужой системный файл и
//! оставляет человеку вечное «ignoring package upgrade». Хук pacman
//! пересобрать пакет внутри транзакции не может (замок базы, сборка не от
//! root, а конфликт pacman находит ещё до хуков). Поэтому:
//!
//! * своё имя [`LOCAL_PACKAGE`] с `provides=deepin-session=<версия>` и
//!   `conflicts=deepin-session`: `-Syu` его не трогает никогда (такого имени
//!   в репозиториях нет), а `treeland` и `deepin-session-shell`, которым
//!   нужен `deepin-session`, довольствуются `provides`;
//! * версия — ровно та, что в репозитории (`pkgrel` берётся из него:
//!   CachyOS пересобирает с `1.1`);
//! * сборка сама не обновляется — и об этом говорят двое: хук pacman внутри
//!   самого пакета (после каждой транзакции сравнивает свою версию с
//!   `deepin-session` в репозитории и печатает, что пора пересобрать) и
//!   карточка Deepin в Rexon (кнопка «Обновить сборку»). Пока не
//!   пересобрали, сеанс Treeland работает на прежней сборке; Plasma это не
//!   задевает никак.
//!
//! Удаление — обычное удаление Deepin: свой пакет остаётся в группе
//! `deepin`, и `pacman -Qg deepin` отдаёт его вместе с остальными; хук
//! уходит вместе с пакетом, `pacman.conf` не тронут.
//!
//! Только Arch и только изменяемая система — как у любой пересборки
//! ([`crate::repo_rebuild::rebuild_ctx_live`]).

use std::sync::Arc;

use crate::i18n::{tr, trf};
use crate::repo_rebuild::{self, RecipeEdit};
use crate::system::PkgFamily;
use crate::system_de_exec::{CancelToken, DeOpOutcome, DeOpSink};
use crate::system_plan::{self, Cmd, InstallPreview, PreviewAnswer};

/// Команда с выводом под LANG=C (его разбирают).
fn cmd(argv: Vec<String>, timeout_secs: Option<u64>) -> Cmd {
    Cmd { argv, timeout_secs, lang_c: true }
}

/// Стол в базе ядра, к которому относится вариант.
pub const DE_ID: &str = "deepin";
/// Группа Deepin в репозиториях Arch.
pub const GROUP: &str = "deepin";
/// Пакет Arch, который пересобирается.
pub const SESSION_PACKAGE: &str = "deepin-session";
/// Под каким именем он встаёт.
pub const LOCAL_PACKAGE: &str = "deepin-session-treeland";
/// Оконный менеджер сеанса X11 — тот, что конфликтует с KWin. Rexon его не
/// ставит никогда.
pub const X11_WM_PACKAGE: &str = "deepin-kwin";
/// Пункт «Deepin» (X11) в экране входа: без `deepin-kwin` он не работает.
const X11_SESSION_FILE: &str = "usr/share/xsessions/deepin.desktop";
const HOOK_FILE: &str = "usr/share/libalpm/hooks/90-deepin-session-treeland.hook";
const CHECK_FILE: &str = "usr/share/libalpm/scripts/deepin-session-treeland-check";

// ── Правка рецепта ──────────────────────────────────────────────────────

/// Строка для `printf` внутри двойных кавычек `sh`.
fn dq(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(c, '"' | '$' | '`' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Хук pacman в самом пакете: после каждой установки и обновления —
/// проверка, не обогнал ли репозиторий нашу сборку.
pub fn hook_text() -> String {
    format!(
        "[Trigger]\n\
         Operation = Install\n\
         Operation = Upgrade\n\
         Type = Package\n\
         Target = *\n\
         \n\
         [Action]\n\
         Description = Checking the local {LOCAL_PACKAGE} build against {SESSION_PACKAGE} in the repositories...\n\
         When = PostTransaction\n\
         Exec = /{CHECK_FILE}\n"
    )
}

/// Сама проверка. Тексты — на языке Rexon в момент сборки (их читает тот,
/// кто собирал).
pub fn check_script() -> String {
    let line1 = dq(&trf(
        "Rexon: the repositories have %1 %2, and your Treeland build of it (without deepin-kwin) is still %3.",
        &[&SESSION_PACKAGE, &"%REPO%", &"%LOCAL%"],
    ))
    .replace("%REPO%", "${repo_ver}")
    .replace("%LOCAL%", "${local_ver}");
    let line2 = dq(&tr(
        "The Treeland session keeps working on the old build. To update it, open Rexon → Desktops → Deepin → “Update the Treeland build”.",
    ));
    format!(
        "#!/bin/sh\n\
         # Rexon: {LOCAL_PACKAGE} is {SESSION_PACKAGE} built from the Arch recipe without {X11_WM_PACKAGE}\n\
         # (Deepin on Treeland). It is never upgraded by pacman -Syu,\n\
         # so this hook says when the repositories move ahead of it.\n\
         local_ver=$(pacman -Q {LOCAL_PACKAGE} 2>/dev/null) || exit 0\n\
         local_ver=${{local_ver#* }}\n\
         repo_ver=$(LC_ALL=C pacman -Si {SESSION_PACKAGE} 2>/dev/null | sed -n 's/^Version *: *//p' | head -n 1)\n\
         [ -n \"$repo_ver\" ] || exit 0\n\
         [ \"$(vercmp \"$repo_ver\" \"$local_ver\")\" -gt 0 ] || exit 0\n\
         printf '%s\\n' \"{line1}\" \"{line2}\"\n\
         exit 0\n"
    )
}

/// Что меняется в рецепте `deepin-session`.
pub fn recipe_edit() -> RecipeEdit {
    RecipeEdit {
        pkgname: LOCAL_PACKAGE.to_string(),
        pkgdesc_note: format!("Rexon build without {X11_WM_PACKAGE}, for the Treeland session"),
        to_optdepends: vec![(
            X11_WM_PACKAGE.to_string(),
            "X11 session (conflicts with kwin from KDE Plasma)".to_string(),
        )],
        drop_files: vec![X11_SESSION_FILE.to_string()],
        add_files: vec![
            (HOOK_FILE.to_string(), "0644".to_string(), hook_text()),
            (CHECK_FILE.to_string(), "0755".to_string(), check_script()),
        ],
        ..RecipeEdit::default()
    }
}

/// Правка для перехода с обычного Deepin: та же сборка ставится заменой
/// установленного `deepin-session`, а `deepin-kwin` (если стоит) снимается
/// той же root-командой — сеанса X11 после перехода нет.
pub fn migration_edit(x11_wm_installed: bool) -> RecipeEdit {
    RecipeEdit {
        replace_conflicts: true,
        remove_after: if x11_wm_installed { vec![X11_WM_PACKAGE.to_string()] } else { Vec::new() },
        ..recipe_edit()
    }
}

// ── Набор и проба ───────────────────────────────────────────────────────

/// Что ставится: члены группы Deepin без `deepin-session` (его заменяет
/// своя сборка) и без `deepin-kwin` (в группе его нет, но вдруг появится).
pub fn variant_targets(group_members: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in group_members {
        if m != SESSION_PACKAGE && m != X11_WM_PACKAGE && m != LOCAL_PACKAGE && !out.contains(m) {
            out.push(m.clone());
        }
    }
    out
}

/// `deepin-kwin` не ставится никогда — ни членом набора, ни зависимостью:
/// проба, в плане которой он есть, считается неставящейся (он в
/// [`InstallPreview::blocked`], и установка отказывает до пароля).
pub fn forbid_x11_wm(p: &mut InstallPreview) {
    let pulled = p.new_packages.iter().chain(&p.targets).any(|n| n == X11_WM_PACKAGE);
    if pulled && !p.blocked.iter().any(|n| n == X11_WM_PACKAGE) {
        p.blocked.push(X11_WM_PACKAGE.to_string());
    }
}

/// Члены группы Deepin в репозиториях (`pacman -Sg deepin`).
fn group_members(family: PkgFamily, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<Vec<String>> {
    let cmd = system_plan::sync_groups_cmd(family, &[GROUP.to_string()])?;
    let (_, stdout, _) = run(&cmd)?;
    system_plan::parse_sync_groups(&stdout)
        .into_iter()
        .find(|(g, _)| g == GROUP)
        .map(|(_, m)| m)
        .filter(|m| !m.is_empty())
}

/// Версия поля `Version` первого блока `pacman -Si`/`-Qi` — того
/// репозитория, из которого pacman и поставил бы пакет.
fn first_version(stdout: &str) -> Option<String> {
    repo_rebuild::si_field(stdout, "Version").into_iter().next().filter(|v| !v.is_empty())
}

/// Версия `deepin-session` в репозиториях.
pub fn repo_session_version(run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<String> {
    let cmd = cmd(vec!["pacman".into(), "-Si".into(), SESSION_PACKAGE.into()], Some(60));
    match run(&cmd)? {
        (0, stdout, _) => first_version(&stdout),
        _ => None,
    }
}

/// Версия установленного пакета `name` по `pacman -Q`, если стоит именно
/// он (а не другой, который его `provides`).
fn installed_version(name: &str, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<String> {
    let cmd = cmd(vec!["pacman".into(), "-Q".into(), name.into()], Some(30));
    match run(&cmd)? {
        (0, stdout, _) => {
            let mut parts = stdout.split_whitespace();
            (parts.next() == Some(name)).then(|| parts.next().map(str::to_string)).flatten()
        }
        _ => None,
    }
}

/// Версия своей сборки, если она стоит.
pub fn local_version(run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<String> {
    installed_version(LOCAL_PACKAGE, run)
}

/// Стоит обычный Deepin: `deepin-session` из репозитория, а не своя сборка.
pub fn ordinary_installed(run: &dyn Fn(&Cmd) -> PreviewAnswer) -> bool {
    installed_version(SESSION_PACKAGE, run).is_some()
}

/// Стоит ли `deepin-kwin` (остаток обычного Deepin).
fn x11_wm_installed(run: &dyn Fn(&Cmd) -> PreviewAnswer) -> bool {
    installed_version(X11_WM_PACKAGE, run).is_some()
}

/// `a` новее `b` — по `vercmp` из pacman (своё сравнение версий Arch
/// повторять незачем). Не ответил — «не новее»: ложная тревога хуже.
pub fn newer(a: &str, b: &str, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> bool {
    let cmd = cmd(vec!["vercmp".into(), a.into(), b.into()], Some(10));
    matches!(run(&cmd), Some((0, out, _)) if out.trim().parse::<i32>().is_ok_and(|n| n > 0))
}

/// Проба `-Sp` того же набора, но как будто `deepin-session` уже стоит:
/// `--assume-installed` закрывает зависимость `treeland` и
/// `deepin-session-shell` от него, и pacman не тянет рецептный
/// `deepin-session` с его `deepin-kwin`.
fn assume_session(cmd: &Cmd, version: &str) -> Cmd {
    let mut argv = cmd.argv.clone();
    if argv.get(1).map(String::as_str) == Some("-Sp") {
        argv.insert(2, format!("{SESSION_PACKAGE}={version}"));
        argv.insert(2, "--assume-installed".to_string());
    }
    Cmd { argv, timeout_secs: cmd.timeout_secs, lang_c: cmd.lang_c }
}

/// Набор варианта и проба его установки (тем же [`system_plan::install_preview`],
/// что у любого стола, со сверкой конфликтов). `assume` — версия
/// `deepin-session`, которую считать стоящей (своей сборки ещё нет);
/// `None` — своя сборка уже стоит и сама его даёт.
pub fn variant_preview(
    family: PkgFamily,
    assume: Option<&str>,
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> Option<(Vec<String>, InstallPreview)> {
    if family != PkgFamily::Arch {
        return None;
    }
    let targets = variant_targets(&group_members(family, run)?);
    let wrapped = |cmd: &Cmd| match assume {
        Some(v) => run(&assume_session(cmd, v)),
        None => run(cmd),
    };
    let mut preview = system_plan::install_preview(family, &targets, &wrapped)?;
    forbid_x11_wm(&mut preview);
    Some((targets, preview))
}

/// Что предложить на карточке Deepin кнопкой под предупреждением.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// Стоит обычный Deepin (X11): перейти на вариант Treeland — своя
    /// сборка заменой установленного `deepin-session` ([`install_live`]).
    Install,
    /// Вариант стоит, а в репозитории `deepin-session` новее своей сборки.
    Update { local: String, repo: String },
}

/// Состояние карточки Deepin: проба, которую показывать вместо обычной
/// (обычный набор тянет `deepin-session` с `deepin-kwin` и Rexon его не
/// ставит), и что предложить.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardState {
    pub preview: Option<InstallPreview>,
    pub offer: Option<Offer>,
}

/// Решение для карточки. `normal` — проба обычного набора стола: она идёт
/// в ход, только если пробу варианта сделать не вышло, и тогда с
/// запретом `deepin-kwin` ([`forbid_x11_wm`]) — обещать обычный Deepin
/// карточка не должна.
pub fn card_state(
    de_id: &str,
    family: PkgFamily,
    normal: Option<&InstallPreview>,
    run: &dyn Fn(&Cmd) -> PreviewAnswer,
) -> CardState {
    if de_id != DE_ID || family != PkgFamily::Arch {
        return CardState::default();
    }
    if ordinary_installed(run) {
        return CardState { preview: None, offer: Some(Offer::Install) };
    }
    if let Some(local) = local_version(run) {
        let preview = variant_preview(family, None, run).map(|(_, p)| p);
        let offer = repo_session_version(run)
            .filter(|repo| newer(repo, &local, run))
            .map(|repo| Offer::Update { local, repo });
        return CardState { preview, offer };
    }
    let preview = repo_session_version(run)
        .and_then(|repo| variant_preview(family, Some(&repo), run))
        .map(|(_, p)| p)
        .or_else(|| {
            normal.cloned().map(|mut p| {
                forbid_x11_wm(&mut p);
                p
            })
        });
    CardState { preview, offer: None }
}

/// Набор для установки Deepin, когда своя сборка уже стоит: члены группы
/// без `deepin-session`, иначе pacman потянул бы его и `deepin-kwin`.
/// Своей сборки нет — отказ: группу целиком Rexon не ставит.
pub fn install_set(family: PkgFamily, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Result<Vec<String>, String> {
    if family != PkgFamily::Arch || local_version(run).is_none() {
        return Err(format!("{}\n\n{}", only_treeland_title(), not_built_text()));
    }
    group_members(family, run)
        .map(|m| variant_targets(&m))
        .ok_or_else(|| format!("{}\n\n{}", only_treeland_title(), group_unknown_text()))
}

/// Вердикт перед установкой и «Починить» (после проверки дистрибутива):
/// `None` — сказать нечего (своя сборка стоит, «Починить» её набор);
/// `Some((false, …))` — стоит обычный Deepin, отказ до пароля;
/// `Some((true, …))` — предупреждение «экспериментально» в том же вопросе.
pub fn install_verdict(family: PkgFamily, run: &dyn Fn(&Cmd) -> PreviewAnswer) -> Option<(bool, String, String)> {
    if family != PkgFamily::Arch {
        return None;
    }
    if ordinary_installed(run) {
        return Some((false, only_treeland_title(), ordinary_installed_text()));
    }
    if local_version(run).is_some() {
        return None;
    }
    Some((true, install_warning_title(), install_warning()))
}

// ── Исполнение ──────────────────────────────────────────────────────────

fn live_run(cmd: &Cmd) -> PreviewAnswer {
    let r = crate::system_exec::SystemIo::run(
        &crate::system_de_exec::LiveDeOpsIo::new(),
        &cmd.argv,
        cmd.timeout_secs,
        cmd.lang_c,
    );
    r.rc().map(|rc| (rc, r.stdout.clone(), r.stderr.clone()))
}

/// [`card_state`] на живой машине (не на неизменяемой системе).
pub fn card_state_live(de_id: &str, family: PkgFamily, normal: Option<&InstallPreview>) -> CardState {
    if de_id != DE_ID || repo_rebuild::rebuild_ctx_live().is_err() {
        return CardState::default();
    }
    card_state(de_id, family, normal, &live_run)
}

/// Своя сборка `deepin-session` (если её ещё нет или надо заменить
/// обычный), затем установка стола Deepin набором варианта
/// ([`install_set`]) — с теми же проверками, пробой конфликтов и защитой
/// экрана входа. `migrate` — согласие заменить обычный Deepin; без него
/// стоящий обычный Deepin — отказ.
fn install_variant(sink: Arc<dyn DeOpSink>, cancel: CancelToken, current_de: &str, migrate: bool) -> DeOpOutcome {
    let ctx = match repo_rebuild::rebuild_ctx_live() {
        Ok(ctx) => ctx,
        Err(message) => return repo_rebuild::refuse(&sink, message),
    };
    let ordinary = ordinary_installed(&live_run);
    if ordinary && !migrate {
        return repo_rebuild::refuse(&sink, format!("{}\n\n{}", only_treeland_title(), ordinary_installed_text()));
    }
    if ordinary || local_version(&live_run).is_none() {
        let edit = if ordinary { migration_edit(x11_wm_installed(&live_run)) } else { recipe_edit() };
        let built = repo_rebuild::rebuild_edited_with(
            &repo_rebuild::LiveRebuildIo::new(),
            sink.clone(),
            &cancel,
            &ctx,
            SESSION_PACKAGE,
            &edit,
            build_done_message(),
        );
        if !built.ok {
            return built;
        }
        sink.progress("rebuild", &built.message, None);
    }
    let mut outcome = crate::system_de_exec::install_de_packages_live(sink, cancel, DE_ID, current_de);
    if outcome.ok {
        outcome.message = format!("{}\n\n{}", outcome.message, install_result_text()).trim().to_string();
    }
    outcome
}

/// Установка и «Починить» Deepin с карточки — путь
/// [`crate::system_de_exec::install_de_live`] для Deepin.
pub fn install_from_card_live(sink: Arc<dyn DeOpSink>, cancel: CancelToken, current_de: &str) -> DeOpOutcome {
    install_variant(sink, cancel, current_de, false)
}

/// Переход с обычного Deepin на вариант Treeland ([`Offer::Install`]).
pub fn install_live(sink: Arc<dyn DeOpSink>, cancel: CancelToken, current_de: &str) -> DeOpOutcome {
    install_variant(sink, cancel, current_de, true)
}

/// Обновление своей сборки до версии репозитория.
pub fn update_live(sink: Arc<dyn DeOpSink>, cancel: CancelToken) -> DeOpOutcome {
    let ctx = match repo_rebuild::rebuild_ctx_live() {
        Ok(ctx) => ctx,
        Err(message) => return repo_rebuild::refuse(&sink, message),
    };
    repo_rebuild::rebuild_edited_with(
        &repo_rebuild::LiveRebuildIo::new(),
        sink,
        &cancel,
        &ctx,
        SESSION_PACKAGE,
        &recipe_edit(),
        update_done_message(),
    )
}

// ── Тексты для человека ─────────────────────────────────────────────────

/// Главная фраза отказа для любого Deepin не на Treeland.
pub fn only_treeland_title() -> String {
    tr("Rexon supports Deepin only on Treeland")
}

/// Отказ «Починить» и установки поверх обычного Deepin.
fn ordinary_installed_text() -> String {
    tr("The usual Deepin with the X11 session is installed here. Rexon doesn't repair or reinstall it. \
        To move to Deepin on Treeland, press “Switch to Deepin on Treeland” on the Deepin card.")
}

fn not_built_text() -> String {
    tr("The Treeland build of deepin-session isn't installed, and the usual Deepin set with deepin-kwin \
        is never installed by Rexon. Install Deepin with the “Install” button on its card.")
}

fn group_unknown_text() -> String {
    tr("The list of Deepin packages couldn't be read from the repositories. Check the internet connection and try again.")
}

/// Заголовок предупреждения в вопросе перед установкой.
pub fn install_warning_title() -> String {
    tr("Deepin on Treeland (experimental)")
}

/// Честное предупреждение в вопросе перед установкой.
pub fn install_warning() -> String {
    tr("Deepin is installed on Treeland, its own Wayland session.\n\n\
        • This is experimental: Treeland is still unstable, some Deepin settings and applets \
        may not work, and the session may crash.\n\
        • There is no X11 session of Deepin: its window manager (deepin-kwin) conflicts with \
        KWin of KDE Plasma and is never installed.\n\
        • deepin-session is built on your computer from the official Arch recipe — the same \
        sources, checksums and signatures, but without deepin-kwin. It is installed as \
        deepin-session-treeland.\n\
        • System updates don't touch this build. When Arch releases a new deepin-session, \
        the update log and the Deepin card in Rexon say so, and the build is redone with one button.\n\
        • Deepin's own login screen (ddm) is installed because Treeland needs it, but it isn't \
        turned on: you keep your login screen. To log in, choose “Treeland User” there.\n\n\
        The password is asked once. If the build takes longer than a few minutes, it may be asked \
        once more before installing. Removing Deepin later removes all of it.")
}

/// Кнопка на карточке, когда стоит обычный Deepin.
pub fn install_button() -> String {
    tr("🧪 Switch to Deepin on Treeland (experimental)")
}

/// Пояснение под кнопкой перехода.
pub fn install_hint() -> String {
    tr("This is the usual Deepin with the X11 session. Rexon supports Deepin only on Treeland, \
        its Wayland session: Rexon builds its own deepin-session without deepin-kwin, puts it in place \
        of the installed one and removes the X11 session. Treeland is unstable. To log in, \
        choose “Treeland User” on the login screen.")
}

/// Заголовок окна перехода.
pub fn install_title() -> String {
    tr("Switching to Deepin on Treeland (experimental)")
}

/// Вопрос перед переходом.
pub fn install_confirm_text() -> String {
    tr("The usual Deepin will be moved to Treeland, its own Wayland session.\n\n\
        • This is experimental: Treeland is still unstable, some Deepin settings and applets \
        may not work, and the session may crash.\n\
        • The X11 session of Deepin goes away: its window manager deepin-kwin is removed, \
        and the “Deepin” entry disappears from the login screen.\n\
        • deepin-session is built on your computer from the official Arch recipe — the same \
        sources, checksums and signatures, but without deepin-kwin — and replaces the installed one \
        as deepin-session-treeland. The rest of Deepin and your settings stay.\n\
        • System updates don't touch this build. When Arch releases a new deepin-session, \
        the update log and the Deepin card in Rexon say so, and the build is redone with one button.\n\
        • To log in, choose “Treeland User” on the login screen.\n\n\
        The password is asked once. If the build takes longer than a few minutes, it may be asked \
        once more before installing.")
}

/// Приписка к итогу удачной установки: как войти.
pub fn install_result_text() -> String {
    tr("To try Deepin, log out and choose “Treeland User” on the login screen.")
}

/// Кнопка на карточке, когда своя сборка отстала от репозитория.
pub fn update_button() -> String {
    tr("🛠 Update the Treeland build")
}

/// Пояснение под кнопкой обновления.
pub fn update_hint(local: &str, repo: &str) -> String {
    trf(
        "The repositories have deepin-session %1, your Treeland build is %2. System updates \
         don't touch it; until you rebuild it, the Treeland session keeps working on the old build.",
        &[&repo, &local],
    )
}

/// Заголовок окна обновления.
pub fn update_title() -> String {
    tr("Updating the Treeland build of deepin-session")
}

/// Вопрос перед обновлением.
pub fn update_confirm_text(local: &str, repo: &str) -> String {
    trf(
        "deepin-session %1 will be built from the official Arch recipe without deepin-kwin and \
         will replace your build %2. The password is asked once. If the build takes longer than \
         a few minutes, it may be asked once more before installing.",
        &[&repo, &local],
    )
}

fn build_done_message() -> String {
    tr("deepin-session was built without deepin-kwin and installed as deepin-session-treeland. \
        Now the rest of Deepin is installed.")
}

fn update_done_message() -> String {
    tr("The Treeland build of deepin-session was updated. The new build is used from the next login to Treeland.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_plan::PkgConflict;
    use std::cell::RefCell;

    /// `LC_ALL=C pacman -Sg deepin` (CachyOS, 20.10.2026), начало и члены,
    /// важные для варианта; один и тот же член из двух репозиториев.
    const SG_DEEPIN: &str = "deepin ddm\ndeepin deepin-daemon\n\
deepin deepin-session\ndeepin deepin-session-shell\ndeepin deepin-shell\ndeepin treeland\n\
deepin ddm\ndeepin deepin-session\n";

    /// `LC_ALL=C pacman -Si deepin-session`: блок cachyos-extra-znver4, затем extra.
    const SI_SESSION: &str = "Repository      : cachyos-extra-znver4\n\
Name            : deepin-session\nVersion         : 2.0.33-1.1\n\
Depends On      : bash  deepin-kwin  deepin-shell\n\n\
Repository      : extra\nName            : deepin-session\nVersion         : 2.0.33-1\n";

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Что стоит на машине.
    #[derive(Clone, Copy)]
    struct Machine {
        /// KDE Plasma (kwin) стоит.
        plasma: bool,
        /// Версия своей сборки, если стоит.
        local: Option<&'static str>,
        /// Стоит обычный Deepin (`deepin-session` и `deepin-kwin` из репозитория).
        ordinary: bool,
        /// Проба набора варианта тянет `deepin-kwin` (на случай, если его
        /// когда-нибудь потребует другой член группы).
        variant_pulls_kwin: bool,
    }

    const FRESH: Machine = Machine { plasma: false, local: None, ordinary: false, variant_pulls_kwin: false };

    /// Ответы pacman для машины `m`; каждая проба `-Sp` записывается в `seen`.
    fn machine(m: Machine, seen: &RefCell<Vec<Vec<String>>>) -> impl Fn(&Cmd) -> PreviewAnswer + '_ {
        move |cmd: &Cmd| {
            let a: Vec<&str> = cmd.argv.iter().map(String::as_str).collect();
            assert!(cmd.lang_c, "вывод pacman разбирается только под LANG=C: {a:?}");
            let missing = |n: &str| Some((1, String::new(), format!("error: package '{n}' was not found\n")));
            match a.as_slice() {
                ["pacman", "-Q", LOCAL_PACKAGE] => match m.local {
                    Some(v) => Some((0, format!("{LOCAL_PACKAGE} {v}\n"), String::new())),
                    None => missing(LOCAL_PACKAGE),
                },
                // pacman -Q отвечает и за того, кто `provides`: своя сборка —
                // своим именем.
                ["pacman", "-Q", SESSION_PACKAGE] => match (m.ordinary, m.local) {
                    (true, _) => Some((0, "deepin-session 2.0.32-1\n".into(), String::new())),
                    (false, Some(v)) => Some((0, format!("{LOCAL_PACKAGE} {v}\n"), String::new())),
                    (false, None) => missing(SESSION_PACKAGE),
                },
                ["pacman", "-Q", X11_WM_PACKAGE] if m.ordinary => {
                    Some((0, "deepin-kwin 6.0.8-12\n".into(), String::new()))
                }
                ["pacman", "-Q", X11_WM_PACKAGE] => missing(X11_WM_PACKAGE),
                ["pacman", "-Si", SESSION_PACKAGE] => Some((0, SI_SESSION.into(), String::new())),
                ["pacman", "-Sg", GROUP] => Some((0, SG_DEEPIN.into(), String::new())),
                // Набор варианта — пакеты, а не группы.
                ["pacman", "-Sg", ..] => Some((1, String::new(), "error: target not found\n".into())),
                ["pacman", "-Sp", rest @ ..] => {
                    seen.borrow_mut().push(rest.iter().map(|s| s.to_string()).collect());
                    assert!(!rest.contains(&SESSION_PACKAGE) && !rest.contains(&GROUP), "{a:?}");
                    assert!(!rest.contains(&X11_WM_PACKAGE), "{a:?}");
                    let mut plan = "ddm\ndeepin-daemon\ndeepin-session-shell\ndeepin-shell\ntreeland\nlightdm\n".to_string();
                    if m.variant_pulls_kwin {
                        plan.push_str("deepin-kwin\n");
                    }
                    Some((0, plan, String::new()))
                }
                ["pacman", "-Qq"] => {
                    let mut out = "glibc\n".to_string();
                    if m.plasma {
                        out.push_str("kwin\nplasma-workspace\n");
                    }
                    Some((0, out, String::new()))
                }
                ["pacman", "-Si", ..] => Some((0, "Name            : ddm\nConflicts With  : None\n".into(), String::new())),
                ["pacman", "-Qi"] => Some((0, "Name            : kwin\nConflicts With  : None\n".into(), String::new())),
                ["vercmp", x, y] => Some((0, if x > y { "1\n" } else if x == y { "0\n" } else { "-1\n" }.into(), String::new())),
                other => panic!("неожиданная команда {other:?}"),
            }
        }
    }

    /// Проба обычного набора на машине с Plasma: упёрлась в `deepin-kwin`.
    fn kwin_blocked() -> InstallPreview {
        InstallPreview {
            conflicts: vec![PkgConflict { new_package: "deepin-kwin".into(), installed: "kwin".into(), ..Default::default() }],
            new_packages: names(&["deepin-kwin", "deepin-session"]),
            ..Default::default()
        }
    }

    /// Проба обычного набора на машине без Plasma: конфликта нет, но
    /// `deepin-kwin` в плане.
    fn plain_group() -> InstallPreview {
        InstallPreview { new_packages: names(&["deepin-kwin", "deepin-session"]), ..Default::default() }
    }

    #[test]
    fn variant_set_is_the_group_without_session_and_kwin() {
        let members = names(&["ddm", "deepin-daemon", "deepin-session", "deepin-kwin", "treeland", "ddm"]);
        assert_eq!(variant_targets(&members), names(&["ddm", "deepin-daemon", "treeland"]));
    }

    #[test]
    fn edit_moves_kwin_to_optdepends_and_drops_x11_session() {
        let e = recipe_edit();
        assert_eq!(e.pkgname, LOCAL_PACKAGE);
        assert_eq!(e.to_optdepends[0].0, X11_WM_PACKAGE);
        assert_eq!(e.drop_files, vec!["usr/share/xsessions/deepin.desktop"]);
        assert!(!e.replace_conflicts && e.remove_after.is_empty());
        let hook = &e.add_files[0];
        assert!(hook.0.starts_with("usr/share/libalpm/hooks/") && hook.0.ends_with(".hook"));
        assert!(hook.2.contains("When = PostTransaction"));
        assert!(hook.2.contains(&format!("Exec = /{CHECK_FILE}")));
        let script = &e.add_files[1];
        assert_eq!(script.0, CHECK_FILE);
        assert_eq!(script.1, "0755");
        assert!(script.2.starts_with("#!/bin/sh\n"));
        assert!(script.2.contains("vercmp \"$repo_ver\" \"$local_ver\""));
        // Подстановки — переменные оболочки, а не текст «%REPO%».
        assert!(script.2.contains("${repo_ver}") && script.2.contains("${local_ver}"));
        assert!(!script.2.contains("%REPO%"));
    }

    /// Переход с обычного Deepin: та же сборка, но заменой установленного
    /// `deepin-session` и со снятием `deepin-kwin`, если он стоит.
    #[test]
    fn migration_replaces_the_session_and_removes_kwin() {
        let e = migration_edit(true);
        assert!(e.replace_conflicts);
        assert_eq!(e.remove_after, names(&[X11_WM_PACKAGE]));
        assert_eq!(RecipeEdit { replace_conflicts: false, remove_after: Vec::new(), ..e }, recipe_edit());
        assert!(migration_edit(false).remove_after.is_empty());
    }

    #[test]
    fn translated_text_cannot_break_out_of_the_printf_quotes() {
        assert_eq!(dq(r#"a "b" $c `d` \e"#), r#"a \"b\" \$c \`d\` \\e"#);
    }

    #[test]
    fn preview_assumes_session_only_in_the_dry_run() {
        let sp = cmd(names(&["pacman", "-Sp", "--needed", "treeland"]), None);
        assert_eq!(
            assume_session(&sp, "2.0.33-1.1").argv,
            names(&["pacman", "-Sp", "--assume-installed", "deepin-session=2.0.33-1.1", "--needed", "treeland"])
        );
        let si = cmd(names(&["pacman", "-Si", "treeland"]), None);
        assert_eq!(assume_session(&si, "1").argv, si.argv);
    }

    #[test]
    fn kwin_in_any_plan_makes_it_not_installable() {
        let mut p = plain_group();
        assert!(p.installable());
        forbid_x11_wm(&mut p);
        forbid_x11_wm(&mut p);
        assert!(!p.installable());
        assert_eq!(p.blocked, names(&[X11_WM_PACKAGE]));
        let mut clean = InstallPreview { new_packages: names(&["treeland"]), ..Default::default() };
        forbid_x11_wm(&mut clean);
        assert!(clean.installable());
    }

    /// Карточка ещё не установленного Deepin — с Plasma и без неё одно и то
    /// же: проба набора варианта (с `--assume-installed`), без `deepin-session`
    /// и `deepin-kwin`, и никакой второй кнопки.
    #[test]
    fn card_is_the_treeland_set_with_and_without_plasma() {
        let mut previews = Vec::new();
        let mut probes = Vec::new();
        for (plasma, normal) in [(true, kwin_blocked()), (false, plain_group())] {
            let seen = RefCell::new(Vec::new());
            let m = Machine { plasma, ..FRESH };
            let state = card_state(DE_ID, PkgFamily::Arch, Some(&normal), &machine(m, &seen));
            assert_eq!(state.offer, None);
            let p = state.preview.expect("проба набора варианта");
            assert!(p.installable(), "{p:?}");
            assert!(!p.new_packages.iter().any(|n| n == X11_WM_PACKAGE || n == SESSION_PACKAGE), "{p:?}");
            assert!(p.new_packages.iter().any(|n| n == "treeland"));
            let seen = seen.into_inner();
            assert!(seen.iter().all(|argv| argv.windows(2).any(|w| w == ["--assume-installed", "deepin-session=2.0.33-1.1"])));
            previews.push(p);
            probes.push(seen);
        }
        assert_eq!(previews[0], previews[1]);
        assert_eq!(probes[0], probes[1]);
    }

    #[test]
    fn variant_that_pulls_kwin_is_not_offered_as_installable() {
        let seen = RefCell::new(Vec::new());
        let m = Machine { variant_pulls_kwin: true, ..FRESH };
        let p = card_state(DE_ID, PkgFamily::Arch, Some(&plain_group()), &machine(m, &seen)).preview.unwrap();
        assert!(!p.installable());
        assert!(p.blocked.iter().any(|n| n == X11_WM_PACKAGE));
    }

    #[test]
    fn other_desktops_and_families_get_nothing() {
        let seen = RefCell::new(Vec::new());
        let run = machine(FRESH, &seen);
        assert_eq!(card_state("pantheon", PkgFamily::Arch, Some(&kwin_blocked()), &run), CardState::default());
        assert_eq!(card_state(DE_ID, PkgFamily::Fedora, Some(&kwin_blocked()), &run), CardState::default());
        assert_eq!(install_verdict(PkgFamily::Fedora, &run), None);
        assert!(install_set(PkgFamily::Fedora, &run).is_err());
    }

    #[test]
    fn installed_build_replaces_the_preview_and_is_quiet_while_current() {
        let seen = RefCell::new(Vec::new());
        let m = Machine { plasma: true, local: Some("2.0.33-1.1"), ..FRESH };
        let state = card_state(DE_ID, PkgFamily::Arch, Some(&kwin_blocked()), &machine(m, &seen));
        let preview = state.preview.expect("проба набора варианта");
        assert!(preview.installable(), "{preview:?}");
        assert!(!preview.targets.iter().any(|t| t == SESSION_PACKAGE));
        assert_eq!(state.offer, None);
        // Своя сборка стоит — `--assume-installed` не нужен.
        assert!(seen.borrow().iter().all(|argv| !argv.iter().any(|a| a == "--assume-installed")));
    }

    #[test]
    fn outdated_build_is_offered_an_update() {
        let seen = RefCell::new(Vec::new());
        let m = Machine { local: Some("2.0.32-1"), ..FRESH };
        let state = card_state(DE_ID, PkgFamily::Arch, None, &machine(m, &seen));
        assert_eq!(state.offer, Some(Offer::Update { local: "2.0.32-1".into(), repo: "2.0.33-1.1".into() }));
    }

    /// Стоит обычный Deepin: Rexon его не трогает, карточка предлагает переход.
    #[test]
    fn ordinary_deepin_is_offered_the_switch() {
        let seen = RefCell::new(Vec::new());
        let m = Machine { ordinary: true, ..FRESH };
        let state = card_state(DE_ID, PkgFamily::Arch, Some(&InstallPreview::default()), &machine(m, &seen));
        assert_eq!(state, CardState { preview: None, offer: Some(Offer::Install) });
        assert!(x11_wm_installed(&machine(m, &seen)));
    }

    /// Вопрос перед установкой: свежая машина — предупреждение
    /// «экспериментально»; обычный Deepin — отказ до пароля («Починить» его
    /// не переустанавливает); своя сборка — сказать нечего.
    #[test]
    fn verdict_warns_refuses_or_stays_quiet() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let seen = RefCell::new(Vec::new());
        let (allowed, title, text) = install_verdict(PkgFamily::Arch, &machine(FRESH, &seen)).unwrap();
        assert!(allowed);
        assert!(title.contains("experimental"), "{title}");
        assert!(text.contains("no X11 session") && text.contains("Treeland User"), "{text}");
        let ordinary = Machine { ordinary: true, ..FRESH };
        let (allowed, title, _) = install_verdict(PkgFamily::Arch, &machine(ordinary, &seen)).unwrap();
        assert!(!allowed);
        assert_eq!(title, "Rexon supports Deepin only on Treeland");
        let built = Machine { local: Some("2.0.33-1.1"), ..FRESH };
        assert_eq!(install_verdict(PkgFamily::Arch, &machine(built, &seen)), None);
    }

    /// Набор установки — только при своей сборке; иначе отказ, а не группа.
    #[test]
    fn install_set_needs_the_own_build() {
        let seen = RefCell::new(Vec::new());
        let built = Machine { local: Some("2.0.33-1.1"), ..FRESH };
        let t = install_set(PkgFamily::Arch, &machine(built, &seen)).unwrap();
        assert_eq!(t, names(&["ddm", "deepin-daemon", "deepin-session-shell", "deepin-shell", "treeland"]));
        let err = install_set(PkgFamily::Arch, &machine(FRESH, &seen)).unwrap_err();
        assert!(err.starts_with(&only_treeland_title()), "{err}");
    }

    /// Стенд в КОНТЕЙНЕРЕ (archlinux с plasma-desktop и kwin, `pkexec` —
    /// обёртка над sudo): шаг из `REXON_DTL_STEP` — `card`, `install`,
    /// `switch`, `update`, `remove`. На живой машине не запускать: ставит пакеты.
    /// `cargo test -p rexon-core --lib deepin_treeland::tests::container_stand -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn container_stand() {
        struct Print;
        impl DeOpSink for Print {
            fn progress(&self, stage: &str, message: &str, _: Option<f64>) {
                eprintln!("[{stage}] {message}");
            }
            fn cancellable(&self, _: bool) {}
        }
        assert!(std::path::Path::new("/run/.containerenv").exists(), "только в контейнере podman");
        let family = PkgFamily::Arch;
        let step = std::env::var("REXON_DTL_STEP").unwrap_or_default();
        let sink: Arc<dyn DeOpSink> = Arc::new(Print);
        match step.as_str() {
            "card" => {
                let packages = crate::system::get_packages(DE_ID, family);
                let normal = system_plan::install_preview_live(family, &packages);
                let state = card_state_live(DE_ID, family, normal.as_ref());
                if let Some(p) = &state.preview {
                    eprintln!(
                        "проба варианта: ставится={} новых={} конфликты={:?} заблокировано={:?}",
                        p.installable(),
                        p.new_packages.len(),
                        p.conflicts,
                        p.blocked
                    );
                }
                eprintln!("предложение: {:?}", state.offer);
            }
            "install" => eprintln!("ИТОГ: {:?}", install_from_card_live(sink, CancelToken::new(), "kde")),
            "switch" => eprintln!("ИТОГ: {:?}", install_live(sink, CancelToken::new(), "kde")),
            "update" => eprintln!("ИТОГ: {:?}", update_live(sink, CancelToken::new())),
            "remove" => eprintln!(
                "ИТОГ: {:?}",
                crate::system_de_exec::remove_de_live(sink, CancelToken::new(), DE_ID, "kde")
            ),
            other => panic!("REXON_DTL_STEP: неизвестный шаг {other:?}"),
        }
    }

    #[test]
    fn versions_are_read_from_recorded_answers() {
        let seen = RefCell::new(Vec::new());
        let built = Machine { local: Some("2.0.33-1.1"), ..FRESH };
        assert_eq!(repo_session_version(&machine(built, &seen)).as_deref(), Some("2.0.33-1.1"));
        assert_eq!(local_version(&machine(built, &seen)).as_deref(), Some("2.0.33-1.1"));
        // Своя сборка `provides` deepin-session, но обычным Deepin не считается.
        assert!(!ordinary_installed(&machine(built, &seen)));
        assert_eq!(local_version(&machine(FRESH, &seen)), None);
    }
}
