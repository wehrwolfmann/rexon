"""Что видит GTK3 / GTK4 / libadwaita-программа: имя темы и вид окна."""
import sys, gi
kind = sys.argv[1]  # gtk3 | gtk4 | adw
if kind == "gtk3":
    gi.require_version("Gtk", "3.0")
    from gi.repository import Gtk, GLib
    w = Gtk.Window(title="GTK3")
    box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
    box.pack_start(Gtk.Label(label="GTK3: тема?"), False, False, 0)
    box.pack_start(Gtk.Button(label="Кнопка GTK3"), False, False, 0)
    box.pack_start(Gtk.CheckButton(label="флажок"), False, False, 0)
    w.add(box); w.set_default_size(300, 160); w.move(0, 0); w.show_all()
    s = Gtk.Settings.get_default()
    print("gtk3 theme =", s.props.gtk_theme_name, "| icons =", s.props.gtk_icon_theme_name,
          "| cursor =", s.props.gtk_cursor_theme_name, s.props.gtk_cursor_theme_size,
          "| font =", s.props.gtk_font_name, flush=True)
    GLib.timeout_add(4000, Gtk.main_quit); Gtk.main()
else:
    gi.require_version("Gtk", "4.0")
    from gi.repository import Gtk, GLib
    if kind == "adw":
        gi.require_version("Adw", "1")
        from gi.repository import Adw
        app = Adw.Application(application_id="org.rexon.probe.adw")
    else:
        app = Gtk.Application(application_id="org.rexon.probe.gtk4")
    def on_activate(a):
        w = (Adw.ApplicationWindow if kind == "adw" else Gtk.ApplicationWindow)(application=a)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        if kind == "adw":
            box.append(Adw.HeaderBar())
        box.append(Gtk.Label(label=f"{kind}: тема?"))
        box.append(Gtk.Button(label=f"Кнопка {kind}"))
        box.append(Gtk.CheckButton(label="флажок"))
        (w.set_content if kind == "adw" else w.set_child)(box)
        w.set_default_size(300, 160); w.present()
        s = Gtk.Settings.get_default()
        print(kind, "theme =", s.props.gtk_theme_name, "| icons =", s.props.gtk_icon_theme_name,
              "| cursor =", s.props.gtk_cursor_theme_name, s.props.gtk_cursor_theme_size,
              "| font =", s.props.gtk_font_name, flush=True)
        GLib.timeout_add(4000, a.quit)
    app.connect("activate", on_activate)
    app.run([])
