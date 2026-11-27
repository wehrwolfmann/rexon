English | [Русский](../ru/extensions.md) | [Deutsch](../de/extensions.md)

# Extensions

This tab holds `.rxn` packages — Rexon's own add-on format: themes, wallpaper
packs, fonts, desktop tools and plugins. You can also drop a Windows theme or
Windows cursors here, and Rexon converts them for Linux.

GNOME Shell and Cinnamon extensions are not installed here but in the
[Appearance](appearance.md) tab, in the "GNOME Extensions" and "Cinnamon
Extensions" sections.

![The Extensions tab with nothing installed yet](../images/extensions-overview.png)

## What's on the tab

- **View switches** "Grid" (⊞) and "List" (☰) — tiles with a picture or
  compact rows.
- **＋ Install from File** — pick a file on disk.
- **Search** — by name, author and tags.
- **Type filters** — "All" plus one button for each type you already have
  installed (for example, only GTK themes or only tools).
- **The count line** — "Installed" and "Active".
- **My Extensions** — the installed packages. Each one has:
  - **▶️ Enable** / **⏸️ Disable** — for tools, plugins and desktop add-ons
    (KWin scripts and effects, GNOME extensions, Cinnamon applets and
    desklets, Nemo actions). They are off after installation — you turn them
    on yourself;
  - **Apply** — for wallpaper packs;
  - **🗑️ Remove**.

  Clicking a tile opens the "Extension" window with the description and an
  **📁 Open Folder** button.
- **⬇️ Install** — three ways:
  - **From File** — open an `.rxn`, a `.themepack` or a Windows theme;
  - **Steam Workshop — coming soon** — not available yet;
  - **Create Your Own** — a short guide to building your own extension.

## How to

### Install an extension

1. Click **＋ Install from File** (or **From File** under "⬇️ Install").
2. Choose an `.rxn` file.
3. Wait for "✅ Installed: …" at the bottom of the window.

Even simpler: drag the file straight into the Rexon window.

### Install a Windows theme or cursors

Drag into the window, or open with **From File**:

- themes: `.themepack`, `.deskthemepack`, `.theme`, `.msstyles`;
- cursors: `.cur`, `.ani`, `.inf`, `.curxptheme`;
- icon packs: `.7tsp`.

Rexon converts them into an `.rxn` package and installs it: the package
appears under "My Extensions", and the theme itself appears in
[Appearance](appearance.md), in the section for its kind. You apply it from
there.

### Find an extension

Start typing a name, an author or a tag in the search field. To show only
one kind of package, click that type's button under the search field; "All"
clears the filter.

### Turn an extension on or off

Click **▶️ Enable** on the tile — "▶️ Enabled: …" appears at the bottom. To
turn it off, click **⏸️ Disable**. Themes and fonts have no such button: they
simply stay installed and are applied in [Appearance](appearance.md).

### Remove

1. Click **🗑️ Remove** on the tile or in the "Extension" window.
2. Confirm in the "Remove Extension?" window. The extension's files are
   deleted permanently.

### Make your own `.rxn` extension

Click **Create Your Own**. Rexon shows a short guide: an `.rxn` is a plain
ZIP archive with a `manifest.json` file (name, version, author, type), a
picture for the catalog and the extension's own files. The guide includes a
sample `manifest.json` and the list of types.

![The "Create Your Own .rxn Extension" guide](../images/extensions-create.png)

## If something goes wrong

- **"⚠️ An installation is already running".** Wait for the previous
  installation or removal to finish and try again.
- **"⚠️ Error: …".** The file is damaged or isn't a Rexon extension. The text
  after the colon tells you why. If the "Installed" line comes with notes
  (for example, a program needed to convert a Windows theme wasn't found),
  the extension was installed, but not completely.
- **"⚠️ Couldn't turn it on or off: …".** The extension couldn't be enabled
  or disabled. Logging out and back in often helps — especially for GNOME and
  Cinnamon extensions.
- **The desktop acts up after enabling an extension.** Turn it off with
  **⏸️ Disable** or remove it. If the desktop is badly broken, log in to
  another desktop and click **🔧 Check / Repair** on the broken one's card in
  the [Desktops](desktops.md) tab.
- **Be careful with tools and plugins.** Packages of these two types run
  their own install and uninstall scripts (as your user, without
  administrator rights). Install them only from people you trust.

## See also

- [Desktops](desktops.md)
- [Appearance](appearance.md) — where themes are applied, and where GNOME and
  Cinnamon extensions live
- [Wallpaper Engine](wallpaper-engine.md)
- [Steam](steam.md)
- [Settings](settings.md)
- [Guide contents](../README.md)
