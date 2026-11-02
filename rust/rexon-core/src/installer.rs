//! Порт ЧИСТОЙ (не-GUI) логики `rexon/installer.py` — системная/пользова-
//! тельская установка и деинсталляция Rexon.
//!
//! Как и `system_plan.rs`, модуль НИЧЕГО не исполняет: он строит ТЕКСТЫ
//! (bash-скрипты install/uninstall, тела privileged-helper'ов, polkit-XML,
//! .desktop, лаунчеры) и ПЛАНЫ команд (`Cmd`/`PlanStep` из `system_plan`) —
//! те же argv/timeout/скрипты, что живой Python передаёт в `subprocess.run`.
//! Реальное исполнение (pkexec, copytree, ФС-проверки, tempfile.mkstemp) —
//! забота исполнителя (пока живой Python через мост, позже rexon-gui). Пути
//! tmp-файлов подаются параметрами, поэтому тексты детерминированы и
//! сверяемы байт-в-байт (см. `installer_parity.rs`).
//!
//! Портировано (чистая логика):
//!   * константы SYSTEM_INSTALL_ACTION / SYSTEM_HELPER_PATH / SYSTEM_POLICY_PATH
//!     + class-level INSTALL_DIR / LIB_DIR / DESKTOP_DIR;
//!   * тексты `system_helper_script` / `system_policy_xml` (текст-в-текст),
//!     `desktop_content` / `recover_desktop_content`, лаунчеры установки в
//!     систему и в домашнюю папку;
//!   * `detect_default_mode` (Steam-env → portable), `install_paths` (пути по
//!     режиму system/user/portable, XDG_DATA_HOME/HOME), `*_installed` предикаты;
//!   * зависимости времени выполнения per `PkgFamily` двумя разрядами (оба с
//!     fallback на DEBIAN): обязательные `dep_packages`/`dep_install_cmd` —
//!     строго, желательные `optional_dep_packages`/`optional_dep_install_cmd`
//!     — отдельной строкой с `|| true`; САМИ команды установки — не здесь,
//!     их отдаёт движок `system_plan::install_cmd`, здесь только форма
//!     строки bash;
//!   * генерация install-скрипта system-режима (icon-блок, helper-блок,
//!     dep-ветка) + план (WriteFile → pkexec → RemoveFiles) + список очистки;
//!   * деинсталляция: таблицы `distro_pkg_owner` / `distro_pkg_remove`,
//!     `parse_owner_output`, кандидаты-бинари, pkg-remove через менеджер,
//!     system-uninstall-скрипт + план, `installed_files`, список путей
//!     user-удаления (`uninstall_user_paths`, включая per-user иконки) и
//!     итоговое сообщение (`uninstall_summary`).
//!
//! НЕ портировано (side-effect / GUI — исполнение при подключении к живому
//! Python): реальные pkexec/subprocess/copytree/mkstemp/rmtree, ФС-проверки
//! `check_if_installed`/`_detect_distro_package` (наличие путей и владелец у
//! пакетного менеджера — подаются параметром/замыканием), user-install и
//! portable-install как последовательности ФС-операций (кроме их чистых
//! текстов — лаунчер/desktop), CLI-диалог `run_installer_dialog`, весь
//! PySide6-класс `QtInstallerWindow`.
//!
//! ⚠ Правило #16 (2026-07-21): починен баг оригинала — в `install_to_system`
//! список `cleanup_paths` НЕ включал `tmp_stage_helper` / `tmp_stage_policy`
//! (утечка двух временных файлов на каждую установку). В порте
//! `install_cleanup_paths` их добавляет. См. пометку у функции.

use serde::Serialize;

use crate::system::PkgFamily;
use crate::system_plan::{
    Cmd, PlanStep, RunStepId, DE_HELPER_PATH, DE_POLICY_PATH, PKG_HELPER_PATH,
    PKG_POLICY_PATH, REXON_POLKIT_RULES_PATH, STAGE_HELPER_PATH,
    STAGE_POLICY_PATH,
};

// ============================================================================
// Константы (порт module-level + class-level RexonInstaller)
// ============================================================================

pub const SYSTEM_INSTALL_ACTION: &str = "org.rexon.system-install";
/// Имя помощника тем экрана входа (SDDM) и экрана загрузки (Plymouth).
pub const SYSTEM_HELPER_NAME: &str = "rexon-system-helper";
/// Путь помощника у РУЧНОЙ установки: по нему установщик кладёт файл и его
/// же пишет в `exec.path` своей политики. Куда смотреть при запуске,
/// отвечает [`system_helper_path`] — у пакета помощник лежит в `/usr/lib`.
pub const SYSTEM_HELPER_PATH: &str = "/usr/local/lib/rexon-system-helper";
pub const SYSTEM_POLICY_PATH: &str =
    "/usr/share/polkit-1/actions/org.rexon.system-install.policy";

/// Где на этой машине лежит помощник тем экрана входа и загрузки.
///
/// ⛔ Дефект, который это закрывает (28.09.2026): путь был прибит к
/// `/usr/local/lib`, а пакет помощника не ставил вовсе — у покупателя пакета
/// темы SDDM и Plymouth из «Оформления» не ставились никогда («системная
/// часть не установлена»). Правило выбора то же, что у остальных помощников
/// ([`crate::system_plan::pick_helper_path`]): пакет раньше ручной установки.
pub fn system_helper_path() -> &'static str {
    static SLOT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    SLOT.get_or_init(|| {
        crate::system_plan::pick_helper_path(SYSTEM_HELPER_NAME, &|p| {
            crate::system_plan::is_exec_file(p)
        })
    })
}

/// Class-level пути system-режима (для деинсталлятора / backward compat).
pub const INSTALL_DIR: &str = "/usr/local/bin";
pub const LIB_DIR: &str = "/usr/local/lib/rexon";
pub const DESKTOP_DIR: &str = "/usr/share/applications";

pub const EXT_ID: &str = "rexon-wallpaper@rexon.app";
/// Прежнее имя расширения обоев: домен достался от чужого проекта
/// (порт Razer Axon), к Rexon отношения не имеет. Каталог со старым
/// именем сносится и при установке, и при удалении — иначе оболочка
/// увидит два одинаковых расширения и обои нарисуются дважды.
pub const EXT_ID_LEGACY: &str = "rexon-wallpaper@openaxon.io";

/// Таймаут корневого скрипта установки в систему: внутри установка
/// недостающих компонентов (600 с — запас для установок под паролем).
/// ⭐ Починка 15.09.2026: было 300 с.
pub const SYSTEM_INSTALL_TIMEOUT_SECS: u64 = 600;

// ============================================================================
// Тексты (текст-в-текст с Python)
// ============================================================================

/// Модули заставки, которые plymouth поставляет сам (`ModuleName=` темы).
/// Имя модуля plymouth превращает в путь `<каталог модулей>/<имя>.so` БЕЗ
/// проверки (`ply_boot_splash_load`), и `ModuleName=../../../share/plymouth/
/// themes/<тема>/evil` загрузил бы `.so` из самой темы — кодом от root,
/// при каждой загрузке и выключении, да ещё и в initramfs. Поэтому тема
/// принимается только на штатном модуле, а `.so` внутри темы — отказ.
pub const PLYMOUTH_SPLASH_MODULES: &[&str] = &[
    "details", "fade-throbber", "script", "space-flares", "text", "tribar", "two-step",
];

/// Тело `rexon-system-helper` — привилегированная установка тем SDDM и
/// Plymouth.
///
/// ⛔ 28.09.2026 (план «один пароль», пункт Б0c) закрыто три вещи, и
/// ослаблять ни одну нельзя:
///   1. **код от root из темы Plymouth.** `ModuleName` — только из
///      [`PLYMOUTH_SPLASH_MODULES`] (каждое вхождение слова в каждом
///      `.plymouth`, строго построчно: разбор plymouth через `fscanf`
///      терпит перевод строки между ключом и `=`, и нестрогая проверка
///      увидела бы не то, что прочтёт он), `.so` в теме — отказ;
///   2. **подмена каталога между проверкой и копией.** Прежде: `realpath`,
///      затем `cp` по пути — каталог пользователя, и любую часть пути можно
///      было успеть подменить ссылкой (копия чужого каталога root'ом в
///      общедоступный `/usr/share`). Теперь помощник сначала ВХОДИТ в каталог
///      (`cd -P`), проверяет место и владельца уже входа, копирует `.` в свой
///      каталог 0700 и проверяет КОПИЮ — её пользователь не тронет;
///   3. **ссылки и особые файлы** в теме — отказ (тема — это обычные файлы и
///      каталоги); запись в назначение — `--remove-destination`, чтобы не
///      писать сквозь ссылку, оставшуюся в старой версии темы.
///
/// Те же правила до пароля проверяет [`system_theme_problem`] — чтобы
/// человек увидел причину сразу, а не после окна пароля.
pub fn system_helper_script() -> String {
    let text = SYSTEM_HELPER_SCRIPT_TMPL
        .replace("__PLY_MODULES__", &PLYMOUTH_SPLASH_MODULES.join("|"));
    // Имя в сообщениях — прежнее «rexon-helper» (так помощник подписывался
    // всегда; журналы и сверки его знают).
    crate::system_l10n::localize_script("rexon-helper", &text)
}

const SYSTEM_HELPER_SCRIPT_TMPL: &str = r#"#!/bin/bash
# Rexon system helper — привилегированная установка и выбор SDDM/Plymouth-тем.
# Запускается ИСКЛЮЧИТЕЛЬНО через pkexec (polkit action org.rexon.system-install,
# auth_admin_keep: подтверждённый пароль держится несколько минут для окна
# Rexon). Принимает только строго валидируемые аргументы. НЕ выполняет
# произвольный код и не пропускает его в систему в составе темы.
set -euo pipefail
export LC_ALL=C LANG=C
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
exec </dev/null
umask 022

__L10N__

[ "$#" -eq 4 ] || die sys_args

category="$1"
theme_id="$2"
staged="$3"
family="$4"

# 1) category — строгий allowlist. `apply-<вид>` — режим «Применить»: тема
#    уже стоит в /usr/share/<вид>/themes, её надо только выбрать (решение
#    04.10.2026: установка тему не включает, включает кнопка). `remove-<вид>`
#    — режим «Удалить»: снять каталог темы (не активной и не из пакета).
mode=install
case "$category" in
    sddm|plymouth) : ;;
    apply-sddm|apply-plymouth) mode=apply; category="${category#apply-}" ;;
    remove-sddm|remove-plymouth) mode=remove; category="${category#remove-}" ;;
    *) die bad_category "$category" ;;
esac

# 2) theme_id — только безопасные символы; без / .. и ведущих '-' и '.'
#    ('-x' стал бы флагом plymouth-set-default-theme, '.' — корнем тем).
case "$theme_id" in
    ""|-*|.*|*/*|*..*) die bad_theme_name "$theme_id" ;;
esac
if ! printf '%s' "$theme_id" | grep -Eq '^[A-Za-z0-9._-]+$'; then
    die bad_theme_chars "$theme_id"
fi

# 3) family — принимается для совместимости вызова; способ активации
#    Plymouth выбирается НЕ по нему, а по факту наличия инструментов.
case "$family" in
    arch|debian) : ;;
    *) die bad_family "$family" ;;
esac

# Пересборка образа ядра после смены темы — первым, что есть. Порядок — как
# у помощника сеанса прав: сначала сценарий самого plymouth (его зовёт и
# `plymouth-set-default-theme -R`; дистрибутив настраивает его под свой
# загрузчик — на CachyOS с Limine это limine-mkinitcpio, а голый
# `mkinitcpio -P` образ Limine не обновляет). Нечем — код 3.
rebuild_initramfs() {
    local p
    for p in /usr/lib/plymouth/plymouth-update-initrd \
             /usr/libexec/plymouth/plymouth-update-initrd; do
        if [ -x "$p" ]; then "$p"; return; fi
    done
    if command -v update-initramfs >/dev/null 2>&1; then
        update-initramfs -u
    elif command -v limine-mkinitcpio >/dev/null 2>&1; then
        limine-mkinitcpio
    elif command -v mkinitcpio >/dev/null 2>&1; then
        mkinitcpio -P
    elif command -v limine-dracut >/dev/null 2>&1; then
        limine-dracut
    elif command -v dracut >/dev/null 2>&1; then
        dracut --regenerate-all --force
    elif [ -x /usr/lib/booster/regenerate_images ]; then
        /usr/lib/booster/regenerate_images
    else
        return 3
    fi
}

# Есть ли в файле `[Theme] Current=` (не в комментарии, именно в группе Theme).
sddm_has_current() {
    awk 'BEGIN { r = 1 }
         { s = $0; sub(/^[ \t]+/, "", s) }
         s ~ /^\[/ { g = s; sub(/^\[[ \t]*/, "", g); sub(/[ \t]*\][ \t\r]*$/, "", g); next }
         g == "Theme" && s ~ /^Current[ \t]*=/ { r = 0 }
         END { exit r }' "$1"
}

# Тема экрана входа SDDM. /etc/sddm.conf SDDM читает ПОСЛЕДНИМ, после всех
# sddm.conf.d/*.conf, и его `Current=` молча перекрывал свой файл (так на
# CachyOS: Current=breeze). Поэтому значение меняется во всех файлах /etc,
# где ключ уже есть (настройки KDE тоже покажут эту тему), а свой файл
# пишется, только если ключа в /etc нет нигде. Две фазы: сначала копии с
# правкой рядом с каждым файлом (не вышло хоть одна, или файл — ссылка —
# копии снимаются, система не тронута), затем mv поверх. Прочие строки
# остаются как были.
sddm_set_theme() {
    local f tmp files=() tmps=()
    for f in /etc/sddm.conf.d/*.conf /etc/sddm.conf; do
        [ -f "$f" ] || continue
        sddm_has_current "$f" || continue
        [ ! -L "$f" ] || die theme_set_elsewhere "$theme_id"
        files+=("$f")
    done
    for f in "${files[@]}"; do
        if ! tmp="$(mktemp "$f.rexon.XXXXXXXX")" \
            || ! awk -v t="$theme_id" '
                { s = $0; sub(/^[ \t]+/, "", s) }
                s ~ /^\[/ { g = s; sub(/^\[[ \t]*/, "", g); sub(/[ \t]*\][ \t\r]*$/, "", g); print; next }
                g == "Theme" && match($0, /^[ \t]*Current[ \t]*=/) { print substr($0, 1, RLENGTH) t; next }
                { print }' "$f" >"$tmp" \
            || ! chmod --reference="$f" -- "$tmp"; then
            rm -f -- "${tmps[@]}" "${tmp:-}"
            exit 1
        fi
        tmps+=("$tmp")
    done
    local i
    for i in "${!files[@]}"; do
        mv -f -- "${tmps[$i]}" "${files[$i]}"
    done
    if [ "${#files[@]}" = 0 ]; then
        install -d /etc/sddm.conf.d
        cat > /etc/sddm.conf.d/zz-rexon-theme.conf <<REXON_EOF
[Theme]
Current=$theme_id
REXON_EOF
    fi
}

