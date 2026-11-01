// rexon-theme-shot — помощник плиток «Оформления»: снимок формы из настоящих
// виджетов в стиле Qt (`qt`) или теме GTK 3 (`gtk`), PNG размером с плитку.
//
// Отдельный процесс намеренно: чужой модуль стиля Qt и движок темы GTK не
// попадают в процесс Rexon, зависший снимок убивается по сроку. Что снимать,
// строку запуска, окружение и палитру решает ядро
// (`rexon_core::widget_shot`), здесь — только исполнение:
//
//     rexon-theme-shot qt --style Fusion --out /tmp/x.png [--color Window=#202326]… [--text …]…
//     rexon-theme-shot gtk --out /tmp/x.png [--text …]…   (тема — GTK_THEME, дисплей сеанса)
//
// Код возврата: 0 — снимок записан; 2 — разбор аргументов; 3 — стиль/тема
// не загрузились; 4 — снимок не записался; 5 — GTK не поднялся (нет
// библиотеки или дисплея).

mod gtk;

use std::ffi::{c_char, CString};

// Ссылка на библиотеку крейта: статический архив шимов (здесь —
// cpp/rexon_theme_shot.cpp) приезжает вместе с ней.
use rexon_gui as _;

extern "C" {
    fn rexon_qt_style_shot(
        style: *const c_char,
        roles: *const *const c_char,
        colors: *const *const c_char,
        count: i32,
        texts: *const *const c_char,
        text_count: i32,
        form_w: i32,
        form_h: i32,
        out_w: i32,
        out_h: i32,
        scale: i32,
        out: *const c_char,
    ) -> i32;
}

fn cstrings<'a>(items: impl Iterator<Item = &'a str>) -> Vec<CString> {
    items.map(|s| CString::new(s.replace('\0', "")).unwrap_or_default()).collect()
}

fn qt(args: &rexon_core::widget_shot::HelperArgs) -> i32 {
    let style = CString::new(args.style.as_str()).unwrap_or_default();
    let out = CString::new(args.out.as_str()).unwrap_or_default();
    let roles = cstrings(args.palette.iter().map(|(r, _)| r.as_str()));
    let colors = cstrings(args.palette.iter().map(|(_, c)| c.as_str()));
    let texts = cstrings(args.texts.iter().map(String::as_str));
    let ptrs = |v: &[CString]| v.iter().map(|c| c.as_ptr()).collect::<Vec<_>>();
    let (role_p, color_p, text_p) = (ptrs(&roles), ptrs(&colors), ptrs(&texts));
    // SAFETY: все указатели живут до конца вызова; длины совпадают с массивами.
    unsafe {
        rexon_qt_style_shot(
            style.as_ptr(),
            role_p.as_ptr(),
            color_p.as_ptr(),
            role_p.len() as i32,
            text_p.as_ptr(),
            text_p.len() as i32,
            args.form.0,
            args.form.1,
            args.size.0,
            args.size.1,
            rexon_core::widget_shot::render_scale(args.form, args.size),
            out.as_ptr(),
        )
    }
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let args = match rexon_core::widget_shot::parse_helper_args(&raw) {
        Ok(a) => a,
        Err(why) => {
            eprintln!("rexon-theme-shot: {why}");
            std::process::exit(2);
        }
    };
    let code = if args.mode == rexon_core::widget_shot::MODE_QT { qt(&args) } else { gtk::shot(&args) };
    std::process::exit(code);
}
