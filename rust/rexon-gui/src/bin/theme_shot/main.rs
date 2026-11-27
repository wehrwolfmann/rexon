// rexon-theme-shot — the helper for "Appearance" tiles: a snapshot of a
// form made of real widgets in a Qt style (`qt`) or a GTK 3 theme (`gtk`),
// a PNG the size of a tile.
//
// A separate process on purpose: a foreign Qt style module and the GTK
// theme engine do not get into the Rexon process, and a hung snapshot is
// killed after a timeout. What to capture, the command line, the
// environment and the palette are decided by the core
// (`rexon_core::widget_shot`); here there is only execution:
//
//     rexon-theme-shot qt --style Fusion --out /tmp/x.png [--color Window=#202326]… [--text …]…
//     rexon-theme-shot gtk --out /tmp/x.png [--text …]…   (theme — GTK_THEME, the session display)
//
// Exit code: 0 — the snapshot is written; 2 — argument parsing; 3 — the
// style/theme did not load; 4 — the snapshot was not written; 5 — GTK did
// not come up (no library or display).

mod gtk;

use std::ffi::{c_char, CString};

// Link to the crate library: the static archive of shims (here —
// cpp/rexon_theme_shot.cpp) comes along with it.
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
    // SAFETY: all pointers live until the end of the call; the lengths match
    // the arrays.
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