# Выбор темы, уже лежащей в /usr/share/<вид>/themes/<имя>.
activate() {
    local dest="/usr/share/$category/themes/$theme_id" prev
    if [ "$category" = "sddm" ]; then
        sddm_set_theme
        return 0
    fi
    # Способ активации выбираем по ФАКТУ, а не по семейству: на Ubuntu
    # бинарь plymouth-set-default-theme вырезан из пакета и остаются только
    # alternatives, а на Debian наоборот — alternatives для plymouth не
    # используются (тема там обычный файл, update-alternatives на нём
    # ошибается), зато бинарь работает и сам зовёт update-initramfs.
    # На Debian и openSUSE бинарь лежит в /usr/sbin (он уже в PATH выше).
    # Тема выбирается без -R, образ пересобирается следом: не вышло —
    # прежняя тема возвращается в настройки (образ и так прежний).
    if command -v plymouth-set-default-theme >/dev/null 2>&1; then
        prev="$(plymouth-set-default-theme 2>/dev/null || true)"
        plymouth-set-default-theme "$theme_id"
        if ! rebuild_initramfs; then
            case "$prev" in
                ""|"$theme_id"|-*|.*|*/*) : ;;
                *) plymouth-set-default-theme "$prev" || true ;;
            esac
            die initramfs_failed
        fi
    elif command -v update-alternatives >/dev/null 2>&1; then
        pl="$dest/$theme_id.plymouth"
        [ -f "$pl" ] || die no_plymouth_file "$theme_id.plymouth"
        prev="$(readlink -f /usr/share/plymouth/themes/default.plymouth 2>/dev/null || true)"
        update-alternatives --install \
            /usr/share/plymouth/themes/default.plymouth \
            default.plymouth "$pl" 100
        update-alternatives --set default.plymouth "$pl"
        if ! rebuild_initramfs; then
            if [ -n "$prev" ] && [ "$prev" != "$pl" ] && [ -f "$prev" ]; then
                update-alternatives --set default.plymouth "$prev" || true
            fi
            die initramfs_failed
        fi
    else
        die no_activation
    fi
}

# Строка `ключ = значение` в файле настроек (ключ без учёта регистра,
# пробелы по краям не в счёт). Разбор — средствами bash, без внешних программ.
conf_has() {
    local line k v
    while IFS= read -r line || [ -n "$line" ]; do
        line="${line%$'\r'}"
        case "$line" in *=*) : ;; *) continue ;; esac
        k="${line%%=*}"; v="${line#*=}"
        k="${k#"${k%%[![:space:]]*}"}"; k="${k%"${k##*[![:space:]]}"}"
        v="${v#"${v%%[![:space:]]*}"}"; v="${v%"${v##*[![:space:]]}"}"
        [ "${k,,}" = "${2,,}" ] && [ "$v" = "$3" ] && return 0
    done < "$1"
    return 1
}

# Выбрана ли тема сейчас. С запасом: любое упоминание в любом источнике, без
# разбора групп и порядка, — лишний отказ лучше снятой активной темы
# (экран входа без темы, заставка без файлов при следующей сборке образа).
theme_is_active() {
    local f link
    if [ "$category" = "sddm" ]; then
        for f in /usr/lib/sddm/sddm.conf.d/*.conf /etc/sddm.conf.d/*.conf /etc/sddm.conf; do
            [ -f "$f" ] && conf_has "$f" Current "$theme_id" && return 0
        done
        return 1
    fi
    for f in /etc/plymouth/plymouthd.conf /usr/share/plymouth/plymouthd.defaults \
             /usr/share/plymouth/plymouthd.conf; do
        [ -f "$f" ] && conf_has "$f" Theme "$theme_id" && return 0
    done
    link="$(readlink -f /usr/share/plymouth/themes/default.plymouth 2>/dev/null || true)"
    case "$link" in "$dest"/*) return 0 ;; esac
    return 1
}

# Каталог принадлежит пакету системы — его снимает менеджер пакетов, а не мы.
theme_from_package() {
    if command -v pacman >/dev/null 2>&1 && pacman -Qoq -- "$dest" >/dev/null 2>&1; then
        return 0
    fi
    if command -v dpkg-query >/dev/null 2>&1 && dpkg-query -S -- "$dest" >/dev/null 2>&1; then
        return 0
    fi
    if command -v rpm >/dev/null 2>&1 && rpm -qf -- "$dest" >/dev/null 2>&1; then
        return 0
    fi
    return 1
}

[ -n "${PKEXEC_UID:-}" ] || die not_via_pkexec

# Режим «Удалить»: только каталог /usr/share/<вид>/themes/<имя> — обычный
# каталог, не ссылка (имя уже без / и ..), не выбранная сейчас тема и не
# файлы пакета системы. Ссылки ВНУТРИ темы rm не разыменовывает.
if [ "$mode" = "remove" ]; then
    [ "$staged" = "-" ] || die sys_args
    dest="/usr/share/$category/themes/$theme_id"
    [ -d "$dest" ] && [ ! -L "$dest" ] || die theme_not_installed "$theme_id"
    if theme_is_active; then die theme_active "$theme_id"; fi
    if theme_from_package; then die theme_from_package "$theme_id"; fi
    rm -rf --one-file-system -- "$dest"
    exit 0
fi

# Режим «Применить»: ничего не копируем, staged — прочерк. Тема обязана уже
# стоять в системном каталоге обычным каталогом (не ссылкой): выбрать можно
# только то, что положил root.
if [ "$mode" = "apply" ]; then
    [ "$staged" = "-" ] || die sys_args
    dest="/usr/share/$category/themes/$theme_id"
    [ -d "$dest" ] && [ ! -L "$dest" ] || die theme_not_installed "$theme_id"
    if [ "$category" = "plymouth" ]; then
        [ -f "$dest/$theme_id.plymouth" ] || die no_plymouth_file "$theme_id.plymouth"
    fi
    activate
    exit 0
fi

# 4) staged — каталог вызвавшего под /tmp|/var/tmp (Rexon создаёт его через
#    mkdtemp). Сначала ВХОДИМ в него: дальше работаем с тем самым каталогом,
#    и подмена любой части пути ссылкой уже ничего не меняет. Место и
#    владельца проверяем у входа, а не у пути.
[ -d "$staged" ] || die staged_not_dir "$staged"
cd -P -- "$staged" 2>/dev/null || die staged_unreachable "$staged"
real_staged="$(pwd -P)"
case "$real_staged" in
    /tmp/?*|/var/tmp/?*) : ;;
    *) die staged_outside_tmp "$real_staged" ;;
esac
[ "$(stat -c %u .)" = "$PKEXEC_UID" ] || die staged_not_callers "$real_staged"

# 5) Копия в свой каталог (0700, root) — проверяется и ставится ИМЕННО она.
work="$(mktemp -d /var/tmp/rexon-system-helper.XXXXXXXX)" || die no_work_dir
trap 'rm -rf -- "$work"' EXIT
src="$work/theme"
cp -R -P -- . "$src" || die theme_copy_failed

# 6) Тема — только обычные файлы и каталоги.
odd="$(find "$src" -mindepth 1 ! -type f ! -type d -print -quit)"
[ -z "$odd" ] || die theme_odd "${odd#"$src"/}"

# 7) Plymouth: ни исполняемого кода, ни модуля мимо каталога модулей.
modline='^[[:space:]]*ModuleName[[:space:]]*=[[:space:]]*(__PLY_MODULES__)[[:space:]]*$'
check_plymouth_conf() {
    local f="$1" rel="${1#"$src"/}" total strict
    # NUL: plymouth читает строку до него, проверка видела бы другое.
    [ "$(tr -d '\000' <"$f" | wc -c)" = "$(wc -c <"$f")" ] \
        || die nul_byte "$rel"
    # Каждое вхождение слова ModuleName (без учёта регистра) обязано быть
    # целой строкой ModuleName=<штатный модуль>.
    total="$({ grep -aoi 'modulename' "$f" || true; } | wc -l)"
    strict="$({ grep -aE "$modline" "$f" || true; } | wc -l)"
    [ "$total" = "$strict" ] \
        || die bad_module "$rel"
    printf '%s' "$strict"
}
if [ "$category" = "plymouth" ]; then
    so="$(find "$src" -type f \( -name '*.so' -o -name '*.so.*' \) -print -quit)"
    [ -z "$so" ] || die theme_has_lib "${so#"$src"/}"
    main="$src/$theme_id.plymouth"
    [ -f "$main" ] || die no_plymouth_file "$theme_id.plymouth"
    while IFS= read -r -d '' conf; do
        check_plymouth_conf "$conf" >/dev/null
    done < <(find "$src" -type f -iname '*.plymouth' -print0)
    [ "$(check_plymouth_conf "$main")" -ge 1 ] \
        || die no_stock_module "$theme_id.plymouth"
fi

# ── Установка ────────────────────────────────────────────────────────
# Только копия: выбирает тему режим `apply-<вид>` (кнопка «Применить»).
dest="/usr/share/$category/themes/$theme_id"
install -d "$dest"
cp -R -P -T --remove-destination -- "$src" "$dest"
chmod -R a+rX "$dest"
exit 0
"#;

/// Почему системная тема не будет поставлена — та же проверка, что делает
/// помощник от root ([`system_helper_script`]), только ДО окна пароля.
///
/// Сама по себе она ничего не защищает (проверяет каталог пользователя, и
/// ставит тему всё равно помощник, который проверяет свою копию), зато
/// человек узнаёт причину сразу. И она же — единственная проверка на
/// запасном пути расширений (сырой `pkexec bash`, когда помощника нет).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemThemeProblem {
    /// Ссылка, устройство, канал и т. п. — тема обязана быть обычными
    /// файлами и каталогами. Внутри — путь относительно корня темы.
    OddFile(String),
    /// Тема загрузки несёт свой исполняемый код: `.so` внутри или
    /// `ModuleName` не из [`PLYMOUTH_SPLASH_MODULES`] (в т. ч. с `../`).
    BootSplashCode(String),
}

impl SystemThemeProblem {
    /// Текст для человека.
    pub fn message(&self) -> String {
        use crate::i18n::trf;
        match self {
            // TRANSLATORS: %1 is a file inside the theme, relative to the theme folder.
            SystemThemeProblem::OddFile(f) => trf(
                "The theme can't be installed: it contains a link or a special file (%1). \
                 Rexon installs only themes made of ordinary files. Nothing was changed.",
                &[f],
            ),
            // TRANSLATORS: %1 is a file inside the theme, relative to the theme folder.
            SystemThemeProblem::BootSplashCode(f) => trf(
                "The boot splash theme can't be installed: it brings its own program code \
                 (%1), which would run with full system rights at every boot. Rexon installs \
                 only themes built on Plymouth's standard modules. Nothing was changed.",
                &[f],
            ),
        }
    }
}

/// Проверить каталог системной темы (`kind` — `sddm` или `plymouth`).
/// `theme_id` — если известно, `<theme_id>.plymouth` обязан назвать
/// штатный модуль. Ссылки не разыменовываются.
pub fn system_theme_problem(
    kind: &str,
    dir: &std::path::Path,
    theme_id: Option<&str>,
) -> Option<SystemThemeProblem> {
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut stack = vec![(String::new(), dir.to_path_buf())];
    while let Some((rel_dir, abs_dir)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&abs_dir) else {
            return Some(SystemThemeProblem::OddFile(rel_dir));
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let rel = if rel_dir.is_empty() { name } else { format!("{rel_dir}/{name}") };
            let Ok(ft) = e.file_type() else {
                return Some(SystemThemeProblem::OddFile(rel));
            };
            if ft.is_dir() {
                stack.push((rel, e.path()));
            } else if ft.is_file() {
                files.push((rel, e.path()));
            } else {
                return Some(SystemThemeProblem::OddFile(rel));
            }
        }
    }
    if kind != "plymouth" {
        return None;
    }
    files.sort();
    for (rel, _) in &files {
        let base = rel.rsplit('/').next().unwrap_or(rel);
        if base.ends_with(".so") || base.contains(".so.") {
            return Some(SystemThemeProblem::BootSplashCode(rel.clone()));
        }
    }
    for (rel, path) in &files {
        if !rel.to_ascii_lowercase().ends_with(".plymouth") {
            continue;
        }
        let stock = std::fs::read(path).ok().and_then(|b| plymouth_stock_module_lines(&b));
        let main = theme_id.map(|id| *rel == format!("{id}.plymouth")).unwrap_or(false);
        match stock {
            None => return Some(SystemThemeProblem::BootSplashCode(rel.clone())),
            Some(0) if main => return Some(SystemThemeProblem::BootSplashCode(rel.clone())),
            Some(_) => {}
        }
    }
    None
}

/// Сколько в `.plymouth` строк `ModuleName=<штатный модуль>`; `None` —
/// есть нулевой байт или вхождение слова `ModuleName` (без учёта регистра)
/// вне такой строки. Правило то же, что у помощника.
fn plymouth_stock_module_lines(bytes: &[u8]) -> Option<usize> {
    if bytes.contains(&0) {
        return None;
    }
    let lower = bytes.to_ascii_lowercase();
    let total = lower.windows(b"modulename".len()).filter(|w| *w == b"modulename").count();
    let is_space = |c: u8| matches!(c, b' ' | b'\t' | b'\r' | b'\x0b' | b'\x0c');
    let strict = bytes
        .split(|&c| c == b'\n')
        .filter(|line| {
            let t = trim_bytes(line, is_space);
            let Some(rest) = t.strip_prefix(b"ModuleName") else { return false };
            let rest = trim_bytes(rest, is_space);
            let Some(value) = rest.strip_prefix(b"=") else { return false };
            let value = trim_bytes(value, is_space);
            PLYMOUTH_SPLASH_MODULES.iter().any(|m| m.as_bytes() == value)
        })
        .count();
    (total == strict).then_some(strict)
}

fn trim_bytes(b: &[u8], is_space: impl Fn(u8) -> bool) -> &[u8] {
    let start = b.iter().position(|&c| !is_space(c)).unwrap_or(b.len());
    let end = b.iter().rposition(|&c| !is_space(c)).map(|i| i + 1).unwrap_or(start);
    &b[start..end.max(start)]
}

/// XML polkit-политики SYSTEM_INSTALL_ACTION — порт `_system_policy_xml`
/// (путь ручной установки; пакет печатает [`system_policy_xml_for`]).
pub fn system_policy_xml() -> String {
    system_policy_xml_for(SYSTEM_HELPER_PATH)
}

/// XML той же политики для помощника, лежащего по заданному пути.
///
/// `exec.path` — договор с polkit: не совпал с тем, что реально
/// запускается, и pkexec наше действие не выберет (пароль каждый раз).
pub fn system_policy_xml_for(helper_path: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE policyconfig PUBLIC\n\
         \x20\"-//freedesktop//DTD polkit Policy Configuration 1.0//EN\"\n\
         \x20\"http://www.freedesktop.org/software/polkit/policyconfig-1.dtd\">\n\
         <policyconfig>\n\
         \x20 <vendor>Rexon</vendor>\n\
         \x20 <vendor_url>https://github.com/Wehrwolfmann</vendor_url>\n\
         \x20 <action id=\"{action}\">\n\
         {description}\
         {message}\
         \x20   <defaults>\n\
         \x20     <allow_any>auth_admin</allow_any>\n\
         \x20     <allow_inactive>auth_admin</allow_inactive>\n\
         \x20     <allow_active>auth_admin_keep</allow_active>\n\
         \x20   </defaults>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.path\">{helper}</annotate>\n\
         \x20   <annotate key=\"org.freedesktop.policykit.exec.allow_gui\">true</annotate>\n\
         \x20 </action>\n\
         </policyconfig>\n",
        action = SYSTEM_INSTALL_ACTION,
        helper = helper_path,
        description = crate::system_l10n::polkit_elem("    ", "description", SYSTEM_POLICY_DESCRIPTION),
        message = crate::system_l10n::polkit_elem("    ", "message", POLICY_MESSAGE_SYSTEM_THEME),
    )
}

// ============================================================================
// Вход Rexon: Rust-окно или классическое окно на Python
// ============================================================================
//
// РЕШЕНИЕ ВЛАДЕЛЬЦА (11.09.2026). Ярлык Rexon ведёт на Rust-окно
// `rexon-main-window`; окно на Python остаётся запасным пунктом меню ярлыка
// «Rexon (классическое окно)».
//
// ⚠️ ВХОД ВЫБИРАЕТ ОДНА ФУНКЦИЯ — [`launch_entry_live`], и зовут её ОБА
// писателя: ярлык ([`desktop_content`]) и запись автозапуска
// (`tray::autostart_exec`, её пишет `rexon-gui/src/tray.rs` и для Python-,
// и для Rust-хозяина). Причина жёсткая: лоток — это весь хозяин процесса, а
// замок «один Rexon на сессию» у обоих входов общий. Ярлык на Rust-окно при
// автозапуске Python-лотка поднял бы ПИТОНОВСКОЕ окно: Rust-бинарь стучится
// в занятый замок и выходит. Разъехаться двум писателям нельзя — поэтому
// вторая копия правила выбора запрещена.
//
// ГДЕ ПРОВЕРКА. При генерации, без обёртки запуска. Обёртка жила бы в
// лаунчере `rexon`, а он — Python: Rust-окно без интерпретатора тогда не
// поднималось бы вовсе, то есть уход с Python потерял бы смысл. Если бинарь
// позже пропадёт, `TryExec` на тот же путь уберёт пункт из меню вместо
// ярлыка, который молча ничего не делает; перегенерация вернёт Python-вход.
//
// Выбор смотрит только на то, что одинаково для любого процесса-писателя:
// переменная [`ENTRY_ENV`], PATH и постоянные пути. Путь СВОЕГО исполняемого
// файла сюда нарочно не входит: из Rust-окна он нашёл бы бинарь, из Python —
// нет, и ярлык с автозапуском, записанные разными хозяевами, разошлись бы.

/// Как продукт называет классическое окно на Python. Одно место на весь
/// продукт: на это имя ссылается ответ Rust-окна про функции, которые пока
/// есть только в классическом окне (`rexon-gui/src/settings_actions.rs`).
///
/// ⚠️ 16.09.2026 пункт с этим именем снят из ярлыка
/// ([`desktop_content_for`]): открывать классическое окно из меню больше
/// нечем, и ответ, который зовёт человека «щёлкнуть по ярлыку», надо
/// переписать вместе с уходом Python.
pub const CLASSIC_WINDOW_ENTRY: &str = "Rexon (классическое окно)";

/// Переменная отката: `REXON_ENTRY=python` уводит основной вход на Python.
pub const ENTRY_ENV: &str = "REXON_ENTRY";

/// Значение [`ENTRY_ENV`], выбирающее Python-вход.
pub const ENTRY_PYTHON: &str = "python";

/// Команда продукта в `PATH` — лаунчер `rexon`, его ставит установщик
/// (и пакет дистрибутива).
///
/// ⚠️ Имя оставлено прежним, но с 16.09.2026 лаунчер ведёт НЕ на Python:
/// и пакет, и ручная установка печатают `sh`-лаунчер на бинарь Rust-окна
/// ([`system_launcher`]). Значение по-прежнему служит запасным `Exec=`
/// ярлыка ([`LaunchEntry::Python`]) — на случай, когда бинарь окна не
/// нашёлся и записать в ярлык полный путь нечем.
pub const PYTHON_ENTRY_CMD: &str = "rexon";

/// Интерпретатор для ОТКАТА на Python-вход, когда лаунчера [`PYTHON_ENTRY_CMD`]
/// в `PATH` нет (дерево разработчика): хуки входа тогда зовут
/// `python3 -m rexon.app …`. Одно место на весь Rust: при Rust-входе
/// ([`LaunchEntry::MainWindow`]) величина не используется вовсе.
pub const PYTHON_FALLBACK_EXE: &str = "python3";

/// Где бинарь окна лежит у пакета (каталог данных Rexon, не в PATH).
pub const PACKAGE_MAIN_WINDOW: &str = "/usr/lib/rexon/rexon-main-window";

/// Бинарь окна внутри дерева разработчика, относительно корня дерева.
pub const DEV_TREE_MAIN_WINDOW_REL: &str = "rust/target/release/rexon-main-window";

/// Каталог данных продукта в дереве разработчика, относительно корня дерева.
/// Раскладка внутри — та же, что у пакета `rexon/` поставки
/// (`qt_wallpaper_engine/qml_ui`, `kpackage`, `themes.json`, `assets`,
/// дополнения GNOME и Cinnamon); до 02.10.2026 данные лежали в `rexon/`
/// вперемешку с кодом Python, теперь Python — архив (`_archived/`).
pub const DEV_TREE_DATA_DIR: &str = "data";

/// Каталог данных Rexon у пакета дистрибутива — родитель бинаря окна
/// ([`PACKAGE_MAIN_WINDOW`]). Там же лежат пакет `rexon/` и каталог
/// QML-модулей (`qml_modules::SYSTEM_MODULE_DIRS`).
pub fn package_data_dir() -> &'static str {
    match PACKAGE_MAIN_WINDOW.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "/usr/lib/rexon",
    }
}

/// Где искать пакет `rexon/` — данные продукта: вендорный интерфейс вкладки
/// обоев (`qt_wallpaper_engine/qml_ui`), вшитый движок и каталоги переводов.
/// По убыванию точности, без повторов.
///
/// Раскладок четыре, и все живые:
///   * дерево разработчика — `<корень>/data` ([`DEV_TREE_DATA_DIR`]; корень выводится из пути
///     своего бинаря, `cli::repo_root`);
///   * пакет дистрибутива — `<package_data_dir>/rexon` (`/usr/lib/rexon/rexon`);
///   * установка в систему — `<LIB_DIR>/rexon` (раскладка та же, что у пакета:
///     `installer_exec::InstallLayout`; прежний Python-установщик клал пакет
///     прямо в `LIB_DIR`, и эта раскладка его не находила);
///   * установка в домашнюю папку — `~/.local/lib/rexon/rexon` (та же
///     раскладка; `home` пустой — кандидата нет).
///
/// Своей переменной-переопределения здесь нет намеренно: у Python-стороны
/// такой нет тоже (она находит себя по `__file__`), а `REXON_PACKAGED`
/// говорит о РЕЖИМЕ пакета, а не о пути. Заводить вторую правду о том, где
/// лежит продукт, нельзя.
///
/// Годен тот каталог, где действительно лежит вендорный интерфейс, — это
/// решает исполнитель (у него живая файловая система).
pub fn package_data_dir_candidates(repo_root: &str, home: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let root = repo_root.trim_end_matches('/');
    if !root.is_empty() {
        out.push(format!("{root}/{DEV_TREE_DATA_DIR}"));
    }
    out.push(format!("{}/rexon", package_data_dir()));
    out.push(format!("{}/rexon", LIB_DIR.trim_end_matches('/')));
    // Установка в домашнюю папку — та же раскладка (`<~/.local/lib/rexon>/rexon`).
    if let Some(lib) = user_install_lib_dir(home) {
        out.push(format!("{lib}/rexon"));
    }
    out.dedup();
    out
}

/// Каталог данных установки в домашнюю папку (`~/.local/lib/rexon`), либо
/// `None` при неизвестном HOME: относительный путь смотрел бы в текущий
/// каталог процесса.
pub fn user_install_lib_dir(home: &str) -> Option<String> {
    let home = home.trim_end_matches('/');
    if !home.starts_with('/') {
        return None;
    }
    install_paths(InstallMode::User, home, None).lib_dir
}

/// Признак «это и есть пакет `rexon/`»: путь его вендорного интерфейса.
/// Относительный, от каталога пакета.
pub const PACKAGE_MARKER_REL: &str = "qt_wallpaper_engine/qml_ui/PageHost.qml";

/// Выбрать каталог данных Rexon из кандидатов: первый, где лежит вендорный
/// интерфейс вкладки обоев. `None` — ни одной раскладки рядом нет, и
/// вкладка честно откроется без вендорных страниц.
///
/// Проба живой файловой системы приходит колбэком: ядро не ходит на диск
/// само, а проверка одна на все три раскладки (правило #12).
pub fn pick_package_data_dir(
    repo_root: &str,
    is_file: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    package_data_dir_candidates(repo_root, &home)
        .into_iter()
        .find(|dir| is_file(&format!("{dir}/{PACKAGE_MARKER_REL}")))
}

/// Какой вход пишут ярлык и автозапуск.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchEntry {
    /// Rust-окно; внутри — полный путь к бинарю.
    MainWindow(String),
    /// Классическое окно на Python (лаунчер [`PYTHON_ENTRY_CMD`]).
    Python,
}

impl LaunchEntry {
    /// Программа для `Exec=` — уже в кавычках, если путь того требует.
    pub fn exec_program(&self) -> String {
        match self {
            LaunchEntry::MainWindow(path) => desktop_exec_arg(path),
            LaunchEntry::Python => PYTHON_ENTRY_CMD.to_string(),
        }
    }

    /// Строка `Exec=` с аргументами (флаги — свои, без кавычек).
    pub fn exec(&self, args: &[&str]) -> String {
        let mut out = self.exec_program();
        for a in args {
            out.push(' ');
            out.push_str(a);
        }
        out
    }

    /// Значение `TryExec=`: путь как есть (это не командная строка).
    pub fn try_exec(&self) -> String {
        match self {
            LaunchEntry::MainWindow(path) => path.replace('\\', "\\\\"),
            LaunchEntry::Python => PYTHON_ENTRY_CMD.to_string(),
        }
    }
}

/// Аргумент командной строки `Exec=` по спецификации Desktop Entry.
///
/// Путь с пробелом или зарезервированным символом берётся в двойные
/// кавычки; внутри кавычек `"`, `` ` ``, `$` и `\` экранируются обратной
/// косой, а сама косая удваивается ещё раз — значение ключа проходит
/// разэкранирование строки раньше разбора аргументов. `%` — всегда `%%`.
pub fn desktop_exec_arg(arg: &str) -> String {
    const RESERVED: &[char] = &[
        ' ', '\t', '"', '\'', '\\', '>', '<', '~', '|', '&', ';', '$', '*', '?', '#', '(',
        ')', '`',
    ];
    let arg = arg.replace('%', "%%");
    if !arg.contains(RESERVED) {
        return arg;
    }
    let mut q = String::with_capacity(arg.len() + 2);
    q.push('"');
    for c in arg.chars() {
        match c {
            '"' | '`' | '$' => q.push_str("\\\\"),
            '\\' => q.push_str("\\\\\\"),
            _ => {}
        }
        q.push(c);
    }
    q.push('"');
    q
}

/// Просили ли переменной отката Python-вход.
pub fn python_entry_forced(entry_env: Option<&str>) -> bool {
    entry_env.is_some_and(|v| v.trim().eq_ignore_ascii_case(ENTRY_PYTHON))
}

/// Где искать бинарь окна — по порядку: каталоги PATH, путь пакета,
/// установка в систему, установка в домашнюю папку, деревья разработчика.
///
/// Относительные и пустые элементы PATH пропускаются: они смотрят в текущий
/// каталог писателя, и два процесса получили бы разные ответы.
/// `home` — домашняя папка (пустая — кандидата домашней установки нет).
/// `dev_roots` — корни деревьев, куда ведёт установленный каталог Rexon
/// (у разработчика `/usr/local/lib/rexon` — симлинк в дерево).
pub fn main_window_candidates(
    path_env: Option<&str>,
    home: &str,
    dev_roots: &[String],
) -> Vec<String> {
    let bin = crate::window_menu::MAIN_WINDOW_BIN;
    let mut out: Vec<String> = Vec::new();
    for dir in path_env.unwrap_or("").split(':') {
        if dir.starts_with('/') {
            out.push(format!("{}/{bin}", dir.trim_end_matches('/')));
        }
    }
    out.push(PACKAGE_MAIN_WINDOW.to_string());
    // Установка в систему кладёт бинарь окна по раскладке пакета
    // (`installer_exec::InstallLayout::main_window`).
    out.push(format!("{}/{bin}", LIB_DIR.trim_end_matches('/')));
    // ⭐ Починка 15.09.2026: установка в домашнюю папку кладёт окно туда же
    // по той же раскладке, а поиск её не знал — ярлык после неё уходил на
    // классическое окно.
    if let Some(lib) = user_install_lib_dir(home) {
        out.push(format!("{lib}/{bin}"));
    }
    for root in dev_roots.iter().filter(|r| !r.is_empty()) {
        out.push(format!("{}/{DEV_TREE_MAIN_WINDOW_REL}", root.trim_end_matches('/')));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(p.clone()));
    out
}

/// Правило выбора входа — ОДНО на ярлык и автозапуск.
///
/// `REXON_ENTRY=python` → Python. Иначе — первый исполняемый кандидат
/// бинаря окна; ни одного нет → Python (не битый ярлык). Путь с управляющим
/// символом в `.desktop` не записать — такой кандидат пропускается.
pub fn choose_launch_entry(
    entry_env: Option<&str>,
    candidates: &[String],
    is_exec: impl Fn(&str) -> bool,
) -> LaunchEntry {
    if python_entry_forced(entry_env) {
        return LaunchEntry::Python;
    }
    candidates
        .iter()
        .filter(|p| !p.chars().any(char::is_control))
        .find(|p| is_exec(p))
        .map(|p| LaunchEntry::MainWindow(p.clone()))
        .unwrap_or(LaunchEntry::Python)
}

/// Корень дерева разработчика, если установленный каталог Rexon —
/// симлинк в пакет `rexon/` внутри дерева. Настоящий каталог (копия
/// user-install, system-install без симлинка) дерева не даёт.
fn dev_root_behind(lib_dir: &str) -> Option<String> {
    let link = std::path::Path::new(lib_dir);
    if !std::fs::symlink_metadata(link).ok()?.file_type().is_symlink() {
        return None;
    }
    let real = std::fs::canonicalize(link).ok()?;
    Some(real.parent()?.to_string_lossy().into_owned())
}

fn is_executable_file(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Вход по живой машине: [`ENTRY_ENV`], PATH и установленные каталоги.
pub fn launch_entry_live() -> LaunchEntry {
    let home = std::env::var("HOME").unwrap_or_default();
    let user_lib = install_paths(InstallMode::User, &home, None).lib_dir;
    let dev_roots: Vec<String> = [Some(LIB_DIR.to_string()), user_lib]
        .into_iter()
        .flatten()
        .filter_map(|d| dev_root_behind(&d))
        .collect();
    let path_env = std::env::var("PATH").ok();
    choose_launch_entry(
        std::env::var(ENTRY_ENV).ok().as_deref(),
        &main_window_candidates(path_env.as_deref(), &home, &dev_roots),
        is_executable_file,
    )
}

/// Открыть окно Rexon отдельным процессом с аргументами `args` (кнопка
/// уведомления: «Открыть Wallpaper Engine» — `--section=wallpaper_engine`).
/// Rexon уже запущен — новая копия только поднимает его окно на нужном
/// разделе (`single_instance`). `Err` — причина для журнала.
pub fn open_rexon_live(args: &[&str]) -> Result<(), String> {
    let program = match launch_entry_live() {
        LaunchEntry::MainWindow(path) => path,
        LaunchEntry::Python => return Err("nothing to open Rexon with".to_string()),
    };
    std::process::Command::new(&program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("couldn't start {program}: {e}"))
}

/// Содержимое `rexon.desktop` по живой машине — вход выбирает
/// [`launch_entry_live`]. Этой функцией ярлык пишет Python-установщик.
pub fn desktop_content() -> String {
    desktop_content_for(&launch_entry_live())
}

// Подписи ярлыков: в файл ложатся все языки каталога сразу (`Name[ru]=`),
// меню рабочего стола выбирает сам по локали сеанса ([`crate::system_l10n`]).
// TRANSLATORS: Menu entry of Rexon: GenericName shown under the name "Rexon".
const DESKTOP_GENERIC_NAME: &str = n_("Desktop Environment Manager");
// TRANSLATORS: Menu entry of Rexon: tooltip/description.
const DESKTOP_COMMENT: &str = n_("Universal desktop environment manager for Linux");
// TRANSLATORS: Search keywords of the menu entry: words separated by ";" and ending with ";". Translate or add words in your language; do not remove the separators.
const DESKTOP_KEYWORDS: &str = n_("desktop;environment;DE;settings;system;cinnamon;xfce;mate;gnome;kde;plasma;");
// TRANSLATORS: Quick action in the right-click menu of the Rexon menu entry.
const DESKTOP_ACTION_INSTALL: &str = n_("Install Desktop Environment");
// TRANSLATORS: Quick action in the right-click menu of the Rexon menu entry.
const DESKTOP_ACTION_BACKUPS: &str = n_("Manage Backups");
// TRANSLATORS: Quick action in the right-click menu of the Rexon menu entry.
const DESKTOP_ACTION_COMPARE: &str = n_("Compare Desktop Environments");
// TRANSLATORS: Quick action in the right-click menu of the Rexon menu entry (KDE only).
const DESKTOP_ACTION_RECOVER: &str = n_("🛟 Recover desktop appearance (emergency)");
// TRANSLATORS: Separate menu entry (KDE only) for the emergency appearance rollback.
const RECOVER_NAME: &str = n_("Rexon — Recover desktop appearance");
// TRANSLATORS: GenericName of the emergency rollback menu entry.
const RECOVER_GENERIC_NAME: &str = n_("Emergency appearance rollback");
// TRANSLATORS: Description of the emergency rollback menu entry.
const RECOVER_COMMENT: &str = n_("Restore the standard desktop look if an applied theme broke it or Rexon won't start");
// TRANSLATORS: Search keywords of the emergency rollback entry, separated by ";" and ending with ";".
const RECOVER_KEYWORDS: &str = n_("rexon;recover;kde;plasma;theme;emergency;");
// TRANSLATORS: Name of the system action in the password dialog (polkit): installing an SDDM or Plymouth theme.
const SYSTEM_POLICY_DESCRIPTION: &str = n_("Install a system theme (SDDM / Plymouth)");
// TRANSLATORS: Text of the system password dialog (polkit).
pub(crate) const POLICY_MESSAGE_SYSTEM_THEME: &str = n_("Authentication is required to install a system theme");

/// Содержимое `rexon.desktop` для заданного входа — порт
/// `_get_desktop_content`.
///
/// Быстрые действия идут на тот же вход, что и основной запуск: Rust-окно
/// понимает `--page=` и `--recover-kde` той же дверью, что Python
/// (`cli::start_section` / `cli::run_headless`).
///
/// ⭐ Починка при переносе: ключи `X-GNOME-UsesNotifications` и
/// `X-Desktop-File-Install-Version` стояли после последнего
/// `[Desktop Action]`, то есть принадлежали группе действия, а не
/// `[Desktop Entry]` — GNOME читает их только из основной группы.
///
/// ⭐ Починка 16.09.2026: из ярлыка снят пункт «Rexon (классическое окно)»
/// (`Exec=rexon --classic`). Классического окна на Python нет ни в пакете
/// дистрибутива, ни в ручной установке — команда `rexon` у обоих ведёт на
/// то же самое окно, и пункт открывал бы копию того, что и так открывает
/// сам ярлык.
///
/// ⭐ Починка 26.09.2026: снят `OnlyShowIn=GNOME;KDE;XFCE;MATE;Cinnamon;`.
/// Ограничение пришло из первого снимка без обоснования, а продукт сам
/// ставит и Hyprland, sway, niri, LXQt, COSMIC — там пункта в меню не было
/// вовсе, и запустить Rexon можно было только из терминала. Главному
/// ярлыку ограничение не нужно ни в одном окружении. У
/// `rexon-recover.desktop` оно остаётся ([`recover_desktop_content_for`]).
pub fn desktop_content_for(entry: &LaunchEntry) -> String {
    use crate::system_l10n::desktop_key as key;
    let actions = "InstallDE;Backup;Compare;Recover;";
    let page = |name: &str| entry.exec(&[&format!("{}{name}", crate::app::PAGE_FLAG)]);
    format!(
        "[Desktop Entry]\n\
         Version=1.5\n\
         Type=Application\n\
         Name=Rexon\n\
         {generic}\
         {comment}\
         {keywords}\
         \n\
         Exec={main}\n\
         TryExec={try_exec}\n\
         Terminal=false\n\
         StartupNotify=true\n\
         \n\
         Icon=rexon\n\
         \n\
         Categories=Qt;Graphics;\n\
         X-GNOME-UsesNotifications=true\n\
         X-Desktop-File-Install-Version=0.26\n\
         \n\
         Actions={actions}\n\
         \n\
         [Desktop Action InstallDE]\n\
         {install_name}\
         Exec={install}\n\
         \n\
         [Desktop Action Backup]\n\
         {backups_name}\
         Exec={backups}\n\
         \n\
         [Desktop Action Compare]\n\
         {compare_name}\
         Exec={compare}\n\
         \n\
         [Desktop Action Recover]\n\
         {recover_name}\
         Exec={recover}\n\
         Icon=emblem-important-symbolic\n",
        generic = key("GenericName", DESKTOP_GENERIC_NAME),
        comment = key("Comment", DESKTOP_COMMENT),
        keywords = key("Keywords", DESKTOP_KEYWORDS),
        install_name = key("Name", DESKTOP_ACTION_INSTALL),
        backups_name = key("Name", DESKTOP_ACTION_BACKUPS),
        compare_name = key("Name", DESKTOP_ACTION_COMPARE),
        recover_name = key("Name", DESKTOP_ACTION_RECOVER),
        main = entry.exec(&[]),
        try_exec = entry.try_exec(),
        install = page("install"),
        backups = page("backups"),
        compare = page("compare"),
        recover = entry.exec(&[crate::app::RECOVER_KDE_FLAG]),
    )
}

/// Содержимое `rexon-recover.desktop` по живой машине — вход тот же, что у
/// ярлыка ([`launch_entry_live`]).
pub fn recover_desktop_content() -> String {
    recover_desktop_content_for(&launch_entry_live())
}

/// Содержимое `rexon-recover.desktop` для заданного входа — порт
/// `_get_recover_desktop_content`.
///
/// `OnlyShowIn=KDE` здесь по делу: `--recover-kde` откатывает Global Theme
/// Plasma, в других окружениях пункту нечего восстанавливать.
///
/// Аварийному откату Rust-вход особенно к месту: маршрут `--recover-kde`
/// целиком в ядре и отрабатывает до подъёма Qt, интерпретатор ему не нужен.
pub fn recover_desktop_content_for(entry: &LaunchEntry) -> String {
    use crate::system_l10n::desktop_key as key;
    format!(
        "[Desktop Entry]\n\
         Version=1.0\n\
         Type=Application\n\
         {name}\
         {generic}\
         {comment}\
         {keywords}\
         \n\
         Exec={recover}\n\
         TryExec={try_exec}\n\
         Terminal=false\n\
         StartupNotify=false\n\
         \n\
         Icon=emblem-important-symbolic\n\
         \n\
         Categories=Qt;Graphics;\n\
         OnlyShowIn=KDE;\n",
        name = key("Name", RECOVER_NAME),
        generic = key("GenericName", RECOVER_GENERIC_NAME),
        comment = key("Comment", RECOVER_COMMENT),
        keywords = key("Keywords", RECOVER_KEYWORDS),
        recover = entry.exec(&[crate::app::RECOVER_KDE_FLAG]),
        try_exec = entry.try_exec(),
    )
}

/// Команда `rexon` — лаунчер ручной установки. `lib_dir` — каталог данных
/// Rexon этой установки (`installer_exec::InstallLayout::lib_dir`); бинарь
/// окна лежит в нём.
///
/// ⭐ Починка 16.09.2026 (правило #16): оба лаунчера были на Python и звали
/// `from rexon.app import main` — классическое окно, которого поставка
/// больше не везёт (коммиты 82e2113, b821779). После ручной установки
/// команда `rexon` и ярлык уходили бы в пустоту. Теперь лаунчер — тот же
/// `sh`, что печатает пакет дистрибутива, и ведёт на бинарь Rust-окна.
///
/// ⚠️ Разница с лаунчером ПАКЕТА ровно одна и она намеренная: здесь НЕ
/// выставляется [`PACKAGED_ENV`]. Ручная установка — не пакет: признак
/// пакета увёл бы [`detect_default_mode`] в переносной режим и человек
/// больше не смог бы ни доустановить, ни переустановить Rexon этим же
/// мастером.
///
/// `note` — строка-пояснение в шапке: человек, открывший
/// `/usr/local/bin/rexon` или `~/.local/bin/rexon`, видит, какая это
/// установка.
fn launcher_script(lib_dir: &str, note: &str) -> String {
    let bin = format!(
        "{}/{}",
        lib_dir.trim_end_matches('/'),
        crate::window_menu::MAIN_WINDOW_BIN
    );
    format!(
        "#!/bin/sh\n\
         # Rexon — лаунчер ({note}).\n\
         exec {bin} \"$@\"\n",
        bin = crate::ext_manager::shlex_quote(&bin),
    )
}

/// Лаунчер установки в систему (`self.LIB_DIR` фиксирован).
pub fn system_launcher(lib_dir: &str) -> String {
    launcher_script(lib_dir, "установка в систему")
}

/// Лаунчер установки в домашнюю папку.
pub fn user_launcher(lib_dir: &str) -> String {
    launcher_script(lib_dir, "установка в домашнюю папку")
}

// ============================================================================
// Режим установки и пути
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallMode {
    System,
    User,
    Portable,
}

impl InstallMode {
    pub fn as_str(self) -> &'static str {
        match self {
            InstallMode::System => "system",
            InstallMode::User => "user",
            InstallMode::Portable => "portable",
        }
    }

    /// Порт валидации `mode` в `RexonInstaller.__init__` (None — ValueError).
    pub fn parse(s: &str) -> Option<InstallMode> {
        match s {
            "system" => Some(InstallMode::System),
            "user" => Some(InstallMode::User),
            "portable" => Some(InstallMode::Portable),
            _ => None,
        }
    }
}

/// Дистрибутивы с неизменяемой системной частью: `/usr` и `/usr/local`
/// лежат в read-only снапшоте, а обновление атомарное (A/B), поэтому всё
/// записанное туда пропадает при следующем обновлении системы.
///
/// SteamOS: юнит `usr-local.mount` удалён из offload-схемы (коммит `d953c8b`
/// в `steamos-customizations`, янв. 2023) — `/usr/local` больше не отдельный
/// раздел в `$HOME`, а часть образа. Персистентны только `$HOME`, Flatpak и
/// offload-каталоги.
const IMMUTABLE_SYSTEM_DISTROS: &[&str] = &["steamos", "holoiso", "bazzite"];

/// Read-only ли системная часть у этого дистрибутива (по `ID` из os-release).
pub fn has_immutable_system(distro_id: &str) -> bool {
    let id = distro_id.trim().to_ascii_lowercase();
    IMMUTABLE_SYSTEM_DISTROS.contains(&id.as_str())
}

/// Переменная, которую выставляет лаунчер пакета дистрибутива
/// (`packaging/aur/PKGBUILD`, `/usr/bin/rexon`).
pub const PACKAGED_ENV: &str = "REXON_PACKAGED";

/// Порт `RexonInstaller.detect_default_mode`. Steam (Deck/Big Picture/клиент)
/// → portable (запуск без установки). `get_env` возвращает значение
/// переменной окружения (None — не задана), `distro_id` — `ID` из
/// os-release (пустая строка, если не определён).
///
/// На SteamOS в режиме рабочего стола (Rexon запущен из меню, а не через
/// Steam) системный режим выбирать НЕЛЬЗЯ: `/usr/local` и `/usr/share` —
/// часть read-only образа, установка либо падает, либо исчезает при
/// следующем обновлении SteamOS. Там ставим в домашнюю папку — она
/// переживает атомарное обновление.
///
/// Пакет дистрибутива ([`PACKAGED_ENV`] выставляет его лаунчер) — тоже
/// portable: файлы уже разложил пакетный менеджер, и предлагать установку в
/// `/usr/local` поверх пакета значило бы показывать диалог установщика при
/// каждом запуске. Прежде эту проверку вставлял sed в PKGBUILD; когда выбор
/// режима переехал в ядро, sed потерял свою строку и сборка пакета встала.
pub fn detect_default_mode(
    get_env: impl Fn(&str) -> Option<String>,
    distro_id: &str,
) -> InstallMode {
    if get_env(PACKAGED_ENV).is_some()
        || get_env("SteamGameId").is_some()
        || get_env("SteamAppId").is_some()
        || get_env("STEAM_COMPAT_DATA_PATH").is_some()
    {
        return InstallMode::Portable;
    }
    if has_immutable_system(distro_id) {
        return InstallMode::User;
    }
    InstallMode::System
}

/// Почему системная установка недоступна — человеческий текст для интерфейса
/// (правило #13), либо `None`, если она доступна.
///
/// Язык — язык каталога перевода, как у [`system_mode_blocked_reason_in`]
/// (прежде здесь была закреплённая русская форма).
pub fn system_mode_blocked_reason(distro_id: &str) -> Option<String> {
    system_mode_blocked_reason_in(distro_id, crate::distro_compat::Lang::En)
}

/// Пути установки по режиму. В portable — все `None` (Rexon работает из
/// текущей папки). Порт `RexonInstaller.__init__`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallPaths {
    pub install_dir: Option<String>,
    pub lib_dir: Option<String>,
    pub desktop_dir: Option<String>,
    pub icon_root: Option<String>,
    pub pixmap_dir: Option<String>,
    pub gnome_ext_dir: Option<String>,
    pub cinnamon_ext_dir: Option<String>,
}

/// Порт вычисления путей в `RexonInstaller.__init__`.
///
/// * `home` — `os.path.expanduser("~")` (нужен для user-режима).
/// * `xdg_data_home` — значение `XDG_DATA_HOME` (None → `{home}/.local/share`).
pub fn install_paths(
    mode: InstallMode,
    home: &str,
    xdg_data_home: Option<&str>,
) -> InstallPaths {
    match mode {
        InstallMode::System => InstallPaths {
            install_dir: Some("/usr/local/bin".into()),
            lib_dir: Some("/usr/local/lib/rexon".into()),
            desktop_dir: Some("/usr/share/applications".into()),
            icon_root: Some("/usr/share/icons/hicolor".into()),
            pixmap_dir: Some("/usr/share/pixmaps".into()),
            gnome_ext_dir: Some("/usr/share/gnome-shell/extensions".into()),
            cinnamon_ext_dir: Some("/usr/share/cinnamon/extensions".into()),
        },
        InstallMode::User => {
            let xdg = match xdg_data_home {
                Some(x) => x.to_string(),
                None => format!("{home}/.local/share"),
            };
            InstallPaths {
                install_dir: Some(format!("{home}/.local/bin")),
                lib_dir: Some(format!("{home}/.local/lib/rexon")),
                desktop_dir: Some(format!("{xdg}/applications")),
                icon_root: Some(format!("{xdg}/icons/hicolor")),
                pixmap_dir: Some(format!("{xdg}/pixmaps")),
                gnome_ext_dir: Some(format!("{xdg}/gnome-shell/extensions")),
                cinnamon_ext_dir: Some(format!("{xdg}/cinnamon/extensions")),
            }
        }
        InstallMode::Portable => InstallPaths {
            install_dir: None,
            lib_dir: None,
            desktop_dir: None,
            icon_root: None,
            pixmap_dir: None,
            gnome_ext_dir: None,
            cinnamon_ext_dir: None,
        },
    }
}

/// Порт `check_if_installed` для system/user режимов: rexon-launcher +
/// каталог lib + rexon.desktop существуют. `exists` — ФС-проверка (исполнитель).
pub fn system_or_user_installed(
    paths: &InstallPaths,
    exists: impl Fn(&str) -> bool,
    is_dir: impl Fn(&str) -> bool,
) -> bool {
    let (Some(install), Some(lib), Some(desktop)) =
        (&paths.install_dir, &paths.lib_dir, &paths.desktop_dir)
    else {
        return false;
    };
    exists(&format!("{install}/rexon"))
        && is_dir(lib)
        && exists(&format!("{desktop}/rexon.desktop"))
}

// ============================================================================
// Первый запуск: предлагать ли мастер установки
// ============================================================================
//
// Одно решение на оба входа: Rust-окно и Python-окно (`qt_main/app.py`)
// обязаны отвечать одинаково, иначе мастер всплывёт в одном и промолчит в
// другом (правило #12).
//
// Решение ЧИСТОЕ: всё, что видно только на живой машине (переменные, путь
// своего исполняемого файла, наличие файлов, симлинк каталога), приходит
// через [`FirstRunProbe`]; настройки — готовой картой. Живая проба —
// [`LiveFirstRunProbe`], вход по живой машине — [`first_run_offer_live`].
//
// ⭐ Починки против Python-гейта (`not RexonInstaller().installed`):
//   * «Не сейчас» не запоминалось — мастер всплывал при КАЖДОМ запуске.
//     Теперь есть «больше не спрашивать» ([`KEY_FIRST_RUN_DECLINED`]).
//   * Rust-окно из пакета не получает [`PACKAGED_ENV`]: ярлык пакета зовёт
//     `/usr/lib/rexon/rexon-main-window` напрямую. Перенесённый как есть
//     гейт показывал бы мастер каждому, кто поставил пакет. Теперь пакет
//     узнаётся и по месту своего файла ([`packaged_location`]).
//   * Проверялся только режим по умолчанию: на Arch это system, и Rexon,
//     установленный в домашнюю папку, получал мастер при каждом запуске.
//     Теперь «установлено» — в любом из двух режимов.
//   * Дерево разработчика не распознавалось вовсе.

/// Переменная для приёмки глазами: [`FIRST_RUN_FORCE`] — показать мастер
/// всегда, [`FIRST_RUN_NEVER`] — не показывать никогда. Иное значение
/// не действует.
pub const FIRST_RUN_ENV: &str = "REXON_FIRST_RUN";
/// Значение [`FIRST_RUN_ENV`]: предложить мастер при любых условиях.
pub const FIRST_RUN_FORCE: &str = "force";
/// Значение [`FIRST_RUN_ENV`]: не предлагать мастер.
pub const FIRST_RUN_NEVER: &str = "never";

/// Ключ настроек «больше не предлагать установку». Живёт в том же
/// `settings.json`, что и ключи обучения (`onboarding::KEY_ENABLED`), и
/// читается тем же способом: не булево значение — «не отказывался».
///
/// ⚠️ Прежний ключ `wizard_done` из `settings::defaults` сюда НЕ годится:
/// его никто не читает, а что он значил раньше, неизвестно — взять его
/// значило бы молча спрятать мастер у тех, у кого там когда-то стояло `true`.
pub const KEY_FIRST_RUN_DECLINED: &str = "installer_first_run_declined";

/// Переменные, которые выставляет Steam (Deck, Big Picture, клиент), —
/// при них Rexon работает без установки. Тот же набор, что в
/// [`detect_default_mode`].
const STEAM_ENVS: &[&str] = &["SteamGameId", "SteamAppId", "STEAM_COMPAT_DATA_PATH"];

/// Каталоги, файлы в которых разложил пакетный менеджер: всё под `/usr`,
/// кроме `/usr/local` (по FHS это территория дистрибутива — `/usr/bin`,
/// `/usr/lib/rexon`, `/usr/lib64/…`), хранилище NixOS и корень
/// приложения Flatpak.
const PACKAGE_PREFIXES: &[&str] = &["/usr/", "/nix/store/", "/app/"];
/// Исключение из [`PACKAGE_PREFIXES`]: сюда ставит ручная установка.
const LOCAL_PREFIX: &str = "/usr/local/";

/// Что решение видит на живой машине. Каждая проба — одно наблюдение без
/// выводов: выводы делает [`first_run_offer`].
pub trait FirstRunProbe {
    /// Значение переменной окружения (`None` — не задана).
    fn env(&self, key: &str) -> Option<String>;
    /// `ID` из os-release (пустая строка — не определён).
    fn distro_id(&self) -> String;
    /// Где лежит сам запущенный Rexon: бинарь окна, а у Python-входа —
    /// файл модуля (НЕ интерпретатор: `/usr/bin/python3` выглядит пакетом).
    fn own_path(&self) -> Option<String>;
    /// Корень дерева разработчика, внутри которого лежит [`Self::own_path`]
    /// (пусто — не в дереве).
    fn repo_root(&self) -> String;
    /// Корень дерева разработчика, если `lib_dir` — симлинк в дерево.
    fn dev_tree_behind(&self, lib_dir: &str) -> Option<String>;
    /// Существует ли путь.
    fn exists(&self, path: &str) -> bool;
    /// Каталог ли это.
    fn is_dir(&self, path: &str) -> bool;
}

/// Почему мастер НЕ предлагается.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirstRunSkip {
    /// `REXON_FIRST_RUN=never`.
    ForcedNever,
    /// Лаунчер пакета выставил [`PACKAGED_ENV`].
    PackagedEnv,
    /// Свой файл лежит там, куда кладёт только пакетный менеджер.
    PackagedLocation(String),
    /// Запуск из Steam — работает без установки.
    Steam(&'static str),
    /// Дерево разработчика: свой файл внутри него либо установленный
    /// каталог — симлинк в него.
    DevTree(String),
    /// Уже установлен в систему.
    InstalledSystem,
    /// Уже установлен в домашнюю папку.
    InstalledUser,
    /// Человек ответил «больше не спрашивать».
    Declined,
}

/// Решение «предлагать ли мастер установки».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FirstRunOffer {
    /// Предложить.
    Offer {
        /// Режим, выбранный заранее: system, а на неизменяемой системе — user.
        default_mode: InstallMode,
        /// Почему установка в систему недоступна (текст для человека).
        system_blocked: Option<String>,
        /// Показано по `REXON_FIRST_RUN=force`, а не по условиям машины.
        forced: bool,
    },
    /// Не предлагать.
    Skip(FirstRunSkip),
}

impl FirstRunOffer {
    /// Предлагать ли мастер.
    pub fn is_offer(&self) -> bool {
        matches!(self, FirstRunOffer::Offer { .. })
    }

    /// Строка для журнала: решение и причина.
    pub fn log_line(&self) -> String {
        match self {
            FirstRunOffer::Offer { default_mode, forced, .. } => format!(
                "мастер установки: предложить (режим {}{})",
                default_mode.as_str(),
                if *forced { ", REXON_FIRST_RUN=force" } else { "" }
            ),
            FirstRunOffer::Skip(reason) => {
                let why = match reason {
                    FirstRunSkip::ForcedNever => "REXON_FIRST_RUN=never".to_string(),
                    FirstRunSkip::PackagedEnv => format!("пакет дистрибутива ({PACKAGED_ENV})"),
                    FirstRunSkip::PackagedLocation(p) => {
                        format!("пакет дистрибутива (файл в {p})")
                    }
                    FirstRunSkip::Steam(var) => format!("запуск из Steam ({var})"),
                    FirstRunSkip::DevTree(root) => format!("дерево разработчика ({root})"),
                    FirstRunSkip::InstalledSystem => "уже установлен в систему".to_string(),
                    FirstRunSkip::InstalledUser => {
                        "уже установлен в домашнюю папку".to_string()
                    }
                    FirstRunSkip::Declined => {
                        format!("человек ответил «больше не спрашивать» ({KEY_FIRST_RUN_DECLINED})")
                    }
                };
                format!("мастер установки: не предлагать — {why}")
            }
        }
    }
}

/// Значение [`FIRST_RUN_ENV`]: `Some(true)` — force, `Some(false)` — never,
/// `None` — не задано или непонятно.
pub fn first_run_override(value: Option<&str>) -> Option<bool> {
    let v = value?.trim();
    if v.eq_ignore_ascii_case(FIRST_RUN_FORCE) {
        Some(true)
    } else if v.eq_ignore_ascii_case(FIRST_RUN_NEVER) {
        Some(false)
    } else {
        None
    }
}

/// Лежит ли свой файл там, куда кладёт только пакетный менеджер. Хвост
/// « (deleted)», который ядро дописывает к пути пересобранного бинаря,
/// снимается. Относительный путь ничего не доказывает — `false`.
pub fn packaged_location(own_path: &str) -> bool {
    let path = own_path
        .strip_suffix(" (deleted)")
        .unwrap_or(own_path);
    !path.starts_with(LOCAL_PREFIX) && PACKAGE_PREFIXES.iter().any(|p| path.starts_with(p))
}

/// Отказался ли человек от мастера — по содержимому настроек. Не булево
/// значение (файл правили руками) читается как «не отказывался»: лишний
/// раз предложить безопаснее, чем молча спрятать установку навсегда.
pub fn first_run_declined_from(settings: &serde_json::Map<String, serde_json::Value>) -> bool {
    settings
        .get(KEY_FIRST_RUN_DECLINED)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// Режим, который мастер выбирает заранее. Portable здесь не бывает: там,
/// где он уместен (пакет, Steam), мастер не предлагается.
fn offered_mode(distro_id: &str) -> InstallMode {
    if has_immutable_system(distro_id) {
        InstallMode::User
    } else {
        InstallMode::System
    }
}

/// Решение «предлагать ли мастер установки».
///
/// Порядок проверок (первая сработавшая даёт ответ):
/// `REXON_FIRST_RUN` → пакет по переменной → пакет по месту файла → Steam →
/// дерево разработчика → установлен в систему → в домашнюю папку →
/// «больше не спрашивать» → предложить.
pub fn first_run_offer(
    probe: &dyn FirstRunProbe,
    settings: &serde_json::Map<String, serde_json::Value>,
) -> FirstRunOffer {
    let distro_id = probe.distro_id();
    let offer = |forced: bool| FirstRunOffer::Offer {
        default_mode: offered_mode(&distro_id),
        system_blocked: system_mode_blocked_reason(&distro_id),
        forced,
    };
    match first_run_override(probe.env(FIRST_RUN_ENV).as_deref()) {
        Some(true) => return offer(true),
        Some(false) => return FirstRunOffer::Skip(FirstRunSkip::ForcedNever),
        None => {}
    }
    if probe.env(PACKAGED_ENV).is_some() {
        return FirstRunOffer::Skip(FirstRunSkip::PackagedEnv);
    }
    if let Some(own) = probe.own_path().filter(|p| packaged_location(p)) {
        return FirstRunOffer::Skip(FirstRunSkip::PackagedLocation(own));
    }
    if let Some(var) = STEAM_ENVS.iter().find(|v| probe.env(v).is_some()) {
        return FirstRunOffer::Skip(FirstRunSkip::Steam(var));
    }

    let home = probe.env("HOME").unwrap_or_default();
    let xdg_data = probe.env("XDG_DATA_HOME").filter(|x| !x.is_empty());
    let user = (!home.is_empty())
        .then(|| install_paths(InstallMode::User, &home, xdg_data.as_deref()));

    let root = probe.repo_root();
    if !root.is_empty() {
        return FirstRunOffer::Skip(FirstRunSkip::DevTree(root));
    }
    let libs = std::iter::once(LIB_DIR.to_string())
        .chain(user.as_ref().and_then(|u| u.lib_dir.clone()));
    for lib in libs {
        if let Some(root) = probe.dev_tree_behind(&lib) {
            return FirstRunOffer::Skip(FirstRunSkip::DevTree(root));
        }
    }

    let exists = |p: &str| probe.exists(p);
    let is_dir = |p: &str| probe.is_dir(p);
    let system = install_paths(InstallMode::System, &home, None);
    if system_or_user_installed(&system, exists, is_dir) {
        return FirstRunOffer::Skip(FirstRunSkip::InstalledSystem);
    }
    if user
        .as_ref()
        .is_some_and(|u| system_or_user_installed(u, exists, is_dir))
    {
        return FirstRunOffer::Skip(FirstRunSkip::InstalledUser);
    }
    if first_run_declined_from(settings) {
        return FirstRunOffer::Skip(FirstRunSkip::Declined);
    }
    offer(false)
}

/// Живая проба. Два входа различаются только тем, ЧТО считать своим файлом.
#[derive(Debug, Clone)]
pub struct LiveFirstRunProbe {
    own_path: Option<String>,
    repo_root: String,
}

impl LiveFirstRunProbe {
    /// Для бинаря Rust-окна: свой исполняемый файл и дерево вокруг него.
    pub fn for_this_binary() -> Self {
        Self {
            own_path: std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
            repo_root: crate::cli::repo_root(),
        }
    }

    /// Для входа, у которого исполняемый файл — чужой (Python-окно:
    /// `current_exe` там `/usr/bin/python3`). `own_path` — настоящий путь
    /// своего модуля (`os.path.realpath(__file__)`), `repo_root` — корень
    /// дерева разработчика вокруг него или пустая строка.
    pub fn for_path(own_path: Option<String>, repo_root: String) -> Self {
        Self { own_path, repo_root }
    }
}

impl FirstRunProbe for LiveFirstRunProbe {
    fn env(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
    fn distro_id(&self) -> String {
        crate::system::DistroDetector::detect_live().distro_id()
    }
    fn own_path(&self) -> Option<String> {
        self.own_path.clone()
    }
    fn repo_root(&self) -> String {
        self.repo_root.clone()
    }
    fn dev_tree_behind(&self, lib_dir: &str) -> Option<String> {
        dev_root_behind(lib_dir)
    }
    fn exists(&self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }
    fn is_dir(&self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }
}

/// Решение по живой машине для бинаря Rust-окна. Настройки только
/// читаются: каталог конфигурации ради ответа не создаётся.
pub fn first_run_offer_live() -> FirstRunOffer {
    let settings = crate::settings::load_settings(&crate::settings::settings_file());
    first_run_offer(&LiveFirstRunProbe::for_this_binary(), &settings)
}

/// Ответ на мастер «Не сейчас». `remember` — отмечено «больше не
/// спрашивать»: тогда отказ записывается и сохраняется сразу. Без отметки
/// ничего не пишется — мастер предложится при следующем запуске.
///
/// Менеджер настроек приходит снаружи, как у `onboarding::session_answer`:
/// сверки гоняют функцию на временном файле.
pub fn session_first_run_decline(
    manager: &mut crate::settings::SettingsManager,
    remember: bool,
) -> std::io::Result<()> {
    if !remember {
        return Ok(());
    }
    manager.set(KEY_FIRST_RUN_DECLINED, serde_json::Value::Bool(true))
}

// ============================================================================
// Тексты мастера первого запуска
// ============================================================================
//
// Все подписи мастера (приветствие с выбором режима, ход, итог) и строки хода
// и итога установщика живут здесь. Окно (Rust-мост и QML) и исполнитель
// установки берут их отсюда — второй копии букв нет (правило #12).
//
// ⭐ Починки против Python-мастера (`rexon/installer.py`, `QtInstallerWindow`):
//   * пути `/usr/local/` и `~/.local/` стояли в подписях режимов — теперь
//     режим назван по-человечески, путь ушёл в подсказку (поле `hint`);
//   * служебные слова покупателю: «Иконки/extensions через pkexec»,
//     «PySide6 / qt6-webengine / layer-shell-qt должны быть установлены
//     отдельно», «User-install», «Portable режим», «$PATH — добавьте в
//     ~/.bashrc», «(pkexec)», «попробуйте выполнить из терминала», сырая
//     команда установки зависимостей в журнале;
//   * «некоторые функции будут недоступны» не говорило, какие
//     ([`without_install_limits`]);
//   * любой сбой корневого скрипта выдавался за «Установка отменена
//     пользователем» — отказ пароля, нечем спросить пароль, таймаут и сбой
//     шага теперь разные исходы со своими советами ([`InstallFailure`]);
//   * «Пропустить» не запоминалось — есть «Больше не спрашивать»
//     ([`KEY_FIRST_RUN_DECLINED`]);
//   * о правиле «без пароля для действий Rexon» человека не спрашивали —
//     теперь это отдельное согласие, по умолчанию выключенное
//     ([`PASSWORDLESS_CONSENT_DEFAULT`]);
//   * строка «Зависимости будут установлены вместе с Rexon» выводилась
//     дважды — теперь одна ([`dependencies_with_rexon_notice`]).
//
// Поля `hint` — техническое уточнение для всплывающей подсказки: пути
// допустимы только там. Поле `details` — сырой вывод для разработчика.
// Сторож — `wizard_texts_tests`.
//
// ⭐ 29.09.2026 подписи переехали на каталог перевода: английский — ключ
// (`msgid`), русский — дословно прежний в `po/ru.po` (снимок «как было» —
// `tests/golden/installer_texts_ru_before.json`). Язык один на процесс и
// выбирается каталогом; параметр `Lang` у функций ниже больше ничего не
// решает и остался ради вызывающих вне этого захода (окно мастера, меню,
// мост в Python). Снимется вместе с `distro_compat::Lang`.

use crate::distro_compat::Lang;
use crate::i18n::{n_, tr, trf};

/// Согласие «не спрашивать пароль для действий Rexon» по умолчанию выключено:
/// разрешение снимает защиту паролем, включить его может только сам человек.
pub const PASSWORDLESS_CONSENT_DEFAULT: bool = false;

/// Режимы, из которых мастер даёт выбрать, в порядке показа. Переносного
/// режима здесь нет: там, где он уместен (пакет, Steam), мастер не
/// предлагается вовсе ([`first_run_offer`]).
pub const WIZARD_MODES: [InstallMode; 2] = [InstallMode::System, InstallMode::User];

/// Почему установка в систему недоступна — на языке интерфейса, либо `None`.
/// Решение то же, что у [`system_mode_blocked_reason`].
pub fn system_mode_blocked_reason_in(distro_id: &str, _lang: Lang) -> Option<String> {
    if !has_immutable_system(distro_id) {
        return None;
    }
    Some(tr(
        "On this system (for example, Steam Deck) system folders are \
         write-protected and wiped by every system update. Rexon will be \
         installed into your home folder so that it survives updates.",
    ))
}

/// Название режима установки для человека.
pub fn mode_title(mode: InstallMode, _lang: Lang) -> String {
    match mode {
        InstallMode::System => {
            tr("For everyone on this computer")
        }
        InstallMode::User => tr("Just for you"),
        InstallMode::Portable => tr("Standalone folder"),
    }
}

/// Что даёт режим и чего он стоит — одна-две фразы под названием.
pub fn mode_description(mode: InstallMode, _lang: Lang) -> String {
    match mode {
        InstallMode::System => tr(
            "Rexon will appear in the application menu for everyone who uses \
             this computer. The administrator password is needed once. Missing \
             components are installed along the way, and Rexon will ask for the \
             password less often when changing the system.",
        ),
        InstallMode::User => tr(
            "Rexon will appear in the application menu for you only. No password \
             is needed, but this option does not install missing components either.",
        ),
        InstallMode::Portable => tr(
            "Rexon runs straight from the folder it is in and installs nothing \
             into the system.",
        ),
    }
}

/// Подсказка к режиму: куда лягут файлы. Пути — из [`install_paths`], своих
/// копий здесь нет. У переносного режима путей нет — пустая строка.
pub fn mode_hint(
    mode: InstallMode,
    home: &str,
    xdg_data_home: Option<&str>,
    _lang: Lang,
) -> String {
    let paths = install_paths(mode, home, xdg_data_home);
    match (paths.lib_dir, paths.desktop_dir) {
        // TRANSLATORS: tooltip of an install option; %1 and %2 are folder paths.
        (Some(lib), Some(desktop)) => trf("Program files: %1\nMenu shortcut: %2", &[&lib, &desktop]),
        _ => String::new(),
    }
}

/// Один режим на странице приветствия.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WizardModeTexts {
    pub mode: InstallMode,
    pub title: String,
    pub description: String,
    /// Куда лягут файлы — для всплывающей подсказки.
    pub hint: String,
    /// Почему режим выбрать нельзя; пусто — можно.
    pub blocked: String,
}

/// Страница приветствия.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WizardWelcomeTexts {
    pub window_title: String,
    pub title: String,
    /// Зачем устанавливать.
    pub description: String,
    /// Что даёт установка — по пункту на строку.
    pub benefits: Vec<String>,
    pub modes_title: String,
    pub modes: Vec<WizardModeTexts>,
    pub install_button: String,
    pub skip_button: String,
    /// Галочка рядом с «Пропустить»; ключ — [`KEY_FIRST_RUN_DECLINED`].
    pub dont_ask_again: String,
    /// Что будет, если отметить галочку.
    pub dont_ask_again_hint: String,
}

/// Заголовок окна мастера.
pub fn wizard_window_title(_lang: Lang) -> String {
    tr("Install Rexon")
}

/// Что не будет работать без установки. Этим списком отвечает итог неудачи
/// и отказ от установки — вместо прежнего «некоторые функции будут
/// недоступны».
pub fn without_install_limits(_lang: Lang) -> Vec<String> {
    vec![
        tr(
            "Rexon will not appear in the application menu — you will have to \
             start it from the folder it is in.",
        ),
        tr(
            "The menu will not have the emergency item for restoring the \
             desktop appearance.",
        ),
        tr(
            "Live wallpapers will not work in GNOME and Cinnamon: the add-on \
             they need is set up only by the installation.",
        ),
        tr(
            "Every system change (desktop environments, login and boot screen \
             themes) will ask for the administrator password again.",
        ),
    ]
}

/// Страница приветствия. `distro_id` решает, доступна ли установка в
/// систему; `home` и `xdg_data_home` — только для подсказок с путями.
pub fn welcome_texts(
    distro_id: &str,
    home: &str,
    xdg_data_home: Option<&str>,
    lang: Lang,
) -> WizardWelcomeTexts {
    let modes = WIZARD_MODES
        .iter()
        .map(|&mode| WizardModeTexts {
            mode,
            title: mode_title(mode, lang),
            description: mode_description(mode, lang),
            hint: mode_hint(mode, home, xdg_data_home, lang),
            blocked: match mode {
                InstallMode::System => {
                    system_mode_blocked_reason_in(distro_id, lang).unwrap_or_default()
                }
                _ => String::new(),
            },
        })
        .collect();
    WizardWelcomeTexts {
        window_title: wizard_window_title(lang),
        title: tr("Welcome to Rexon"),
        description: tr(
            "Rexon is a desktop environment manager for Linux. Install it to use \
             it like any other application:",
        ),
        benefits: vec![
            tr(
                "Rexon appears in the application menu, together with an \
                 emergency item for restoring the desktop appearance.",
            ),
            tr("Live wallpapers work in GNOME and Cinnamon too."),
        ],
        modes_title: tr("Install for"),
        modes,
        install_button: tr("Install"),
        skip_button: tr("Skip"),
        dont_ask_again: tr("Don't ask again"),
        dont_ask_again_hint: tr(
            "This window will not appear again. Rexon will keep running from \
             the folder it is in.",
        ),
    }
}

// ── Кнопка «Установить Rexon в систему…» на странице «Настройки» ────────────
//
// ⭐ Решение Wehrwolfmann 15.09.2026: кнопка стоит рядом с «Пройти обучение
// заново». Она снимает ответ «больше не спрашивать»
// ([`KEY_FIRST_RUN_DECLINED`]) и открывает тот же мастер, поэтому и
// показывается, пока Rexon не установлен: решение [`first_run_offer`] БЕЗ
// учёта отказа. У установленного Rexon (пакет, Steam, установка для всех
// или для себя) кнопки нет вовсе — нажимать её было бы не на что.

/// Подпись кнопки на странице «Настройки».
pub fn settings_install_button(_lang: Lang) -> String {
    tr("Install Rexon on this computer…")
}

/// Пояснение под кнопкой: зачем её нажимать.
pub fn settings_install_hint(_lang: Lang) -> String {
    tr("Rexon will appear in the application menu and work like any other program.")
}

/// Ответ кнопки: окно установки открылось.
pub fn settings_install_started(_lang: Lang) -> String {
    tr("Opening the Rexon installation window.")
}

/// Ответ кнопки, когда открывать окно установки некому: вкладка «Настройки»
/// показана без главного окна Rexon. Тем же отвечает «Пройти обучение
/// заново» (`qt_settings`, ключ `ob.no_main_window`).
pub fn settings_install_no_window(_lang: Lang) -> String {
    tr(
        "Installation is available only in the main Rexon window — open the \
         whole window, not a single tab.",
    )
}

/// Отдельный шаг «не спрашивать пароль для действий Rexon».
///
/// Честность текста сверена с самим правилом
/// ([`crate::system_plan::passwordless_rules_js`]): без пароля — только
/// перечисленные действия ([`crate::system_plan::PASSWORDLESS_ACTIONS`]),
/// только для группы администраторов, только в активном локальном сеансе.
/// Установка и удаление пакетов (`org.rexon.pkg`) сюда НЕ входят. Снимается удалением Rexon
/// (`uninstall_system_paths` содержит путь правила).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordlessConsentTexts {
    pub title: String,
    /// Подпись галочки.
    pub checkbox: String,
    pub description: String,
    /// Что именно разрешается.
    pub what: String,
    /// Кому.
    pub who: String,
    /// Чем это рискованно.
    pub risk: String,
    /// Как отменить.
    pub undo: String,
    /// Где лежит разрешение — для всплывающей подсказки.
    pub hint: String,
    /// Состояние галочки при показе — [`PASSWORDLESS_CONSENT_DEFAULT`].
    pub default_on: bool,
}

/// Тексты согласия «не спрашивать пароль для действий Rexon». Шаг есть
/// только у установки для всех пользователей: у установки «только для вас»
/// разрешение не ставится.
///
/// ⭐ Починка 29.09.2026: подпись пункта меню вставлялась в русскую фразу
/// всегда на языке каталога («Uninstall Rexon» посреди русского текста при
/// английском интерфейсе), а английская фраза пункт не называла вовсе.
/// Теперь фраза одна, из каталога, и пункт в ней — на том же языке.
pub fn passwordless_consent_texts(_lang: Lang) -> PasswordlessConsentTexts {
    let menu = crate::window_menu::MENU_BUTTON;
    let uninstall_label = crate::window_menu::menu_items()
        .into_iter()
        .find(|i| i.id == crate::window_menu::ID_UNINSTALL)
        .map(|i| i.label)
        .unwrap_or_default();
    PasswordlessConsentTexts {
        title: tr("Don't ask for the password for Rexon actions"),
        checkbox: tr("Allow Rexon to change the system without a password"),
        description: tr(
            "Normally Rexon asks for the administrator password before changing \
             the system. With this permission on, Rexon actions will not ask for \
             the password. The permission is off by default.",
        ),
        // Правду о разрешении говорит `system_plan::PASSWORDLESS_ACTIONS`:
        // установка и удаление произвольных программ туда не входят.
        what: tr(
            "What is allowed: only installing desktop environments from Rexon's \
             list. Login and boot screen themes and installing or removing other \
             software still ask for the password.",
        ),
        who: tr(
            "Who: members of the administrators group, and only while they are \
             sitting at this computer. Remote logins are asked for the password \
             as usual.",
        ),
        risk: tr(
            "The risk: any program running under your account will be able to \
             install desktop environments from Rexon's list through Rexon (and \
             turn on their login screen if the system has none) without your \
             confirmation.",
        ),
        // TRANSLATORS: %1 is the menu button (☰), %2 is the menu item that
        // uninstalls Rexon, both exactly as the window shows them.
        undo: trf(
            "How to undo: uninstall Rexon from its menu (%1 → “%2”) — the \
             permission is removed together with the application.",
            &[&menu, &uninstall_label],
        ),
        // Группа — по семейству дистрибутива (`system_plan::polkit_admin_groups`);
        // название группы Debian в подсказку не выносится: сторож жаргона.
        hint: {
            let arch = crate::system_plan::polkit_admin_groups(PkgFamily::Arch).join(", ");
            // TRANSLATORS: tooltip; %1 is the path of the permission file, %2 the
            // name(s) of the administrators group (e.g. "wheel").
            trf(
                "Permission file: %1\nAdministrators group: %2 (Debian and Ubuntu use their own administrators group)",
                &[&crate::system_plan::REXON_POLKIT_RULES_PATH, &arch],
            )
        },
        default_on: PASSWORDLESS_CONSENT_DEFAULT,
    }
}

/// Пояснение сухого прогона (`REXON_INSTALL_DRY_RUN`): мастер проходится
/// целиком, но машину не трогает.
///
/// ⚠️ Нужно на странице приветствия: сухой прогон доходит до итога «Готово —
/// Rexon установлен», ничего не установив, и без этой строки итог читался бы
/// как настоящая установка. Строка живёт здесь, а не в разметке, по тому же
/// правилу, что и остальные подписи мастера (правило #12).
pub fn dry_run_notice(_lang: Lang) -> String {
    tr(
        "Trial run: Rexon will walk through the wizard to the end, but will \
         install nothing and change nothing on this computer.",
    )
}

/// Страница хода установки.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WizardProgressTexts {
    pub title: String,
    pub subtitle: String,
}

/// Тексты страницы хода. У установки для всех пользователей подзаголовок
/// заранее говорит про окно пароля — чтобы оно не застало врасплох.
pub fn progress_texts(mode: InstallMode, _lang: Lang) -> WizardProgressTexts {
    WizardProgressTexts {
        title: tr("Installing Rexon"),
        subtitle: match mode {
            InstallMode::System => tr(
                "Please wait. When the password window appears, enter the \
                 administrator password.",
            ),
            _ => tr("Please wait."),
        },
    }
}

/// Шаг установки — для журнала хода и для объяснения сбоя.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStep {
    /// Копия файлов программы (Python: «Копирование пакета rexon/...»).
    CopyFiles,
    /// Команда запуска (Python: «Создание лаунчера...»).
    Launcher,
    /// Ярлыки в меню (Python: «Создание .desktop файла...»).
    MenuShortcuts,
    /// Значки (Python: «Копирование иконок (все размеры + SVG)...»).
    Icons,
    /// Помощники «пароль реже» (Python: «Polkit helper (один пароль за сессию)...»).
    FewerPasswords,
    /// Недостающие компоненты (Python: «Зависимости: <сырая команда>»).
    Dependencies,
    /// Запись в систему одним паролем (Python: «Установка в систему (один пароль)...»).
    SystemInstall,
    /// Дополнения обоев GNOME и Cinnamon (Python: «GNOME + Cinnamon extensions»).
    WallpaperAddons,
    /// Обновление меню приложений (Python: кэши без подписи).
    MenuRefresh,
    /// Папки настроек и резервных копий (Python: голые пути).
    UserFolders,
}

impl InstallStep {
    /// Все шаги в порядке установки.
    pub const ALL: [InstallStep; 10] = [
        InstallStep::CopyFiles,
        InstallStep::Launcher,
        InstallStep::MenuShortcuts,
        InstallStep::Icons,
        InstallStep::FewerPasswords,
        InstallStep::Dependencies,
        InstallStep::SystemInstall,
        InstallStep::WallpaperAddons,
        InstallStep::MenuRefresh,
        InstallStep::UserFolders,
    ];

    /// Название шага со строчной буквы — для фразы «Не получилось: …».
    pub fn name(self, _lang: Lang) -> String {
        match self {
            InstallStep::CopyFiles => {
                tr("preparing the program files")
            }
            InstallStep::Launcher => {
                tr("preparing the start command")
            }
            InstallStep::MenuShortcuts => tr("creating application menu shortcuts"),
            InstallStep::Icons => tr("preparing the icons"),
            InstallStep::FewerPasswords => tr("setting up fewer password prompts"),
            InstallStep::Dependencies => tr("installing missing components"),
            InstallStep::SystemInstall => {
                tr("installing into the system")
            }
            InstallStep::WallpaperAddons => tr("installing live wallpaper add-ons for GNOME and Cinnamon"),
            InstallStep::MenuRefresh => {
                tr("refreshing the application menu")
            }
            InstallStep::UserFolders => tr("creating folders for settings and backups"),
        }
    }

    /// Строка журнала хода: «Подготовка файлов программы…».
    pub fn progress_line(self, lang: Lang) -> String {
        let name = self.name(lang);
        let mut chars = name.chars();
        match chars.next() {
            Some(first) => format!("{}{}…", first.to_uppercase(), chars.as_str()),
            None => String::new(),
        }
    }
}

/// Значков в поставке нет (Python: «⚠ Иконки не найдены в проекте»).
pub fn icons_missing_notice(_lang: Lang) -> String {
    tr("Program icons were not found — Rexon will have a generic icon in the menu.")
}

/// Меню не обновилось сразу (Python: «Ярлык появится в меню после
/// перезахода в систему (<пакет>)»). Имя пакета — в журнал разработчика.
pub fn menu_refresh_later_notice(_lang: Lang) -> String {
    tr("The Rexon shortcut will appear in the application menu after you log in again.")
}

/// Недостающие компоненты поставятся тем же паролем (Python: «Зависимости
/// будут установлены вместе с Rexon» — выводилось дважды).
pub fn dependencies_with_rexon_notice(_lang: Lang) -> String {
    tr("Missing components will be installed together with Rexon.")
}

/// Переносной режим (Python: «Portable режим: установка не требуется»).
pub fn portable_no_install_notice(_lang: Lang) -> String {
    tr("No installation needed: Rexon runs straight from its folder.")
}

/// Команда `rexon` не найдётся в терминале (Python: «~/.local/bin не в
/// $PATH — добавьте в ~/.bashrc / ~/.zshrc / ~/.config/fish/config.fish»).
/// Покупателю терминал не нужен вовсе: главное — меню.
pub fn command_path_notice(_lang: Lang) -> String {
    tr(
        "Start Rexon from the application menu: the “rexon” command may not be \
         found in a terminal with this option.",
    )
}

/// Подсказка к [`command_path_notice`]: где лежит команда.
pub fn command_path_hint(install_dir: &str, _lang: Lang) -> String {
    // TRANSLATORS: tooltip; %1 is a folder path.
    trf(
        "The command is in %1, and that folder is not on the list of places the \
         terminal searches for programs.",
        &[&install_dir],
    )
}

/// Входит ли каталог в список поиска программ (`PATH`). Косая черта в конце
/// не мешает совпадению — Python сравнивал строки как есть, и
/// `~/.local/bin/` в `PATH` давал ложное «не входит».
pub fn command_dir_in_path(dir: &str, path_env: Option<&str>) -> bool {
    let want = dir.trim_end_matches('/');
    !want.is_empty()
        && path_env
            .unwrap_or("")
            .split(':')
            .any(|p| p.trim_end_matches('/') == want)
}

/// Последняя строка журнала успешной установки (Python: «🎉 Rexon успешно
/// установлен!» / «🎉 Rexon установлен в ~/.local/ (без root)»).
pub fn install_done_line(mode: InstallMode, lang: Lang) -> String {
    match mode {
        InstallMode::System => tr("Rexon is installed for everyone on this computer."),
        InstallMode::User => tr("Rexon is installed just for you."),
        InstallMode::Portable => portable_no_install_notice(lang),
    }
}

/// Почему установка не завершилась. Каждый исход — свой совет: прежде всё
/// сливалось в «Установка отменена пользователем (pkexec)».
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallFailure {
    /// Окно пароля закрыли или пароль не подошёл — ничего не изменено.
    PasswordDenied,
    /// Спросить пароль нечем — ничего не изменено.
    PasswordPromptMissing,
    /// Установка шла дольше отведённого и остановлена.
    TimedOut { secs: u64 },
    /// Шаг упал; `detail` — хвост вывода для разработчика.
    StepFailed { step: InstallStep, detail: String },
}

/// Общий отказ «нечем запросить пароль» — на языке интерфейса.
fn prompt_missing_message() -> String {
    crate::system_plan::pkexec_missing_message()
}

/// Инструкция из общего отказа «нечем запросить пароль» — вторая строка
/// [`crate::system_plan::pkexec_missing_message`] (имя пакета в ней — то, что
/// надо поставить, а не объяснение). Своей копии фразы нет.
fn prompt_missing_instruction() -> String {
    let message = prompt_missing_message();
    message.split_once('\n').map(|(_, instruction)| instruction.to_string()).unwrap_or_default()
}

/// Объяснение из того же общего отказа — первая строка.
fn prompt_missing_explanation() -> String {
    let message = prompt_missing_message();
    match message.split_once('\n') {
        Some((explanation, _)) => explanation.to_string(),
        None => message,
    }
}

/// Отказ на шаге «недостающие части»: ЧТО не встало, названное человеку
/// словами, а не именами пакетов.
///
/// ⭐ Починка 16.09.2026: прежде здесь была общая фраза «Не получилось:
/// установка недостающих компонентов» — человек не понимал ни чего не
/// хватило, ни тронута ли уже система. Строгая строка в скрипте одна
/// ([`installer_exec::deps_script_lines`]), стоит первой, и ставит она ровно
/// четыре вещи — их и называем. Желательная база едет следом с `|| true` и
/// сюда привести не может.
pub fn dependencies_failed_problem(_lang: Lang) -> String {
    tr(
        "The parts Rexon cannot start without could not be installed: the Qt6 \
         framework, the wallpaper layer under the desktop, the video player and \
         the Vulkan loader. Nothing in the system was changed — Rexon was not \
         installed.",
    )
}

impl InstallFailure {
    /// Заголовок итога.
    pub fn title(&self, _lang: Lang) -> String {
        match self {
            InstallFailure::PasswordDenied => {
                tr("Password not confirmed")
            }
            InstallFailure::PasswordPromptMissing => {
                tr("Could not ask for the password")
            }
            InstallFailure::TimedOut { .. } => tr("The installation took too long"),
            InstallFailure::StepFailed { .. } => {
                tr("The installation did not finish")
            }
        }
    }

    /// Что пошло не так.
    pub fn problem(&self, lang: Lang) -> String {
        match self {
            InstallFailure::PasswordDenied => tr(
                "The password window was closed, or the password did not match. \
                 Nothing was changed.",
            ),
            InstallFailure::PasswordPromptMissing => prompt_missing_explanation(),
            InstallFailure::TimedOut { secs } => {
                let minutes = secs.div_ceil(60).max(1);
                // TRANSLATORS: %1 is a number of minutes.
                trf(
                    "The installation ran longer than %1 min and was stopped. Some \
                     files may already have been installed.",
                    &[&minutes],
                )
            }
            // Недостающие части ставятся ПЕРВЫМ шагом корневого скрипта,
            // поэтому здесь ещё нечему остаться — и здесь единственное
            // место, где можно назвать человеку, чего именно не хватило.
            InstallFailure::StepFailed { step: InstallStep::Dependencies, .. } => {
                dependencies_failed_problem(lang)
            }
            // TRANSLATORS: %1 is the name of an installation step in lower
            // case, e.g. "preparing the icons".
            InstallFailure::StepFailed { step, .. } => trf(
                "This step failed: %1. Some changes made before it may remain.",
                &[&step.name(lang)],
            ),
        }
    }

    /// Что делать — по совету на строку.
    pub fn advice(&self, lang: Lang) -> Vec<String> {
        let just_for_you = mode_title(InstallMode::User, lang);
        match self {
            InstallFailure::PasswordDenied => vec![
                tr("Press “Retry” and enter the administrator password of this computer."),
                // TRANSLATORS: %1 is the install option "Just for you".
                trf(
                    "If you do not have the administrator password, choose “%1” — it \
                     needs no password.",
                    &[&just_for_you],
                ),
            ],
            InstallFailure::PasswordPromptMissing => vec![
                // TRANSLATORS: %1 is the install option "Just for you".
                trf("Choose “%1” — it needs no password.", &[&just_for_you]),
                prompt_missing_instruction(),
            ],
            InstallFailure::TimedOut { .. } => vec![tr(
                "Check your internet connection — missing components are \
                 downloaded — and press “Retry”.",
            )],
            // Недостающие части скачиваются из сети, и первая причина
            // отказа — не поломка Rexon, а недоступный источник программ.
            InstallFailure::StepFailed { step: InstallStep::Dependencies, .. } => vec![
                tr(
                    "Check your internet connection — the missing parts are \
                     downloaded — and press “Retry”.",
                ),
                tr(
                    "If the error happens again, copy the details below and send \
                     them to the developer: these parts may be named differently \
                     in your system.",
                ),
            ],
            InstallFailure::StepFailed { .. } => vec![tr(
                "Press “Retry”. If the error happens again, copy the details below \
                 and send them to the developer.",
            )],
        }
    }

    /// Сырой вывод для разработчика; пусто — подробностей нет.
    pub fn details(&self) -> String {
        match self {
            InstallFailure::StepFailed { detail, .. } => detail.clone(),
            _ => String::new(),
        }
    }

    /// Одна строка для журнала хода и для возврата `(ok, сообщение)`.
    pub fn log_line(&self, lang: Lang) -> String {
        format!("{}. {}", self.title(lang), self.problem(lang))
    }
}

/// Чем закончилась установка — вход для [`result_texts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WizardOutcome {
    Installed {
        mode: InstallMode,
        /// Просили установку в систему, а она недоступна — текст
        /// [`system_mode_blocked_reason_in`]; `None` — режим не подменялся.
        fell_back: Option<String>,
        /// Каталог команды `rexon`, если он не в `PATH`
        /// ([`command_dir_in_path`]); `None` — всё в порядке.
        command_dir_not_in_path: Option<String>,
    },
    Failed(InstallFailure),
}

/// Страница итога.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WizardResultTexts {
    pub ok: bool,
    pub title: String,
    pub description: String,
    /// Советы и оговорки — по строке.
    pub notes: Vec<String>,
    /// Техническое уточнение для всплывающей подсказки.
    pub hint: String,
    pub details_title: String,
    /// Сырой вывод для разработчика.
    pub details: String,
    pub unavailable_title: String,
    /// Что не будет работать без установки.
    pub unavailable: Vec<String>,
    /// Главная кнопка: «Продолжить» при успехе, «Повторить» при неудаче.
    pub primary_button: String,
    /// Вторая кнопка; пусто — её нет.
    pub secondary_button: String,
}

/// Подпись главной кнопки итога успеха.
///
/// ⭐ Починка против Python-мастера: там кнопка звалась «Запустить Rexon» и
/// ничего не запускала (`rexon/installer.py:1364, 1379`). Запускать ей и
/// нечего: мастер работает ВНУТРИ уже запущенного Rexon, и человек после
/// установки возвращается в то же окно. Кнопка названа тем, что делает, —
/// перезапуск процесса ей не поручен.
pub fn continue_button(_lang: Lang) -> String {
    tr("Continue")
}

/// Подпись кнопки «Повторить» итога неудачи.
pub fn retry_button(_lang: Lang) -> String {
    tr("Retry")
}

/// Подпись кнопки «Продолжить без установки» итога неудачи.
pub fn continue_without_install_button(_lang: Lang) -> String {
    tr("Continue without installing")
}

/// Тексты страницы итога.
pub fn result_texts(outcome: &WizardOutcome, lang: Lang) -> WizardResultTexts {
    match outcome {
        WizardOutcome::Installed { mode, fell_back, command_dir_not_in_path } => {
            let mut notes = Vec::new();
            if let Some(reason) = fell_back {
                notes.push(reason.clone());
            }
            let hint = match command_dir_not_in_path {
                Some(dir) => {
                    notes.push(command_path_notice(lang));
                    command_path_hint(dir, lang)
                }
                None => String::new(),
            };
            WizardResultTexts {
                ok: true,
                title: tr("Done — Rexon is installed"),
                description: match mode {
                    InstallMode::System => tr("Rexon is in the application menu for everyone on this computer."),
                    InstallMode::User => tr("Rexon is in the application menu — just for you."),
                    InstallMode::Portable => portable_no_install_notice(lang),
                },
                notes,
                hint,
                details_title: String::new(),
                details: String::new(),
                unavailable_title: String::new(),
                unavailable: Vec::new(),
                primary_button: continue_button(lang),
                secondary_button: String::new(),
            }
        }
        WizardOutcome::Failed(failure) => {
            let details = failure.details();
            WizardResultTexts {
                ok: false,
                title: failure.title(lang),
                description: failure.problem(lang),
                notes: failure.advice(lang),
                hint: String::new(),
                details_title: if details.is_empty() {
                    String::new()
                } else {
                    tr("Details for the developer")
                },
                details,
                unavailable_title: tr("Without installing:"),
                unavailable: without_install_limits(lang),
                primary_button: retry_button(lang),
                secondary_button: continue_without_install_button(lang),
            }
        }
    }
}

// ── Удаление Rexon: ход и итог ──────────────────────────────────────────────
//
// Этими словами говорят исполнитель удаления
// ([`crate::installer_exec::uninstall`]) и окно меню «Удалить Rexon».
// ⭐ Починка G8: прежде окно показывало «Ошибка удаления system: код 126: …» —
// отказ пароля, нечем спросить пароль, таймаут и сбой самого удаления одной
// строкой со служебными словами.

/// Часть удаления — для строки хода.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UninstallPart {
    /// Пакет дистрибутива.
    Package,
    /// Установка для всех пользователей.
    System,
    /// Установка только для себя.
    User,
    /// Временные файлы (кэш) — при любом выборе.
    Cache,
    /// Настройки, резервные копии и установленное содержимое — только при
    /// «Удалить всё».
    Profile,
}

impl UninstallPart {
    pub const ALL: [UninstallPart; 5] = [
        UninstallPart::Package,
        UninstallPart::System,
        UninstallPart::User,
        UninstallPart::Cache,
        UninstallPart::Profile,
    ];

    /// Строка журнала хода.
    pub fn progress_line(self, _lang: Lang) -> String {
        match self {
            UninstallPart::Package => {
                tr("Removing the Rexon package…")
            }
            UninstallPart::System => tr("Removing Rexon for everyone on this computer…"),
            UninstallPart::User => tr("Removing Rexon from your home folder…"),
            UninstallPart::Cache => tr("Removing Rexon's temporary files…"),
            UninstallPart::Profile => tr("Removing settings, backups and installed wallpapers, themes and add-ons…"),
        }
    }
}

// ── Пользовательские данные Rexon ───────────────────────────────────────────
//
// ⭐ Решение Wehrwolfmann 15.09.2026. Одна правда о том, ЧТО Rexon пишет у
// человека в домашней папке помимо самой программы, — для удаления и для
// создания папок профиля при установке
// ([`crate::installer_exec::profile_dirs`]). Прежде удаление знало только
// настройки и резервные копии: кэш и установленные обои и темы оставались
// после любого выбора.
//
// Кто пишет в эти корни (сверено grep'ом по ядру 15.09.2026):
//   * кэш `<XDG_CACHE_HOME|~/.cache>/rexon` — `youtube_wallpaper::cache_dir`,
//     `session_splash::preview_cache_dir`, `qt_steam::rexon_cache_dir`,
//     `qml_modules::net_cache_dir`, кадры размытия `wallpaper_exec`; ЛИТЕРАЛОМ
//     `~/.cache/rexon` мимо XDG — `appearance_cache::cache_dir` (OCS),
//     `installed_scan` (превью), запасной PID-файл обоев `wallpaper_exec`;
//   * данные `<XDG_DATA_HOME|~/.local/share>/rexon` — `rxn_v4_installer`
//     (`wallpapers/`), резервные копии рабочих столов (`backups/`),
//     `qml_modules::module_dirs` (`qml_modules/`, сборка разработчика — НЕ
//     часть установки: установка кладёт модули в `~/.local/lib/rexon`);
//     ЛИТЕРАЛОМ `~/.local/share/rexon` мимо XDG — `de_customization`
//     (`login-themes/`, `plymouth-themes/`, `bundles/`, `wallpapers/`),
//     `upscaler::esrgan_local_exe` (`bin/realesrgan`);
//   * настройки `<XDG_CONFIG_HOME|~/.config>/rexon`; литеральный
//     `~/.config/rexon` нужен ради установок, сделанных ДО починки 8a45776:
//     до неё мост и снимки экрана писали настройки литералом даже при
//     заданном `XDG_CONFIG_HOME`, и без второго корня они пережили бы
//     «Удалить всё».
// Поэтому корни всех трёх категорий берутся ОБА — по XDG и литеральный, если
// они различаются.

/// Имя папки резервных копий рабочих столов внутри корня данных Rexon.
pub const USER_DATA_BACKUPS_DIR: &str = "backups";

/// Что лежит у человека: для окна вопроса, строк хода и решения, что
/// удаляет какой выбор.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserDataKind {
    /// Настройки Rexon (в той же папке журнал установленных тем).
    Settings,
    /// Резервные копии рабочих столов.
    Backups,
    /// Установленные обои, темы и дополнения — всё в корне данных Rexon,
    /// кроме резервных копий.
    Content,
    /// Временные файлы (кэш).
    Cache,
}

impl UserDataKind {
    pub const ALL: [UserDataKind; 4] =
        [UserDataKind::Cache, UserDataKind::Settings, UserDataKind::Backups, UserDataKind::Content];

    /// Удаляется ли и при «Удалить, но сохранить настройки и резервные
    /// копии». Кэш — да: без программы он никому не нужен.
    pub fn removed_when_keeping(self) -> bool {
        matches!(self, UserDataKind::Cache)
    }

    /// Название для человека — из каталога перевода, на языке интерфейса.
    pub fn title(self) -> String {
        match self {
            UserDataKind::Settings => tr("Rexon settings"),
            UserDataKind::Backups => {
                tr("Desktop backups")
            }
            UserDataKind::Content => tr("Installed wallpapers, themes and add-ons"),
            UserDataKind::Cache => tr("Rexon's temporary files"),
        }
    }
}

/// Корни пользовательских данных Rexon по категориям, без повторов, в
/// порядке [`UserDataKind::ALL`] внутри каждой категории: сначала путь по
/// XDG, потом литеральный. `Content` — это КОРЕНЬ данных; его содержимое
/// кроме [`USER_DATA_BACKUPS_DIR`] исполнитель разворачивает сам.
/// `Backups` — `<корень данных>/backups` каждого корня.
///
/// Пусто, если домашняя папка не абсолютна: относительный путь смотрел бы в
/// текущий каталог процесса. Относительное значение `XDG_*` по спецификации
/// не учитывается.
pub fn user_data_roots(
    home: &str,
    xdg_config_home: &str,
    xdg_data_home: &str,
    xdg_cache_home: &str,
) -> Vec<(UserDataKind, String)> {
    if !home.starts_with('/') {
        return Vec::new();
    }
    fn xdg(v: &str) -> Option<&str> {
        Some(v).filter(|v| v.starts_with('/'))
    }
    let data: Vec<String> = [
        crate::utils::data_home(xdg(xdg_data_home), home),
        crate::utils::data_home(None, home),
    ]
    .into_iter()
    .map(|base| format!("{base}/rexon"))
    .collect();
    let config: Vec<String> = [
        crate::utils::config_home(xdg(xdg_config_home), home),
        crate::utils::config_home(None, home),
    ]
    .into_iter()
    .map(|base| format!("{base}/rexon"))
    .collect();
    let cache: Vec<String> = [
        crate::utils::cache_home(xdg(xdg_cache_home), home),
        crate::utils::cache_home(None, home),
    ]
    .into_iter()
    .map(|base| format!("{base}/rexon"))
    .collect();
    let mut out: Vec<(UserDataKind, String)> = Vec::new();
    let mut push = |kind: UserDataKind, path: String| {
        if !out.iter().any(|(_, p)| *p == path) {
            out.push((kind, path));
        }
    };
    for p in cache {
        push(UserDataKind::Cache, p);
    }
    for p in config {
        push(UserDataKind::Settings, p);
    }
    for root in &data {
        push(UserDataKind::Backups, format!("{root}/{USER_DATA_BACKUPS_DIR}"));
    }
    for root in data {
        push(UserDataKind::Content, root);
    }
    out
}

/// Итог успешного удаления. `keep_profile` — выбран вариант «Удалить, но
/// сохранить настройки и резервные копии»: итог называет ВСЁ сохранённое,
/// говорит, зачем оно осталось, и что временные файлы удалены и тут.
pub fn uninstall_done_line(keep_profile: bool, _lang: Lang) -> String {
    if keep_profile {
        tr(
            "Rexon has been removed. Your settings, desktop backups and installed \
             wallpapers, themes and add-ons were kept: if you install Rexon again, it \
             will pick up where you left off. Rexon's temporary files were deleted.",
        )
    } else {
        tr(
            "Rexon has been removed completely, together with its settings, desktop \
             backups, installed wallpapers, themes, add-ons and temporary files.",
        )
    }
}

/// На месте временных файлов или установленного содержимого стояла ссылка
/// на другую папку: удалена сама ссылка, по ней Rexon не ходил. Путь в
/// тексте нужен: файлы по ту сторону ссылки остались.
pub fn uninstall_removed_link_note(path: &str, _lang: Lang) -> String {
    // TRANSLATORS: %1 is a folder path.
    trf(
        "%1 was a link to another folder. Rexon removed only the link and did not \
         touch what it pointed to.",
        &[&path],
    )
}

/// Удалять нечего: ни пакета, ни копии для всех, ни копии для себя.
pub fn uninstall_nothing_line(_lang: Lang) -> String {
    tr("Rexon is not installed — there is nothing to remove. Nothing was changed.")
}

/// Путь из списка удаления оказался ссылкой на другую папку (у разработчика
/// папка программы ведёт в рабочее дерево) — её не трогали. Путь в тексте
/// нужен: иначе человек не узнает, что осталось.
pub fn uninstall_kept_link_note(path: &str, _lang: Lang) -> String {
    // TRANSLATORS: %1 is a folder path.
    trf(
        "%1 is a link to another folder. Rexon left it alone: neither the link nor \
         what it points to was touched.",
        &[&path],
    )
}

/// Rexon поставлен пакетом, удалять который Rexon не умеет. Отказ — до
/// первого побочного действия.
pub fn uninstall_package_unsupported_line(name: &str, _lang: Lang) -> String {
    // TRANSLATORS: %1 is the name of the distribution package.
    trf(
        "Rexon was installed from the “%1” package, which Rexon cannot remove by \
         itself. Remove it with your system's software installer. Nothing was changed.",
        &[&name],
    )
}

/// Итог неудачного удаления. Разбор исхода общий с установкой
/// ([`crate::installer_exec::classify_script_run`]); слова свои: заголовки
/// [`InstallFailure`] говорят об установке. Сырой вывод — хвостом, под
/// заголовком для разработчика.
pub fn uninstall_failure_line(failure: &InstallFailure, _lang: Lang) -> String {
    let problem = match failure {
        InstallFailure::PasswordDenied => tr(
            "The password window was closed, or the password did not match — removal \
             was stopped.",
        ),
        InstallFailure::PasswordPromptMissing => prompt_missing_message(),
        InstallFailure::TimedOut { secs } => {
            let minutes = secs.div_ceil(60).max(1);
            // TRANSLATORS: %1 is a number of minutes.
            trf(
                "The removal ran longer than %1 min and was stopped. Some Rexon files \
                 may remain — try removing it again.",
                &[&minutes],
            )
        }
        InstallFailure::StepFailed { .. } => tr(
            "Rexon could not be removed completely: some files may remain. Try \
             removing it again; if that does not help, tell the developer.",
        ),
    };
    let details = failure.details();
    if details.is_empty() {
        problem
    } else {
        format!(
            "{problem}\n\n{}:\n{details}",
            tr("Details for the developer")
        )
    }
}

// ============================================================================
// Зависимости времени выполнения per family (порт _get_dep_packages /
// _get_dep_install_cmd)
// ============================================================================

/// Пакеты, без которых поставка Rexon не поднимется ВОВСЕ, для семейства.
/// Fallback — DEBIAN (как `dep_map.get(family, dep_map.get(DEBIAN, []))` в
/// Python).
///
/// ⭐ Починка 16.09.2026 (правило #16): список был про PySide6, python-cairo и
/// классическое окно на Python — того, чего поставка больше не везёт. На
/// машине покупателя он ставил ненужный интерпретатор с обвязкой и при этом
/// НЕ ставил ни одной библиотеки, которой живёт Rust-окно. Проба «чего не
/// хватает» к тому времени уже спрашивала о том же, о чём этот список теперь
/// говорит: динамический компоновщик по файлам самой поставки
/// (`installer_exec::runtime_deps_present`).
///
/// ⭐⭐ Починка того же дня, вторым заходом: список разрезан надвое. Строка
/// обязательных идёт в корневом скрипте под `set -e`, и ОДНО неверное имя
/// обрывало всю установку. Имена выписаны по правилам именования каждого
/// семейства, а вживую собирался только Arch (`packaging/aur/PKGBUILD`) —
/// значит цена ошибки должна быть по разряду пакета:
///   * здесь — то, без чего окно не откроется: Qt6 (база, QML, веб, звук,
///     веб-сокеты, gRPC), layer-shell, загрузчик Vulkan (Flash-обои) и
///     libmpv (заставка входа). Не встало — установка честно отказывает и
///     называет, чего не хватило ([`InstallStep::Dependencies`]);
///   * в [`optional_dep_packages`] — база, которая на живой машине с
///     графикой стоит почти всегда (bash, D-Bus, Wayland, GL, OpenSSL,
///     polkit, тема значков). Она едет отдельной строкой с `|| true`, ровно
///     как переключатель стиля Qt ([`qt_style_install_cmd`]): опечатка в
///     имени там стоит ненужной строки в журнале, а не сорванной установки.
///
/// Набор обоих разрядов вместе — тот же, что у `depends` пакета дистрибутива
/// (`packaging/aur/PKGBUILD`), без `glibc` и `gcc-libs`: это база, без
/// которой не запустится и сам пакетный менеджер. `qt6ct` в набор не входит
/// и никогда не входил — он не обязателен (без него тема Kvantum применится
/// через `QT_STYLE_OVERRIDE`, только без шрифтов и значков).
///
/// Порядок внутри записи один на все семейства и читается сверху вниз:
/// Qt6 (база, QML, веб, звук, веб-сокеты, gRPC), layer-shell, загрузчик
/// Vulkan, libmpv.
///
/// ⚠️ Имена выписаны по правилам именования каждого семейства; вживую
/// проверено только семейство Arch. Поэтому разряд и разрезан: ошибка в
/// имени здесь видна человеку сразу, а не превращается в молчаливо
/// недоустановленный продукт.
pub fn dep_packages(family: PkgFamily) -> Vec<String> {
    let list: &[&str] = match family {
        PkgFamily::Arch => &[
            "qt6-base", "qt6-declarative", "qt6-webengine", "qt6-multimedia",
            "qt6-websockets", "qt6-grpc", "layer-shell-qt", "vulkan-icd-loader",
            "mpv",
        ],
        PkgFamily::Fedora => &[
            "qt6-qtbase-gui", "qt6-qtdeclarative", "qt6-qtwebengine",
            "qt6-qtmultimedia", "qt6-qtwebsockets", "qt6-qtgrpc",
            "layer-shell-qt", "vulkan-loader", "mpv-libs",
        ],
        PkgFamily::Suse => &[
            "qt6-base", "qt6-declarative", "qt6-webengine", "qt6-multimedia",
            "qt6-websockets", "qt6-grpc", "layer-shell-qt", "libvulkan1",
            "libmpv2",
        ],
        PkgFamily::Gentoo => &[
            "dev-qt/qtbase", "dev-qt/qtdeclarative", "dev-qt/qtwebengine",
            "dev-qt/qtmultimedia", "dev-qt/qtwebsockets", "dev-qt/qtgrpc",
            "kde-frameworks/layer-shell-qt", "media-libs/vulkan-loader",
            "media-video/mpv",
        ],
        PkgFamily::Alpine => &[
            "qt6-qtbase", "qt6-qtdeclarative", "qt6-qtwebengine",
            "qt6-qtmultimedia", "qt6-qtwebsockets", "qt6-qtgrpc",
            "layer-shell-qt", "vulkan-loader", "mpv-libs",
        ],
        PkgFamily::Void => &[
            "qt6-base", "qt6-declarative", "qt6-webengine", "qt6-multimedia",
            "qt6-websockets", "qt6-grpc", "layer-shell-qt", "Vulkan-Loader",
            "mpv",
        ],
        PkgFamily::Solus => &[
            "qt6-base", "qt6-declarative", "qt6-webengine", "qt6-multimedia",
            "qt6-websockets", "layer-shell-qt", "vulkan", "mpv",
        ],
        // DEBIAN + всё, чего нет в dep_map (nix/clear/alt/unknown) → DEBIAN.
        // Debian и Ubuntu режут Qt6 на библиотеки и QML-модули по отдельности,
        // поэтому имена здесь непохожи на остальные семейства.
        _ => &[
            "qt6-qpa-plugins", "qt6-wayland", "qml6-module-qtquick-controls",
            "qml6-module-qtwebengine", "qml6-module-qtmultimedia",
            "libqt6websockets6", "libqt6grpc6",
            "qml6-module-org-kde-layershell", "libvulkan1", "libmpv2",
        ],
    };
    list.iter().map(|s| s.to_string()).collect()
}

/// Пакеты ЖЕЛАТЕЛЬНЫЕ: без них Rexon тоже не работает, но на машине, где
/// вообще есть рабочий стол, они стоят уже — их тянет за собой всё
/// остальное. Fallback — DEBIAN, как и у [`dep_packages`].
///
/// Зачем отдельный разряд: имена в одиннадцати семействах из двенадцати
/// выписаны по правилам именования, а не проверены на живой машине, и ОДНО
/// неверное имя в строке под `set -e` обрывало установку у человека, которому
/// продукт обещал «просто нажмите Установить». Здесь собрано то, за что такой
/// ценой платить нельзя: bash, D-Bus, Wayland, GL, OpenSSL, polkit и тема
/// значков. Исполнитель дописывает к этой строке `|| true`
/// ([`installer_exec::deps_script_lines`]) — не встало, и ладно: если чего-то
/// из этого на машине правда нет, окно не откроется, и человек увидит это
/// сразу, а не вместо установки.
///
/// Порядок один на все семейства: Wayland, GL, D-Bus, OpenSSL, polkit, bash,
/// тема значков. У Debian место polkit занимает `pkexec` — там политика
/// приезжает отдельным пакетом.
pub fn optional_dep_packages(family: PkgFamily) -> Vec<String> {
    let list: &[&str] = match family {
        PkgFamily::Arch => &[
            "wayland", "libglvnd", "dbus", "openssl", "polkit", "bash",
            "hicolor-icon-theme",
        ],
        PkgFamily::Fedora => &[
            "libwayland-client", "libglvnd-egl", "dbus-libs", "openssl-libs",
            "polkit", "bash", "hicolor-icon-theme",
        ],
        PkgFamily::Suse => &[
            "libwayland-client0", "libglvnd", "dbus-1", "libopenssl3",
            "polkit", "bash", "hicolor-icon-theme",
        ],
        PkgFamily::Gentoo => &[
            "dev-libs/wayland", "media-libs/libglvnd", "sys-apps/dbus",
            "dev-libs/openssl", "sys-auth/polkit", "app-shells/bash",
            "x11-themes/hicolor-icon-theme",
        ],
        PkgFamily::Alpine => &[
            "wayland-libs-client", "libglvnd", "dbus", "openssl", "polkit",
            "bash", "hicolor-icon-theme",
        ],
        PkgFamily::Void => &[
            "wayland", "libglvnd", "dbus", "openssl", "polkit", "bash",
            "hicolor-icon-theme",
        ],
        PkgFamily::Solus => &[
            "wayland", "libglvnd", "dbus", "openssl", "polkit", "bash",
            "hicolor-icon-theme",
        ],
        _ => &[
            "libwayland-client0", "libglvnd0", "dbus", "openssl", "pkexec",
            "bash", "hicolor-icon-theme",
        ],
    };
    list.iter().map(|s| s.to_string()).collect()
}

/// Команда установки произвольного набора пакетов **строкой для корневого
/// скрипта** установки. Пусто — ставить нечем (см. ниже три случая).
///
/// Один форматтер на всё, что ставится этим скриптом (правило #12): и
/// обязательные зависимости, и доустановка переключателя стиля Qt.
///
/// Своей таблицы команд здесь НЕТ: сами команды берутся у движка
/// ([`crate::system_plan::install_cmd`]) — того же, которым Rexon ставит
/// пакеты во всех остальных местах. Отличие этой точки от прочих ровно одно:
/// движок отдаёт argv, а сюда нужна строка bash, поэтому каждое слово
/// проходит `shlex_quote` (имя пакета приходит из таблиц, но экранирование —
/// не «на всякий случай»: строка попадает в скрипт, который выполняется от
/// root).
///
/// ⚠️ `pkexec` НЕ дописывается, и это не забывчивость: корневой скрипт целиком
/// уже выполняется одним `pkexec` (см. [`system_install_script`]), второй
/// внутри был бы вложенной авторизацией. Поэтому берётся голая
/// [`crate::system_plan::install_cmd`], а не `install_argv`, который выбирает
/// привилегированный путь.
///
/// Пусто отдаётся в трёх случаях:
/// * список пакетов пуст — «поставить ничего» командой не выражается (то же
///   правило, что у `install_argv`);
/// * семейство `UNKNOWN` — менеджер не опознан, движок отвечает ошибкой;
/// * **NixOS** — единственное расхождение с движком, и оно осознанное.
///   Движок знает `nix-env -iA nixpkgs.<пакет>` и специально НЕ ставит перед
///   ним `pkexec`: эта команда наполняет профиль ТОГО пользователя, который
///   её запустил. Здесь запускающий — root, потому что скрипт целиком идёт
///   под `pkexec`, и сбросить права внутри нечем. То есть команда отработала
///   бы успешно и положила библиотеки в профиль root — покупатель не увидел
///   бы ни ошибки, ни пакетов. Молчаливая пустышка хуже честного «нечем»:
///   пакеты в NixOS ставятся правкой конфигурации системы. Тот же факт про
///   профиль пользователя, что и у движка, в корневом скрипте указывает в
///   другую сторону.
pub fn pkg_install_cmd(family: PkgFamily, packages: &[String]) -> String {
    if packages.is_empty() || family == PkgFamily::Nix {
        return String::new();
    }
    match crate::system_plan::install_cmd(family, packages) {
        // Строка bash из argv: каждое слово экранировано отдельно. Заголовки
        // таблицы (`pacman`, `-S`, `--needed`, `--ask=n`…) состоят из
        // безопасных знаков, поэтому кавычек не получают и вид команды не
        // меняется — экранирование срабатывает только на именах пакетов.
        Ok(argv) => argv
            .iter()
            .map(|word| crate::ext_manager::shlex_quote(word))
            .collect::<Vec<String>>()
            .join(" "),
        // UNKNOWN: движок отвечает «Пакетный менеджер не определён».
        Err(_) => String::new(),
    }
}

/// Команда установки ОБЯЗАТЕЛЬНЫХ зависимостей времени выполнения для
/// семейства. В корневом скрипте эта строка идёт строго, под `set -e`.
///
/// Пусто — по правилам [`pkg_install_cmd`]: пустой список пакетов, `UNKNOWN`
/// или NixOS (там корневой скрипт наполнил бы профиль root).
pub fn dep_install_cmd(family: PkgFamily) -> String {
    pkg_install_cmd(family, &dep_packages(family))
}

/// Команда доустановки ЖЕЛАТЕЛЬНЫХ зависимостей ([`optional_dep_packages`]).
///
/// Отдельная команда, а не хвост предыдущей: исполнитель дописывает к ней
/// `|| true` ([`installer_exec::deps_script_lines`]), и неверное имя пакета
/// в этом разряде не обрывает установку. Пусто — по тем же трём правилам
/// [`pkg_install_cmd`].
pub fn optional_dep_install_cmd(family: PkgFamily) -> String {
    pkg_install_cmd(family, &optional_dep_packages(family))
}

/// Команда доустановки переключателя стиля Qt (`qt6ct`) в ТОМ ЖЕ заходе, что
/// и сам Rexon.
///
/// Зачем отдельно от [`dep_install_cmd`]: без него Rexon работает — тема
/// Kvantum вне KDE и LXQt применится через `QT_STYLE_OVERRIDE`, только без
/// шрифтов и значков. Значит недоступный репозиторий не должен обрывать
/// установку целиком (`set -e` в корневом скрипте), и исполнитель дописывает
/// к этой строке `|| true`. Обязательные зависимости остаются строгими.
///
/// Пусто — ставить нечем: в NixOS пакеты ставятся правкой конфигурации, в
/// Clear Linux бандла нет.
pub fn qt_style_install_cmd(family: PkgFamily) -> String {
    match crate::themes::qt_style_helper_pkg(family.as_str()) {
        Some(pkg) => pkg_install_cmd(family, &[pkg]),
        None => String::new(),
    }
}

// ============================================================================
// Генерация install-скрипта (system-режим)
// ============================================================================

/// Набор иконок, подготовленных исполнителем во временных файлах.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IconSet {
    /// (размер, tmp-путь) для существующих PNG — порядок 16..512.
    pub sizes: Vec<(u32, String)>,
    /// tmp-путь к SVG (scalable), если есть.
    pub svg: Option<String>,
    /// tmp-путь к master-копии (rexon_icon.png) — только если нет 128/256/512.
    pub master: Option<String>,
}

impl IconSet {
    fn size(&self, s: u32) -> Option<&String> {
        self.sizes.iter().find(|(sz, _)| *sz == s).map(|(_, p)| p)
    }

    /// Источник для /usr/share/pixmaps/rexon.png: 128 → 256 → 512 → master.
    fn pixmap_src(&self) -> Option<&String> {
        self.size(128)
            .or_else(|| self.size(256))
            .or_else(|| self.size(512))
            .or(self.master.as_ref())
    }

    /// Все tmp-пути (для списка очистки) в порядке dict-insertion Python:
    /// размеры, затем svg, затем master.
    fn tmp_paths(&self) -> Vec<String> {
        let mut out: Vec<String> =
            self.sizes.iter().map(|(_, p)| p.clone()).collect();
        if let Some(s) = &self.svg {
            out.push(s.clone());
        }
        if let Some(m) = &self.master {
            out.push(m.clone());
        }
        out
    }
}

/// Куда кладётся каждый значок: пары (источник, место назначения) под
/// `icon_root` (hicolor) и `pixmap_dir`. Правило ОДНО на установку в систему
/// ([`icon_install_block`]) и в домашнюю папку (`installer_exec`).
///
/// ⭐ Починка при переносе: Python-установка в домашнюю папку брала значок
/// для pixmaps только из `rexon_icon_128.png` и не знала запасных 256/512 и
/// мастер-копии, которые знала установка в систему.
pub fn icon_targets(icons: &IconSet, icon_root: &str, pixmap_dir: &str) -> Vec<(String, String)> {
    let root = icon_root.trim_end_matches('/');
    let mut out: Vec<(String, String)> = Vec::new();
    for (size, src) in &icons.sizes {
        out.push((src.clone(), format!("{root}/{size}x{size}/apps/rexon.png")));
    }
    if let Some(svg) = &icons.svg {
        out.push((svg.clone(), format!("{root}/scalable/apps/rexon.svg")));
    }
    if let Some(px) = icons.pixmap_src() {
        out.push((px.clone(), format!("{}/rexon.png", pixmap_dir.trim_end_matches('/'))));
    }
    out
}

/// Блок `install -Dm644 …` для иконок — порт построения `icon_install_cmds`.
pub fn icon_install_block(icons: &IconSet) -> String {
    let cmds: Vec<String> = icon_targets(icons, "/usr/share/icons/hicolor", "/usr/share/pixmaps")
        .into_iter()
        .map(|(src, dst)| format!("install -Dm644 {src} {dst}"))
        .collect();
    if cmds.is_empty() {
        "# Иконки не найдены".to_string()
    } else {
        cmds.join("\n")
    }
}

/// Пути девяти helper/policy/rules tmp-файлов (создаются исполнителем).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperTmp {
    pub tmp_helper: String,
    pub tmp_policy: String,
    pub tmp_de_helper: String,
    pub tmp_de_policy: String,
    pub tmp_pkg_helper: String,
    pub tmp_pkg_policy: String,
    pub tmp_stage_helper: String,
    pub tmp_stage_policy: String,
    pub tmp_de_rules: String,
}

/// Блок установки privileged helper'ов + polkit-политик — порт
/// `helper_install_block`.
pub fn helper_install_block(h: &HelperTmp) -> String {
    format!(
        "install -Dm755 {th} {SYSTEM_HELPER_PATH}\n\
         install -Dm644 {tp} {SYSTEM_POLICY_PATH}\n\
         install -Dm755 {tdh} {DE_HELPER_PATH}\n\
         install -Dm644 {tdp} {DE_POLICY_PATH}\n\
         install -Dm755 {tph} {PKG_HELPER_PATH}\n\
         install -Dm644 {tpp} {PKG_POLICY_PATH}\n\
         install -Dm755 {tsh} {STAGE_HELPER_PATH}\n\
         install -Dm644 {tsp} {STAGE_POLICY_PATH}",
        th = h.tmp_helper,
        tp = h.tmp_policy,
        tdh = h.tmp_de_helper,
        tdp = h.tmp_de_policy,
        tph = h.tmp_pkg_helper,
        tpp = h.tmp_pkg_policy,
        tsh = h.tmp_stage_helper,
        tsp = h.tmp_stage_policy,
    )
}

/// Шаг правила «без повторного пароля» в корневом скрипте.
///
/// ⭐⭐ Починка 15.09.2026: правило ставилось ВСЕГДА и молча. Теперь — только
/// при согласии человека ([`PASSWORDLESS_CONSENT_DEFAULT`] = выключено).
/// Без согласия прежнее правило, поставленное молча, снимается: иначе
/// повторная установка оставляла бы в системе разрешение, о котором человека
/// никто не спрашивал.
pub fn passwordless_rule_block(tmp_rules: &str, consent: bool) -> String {
    if consent {
        format!("install -Dm644 {tmp_rules} {REXON_POLKIT_RULES_PATH}")
    } else {
        format!(
            "# Правило «без пароля» ставится только с согласия; прежнее, поставленное молча, снимается.\n\
             rm -f {REXON_POLKIT_RULES_PATH}"
        )
    }
}

/// Параметры генерации install-скрипта system-режима. Пути tmp-файлов —
/// от исполнителя (mkstemp/mkdtemp), поэтому текст детерминирован.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallScriptParams {
    pub lib_dir: String,
    pub install_dir: String,
    pub desktop_dir: String,
    pub tmp_pkg: String,
    pub tmp_launcher: String,
    pub tmp_desktop: String,
    pub tmp_recover: String,
    pub icons: IconSet,
    pub helpers: HelperTmp,
    /// Команда установки зависимостей ("" → «Зависимости уже установлены»).
    pub dep_cmd: String,
    /// Человек согласился на правило «без повторного пароля»
    /// ([`passwordless_rule_block`]). По умолчанию — нет.
    pub passwordless_rule: bool,
}

/// Полный текст bash-скрипта system-установки — порт `install_script`
/// f-string в `install_to_system`.
pub fn system_install_script(p: &InstallScriptParams) -> String {
    let dep_line = if p.dep_cmd.is_empty() {
        "# Зависимости уже установлены".to_string()
    } else {
        format!("# Установка зависимостей\n{}", p.dep_cmd)
    };
    let icon_block = icon_install_block(&p.icons);
    let helper_block = format!(
        "{}\n{}",
        helper_install_block(&p.helpers),
        passwordless_rule_block(&p.helpers.tmp_de_rules, p.passwordless_rule)
    );
    format!(
        "#!/bin/bash\n\
         set -e\n\
         {dep_line}\n\
         rm -rf {lib}\n\
         mkdir -p {lib}\n\
         cp -r {tmp_pkg}/* {lib}/\n\
         chmod -R 755 {lib}\n\
         install -Dm755 {tmp_launcher} {install_dir}/rexon\n\
         install -Dm644 {tmp_desktop} {desktop_dir}/rexon.desktop\n\
         install -Dm644 {tmp_recover} {desktop_dir}/rexon-recover.desktop\n\
         {icon_block}\n\
         {helper_block}\n\
         update-desktop-database {desktop_dir} 2>/dev/null || true\n\
         gtk4-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || \\\n\
         \x20 gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true\n\
         # KDE SyCoCa rebuild (чтобы панель/меню сразу увидели иконку)\n\
         ( kbuildsycoca6 --noincremental 2>/dev/null || \\\n\
         \x20 kbuildsycoca5 --noincremental 2>/dev/null ) || true\n\
         \n\
         # Wehrwolfmann: GNOME Shell + Cinnamon extensions для wallpaper-режима.\n\
         # Mutter (GNOME) и Muffin (Cinnamon) не поддерживают wlr-layer-shell —\n\
         # extension через Clutter.Actor + mmap-streaming. Ставим всегда без выбора\n\
         # пользователя (на случай переключения на GNOME/Cinnamon-Wayland).\n\
         for de_pair in \"gnome-extension|/usr/share/gnome-shell/extensions\" \\\n\
         \x20              \"cinnamon-extension|/usr/share/cinnamon/extensions\"; do\n\
         \x20   DE_DIR=\"${{de_pair%|*}}\"\n\
         \x20   DEST_BASE=\"${{de_pair#*|}}\"\n\
         \x20   EXT_SRC=\"{lib}/$DE_DIR/{ext}\"\n\
         \x20   EXT_DEST=\"$DEST_BASE/{ext}\"\n\
         \x20   # Расширение раньше называлось {ext_legacy} (домен от\n\
         \x20   # чужого проекта). Старый каталог сносим: два расширения с одним и тем\n\
         \x20   # же кодом рисовали бы обои дважды.\n\
         \x20   rm -rf \"$DEST_BASE/{ext_legacy}\"\n\
         \x20   if [ -d \"$EXT_SRC\" ]; then\n\
         \x20       rm -rf \"$EXT_DEST\"\n\
         \x20       mkdir -p \"$(dirname \"$EXT_DEST\")\"\n\
         \x20       cp -r \"$EXT_SRC\" \"$EXT_DEST\"\n\
         \x20       chmod -R 755 \"$EXT_DEST\"\n\
         \x20       if command -v glib-compile-schemas >/dev/null 2>&1 \\\n\
         \x20           && [ -d \"$EXT_DEST/schemas\" ]; then\n\
         \x20           glib-compile-schemas \"$EXT_DEST/schemas/\" 2>/dev/null || true\n\
         \x20       fi\n\
         \x20   fi\n\
         done\n",
        dep_line = dep_line,
        lib = p.lib_dir,
        tmp_pkg = p.tmp_pkg,
        tmp_launcher = p.tmp_launcher,
        install_dir = p.install_dir,
        tmp_desktop = p.tmp_desktop,
        desktop_dir = p.desktop_dir,
        tmp_recover = p.tmp_recover,
        icon_block = icon_block,
        helper_block = helper_block,
        ext = EXT_ID,
        ext_legacy = EXT_ID_LEGACY,
    )
}

/// Команда запуска install-скрипта: `pkexec bash {tmp_sh}`, timeout
/// [`SYSTEM_INSTALL_TIMEOUT_SECS`].
pub fn system_install_cmd(tmp_sh: &str) -> Cmd {
    Cmd {
        argv: vec!["pkexec".into(), "bash".into(), tmp_sh.to_string()],
        timeout_secs: Some(SYSTEM_INSTALL_TIMEOUT_SECS),
        lang_c: false,
    }
}

/// План system-установки: записать скрипт → pkexec → очистить временные
/// файлы. Тексты helper/policy/desktop/launcher пишутся исполнителем из
/// соответствующих pub-функций (здесь — только скрипт-обёртка и argv).
pub fn system_install_plan(p: &InstallScriptParams, tmp_sh: &str) -> Vec<PlanStep> {
    vec![
        PlanStep::WriteFile {
            path: tmp_sh.to_string(),
            content: system_install_script(p),
            mode: 0o755,
        },
        PlanStep::Run {
            id: RunStepId::AppInstall,
            cmd: system_install_cmd(tmp_sh),
            needs_root: true,
            description: "Установка в систему (один пароль)".into(),
        },
        PlanStep::RemoveFiles {
            paths: install_cleanup_paths(p, tmp_sh),
        },
    ]
}

/// Список временных путей для очистки после установки.
///
/// ⚠ Починен баг оригинала (правило #16): Python `cleanup_paths` НЕ включал
/// `tmp_stage_helper` / `tmp_stage_policy` (оба создавались через mkstemp и
/// оставались в /tmp мусором после каждой установки). Порт их добавляет
/// (после pkg-policy, зеркалом порядка установки stage-хелпера).
pub fn install_cleanup_paths(p: &InstallScriptParams, tmp_sh: &str) -> Vec<String> {
    let h = &p.helpers;
    let mut out = vec![
        p.tmp_pkg.clone(),
        tmp_sh.to_string(),
        p.tmp_launcher.clone(),
        p.tmp_desktop.clone(),
        p.tmp_recover.clone(),
        h.tmp_helper.clone(),
        h.tmp_policy.clone(),
        h.tmp_de_helper.clone(),
        h.tmp_de_policy.clone(),
        h.tmp_de_rules.clone(),
        h.tmp_pkg_helper.clone(),
        h.tmp_pkg_policy.clone(),
        // ── починка утечки: два stage-файла, забытых в оригинале ──
        h.tmp_stage_helper.clone(),
        h.tmp_stage_policy.clone(),
    ];
    out.extend(p.icons.tmp_paths());
    out
}

// ============================================================================
// Деинсталляция
// ============================================================================

/// Аргументы «какой пакет владеет файлом» — порт `_DISTRO_PKG_OWNER`.
/// None — семейство без поддержки детекта владельца (ручная/portable).
pub fn distro_pkg_owner(family: PkgFamily) -> Option<Vec<String>> {
    let v: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-Qoq"],
        PkgFamily::Debian => &["dpkg", "-S"],
        PkgFamily::Fedora | PkgFamily::Suse | PkgFamily::Alt => {
            &["rpm", "-qf", "--qf", "%{NAME}"]
        }
        _ => return None,
    };
    Some(v.iter().map(|s| s.to_string()).collect())
}

/// Команда удаления одного пакета (без каскада) — порт `_DISTRO_PKG_REMOVE`.
/// None — авто-удаление для семейства не поддержано.
pub fn distro_pkg_remove(family: PkgFamily) -> Option<Vec<String>> {
    let v: &[&str] = match family {
        PkgFamily::Arch => &["pacman", "-R", "--noconfirm"],
        PkgFamily::Debian | PkgFamily::Alt => &["apt-get", "remove", "-y"],
        PkgFamily::Fedora => &["dnf", "remove", "-y"],
        PkgFamily::Suse => &["zypper", "remove", "-y"],
        PkgFamily::Gentoo => &["emerge", "--unmerge"],
        PkgFamily::Alpine => &["apk", "del"],
        PkgFamily::Void => &["xbps-remove", "-y"],
        PkgFamily::Solus => &["eopkg", "rm", "-y"],
        _ => return None,
    };
    Some(v.iter().map(|s| s.to_string()).collect())
}

/// Кандидаты-бинари, у которых спрашиваем владельца (кроме самого модуля) —
/// порт списка в `_detect_distro_package`.
pub fn distro_candidate_bins() -> [&'static str; 2] {
    ["/usr/bin/rexon", "/usr/local/bin/rexon"]
}

/// Разбор вывода менеджера про владельца файла — порт логики
/// `_detect_distro_package`. `tool` = `owner[0]` (pacman/dpkg/rpm).
/// dpkg печатает «pkg: /path» → берём часть до ':'; остальные (`pacman -Qoq`
/// / `rpm -qf`) → первая строка. Пусто → None.
pub fn parse_owner_output(tool: &str, stdout: &str) -> Option<String> {
    let out = stdout.trim();
    if out.is_empty() {
        return None;
    }
    let name = if tool == "dpkg" {
        out.split(':').next().unwrap_or("").trim().to_string()
    } else {
        out.lines().next().unwrap_or("").trim().to_string()
    };
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Команда удаления дистрибутивного пакета через менеджер:
/// `pkexec {remove...} {pkg_name}`, timeout 300.
pub fn pkg_remove_cmd(remove: &[String], pkg_name: &str) -> Cmd {
    let mut argv = vec!["pkexec".to_string()];
    argv.extend(remove.iter().cloned());
    argv.push(pkg_name.to_string());
    Cmd { argv, timeout_secs: Some(300), lang_c: false }
}

/// Файлы установки в каталоге команд сверх лаунчера — хост обоев и заставка.
/// Правда одна с установкой: [`crate::installer_exec::InstallLayout::extra_bin_paths`].
///
/// ⭐ Починка G8: удаление обоих режимов их не знало, и после «Удалить Rexon»
/// в `bin` оставались два исполняемых файла.
fn extra_bin_paths(p: &InstallPaths) -> Vec<String> {
    match (&p.lib_dir, &p.install_dir) {
        (Some(lib), Some(install)) => {
            crate::installer_exec::InstallLayout::new(lib, install).extra_bin_paths()
        }
        _ => Vec::new(),
    }
}

/// Список путей для удаления в system-uninstall-скрипте (порядок оригинала,
/// хост обоев и заставка — сразу за лаунчером). Порт `paths_to_remove` в
/// `RexonUninstaller.uninstall`.
pub fn uninstall_system_paths(sys: &InstallPaths) -> Vec<String> {
    let lib = sys.lib_dir.clone().unwrap_or_default();
    let install = sys.install_dir.clone().unwrap_or_default();
    let desktop = sys.desktop_dir.clone().unwrap_or_default();
    let gnome = sys.gnome_ext_dir.clone().unwrap_or_default();
    let cinnamon = sys.cinnamon_ext_dir.clone().unwrap_or_default();
    let mut paths = vec![lib, format!("{install}/rexon")];
    paths.extend(extra_bin_paths(sys));
    paths.extend([
        format!("{desktop}/rexon.desktop"),
        format!("{desktop}/rexon-recover.desktop"),
        format!("{gnome}/{EXT_ID}"),
        format!("{cinnamon}/{EXT_ID}"),
        format!("{gnome}/{EXT_ID_LEGACY}"),
        format!("{cinnamon}/{EXT_ID_LEGACY}"),
        SYSTEM_HELPER_PATH.to_string(),
        SYSTEM_POLICY_PATH.to_string(),
        DE_HELPER_PATH.to_string(),
        DE_POLICY_PATH.to_string(),
        PKG_HELPER_PATH.to_string(),
        PKG_POLICY_PATH.to_string(),
        STAGE_HELPER_PATH.to_string(),
        STAGE_POLICY_PATH.to_string(),
        // Помощник сеанса прав («один пароль на запуск») и его политика.
        crate::system_plan::SESSION_HELPER_PATH.to_string(),
        crate::system_plan::SESSION_POLICY_PATH.to_string(),
        // Он же на SteamOS (вне `/usr`, кладёт `rexon-sysext-helper`).
        crate::steamos_sysext::SESSION_HELPER_PATH.to_string(),
        crate::steamos_sysext::SESSION_POLICY_PATH.to_string(),
        REXON_POLKIT_RULES_PATH.to_string(),
        // Следы НАСТРОЙКИ СТОЛА, а не самой программы. Их тоже убираем:
        // файлы ничьи по учёту пакетов, лежат в общесистемных каталогах и
        // прежде переживали удаление Rexon — человек сносил программу, а
        // её след оставался в машине навсегда.
        crate::system_plan::MATE_GSCHEMA_PATH.to_string(),
    ]);
    paths
}

/// Текст bash-скрипта system-деинсталляции — порт `script_lines`.
pub fn uninstall_system_script(sys: &InstallPaths) -> String {
    uninstall_system_script_for(sys, &uninstall_system_paths(sys))
}

/// Тот же скрипт, но снимает ровно `paths` — исполнитель
/// ([`crate::installer_exec::uninstall`]) вычёркивает из
/// [`uninstall_system_paths`] ссылки на чужие папки.
///
/// Первой строкой после `set -e` скрипт печатает метку начала
/// ([`crate::installer_exec::STEP_MARK`]): исход запуска различается по
/// ФАКТУ, как у установки ([`crate::installer_exec::classify_script_run`]) —
/// меток нет и pkexec отказал → пароль не подтверждён; метка есть → сломалось
/// само удаление.
pub fn uninstall_system_script_for(sys: &InstallPaths, paths: &[String]) -> String {
    let desktop = sys.desktop_dir.clone().unwrap_or_default();
    let mut lines = vec![
        "#!/bin/bash".to_string(),
        "set -e".to_string(),
        format!("echo {}{}", crate::installer_exec::STEP_MARK, UNINSTALL_SCRIPT_MARK),
    ];
    for p in paths {
        lines.push(format!("rm -rf {p}"));
    }
    lines.push(format!(
        "update-desktop-database {desktop} 2>/dev/null || true"
    ));
    // Схемы после удаления оверрайда надо пересобрать, иначе снятые
    // умолчания продолжают действовать из `gschemas.compiled`.
    lines.push(
        "glib-compile-schemas /usr/share/glib-2.0/schemas/ 2>/dev/null || true".to_string(),
    );
    // Раскладки панели Ubuntu MATE, которые Rexon разложил в общесистемный
    // каталог. Удаляем ТОЛЬКО ничьи по учёту пакетов: файл с тем же именем
    // может прийти из дистрибутива, и снести его — сломать чужой стол.
    // Там, где учёта нет (не Arch), не трогаем ничего.
    lines.push(
        "if command -v pacman >/dev/null 2>&1; then\n\
\x20 for f in /usr/share/mate-panel/layouts/*.layout; do\n\
\x20   [ -e \"$f\" ] || continue\n\
\x20   pacman -Qo \"$f\" >/dev/null 2>&1 || rm -f \"$f\"\n\
\x20 done\n\
fi"
            .to_string(),
    );
    lines.push(dm_fallback_cleanup_script());
    lines.join("\n") + "\n"
}

/// Снять страховку экрана входа, которую разложил помощник `switch-dm`
/// («Сделать основным»): каталог [`crate::system_plan::DM_FALLBACK_DIR`]
/// (сценарий отката, его данные и подсказка для консоли), служба-откат и
/// её drop-in с `OnFailure=` у выбранного экрана входа. Файлы ничьи по
/// учёту пакетов: без этого они переживали удаление Rexon, а `OnFailure=`
/// указывал бы на снятую службу. Тот же текст выполняет `post_remove`
/// пакета (`packaging/aur/rexon-git.install`).
///
/// ⚠️ Выбранное приветствие LightDM
/// ([`crate::system_plan::LIGHTDM_GREETER_CONF`]) НЕ снимается: это
/// настройка экрана входа, который сейчас основной, и без неё LightDM мог
/// бы остаться с неустановленным приветствием — то есть без входа
/// (правило #13). Служб не запускает и не останавливает.
pub fn dm_fallback_cleanup_script() -> String {
    use crate::system_plan::{DM_FALLBACK_DIR, DM_FALLBACK_DROPIN, DM_FALLBACK_SERVICE};
    format!(
        "for f in /etc/systemd/system/*.service.d/{DM_FALLBACK_DROPIN}; do\n\
\x20 [ -e \"$f\" ] || continue\n\
\x20 rm -f \"$f\"\n\
\x20 rmdir \"$(dirname \"$f\")\" 2>/dev/null || true\n\
done\n\
rm -f /etc/systemd/system/{DM_FALLBACK_SERVICE}\n\
rm -rf {DM_FALLBACK_DIR}\n\
if command -v systemctl >/dev/null 2>&1; then systemctl daemon-reload 2>/dev/null || true; fi"
    )
}

/// Команда запуска uninstall-скрипта: `pkexec bash {tmp_sh}`, timeout 300.
pub fn uninstall_system_cmd(tmp_sh: &str) -> Cmd {
    Cmd {
        argv: vec!["pkexec".into(), "bash".into(), tmp_sh.to_string()],
        timeout_secs: Some(300),
        lang_c: false,
    }
}

/// Метка начала корневого скрипта удаления (после
/// [`crate::installer_exec::STEP_MARK`]).
pub const UNINSTALL_SCRIPT_MARK: &str = "files";

/// План system-деинсталляции: записать скрипт → pkexec → удалить tmp_sh.
pub fn uninstall_system_plan(sys: &InstallPaths, tmp_sh: &str) -> Vec<PlanStep> {
    uninstall_system_plan_for(sys, &uninstall_system_paths(sys), tmp_sh)
}

/// План, снимающий ровно `paths` ([`uninstall_system_script_for`]).
pub fn uninstall_system_plan_for(sys: &InstallPaths, paths: &[String], tmp_sh: &str) -> Vec<PlanStep> {
    vec![
        PlanStep::WriteFile {
            path: tmp_sh.to_string(),
            content: uninstall_system_script_for(sys, paths),
            mode: 0o755,
        },
        PlanStep::Run {
            id: RunStepId::AppUninstall,
            cmd: uninstall_system_cmd(tmp_sh),
            needs_root: true,
            description: "Удаление system (pkexec)".into(),
        },
        PlanStep::RemoveFiles { paths: vec![tmp_sh.to_string()] },
    ]
}

/// Кандидаты в список удаляемого для одной установки (system/user) — шесть
/// путей в порядке `get_installed_files`.
pub fn install_candidate_files(p: &InstallPaths) -> Vec<String> {
    let install = p.install_dir.clone().unwrap_or_default();
    let lib = p.lib_dir.clone().unwrap_or_default();
    let desktop = p.desktop_dir.clone().unwrap_or_default();
    let gnome = p.gnome_ext_dir.clone().unwrap_or_default();
    let cinnamon = p.cinnamon_ext_dir.clone().unwrap_or_default();
    let mut out = vec![format!("{install}/rexon")];
    out.extend(extra_bin_paths(p));
    out.extend([
        lib,
        format!("{desktop}/rexon.desktop"),
        format!("{desktop}/rexon-recover.desktop"),
        format!("{gnome}/{EXT_ID}"),
        format!("{cinnamon}/{EXT_ID}"),
        format!("{gnome}/{EXT_ID_LEGACY}"),
        format!("{cinnamon}/{EXT_ID_LEGACY}"),
    ]);
    out
}

/// Порт `RexonUninstaller.get_installed_files`: пакет + system + user + user
/// data, фильтруя каждый путь через `exists`. `pkg_name` — имя дистрибутивного
/// пакета (None — не установлен пакетом). `config_dir`/`backup_dir` — пути
/// пользовательских данных.
#[allow(clippy::too_many_arguments)]
pub fn get_installed_files(
    pkg_name: Option<&str>,
    sys: Option<&InstallPaths>,
    usr: Option<&InstallPaths>,
    config_dir: &str,
    backup_dir: &str,
    exists: impl Fn(&str) -> bool,
) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    if let Some(name) = pkg_name {
        // TRANSLATORS: a line in the list of what "Uninstall Rexon" removes,
        // among folder paths; %1 is the name of the distribution package.
        files.push(trf("the “%1” package (with all its files on the system)", &[&name]));
    }
    for inst in [sys, usr].into_iter().flatten() {
        for p in install_candidate_files(inst) {
            if exists(&p) {
                files.push(p);
            }
        }
    }
    if exists(config_dir) {
        files.push(config_dir.to_string());
    }
    if exists(backup_dir) {
        files.push(backup_dir.to_string());
    }
    files
}

// ============================================================================
// User-режим деинсталляции (порт второй половины `RexonUninstaller.uninstall`)
// ============================================================================

/// Каталоги размеров hicolor, куда user-install кладёт иконку (порядок
/// оригинала: PNG-размеры возрастанием + `scalable`).
pub const ICON_SIZE_DIRS: &[&str] = &[
    "16x16", "24x24", "32x32", "48x48", "64x64", "128x128", "256x256",
    "512x512", "scalable",
];

/// Что чистит user-деинсталляция: шесть основных путей + per-user иконки
/// (только наши `rexon.png`/`rexon.svg`, не вся папка размера) + pixmap.
///
/// `lexists` — `os.path.lexists` живого Python: битый симлинк тоже подлежит
/// удалению, поэтому именно `lexists`, а не `exists`. Основные шесть путей
/// оригинал не фильтровал вовсе (удалял «как получится»), поэтому они всегда
/// в списке — фильтр применяется только к иконкам, как и было.
pub fn uninstall_user_paths(
    usr: &InstallPaths,
    lexists: impl Fn(&str) -> bool,
) -> Vec<String> {
    let lib = usr.lib_dir.clone().unwrap_or_default();
    let install = usr.install_dir.clone().unwrap_or_default();
    let desktop = usr.desktop_dir.clone().unwrap_or_default();
    let gnome = usr.gnome_ext_dir.clone().unwrap_or_default();
    let cinnamon = usr.cinnamon_ext_dir.clone().unwrap_or_default();
    let icon_root = usr.icon_root.clone().unwrap_or_default();
    let pixmap = usr.pixmap_dir.clone().unwrap_or_default();
    let mut paths = vec![lib, format!("{install}/rexon")];
    paths.extend(extra_bin_paths(usr));
    paths.extend([
        format!("{desktop}/rexon.desktop"),
        format!("{desktop}/rexon-recover.desktop"),
        format!("{gnome}/{EXT_ID}"),
        format!("{cinnamon}/{EXT_ID}"),
        format!("{gnome}/{EXT_ID_LEGACY}"),
        format!("{cinnamon}/{EXT_ID_LEGACY}"),
    ]);
    for size_dir in ICON_SIZE_DIRS {
        for ext in ["png", "svg"] {
            let p = format!("{icon_root}/{size_dir}/apps/rexon.{ext}");
            if lexists(&p) {
                paths.push(p);
            }
        }
    }
    let pm = format!("{pixmap}/rexon.png");
    if lexists(&pm) {
        paths.push(pm);
    }
    paths
}

/// Итог деинсталляции: `ok=false` — ничего не было установлено.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UninstallSummary {
    pub ok: bool,
    pub message: String,
}

/// Порт хвоста `RexonUninstaller.uninstall`: перечисление того, что снесли,
/// либо честное «не установлен».
pub fn uninstall_summary(
    pkg_name: Option<&str>,
    has_system: bool,
    has_user: bool,
) -> UninstallSummary {
    if pkg_name.is_none() && !has_system && !has_user {
        return UninstallSummary {
            ok: false,
            message: "Rexon не установлен".to_string(),
        };
    }
    let mut which: Vec<String> = Vec::new();
    if let Some(name) = pkg_name {
        which.push(format!("пакет {name}"));
    }
    if has_system {
        which.push("system".to_string());
    }
    if has_user {
        which.push("user".to_string());
    }
    UninstallSummary {
        ok: true,
        message: format!("Rexon удалён ({})", which.join(", ")),
    }
}

#[cfg(test)]
mod package_data_dir_tests {
    use super::*;
    use std::path::Path;

    /// Сложить раскладку: `<base>/<rel>/qt_wallpaper_engine/qml_ui/PageHost.qml`.
    fn lay_out(base: &Path, rel: &str) {
        let dir = base.join(rel).join("qt_wallpaper_engine/qml_ui");
        std::fs::create_dir_all(&dir).expect("каталог раскладки создан");
        std::fs::write(dir.join("PageHost.qml"), b"// stub").expect("метка записана");
    }

    /// Обе живые раскладки — дерево разработчика и пакет — собраны
    /// настоящими файлами; пакетный префикс подменён пробой, чтобы тест не
    /// писал в `/usr`.
    #[test]
    fn picks_dev_tree_then_package_layout() {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let base = Path::new(&home).join(format!("rexon-pkgdir-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        lay_out(&base, &format!("tree/{DEV_TREE_DATA_DIR}"));
        lay_out(&base, "usr/lib/rexon/rexon");

        let pkg_prefix = package_data_dir().to_string();
        let base_str = base.to_string_lossy().into_owned();
        let probe = |path: &str| {
            let mapped = match path.strip_prefix(&pkg_prefix) {
                Some(rest) => format!("{base_str}/usr/lib/rexon{rest}"),
                None => path.to_string(),
            };
            Path::new(&mapped).is_file()
        };

        // Дерево разработчика точнее пакета: рядом с ним и правят.
        let dev_root = format!("{base_str}/tree");
        assert_eq!(
            pick_package_data_dir(&dev_root, &probe),
            Some(format!("{dev_root}/{DEV_TREE_DATA_DIR}"))
        );
        // Дерева нет (запуск из пакета) — берётся раскладка пакета.
        assert_eq!(
            pick_package_data_dir("", &probe),
            Some(format!("{pkg_prefix}/rexon"))
        );
        // Ни одной раскладки рядом — честное «не нашли».
        assert_eq!(pick_package_data_dir("", &|_p: &str| false), None);

        std::fs::remove_dir_all(&base).expect("временная раскладка убрана");
    }
}

#[cfg(test)]
mod first_run_tests {
    use super::*;
    use serde_json::{Map, Value};
    use std::collections::{HashMap, HashSet};

    const HOME: &str = "/home/tester";

    /// Подставная машина: ничего не читает с диска и из окружения.
    #[derive(Default)]
    struct FakeProbe {
        env: HashMap<String, String>,
        distro: String,
        own: Option<String>,
        root: String,
        symlinks: HashMap<String, String>,
        files: HashSet<String>,
        dirs: HashSet<String>,
    }

    impl FakeProbe {
        /// Обычный неустановленный запуск Rust-окна из распакованного архива на Arch.
        fn plain() -> Self {
            let mut p = FakeProbe {
                distro: "arch".into(),
                own: Some("/home/tester/Rexon/rexon-main-window".into()),
                ..Default::default()
            };
            p.env.insert("HOME".into(), HOME.into());
            p
        }
        fn with_env(mut self, k: &str, v: &str) -> Self {
            self.env.insert(k.into(), v.into());
            self
        }
        fn with_own(mut self, path: &str) -> Self {
            self.own = Some(path.into());
            self
        }
        /// Разложить признаки установки по путям режима.
        fn installed(mut self, paths: &InstallPaths) -> Self {
            let install = paths.install_dir.clone().unwrap();
            let desktop = paths.desktop_dir.clone().unwrap();
            self.files.insert(format!("{install}/rexon"));
            self.files.insert(format!("{desktop}/rexon.desktop"));
            self.dirs.insert(paths.lib_dir.clone().unwrap());
            self
        }
    }

    impl FirstRunProbe for FakeProbe {
        fn env(&self, key: &str) -> Option<String> {
            self.env.get(key).cloned()
        }
        fn distro_id(&self) -> String {
            self.distro.clone()
        }
        fn own_path(&self) -> Option<String> {
            self.own.clone()
        }
        fn repo_root(&self) -> String {
            self.root.clone()
        }
        fn dev_tree_behind(&self, lib_dir: &str) -> Option<String> {
            self.symlinks.get(lib_dir).cloned()
        }
        fn exists(&self, path: &str) -> bool {
            self.files.contains(path) || self.dirs.contains(path)
        }
        fn is_dir(&self, path: &str) -> bool {
            self.dirs.contains(path)
        }
    }

    fn no_settings() -> Map<String, Value> {
        Map::new()
    }

    fn declined_settings() -> Map<String, Value> {
        let mut m = crate::settings::defaults();
        m.insert(KEY_FIRST_RUN_DECLINED.into(), Value::Bool(true));
        m
    }

    fn system_paths() -> InstallPaths {
        install_paths(InstallMode::System, HOME, None)
    }

    fn user_paths() -> InstallPaths {
        install_paths(InstallMode::User, HOME, None)
    }

    fn skip(reason: FirstRunSkip) -> FirstRunOffer {
        FirstRunOffer::Skip(reason)
    }

    #[test]
    fn plain_uninstalled_run_offers_system_mode() {
        assert_eq!(
            first_run_offer(&FakeProbe::plain(), &no_settings()),
            FirstRunOffer::Offer {
                default_mode: InstallMode::System,
                system_blocked: None,
                forced: false
            }
        );
    }

    #[test]
    fn immutable_system_offers_user_mode_with_reason() {
        let mut p = FakeProbe::plain();
        p.distro = "steamos".into();
        match first_run_offer(&p, &no_settings()) {
            FirstRunOffer::Offer { default_mode, system_blocked, forced } => {
                assert_eq!(default_mode, InstallMode::User);
                assert_eq!(system_blocked, system_mode_blocked_reason("steamos"));
                assert!(system_blocked.is_some());
                assert!(!forced);
            }
            other => panic!("ожидалось предложение, получено {other:?}"),
        }
    }

    #[test]
    fn packaged_env_skips() {
        let p = FakeProbe::plain().with_env(PACKAGED_ENV, "1");
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::PackagedEnv));
        // Пустое значение — тоже признак: так же читает detect_default_mode.
        let p = FakeProbe::plain().with_env(PACKAGED_ENV, "");
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::PackagedEnv));
    }

    /// Дефект 7 плана: ярлык пакета зовёт бинарь окна без REXON_PACKAGED.
    #[test]
    fn package_binary_without_env_skips() {
        for own in [
            PACKAGE_MAIN_WINDOW,
            "/usr/bin/rexon-main-window",
            "/usr/lib64/rexon/rexon-main-window",
            "/usr/lib/rexon/rexon-main-window (deleted)",
            "/nix/store/abc-rexon/bin/rexon-main-window",
            "/app/bin/rexon-main-window",
            // Python-вход передаёт путь модуля, а не интерпретатора.
            "/usr/lib/rexon/rexon/installer.py",
        ] {
            let p = FakeProbe::plain().with_own(own);
            assert_eq!(
                first_run_offer(&p, &no_settings()),
                skip(FirstRunSkip::PackagedLocation(own.into())),
                "{own}"
            );
        }
    }

    #[test]
    fn local_and_home_locations_are_not_package() {
        for own in [
            "/usr/local/bin/rexon-main-window",
            "/usr/local/lib/rexon/rexon/installer.py",
            "/home/tester/.local/bin/rexon-main-window",
            "/opt/rexon/rexon-main-window",
            "rexon-main-window",
            "/usrx/bin/rexon",
        ] {
            assert!(!packaged_location(own), "{own}");
            let p = FakeProbe::plain().with_own(own);
            assert!(first_run_offer(&p, &no_settings()).is_offer(), "{own}");
        }
        // Своего пути нет вовсе — по месту не решаем, остальное в силе.
        let mut p = FakeProbe::plain();
        p.own = None;
        assert!(first_run_offer(&p, &no_settings()).is_offer());
    }

    #[test]
    fn steam_envs_skip() {
        for var in ["SteamGameId", "SteamAppId", "STEAM_COMPAT_DATA_PATH"] {
            let p = FakeProbe::plain().with_env(var, "123");
            assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::Steam(var)));
        }
    }

    #[test]
    fn dev_tree_by_own_path_skips() {
        let mut p = FakeProbe::plain()
            .with_own("/home/tester/src/rexon_v5/rust/target/release/rexon-main-window");
        p.root = "/home/tester/src/rexon_v5".into();
        assert_eq!(
            first_run_offer(&p, &no_settings()),
            skip(FirstRunSkip::DevTree("/home/tester/src/rexon_v5".into()))
        );
    }

    /// Машина разработчика: `/usr/local/lib/rexon` — симлинк в дерево, окно
    /// запущено не из дерева (например, скопированный бинарь).
    #[test]
    fn dev_tree_by_symlinked_lib_dir_skips() {
        let mut p = FakeProbe::plain();
        p.symlinks.insert(LIB_DIR.into(), "/home/tester/src/rexon_v5".into());
        assert_eq!(
            first_run_offer(&p, &no_settings()),
            skip(FirstRunSkip::DevTree("/home/tester/src/rexon_v5".into()))
        );
        let mut p = FakeProbe::plain();
        p.symlinks.insert(user_paths().lib_dir.unwrap(), "/home/tester/tree".into());
        assert_eq!(
            first_run_offer(&p, &no_settings()),
            skip(FirstRunSkip::DevTree("/home/tester/tree".into()))
        );
    }

    #[test]
    fn installed_system_skips() {
        let p = FakeProbe::plain().installed(&system_paths());
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::InstalledSystem));
    }

    /// Починка: Python проверял только режим по умолчанию (на Arch — system).
    #[test]
    fn installed_user_on_system_default_distro_skips() {
        let p = FakeProbe::plain().installed(&user_paths());
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::InstalledUser));
    }

    #[test]
    fn installed_user_respects_xdg_data_home() {
        let paths = install_paths(InstallMode::User, HOME, Some("/data/xdg"));
        let p = FakeProbe::plain()
            .with_env("XDG_DATA_HOME", "/data/xdg")
            .installed(&paths);
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::InstalledUser));
        // Пустой XDG_DATA_HOME по спецификации означает умолчание.
        let p = FakeProbe::plain()
            .with_env("XDG_DATA_HOME", "")
            .installed(&user_paths());
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::InstalledUser));
    }

    #[test]
    fn partial_install_still_offers() {
        let sys = system_paths();
        let mut p = FakeProbe::plain();
        p.files.insert(format!("{}/rexon", sys.install_dir.unwrap()));
        assert!(first_run_offer(&p, &no_settings()).is_offer());
    }

    #[test]
    fn no_home_skips_only_user_check() {
        let mut p = FakeProbe::plain();
        p.env.remove("HOME");
        assert!(first_run_offer(&p, &no_settings()).is_offer());
        let p = {
            let mut p = FakeProbe::plain().installed(&system_paths());
            p.env.remove("HOME");
            p
        };
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::InstalledSystem));
    }

    #[test]
    fn declined_is_remembered() {
        assert_eq!(
            first_run_offer(&FakeProbe::plain(), &declined_settings()),
            skip(FirstRunSkip::Declined)
        );
    }

    #[test]
    fn declined_non_bool_or_false_still_offers() {
        for v in [Value::Bool(false), Value::from("true"), Value::from(1), Value::Null] {
            let mut m = Map::new();
            m.insert(KEY_FIRST_RUN_DECLINED.into(), v.clone());
            assert!(!first_run_declined_from(&m), "{v}");
            assert!(first_run_offer(&FakeProbe::plain(), &m).is_offer(), "{v}");
        }
        // Осиротевший `wizard_done` на решение не влияет.
        let mut m = crate::settings::defaults();
        m.insert("wizard_done".into(), Value::Bool(true));
        assert!(first_run_offer(&FakeProbe::plain(), &m).is_offer());
    }

    #[test]
    fn force_overrides_everything() {
        let mut p = FakeProbe::plain()
            .with_env(FIRST_RUN_ENV, " Force ")
            .with_env(PACKAGED_ENV, "1")
            .with_env("SteamAppId", "1")
            .with_own(PACKAGE_MAIN_WINDOW)
            .installed(&system_paths())
            .installed(&user_paths());
        p.root = "/tree".into();
        assert_eq!(
            first_run_offer(&p, &declined_settings()),
            FirstRunOffer::Offer {
                default_mode: InstallMode::System,
                system_blocked: None,
                forced: true
            }
        );
        // На неизменяемой системе и force предлагает домашнюю папку.
        p.distro = "bazzite".into();
        match first_run_offer(&p, &no_settings()) {
            FirstRunOffer::Offer { default_mode, system_blocked, forced } => {
                assert_eq!(default_mode, InstallMode::User);
                assert!(system_blocked.is_some());
                assert!(forced);
            }
            other => panic!("ожидалось предложение, получено {other:?}"),
        }
    }

    #[test]
    fn never_overrides_everything() {
        let p = FakeProbe::plain().with_env(FIRST_RUN_ENV, "NEVER");
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::ForcedNever));
    }

    #[test]
    fn unknown_override_value_is_ignored() {
        assert_eq!(first_run_override(None), None);
        for v in ["", "1", "yes", "forced", "no"] {
            assert_eq!(first_run_override(Some(v)), None, "{v}");
            let p = FakeProbe::plain().with_env(FIRST_RUN_ENV, v);
            assert!(first_run_offer(&p, &no_settings()).is_offer(), "{v}");
        }
        assert_eq!(first_run_override(Some("force")), Some(true));
        assert_eq!(first_run_override(Some("never")), Some(false));
    }

    /// Порядок: пакет раньше дерева разработчика, дерево раньше «установлен»,
    /// «установлен» раньше отказа — причина в журнале самая точная.
    #[test]
    fn reasons_follow_documented_order() {
        let mut p = FakeProbe::plain()
            .with_own(PACKAGE_MAIN_WINDOW)
            .installed(&system_paths());
        p.root = "/tree".into();
        assert!(matches!(
            first_run_offer(&p, &declined_settings()),
            FirstRunOffer::Skip(FirstRunSkip::PackagedLocation(_))
        ));
        p.own = None;
        assert!(matches!(
            first_run_offer(&p, &declined_settings()),
            FirstRunOffer::Skip(FirstRunSkip::DevTree(_))
        ));
        p.root.clear();
        assert_eq!(
            first_run_offer(&p, &declined_settings()),
            skip(FirstRunSkip::InstalledSystem)
        );
        // Пакет по переменной раньше Steam.
        let p = FakeProbe::plain()
            .with_env(PACKAGED_ENV, "1")
            .with_env("SteamGameId", "1");
        assert_eq!(first_run_offer(&p, &no_settings()), skip(FirstRunSkip::PackagedEnv));
    }

    #[test]
    fn log_lines_name_every_reason() {
        let cases = [
            FirstRunOffer::Offer {
                default_mode: InstallMode::System,
                system_blocked: None,
                forced: false,
            },
            FirstRunOffer::Offer {
                default_mode: InstallMode::User,
                system_blocked: Some("x".into()),
                forced: true,
            },
            skip(FirstRunSkip::ForcedNever),
            skip(FirstRunSkip::PackagedEnv),
            skip(FirstRunSkip::PackagedLocation("/usr/bin/rexon".into())),
            skip(FirstRunSkip::Steam("SteamAppId")),
            skip(FirstRunSkip::DevTree("/tree".into())),
            skip(FirstRunSkip::InstalledSystem),
            skip(FirstRunSkip::InstalledUser),
            skip(FirstRunSkip::Declined),
        ];
        let lines: HashSet<String> = cases.iter().map(FirstRunOffer::log_line).collect();
        assert_eq!(lines.len(), cases.len(), "у каждой причины своя строка");
        assert!(cases[1].log_line().contains("force"));
        assert!(cases[4].log_line().contains("/usr/bin/rexon"));
        assert!(cases[5].log_line().contains("SteamAppId"));
    }

    /// «Больше не спрашивать» пишется в настоящий файл — временный, профиль
    /// человека не трогается; без отметки не пишется ничего.
    #[test]
    fn decline_session_writes_only_when_remembered() {
        let dir = std::env::temp_dir()
            .join(format!("rexon-first-run-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("временный каталог создан");
        let file = dir.join("settings.json");
        let mut manager = crate::settings::SettingsManager {
            config_dir: dir.clone(),
            config_file: file.clone(),
            settings: crate::settings::load_settings(&file),
        };

        session_first_run_decline(&mut manager, false).expect("без отметки — без ошибки");
        assert!(!file.exists(), "без отметки файл не пишется");
        assert!(first_run_offer(&FakeProbe::plain(), &manager.settings).is_offer());

        session_first_run_decline(&mut manager, true).expect("отказ сохранён");
        let reloaded = crate::settings::load_settings(&file);
        assert!(first_run_declined_from(&reloaded));
        assert_eq!(
            first_run_offer(&FakeProbe::plain(), &reloaded),
            skip(FirstRunSkip::Declined)
        );

        std::fs::remove_dir_all(&dir).expect("временный каталог убран");
    }
}

#[cfg(test)]
mod wizard_texts_tests {
    use super::*;
    use serde_json::Value;

    const HOME: &str = "/home/tester";

    /// Язык сверок — исходный английский, что бы ни стояло в локали машины
    /// и какой бы каталог ни лежал в системе. Русский слово в слово сверяет
    /// `tests/installer_parity.rs` под закреплённым русским каталогом.
    fn english() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
    }

    /// Служебные слова, которых покупатель видеть не должен. Сравнение — в
    /// нижнем регистре.
    const JARGON: &[&str] = &[
        "pkexec", "polkit", "helper", "хелпер", "лаунчер", "launcher", ".desktop", "$path",
        "bashrc", "zshrc", "config.fish", "pyside", "qt6-webengine", "layer-shell",
        "user-install", "user-режим", "portable", "extensions", "sudo", "из терминала",
        "/usr/", "~/.local",
    ];

    /// Жаргон в тексте; пути в подсказках разрешены (`allow_paths`).
    fn jargon_in(text: &str, allow_paths: bool) -> Option<String> {
        let checked: String = if allow_paths {
            text.split_whitespace()
                .filter(|w| {
                    let w = w.trim_start_matches(['(', '«', '“', '"']);
                    !(w.starts_with('/') || w.starts_with('~'))
                })
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            text.to_string()
        };
        let lower = checked.to_lowercase();
        if let Some(word) = JARGON.iter().find(|j| lower.contains(*j)) {
            return Some((*word).to_string());
        }
        if !allow_paths {
            let path = text.split_whitespace().find(|w| {
                let w = w.trim_start_matches(['(', '«', '“', '"']);
                w.starts_with('/') || w.starts_with("~/")
            });
            if let Some(p) = path {
                return Some(format!("путь {p}"));
            }
        }
        None
    }

    /// Все строки значения: (имя поля, текст). Поле `mode` — идентификатор,
    /// не подпись; `details` — сырой вывод для разработчика.
    fn walk(value: &Value, key: &str, out: &mut Vec<(String, String)>) {
        match value {
            Value::String(s) if key != "mode" && key != "details" => {
                out.push((key.to_string(), s.clone()))
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, key, out)),
            Value::Object(map) => map.iter().for_each(|(k, v)| walk(v, k, out)),
            _ => {}
        }
    }

    fn outcomes(lang: Lang) -> Vec<WizardOutcome> {
        let mut out = Vec::new();
        for mode in [InstallMode::System, InstallMode::User, InstallMode::Portable] {
            out.push(WizardOutcome::Installed {
                mode,
                fell_back: None,
                command_dir_not_in_path: None,
            });
        }
        out.push(WizardOutcome::Installed {
            mode: InstallMode::User,
            fell_back: system_mode_blocked_reason_in("steamos", lang),
            command_dir_not_in_path: Some(format!("{HOME}/.local/bin")),
        });
        out.push(WizardOutcome::Failed(InstallFailure::PasswordDenied));
        out.push(WizardOutcome::Failed(InstallFailure::PasswordPromptMissing));
        out.push(WizardOutcome::Failed(InstallFailure::TimedOut { secs: 300 }));
        for step in InstallStep::ALL {
            out.push(WizardOutcome::Failed(InstallFailure::StepFailed {
                step,
                detail: "pkexec: /usr/local/lib/rexon: PySide6 not found".into(),
            }));
        }
        out
    }

    /// Все строки мастера на одном языке — в одном и том же порядке для
    /// обоих языков.
    fn all_texts(lang: Lang) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut push_json = |v: Value| walk(&v, "", &mut out);
        for distro in ["arch", "steamos"] {
            push_json(serde_json::to_value(welcome_texts(distro, HOME, None, lang)).unwrap());
        }
        push_json(serde_json::to_value(passwordless_consent_texts(lang)).unwrap());
        for mode in [InstallMode::System, InstallMode::User, InstallMode::Portable] {
            push_json(serde_json::to_value(progress_texts(mode, lang)).unwrap());
        }
        for outcome in outcomes(lang) {
            push_json(serde_json::to_value(result_texts(&outcome, lang)).unwrap());
        }
        let text = |s: String| (String::from("text"), s);
        for step in InstallStep::ALL {
            out.push(text(step.name(lang)));
            out.push(text(step.progress_line(lang)));
        }
        for mode in [InstallMode::System, InstallMode::User, InstallMode::Portable] {
            out.push(text(install_done_line(mode, lang)));
            out.push(text(mode_title(mode, lang)));
            out.push(text(mode_description(mode, lang)));
        }
        for outcome in outcomes(lang) {
            if let WizardOutcome::Failed(f) = outcome {
                out.push(text(f.log_line(lang)));
            }
        }
        out.extend([
            text(icons_missing_notice(lang)),
            text(menu_refresh_later_notice(lang)),
            text(dependencies_with_rexon_notice(lang)),
            text(portable_no_install_notice(lang)),
            text(command_path_notice(lang)),
            (String::from("hint"), command_path_hint(&format!("{HOME}/.local/bin"), lang)),
            text(wizard_window_title(lang)),
            text(dry_run_notice(lang)),
            text(system_mode_blocked_reason_in("steamos", lang).unwrap()),
        ]);
        out.extend(without_install_limits(lang).into_iter().map(text));
        // Кнопка «Установить Rexon в систему…» на странице «Настройки» (G5).
        out.extend([
            text(settings_install_button(lang)),
            text(settings_install_hint(lang)),
            text(settings_install_started(lang)),
            text(settings_install_no_window(lang)),
        ]);
        // Удаление Rexon (G8). Сырого вывода в проверяемых строках нет:
        // подробности идут хвостом, как `details` у мастера.
        for part in UninstallPart::ALL {
            out.push(text(part.progress_line(lang)));
        }
        for keep in [true, false] {
            out.push(text(uninstall_done_line(keep, lang)));
        }
        for kind in UserDataKind::ALL {
            out.push(text(kind.title()));
        }
        out.push(text(uninstall_nothing_line(lang)));
        out.push((String::from("hint"), uninstall_kept_link_note("/usr/local/lib/rexon", lang)));
        out.push((String::from("hint"), uninstall_removed_link_note("/home/u/.cache/rexon", lang)));
        out.push(text(uninstall_package_unsupported_line("rexon-git", lang)));
        for failure in [
            InstallFailure::PasswordDenied,
            InstallFailure::PasswordPromptMissing,
            InstallFailure::TimedOut { secs: 300 },
            InstallFailure::StepFailed { step: InstallStep::SystemInstall, detail: String::new() },
        ] {
            out.push(text(uninstall_failure_line(&failure, lang)));
        }
        out
    }

    /// Итог удаления: подробности только хвостом и под своим заголовком;
    /// «system», «код 126» и прочие служебные слова в словах окна не
    /// появляются (прежде: «Ошибка удаления system: код 126: …»).
    /// Страховка экрана входа снимается и удалением из программы, и
    /// `pacman -R` (сценарий пакета) — одним и тем же текстом; выбранное
    /// приветствие LightDM остаётся, службы не запускаются и не
    /// останавливаются.
    #[test]
    fn package_post_remove_matches_core_cleanup() {
        let core = dm_fallback_cleanup_script();
        let sys = install_paths(InstallMode::System, "", None);
        assert!(uninstall_system_script(&sys).ends_with(&format!("{core}\n")));
        for want in [
            "/etc/systemd/system/*.service.d/50-rexon-fallback.conf",
            "rm -f /etc/systemd/system/rexon-dm-fallback.service",
            "rm -rf /etc/rexon",
        ] {
            assert!(core.contains(want), "{want}");
        }
        assert!(!core.contains(crate::system_plan::LIGHTDM_GREETER_CONF));
        assert!(!core.contains("50-rexon-greeter"), "приветствие LightDM не трогаем");
        for word in [" start", " stop", "restart", "--now", "isolate"] {
            assert!(!core.contains(word), "служба в живом сеансе: {word}");
        }
        let install = include_str!("../../../packaging/aur/rexon-git.install");
        let body = install
            .split_once("post_remove() {\n")
            .and_then(|(_, rest)| rest.split_once("\n}\n"))
            .map(|(body, _)| body)
            .expect("post_remove в rexon-git.install");
        let lines = |t: &str| t.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect::<Vec<_>>();
        assert_eq!(lines(body), lines(&core), "post_remove пакета разошёлся с ядром");
        assert!(include_str!("../../../packaging/aur/PKGBUILD").contains("\ninstall=rexon-git.install\n"));
    }

    #[test]
    fn uninstall_texts_speak_plainly_and_keep_raw_output_apart() {
        english();
        let step = InstallFailure::StepFailed {
            step: InstallStep::SystemInstall,
            detail: "rm: cannot remove '/usr/local/lib/rexon': Read-only file system".into(),
        };
        let line = uninstall_failure_line(&step, Lang::En);
        let (words, raw) = line.split_once("\n\nDetails for the developer:\n").expect("хвост подробностей");
        assert!(raw.contains("Read-only file system"));
        assert!(jargon_in(words, false).is_none(), "{words}");
        for (_, t) in all_texts(Lang::En) {
            assert!(!t.contains("system:") && !t.contains("code 126"), "{t}");
        }
        assert_ne!(uninstall_done_line(true, Lang::En), uninstall_done_line(false, Lang::En));
        // «Сохранить» называет всё сохранённое и что временные файлы удалены;
        // «удалить всё» — всё удалённое (решение 15.09.2026: кэш — при любом
        // выборе, обои и темы — только при «удалить всё»). Русский слово в
        // слово сверяет снимок `installer_parity::russian_installer_texts_as_before`.
        let (kept, gone) = (uninstall_done_line(true, Lang::En), uninstall_done_line(false, Lang::En));
        for w in ["settings", "backups", "wallpapers", "themes", "add-ons", "kept", "temporary files were deleted"] {
            assert!(kept.contains(w), "итог «сохранить» молчит про «{w}»: {kept}");
        }
        for w in ["settings", "backups", "wallpapers", "themes", "add-ons", "temporary files"] {
            assert!(gone.contains(w), "итог «удалить всё» молчит про «{w}»: {gone}");
        }
        assert!(jargon_in(&kept, false).is_none() && jargon_in(&gone, false).is_none());
        // Без жаргона «кэш» в словах для человека.
        for t in [&kept, &gone, &UninstallPart::Cache.progress_line(Lang::En), &UserDataKind::Cache.title()] {
            assert!(!t.to_lowercase().contains("cache"), "{t}");
        }
        assert!(uninstall_kept_link_note("/x/y", Lang::En).starts_with("/x/y"));
        assert!(uninstall_removed_link_note("/x/y", Lang::En).starts_with("/x/y"));
        // Строго: кэш — единственное, что уходит и при «сохранить».
        let when_keeping: Vec<_> =
            UserDataKind::ALL.into_iter().filter(|k| k.removed_when_keeping()).collect();
        assert_eq!(when_keeping, vec![UserDataKind::Cache]);
    }

    /// Корни данных: XDG и литеральные, без повторов; относительные XDG и
    /// относительная домашняя папка не дают ни одного пути.
    #[test]
    fn user_data_roots_follow_xdg_and_keep_literal_roots() {
        use UserDataKind::*;
        let plain = user_data_roots("/h", "", "", "");
        assert_eq!(
            plain,
            vec![
                (Cache, "/h/.cache/rexon".to_string()),
                (Settings, "/h/.config/rexon".into()),
                (Backups, "/h/.local/share/rexon/backups".into()),
                (Content, "/h/.local/share/rexon".into()),
            ]
        );
        // Тот же корень, записанный с хвостовым слешем, — не второй корень.
        assert_eq!(user_data_roots("/h/", "/h/.config/", "/h/.local/share/", "/h/.cache/"), plain);
        let xdg = user_data_roots("/h", "/x/conf", "/x/data", "/x/cache");
        assert_eq!(
            xdg,
            vec![
                (Cache, "/x/cache/rexon".to_string()),
                (Cache, "/h/.cache/rexon".into()),
                (Settings, "/x/conf/rexon".into()),
                (Settings, "/h/.config/rexon".into()),
                (Backups, "/x/data/rexon/backups".into()),
                (Backups, "/h/.local/share/rexon/backups".into()),
                (Content, "/x/data/rexon".into()),
                (Content, "/h/.local/share/rexon".into()),
            ]
        );
        assert_eq!(user_data_roots("/h", "conf", "data", "cache"), plain, "относительный XDG не учитывается");
        assert!(user_data_roots("", "", "", "").is_empty());
        assert!(user_data_roots("h", "/x", "/x", "/x").is_empty());
    }

    /// ⭐ Сторож: ни одна подпись мастера не несёт служебных слов и путей;
    /// пути допустимы только в подсказках.
    #[test]
    fn wizard_texts_have_no_jargon() {
        english();
        // Инструкция общего отказа «нечем запросить пароль» называет пакет —
        // это то, что надо поставить (так же решено в system_plan_parity).
        let instruction = prompt_missing_instruction();
        assert!(instruction.starts_with("Install the polkit package "), "{instruction}");
        let texts = all_texts(Lang::En);
        assert!(texts.len() > 100, "собрано подозрительно мало строк: {}", texts.len());
        for (key, text) in texts {
            // Та же инструкция бывает хвостом отказа удаления.
            if text.contains(&instruction) {
                continue;
            }
            if let Some(word) = jargon_in(&text, key == "hint") {
                panic!("жаргон «{word}» в поле «{key}»: {text}");
            }
        }
    }

    /// Сторож сам себя проверяет на прежних Python-строках.
    #[test]
    fn jargon_guard_catches_python_wizard_strings() {
        for bad in [
            "🛡  Системно  (/usr/local/, требуется пароль администратора)",
            "👤  В мою папку  (~/.local/, без пароля)",
            "Доступно всем пользователям машины. Иконки/extensions через pkexec.",
            "PySide6 / qt6-webengine / layer-shell-qt должны быть установлены отдельно.",
            "📁 Создание лаунчера...",
            "📝 Создание .desktop файла...",
            "🔐 Polkit helper (один пароль за сессию)...",
            "📦 Portable режим: установка не требуется",
            "📦 User-install в /home/x/.local/lib/rexon/...",
            "⚠️  /home/x/.local/bin не в $PATH — добавьте в ~/.bashrc",
            "User-режим установка завершена",
            "Установка отменена пользователем (pkexec)",
            "Таймаут установки — попробуйте выполнить из терминала",
        ] {
            assert!(jargon_in(bad, false).is_some(), "сторож не заметил: {bad}");
        }
        assert!(jargon_in("Файлы программы: /usr/local/lib/rexon", true).is_none());
        assert!(jargon_in("Файл: /usr/x — через pkexec", true).is_some());
    }

    /// В английском интерфейсе ни одной русской буквы — и ни одной пустой
    /// подписи там, где она обязана быть. Русский слово в слово сверяет
    /// снимок `installer_parity::russian_installer_texts_as_before`.
    #[test]
    fn english_wizard_texts_have_no_russian() {
        english();
        let cyrillic = |s: &str| s.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'));
        for (key, text) in all_texts(Lang::En) {
            assert!(!cyrillic(&text), "в английской строке кириллица ({key}): {text}");
        }
    }

    #[test]
    fn welcome_buttons_and_modes() {
        english();
        let w = welcome_texts("arch", HOME, None, Lang::En);
        assert_eq!(w.install_button, "Install");
        assert_eq!(w.skip_button, "Skip");
        assert_eq!(w.dont_ask_again, "Don't ask again");
        let modes: Vec<InstallMode> = w.modes.iter().map(|m| m.mode).collect();
        assert_eq!(modes, WIZARD_MODES.to_vec());
        assert!(w.modes.iter().all(|m| m.blocked.is_empty()));
        assert_eq!(w.modes[0].title, "For everyone on this computer");
        assert_eq!(w.modes[1].title, "Just for you");
        // Пути — только в подсказке и ровно те, что у install_paths.
        assert!(w.modes[0].hint.contains(LIB_DIR));
        assert!(w.modes[1].hint.contains(&format!("{HOME}/.local/lib/rexon")));
        let xdg = welcome_texts("arch", HOME, Some("/data/xdg"), Lang::En);
        assert!(xdg.modes[1].hint.contains("/data/xdg/applications"));
        assert_eq!(mode_hint(InstallMode::Portable, HOME, None, Lang::En), "");

        let deck = welcome_texts("steamos", HOME, None, Lang::En);
        assert_eq!(
            Some(deck.modes[0].blocked.clone()),
            system_mode_blocked_reason_in("steamos", Lang::En)
        );
        assert!(deck.modes[1].blocked.is_empty());
    }

    /// Прежняя функция без языка говорит то же, что и с языком: её читают
    /// G1 и Python.
    #[test]
    fn blocked_reason_forms_agree() {
        english();
        assert_eq!(
            system_mode_blocked_reason("steamos"),
            system_mode_blocked_reason_in("steamos", Lang::En)
        );
        assert!(system_mode_blocked_reason("steamos")
            .unwrap()
            .starts_with("On this system (for example, Steam Deck)"));
        assert_eq!(system_mode_blocked_reason_in("arch", Lang::En), None);
    }

    #[test]
    fn success_result_says_done_and_offers_to_continue() {
        english();
        let r = result_texts(
            &WizardOutcome::Installed {
                mode: InstallMode::System,
                fell_back: None,
                command_dir_not_in_path: None,
            },
            Lang::En,
        );
        assert!(r.ok);
        assert_eq!(r.title, "Done — Rexon is installed");
        // ⭐ Не «Запустить Rexon»: мастер идёт внутри уже запущенного Rexon.
        assert_eq!(r.primary_button, continue_button(Lang::En));
        assert!(r.secondary_button.is_empty() && r.notes.is_empty() && r.hint.is_empty());
        assert!(r.unavailable.is_empty());

        let dir = format!("{HOME}/.local/bin");
        let r = result_texts(
            &WizardOutcome::Installed {
                mode: InstallMode::User,
                fell_back: system_mode_blocked_reason("steamos"),
                command_dir_not_in_path: Some(dir.clone()),
            },
            Lang::En,
        );
        assert_eq!(r.notes.len(), 2);
        assert_eq!(r.notes[1], command_path_notice(Lang::En));
        assert!(r.hint.contains(&dir));
    }

    /// Отказ пароля и сбой шага — разные исходы; ни один не «отменён
    /// пользователем».
    #[test]
    fn failures_are_told_apart() {
        english();
        let failures = [
            InstallFailure::PasswordDenied,
            InstallFailure::PasswordPromptMissing,
            InstallFailure::TimedOut { secs: 300 },
            InstallFailure::StepFailed { step: InstallStep::Dependencies, detail: "x".into() },
        ];
        let lang = Lang::En;
        let titles: std::collections::HashSet<String> =
            failures.iter().map(|f| f.title(lang)).collect();
        assert_eq!(titles.len(), failures.len(), "у каждого исхода свой заголовок");
        for f in &failures {
            let r = result_texts(&WizardOutcome::Failed(f.clone()), lang);
            assert!(!r.ok);
            assert_eq!(r.primary_button, retry_button(lang));
            assert_eq!(r.secondary_button, continue_without_install_button(lang));
            assert_eq!(r.unavailable, without_install_limits(lang));
            assert!(!r.notes.is_empty(), "что делать — сказано: {f:?}");
            let all = format!("{} {}", r.title, r.description).to_lowercase();
            assert!(!all.contains("cancel"), "{all}");
        }
        assert!(InstallFailure::PasswordDenied.title(lang).starts_with("Password"));
        // Первая строка общего отказа пароля та же, что во всём продукте.
        assert_eq!(
            InstallFailure::PasswordPromptMissing.problem(lang),
            crate::system_plan::pkexec_missing_message().split('\n').next().unwrap()
        );
        // Сырой вывод — только в подробностях.
        let step = InstallFailure::StepFailed {
            step: InstallStep::SystemInstall,
            detail: "exit 1".into(),
        };
        let r = result_texts(&WizardOutcome::Failed(step.clone()), lang);
        assert_eq!(r.details, "exit 1");
        assert!(!r.details_title.is_empty());
        assert!(!r.description.contains("exit 1"));
        assert!(r.description.contains("installing into the system"));
        let r = result_texts(&WizardOutcome::Failed(InstallFailure::PasswordDenied), lang);
        assert!(r.details.is_empty() && r.details_title.is_empty());
        assert!(InstallFailure::TimedOut { secs: 300 }.problem(lang).contains("5 min"));
        assert!(InstallFailure::TimedOut { secs: 1 }.problem(lang).contains("1 min"));
    }

    #[test]
    fn consent_is_off_by_default_and_honest() {
        english();
        assert!(!PASSWORDLESS_CONSENT_DEFAULT);
        let c = passwordless_consent_texts(Lang::En);
        assert!(!c.default_on);
        assert!(c.who.contains("administrators group"));
        assert!(c.risk.contains("without your confirmation"));
        // ⭐ Пункт меню назван, и ровно той подписью, что в меню, — на том
        // же языке, что и вся фраза (29.09.2026: русская фраза несла
        // английскую подпись, английская не называла пункт вовсе).
        let item = crate::window_menu::menu_items()
            .into_iter()
            .find(|i| i.id == crate::window_menu::ID_UNINSTALL)
            .expect("пункт удаления в меню есть")
            .label;
        assert!(item.contains("Uninstall Rexon"), "{item}");
        assert!(c.undo.contains(&format!("“{item}”")), "{}", c.undo);
        assert!(c.undo.contains(crate::window_menu::MENU_BUTTON), "{}", c.undo);
        assert!(c.hint.contains(crate::system_plan::REXON_POLKIT_RULES_PATH));
        // Правило, о котором говорит текст: группа администраторов
        // семейства, только локально, без пакетного помощника.
        let rule = crate::system_plan::passwordless_rules_js(PkgFamily::Arch);
        assert!(rule.contains("isInGroup(\"wheel\")") && rule.contains("subject.local"));
        assert!(c.hint.contains("wheel"));
        let deb = crate::system_plan::passwordless_rules_js(PkgFamily::Debian);
        assert!(deb.contains("isInGroup(\"sudo\")") && !deb.contains("\"wheel\""));
        assert!(!rule.contains(crate::system_plan::PKG_ACTION));
        assert!(c.what.contains("still ask for the password"), "{}", c.what);
        // 28.09.2026: темы экрана входа и загрузки из правила сняты — текст
        // согласия не обещает их «без пароля» ни словом.
        assert!(!c.risk.to_lowercase().contains("theme"), "{}", c.risk);
        // Правило снимается удалением Rexon — иначе «как отменить» лжёт.
        let sys = install_paths(InstallMode::System, HOME, None);
        assert!(uninstall_system_paths(&sys)
            .contains(&crate::system_plan::REXON_POLKIT_RULES_PATH.to_string()));
    }

    /// ⛔ Удаление Rexon снимает и помощника сеанса прав, и его политику
    /// (Б11): иначе в системе остался бы root-помощник с действием polkit,
    /// которое можно поднять и после удаления программы.
    #[test]
    fn uninstall_removes_session_helper_and_policy() {
        let sys = install_paths(InstallMode::System, HOME, None);
        let paths = uninstall_system_paths(&sys);
        for p in [
            crate::system_plan::SESSION_HELPER_PATH,
            crate::system_plan::SESSION_POLICY_PATH,
            crate::steamos_sysext::SESSION_HELPER_PATH,
            crate::steamos_sysext::SESSION_POLICY_PATH,
        ] {
            assert!(paths.contains(&p.to_string()), "{p} не снимается");
            assert!(uninstall_system_script(&sys).contains(&format!("rm -rf {p}\n")), "{p}");
        }
    }

    #[test]
    fn steps_have_distinct_lines() {
        english();
        let lines: std::collections::HashSet<String> =
            InstallStep::ALL.iter().map(|s| s.progress_line(Lang::En)).collect();
        assert_eq!(lines.len(), InstallStep::ALL.len());
        assert_eq!(
            InstallStep::CopyFiles.progress_line(Lang::En),
            "Preparing the program files…"
        );
    }

    #[test]
    fn command_dir_in_path_ignores_trailing_slash() {
        let dir = "/home/tester/.local/bin";
        assert!(command_dir_in_path(dir, Some("/usr/bin:/home/tester/.local/bin/")));
        assert!(command_dir_in_path("/home/tester/.local/bin/", Some("/home/tester/.local/bin")));
        assert!(!command_dir_in_path(dir, Some("/usr/bin:/home/tester/.local/binx")));
        assert!(!command_dir_in_path(dir, None));
        assert!(!command_dir_in_path("", Some("")));
    }
}

// ============================================================================
// Помощник тем экрана входа и загрузки: что он пропускает в систему
// ============================================================================
//
// ⛔ Сверки стоят на месте настоящей дыры (28.09.2026): plymouth строит
// путь модуля заставки как `<каталог модулей>/<ModuleName>.so` без проверки
// `../`, и тема с `.so` внутри давала код от root при каждой загрузке.
// Помощник гоняется НАСТОЯЩИМ текстом, от обычного пользователя и только до
// строки «Установка» — в систему тест ничего не пишет.

#[cfg(test)]
mod system_helper_tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// Каталог темы под /tmp (помощник принимает только /tmp и /var/tmp).
    fn theme_dir(tag: &str) -> PathBuf {
        let dir = PathBuf::from("/tmp").join(format!(
            "rexon-system-helper-test-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(dir: &Path, rel: &str, body: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    /// Текст помощника до установки, затем отметка «проверки пройдены».
    fn validation_part() -> String {
        let full = system_helper_script();
        let cut = full.find("# ── Установка").expect("раздел установки в помощнике");
        format!("{}echo REXON_VALIDATED\nexit 0\n", &full[..cut])
    }

    fn uid() -> String {
        let out = Command::new("id").arg("-u").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// (прошла ли проверка, stderr помощника).
    fn run_helper(category: &str, theme_id: &str, staged: &Path, pkexec_uid: &str) -> (bool, String) {
        let out = Command::new("bash")
            .arg("-c")
            .arg(validation_part())
            .arg("rexon-system-helper")
            .args([category, theme_id, &staged.to_string_lossy(), "arch"])
            .env("PKEXEC_UID", pkexec_uid)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let ok = out.status.success() && stdout.contains("REXON_VALIDATED");
        (ok, String::from_utf8_lossy(&out.stderr).into_owned())
    }

    fn good_plymouth(dir: &Path) {
        write(dir, "neon.plymouth", b"[Plymouth Theme]\nName=Neon\nModuleName=script\n\n[script]\nScriptFile=/usr/share/plymouth/themes/neon/neon.script\n");
        write(dir, "neon.script", b"Window.SetBackgroundTopColor(0, 0, 0);\n");
        write(dir, "img/logo.png", b"\x89PNG");
    }

    /// Оба стража — помощник от root и проверка до пароля — отвечают одно.
    fn both_refuse(tag: &str, category: &str, theme_id: &str, build: impl Fn(&Path)) -> String {
        let dir = theme_dir(tag);
        build(&dir);
        let (ok, err) = run_helper(category, theme_id, &dir, &uid());
        let early = system_theme_problem(category, &dir, Some(theme_id));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!ok, "{tag}: помощник пропустил тему");
        assert!(early.is_some(), "{tag}: проверка до пароля пропустила тему");
        err
    }

    #[test]
    fn штатная_тема_plymouth_проходит() {
        let dir = theme_dir("good");
        good_plymouth(&dir);
        let (ok, err) = run_helper("plymouth", "neon", &dir, &uid());
        assert!(ok, "{err}");
        assert_eq!(system_theme_problem("plymouth", &dir, Some("neon")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn модуль_из_темы_через_точки_отвергнут() {
        let err = both_refuse("dotdot", "plymouth", "neon", |d| {
            write(d, "neon.plymouth", b"[Plymouth Theme]\nModuleName=../../../share/plymouth/themes/neon/evil\n");
            write(d, "evil.so", b"\x7fELF");
        });
        assert!(err.contains("library") || err.contains("ModuleName"), "{err}");
        // Без .so — всё равно отказ: имя модуля обязано быть штатным.
        both_refuse("dotdot-noso", "plymouth", "neon", |d| {
            write(d, "neon.plymouth", b"[Plymouth Theme]\nModuleName=../evil\n");
        });
    }

    /// Разбор plymouth (`fscanf " %m[^= \t\n] = %m[^\n] "`) терпит перевод
    /// строки между ключом и `=`: построчная проверка обязана это видеть.
    #[test]
    fn ключ_и_значение_на_разных_строках_отвергнуты() {
        both_refuse("split", "plymouth", "neon", |d| {
            write(d, "neon.plymouth", b"[Plymouth Theme]\nModuleName\n= ../evil\n");
        });
        both_refuse("case", "plymouth", "neon", |d| {
            write(d, "neon.plymouth", b"[Plymouth Theme]\nmodulename=script\n");
        });
        both_refuse("nul", "plymouth", "neon", |d| {
            write(d, "neon.plymouth", b"[Plymouth Theme]\nModuleName=script\0/../evil\n");
        });
        both_refuse("second", "plymouth", "neon", |d| {
            good_plymouth(d);
            write(d, "alt/other.plymouth", b"[Plymouth Theme]\nModuleName=/tmp/evil\n");
        });
        both_refuse("nomodule", "plymouth", "neon", |d| {
            write(d, "neon.plymouth", b"[Plymouth Theme]\nName=Neon\n");
        });
    }

    #[test]
    fn библиотека_в_теме_загрузки_отвергнута() {
        for name in ["lib/two-step.so", "x.so.1"] {
            both_refuse("so", "plymouth", "neon", |d| {
                good_plymouth(d);
                write(d, name, b"\x7fELF");
            });
        }
    }

    #[test]
    fn ссылка_в_теме_отвергнута() {
        for category in ["sddm", "plymouth"] {
            let err = both_refuse("link", category, "neon", |d| {
                good_plymouth(d);
                write(d, "Main.qml", b"import QtQuick 2.0\nItem {}\n");
                symlink("/etc/shadow", d.join("shadow")).unwrap();
            });
            assert!(err.contains("link"), "{err}");
        }
    }

    /// SDDM-тема работает от пользователя `sddm`, а не root: QML-модуль в
    /// ней — не повод для отказа.
    #[test]
    fn тема_sddm_с_модулем_qml_проходит() {
        let dir = theme_dir("sddm");
        write(&dir, "Main.qml", b"import QtQuick 2.0\nItem {}\n");
        write(&dir, "plugin/libx.so", b"\x7fELF");
        let (ok, err) = run_helper("sddm", "neon", &dir, &uid());
        assert!(ok, "{err}");
        assert_eq!(system_theme_problem("sddm", &dir, Some("neon")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Каталог — по месту входа, а не по строке пути; чужой — отказ.
    #[test]
    fn чужой_или_внешний_каталог_отвергнут() {
        let dir = theme_dir("where");
        good_plymouth(&dir);
        // Ссылка под /tmp на каталог вне /tmp.
        let link = PathBuf::from("/tmp").join(format!("rexon-system-helper-test-lnk-{}", std::process::id()));
        let _ = std::fs::remove_file(&link);
        symlink("/usr", &link).unwrap();
        let (ok, err) = run_helper("plymouth", "neon", &link, &uid());
        assert!(!ok && err.contains("outside /tmp"), "{err}");
        let _ = std::fs::remove_file(&link);
        // Владелец каталога — не вызвавший.
        let (ok, err) = run_helper("plymouth", "neon", &dir, "0");
        assert!(!ok && err.contains("does not belong to the caller"), "{err}");
        // Без pkexec — отказ.
        let out = Command::new("bash")
            .arg("-c")
            .arg(validation_part())
            .arg("rexon-system-helper")
            .args(["plymouth", "neon", &dir.to_string_lossy(), "arch"])
            .env_remove("PKEXEC_UID")
            .output()
            .unwrap();
        assert!(!out.status.success());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn имя_темы_не_флаг_и_не_корень() {
        let dir = theme_dir("names");
        good_plymouth(&dir);
        for bad in ["-R", ".", "..", "a/b", "", ".hidden", "a b"] {
            let (ok, _) = run_helper("plymouth", bad, &dir, &uid());
            assert!(!ok, "имя {bad:?} пропущено");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Каталог модулей и сам список — одна правда в помощнике и в Rust.
    #[test]
    fn список_модулей_в_помощнике_из_ядра() {
        let text = system_helper_script();
        assert!(text.contains(&PLYMOUTH_SPLASH_MODULES.join("|")));
        assert!(!text.contains("__PLY_MODULES__"));
        for m in PLYMOUTH_SPLASH_MODULES {
            assert!(m.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{m}");
        }
        // Помощник не исполняет ничего из темы и не принимает путь назначения.
        for banned in ["eval ", "source ", "bash -c", "sh -c"] {
            assert!(!text.contains(banned), "{banned} в помощнике");
        }
    }

    // ── Режим «Удалить» (remove-<вид>) в подменённом корне ────────────
    //
    // Помощник гоняется ЦЕЛИКОМ, от обычного пользователя: системные пути
    // текста (`/usr/share/`, `/etc/`, `/usr/lib/sddm`) переписываются на
    // песочницу под /tmp, менеджеры пакетов — заглушки впереди PATH.
    // Живой `/usr` тест не трогает.

    struct Sandbox {
        root: PathBuf,
    }

    impl Sandbox {
        fn new(tag: &str) -> Self {
            let root = theme_dir(&format!("rm-{tag}"));
            let stub = root.join("stub");
            std::fs::create_dir_all(&stub).unwrap();
            // «Пакет владеет» только каталогом pkgtheme.
            write(&stub, "pacman", b"#!/bin/bash\ncase \"${@: -1}\" in */pkgtheme) exit 0 ;; esac\nexit 1\n");
            write(&stub, "dpkg-query", b"#!/bin/bash\nexit 1\n");
            write(&stub, "rpm", b"#!/bin/bash\nexit 1\n");
            for f in ["pacman", "dpkg-query", "rpm"] {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(stub.join(f), std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            for kind in ["plymouth", "sddm"] {
                std::fs::create_dir_all(root.join(format!("usr/share/{kind}/themes"))).unwrap();
            }
            Sandbox { root }
        }

        fn theme(&self, kind: &str, name: &str) -> PathBuf {
            let d = self.root.join(format!("usr/share/{kind}/themes/{name}"));
            write(&d, &format!("{name}.plymouth"), b"[Plymouth Theme]\nModuleName=script\n");
            d
        }

        fn script(&self) -> String {
            let r = self.root.to_string_lossy();
            system_helper_script()
                .replace("/usr/share/", &format!("{r}/usr/share/"))
                .replace("/etc/", &format!("{r}/etc/"))
                .replace("/usr/lib/sddm", &format!("{r}/usr/lib/sddm"))
                .replace("export PATH=/usr/sbin", &format!("export PATH={r}/stub:/usr/sbin"))
        }

        /// (успех, stderr).
        fn remove(&self, category: &str, theme: &str, staged: &str) -> (bool, String) {
            let out = Command::new("bash")
                .arg("-c")
                .arg(self.script())
                .arg("rexon-system-helper")
                .args([category, theme, staged, "arch"])
                .env("PKEXEC_UID", uid())
                .output()
                .unwrap();
            (out.status.success(), String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn удаление_снимает_только_свой_каталог() {
        let sb = Sandbox::new("ok");
        let neon = sb.theme("plymouth", "neon");
        let other = sb.theme("plymouth", "other");
        // Ссылка внутри темы наружу: снимается сама ссылка, не то, куда она ведёт.
        let outside = sb.root.join("outside");
        write(&outside, "keep.txt", b"keep");
        symlink(&outside, neon.join("out")).unwrap();
        let (ok, err) = sb.remove("remove-plymouth", "neon", "-");
        assert!(ok, "{err}");
        assert!(!neon.exists(), "тема не снята");
        assert!(other.is_dir(), "соседняя тема задета");
        assert!(outside.join("keep.txt").is_file(), "удаление прошло по ссылке наружу");
        let sddm = sb.theme("sddm", "sugar");
        let (ok, err) = sb.remove("remove-sddm", "sugar", "-");
        assert!(ok && !sddm.exists(), "{err}");
    }

    #[test]
    fn удаление_выбранной_темы_отвергнуто() {
        let sb = Sandbox::new("active");
        let neon = sb.theme("plymouth", "neon");
        write(&sb.root, "etc/plymouth/plymouthd.conf", b"[Daemon]\nTheme = neon\n");
        let (ok, err) = sb.remove("remove-plymouth", "neon", "-");
        assert!(!ok && neon.is_dir() && err.contains("selected right now"), "{err}");
        // Выбор через update-alternatives: default.plymouth ведёт в тему.
        let sb = Sandbox::new("alt");
        let neon = sb.theme("plymouth", "neon");
        symlink(neon.join("neon.plymouth"), sb.root.join("usr/share/plymouth/themes/default.plymouth")).unwrap();
        let (ok, err) = sb.remove("remove-plymouth", "neon", "-");
        assert!(!ok && neon.is_dir(), "{err}");
        // SDDM: любой файл настроек, в т. ч. системные умолчания.
        for conf in ["etc/sddm.conf.d/kde_settings.conf", "usr/lib/sddm/sddm.conf.d/default.conf", "etc/sddm.conf"] {
            let sb = Sandbox::new("sddm-active");
            let sugar = sb.theme("sddm", "sugar");
            write(&sb.root, conf, b"[Theme]\r\n  Current=sugar  \r\n");
            let (ok, err) = sb.remove("remove-sddm", "sugar", "-");
            assert!(!ok && sugar.is_dir() && err.contains("selected right now"), "{conf}: {err}");
        }
        // Похожее имя — не та тема.
        let sb = Sandbox::new("similar");
        let sugar = sb.theme("sddm", "sugar");
        write(&sb.root, "etc/sddm.conf", b"[Theme]\nCurrent=sugar-dark\n");
        let (ok, err) = sb.remove("remove-sddm", "sugar", "-");
        assert!(ok && !sugar.exists(), "{err}");
    }

    #[test]
    fn удаление_мимо_каталога_тем_отвергнуто() {
        let sb = Sandbox::new("guard");
        // Каталог пакета системы.
        let pkg = sb.theme("plymouth", "pkgtheme");
        let (ok, err) = sb.remove("remove-plymouth", "pkgtheme", "-");
        assert!(!ok && pkg.is_dir() && err.contains("package"), "{err}");
        // Тема-ссылка на чужой каталог.
        let target = sb.root.join("elsewhere");
        write(&target, "x", b"x");
        symlink(&target, sb.root.join("usr/share/sddm/themes/lnk")).unwrap();
        let (ok, err) = sb.remove("remove-sddm", "lnk", "-");
        assert!(!ok && target.join("x").is_file() && err.contains("not installed"), "{err}");
        // Нет такой темы.
        let (ok, _) = sb.remove("remove-sddm", "absent", "-");
        assert!(!ok);
        // Каталог вместо прочерка, недопустимые имена, чужой вид.
        let keep = sb.theme("plymouth", "neon");
        let (ok, _) = sb.remove("remove-plymouth", "neon", "/tmp");
        assert!(!ok && keep.is_dir());
        for bad in ["..", ".", "-R", "a/b", "../themes", "", "neon/.."] {
            let (ok, _) = sb.remove("remove-plymouth", bad, "-");
            assert!(!ok, "имя {bad:?} пропущено");
        }
        let (ok, _) = sb.remove("remove-gtk", "neon", "-");
        assert!(!ok && keep.is_dir());
        assert!(sb.root.join("usr/share/plymouth/themes").is_dir());
    }

    #[test]
    fn текст_отказа_для_человека() {
        crate::i18n::set_language(Some(crate::i18n::SOURCE_LANGUAGE));
        let m = SystemThemeProblem::BootSplashCode("evil.so".into()).message();
        assert!(m.contains("evil.so") && m.contains("Nothing was changed"), "{m}");
        let m = SystemThemeProblem::OddFile("shadow".into()).message();
        assert!(m.contains("shadow"), "{m}");
    }
}
