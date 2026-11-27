English | [Русский](../ru/wallpaper-engine.md) | [Deutsch](../de/wallpaper-engine.md) | [简体中文](../zh-CN/wallpaper-engine.md) | [Français](../fr/wallpaper-engine.md) | [Español](../es/wallpaper-engine.md)

# Wallpaper Engine

This is where live wallpapers live: Wallpaper Engine scenes, videos and web
wallpapers, plus pictures from the free Wallhaven catalog. You can put a
wallpaper on the whole desktop or on a single monitor, build a playlist that
changes wallpapers on a timer, and subscribe to new wallpapers in the Steam
Workshop right from Rexon.

The tab is drawn by the live wallpaper engine
[waywallen](https://github.com/hypengw/waywallen): its windows are built into
Rexon as they are, so the look and labels here differ a little from the other
tabs. Rexon adds its own hints and puts the wallpapers on your desktop by
itself.

> To use wallpapers from the Steam Workshop you need to **own Wallpaper Engine
> on Steam**: the Workshop only serves its wallpapers to owners. Videos and
> pictures from your own folders and the Wallhaven catalog work without it.

![The Wallpaper Engine tab: section panel on the left, wallpaper library on the right](../images/wallpaper-engine-library.png)

## What's on the tab

On the left is the section panel (the "☰" button at the top folds it down to
icons):

- **Wallpapers** — your library: everything stored on this computer.
- **Discover** — online catalogs: **Steam Workshop** and **Wallhaven**.
- **Displays** — your screens and what each of them shows.
- **Status** — whether the wallpaper engine is running and what it is drawing.
- At the bottom: **Plugins**, **Settings** (how the wallpapers behave) and
  **About**.

Above the library are sorting (by name, size, last modified), search and the
buttons on the right: **Playlists**, **Tweak** (card size), **Filters** (by
type, tags, content rating), **Library Manager** (which folders to scan) and
**Refresh**.

## Which desktops it works on

Rexon picks how to show the wallpaper on your desktop by itself — there is
nothing to set up:

- **KDE Plasma** — the wallpaper is built into the Plasma desktop: desktop
  icons and clicks work as usual.
- **Hyprland, Sway, niri, river, COSMIC, Wayfire** — the engine shows the
  wallpaper itself.
- **GNOME and Cinnamon in a Wayland session** — Rexon's shell extension shows
  the wallpaper. If it doesn't appear, turn the extension on and log in again.
- **MATE, Xfce, Cinnamon, Budgie, LXQt, LXDE, Pantheon, Deepin and other
  desktops in an X11 session** — Rexon's own wallpaper player shows it. Some
  engine features may be unavailable here. In MATE the desktop icons stay on
  top of the live wallpaper when Caja can draw a see-through desktop — Rexon
  turns that on by itself. If it can't, the icons are hidden while the live
  wallpaper runs (the tab says so in one line) and come back when it stops.

On a Wayland compositor Rexon doesn't know there are no live wallpapers.

If your desktop draws its own background (for example the Noctalia or
DankMaterialShell shells), a strip with an explanation appears at the top of
the tab, with the buttons **"Give Wallpaper to Rexon"** (turn the shell's
background off) and **"Return to Shell"** (turn it back on with its previous
setting).

## How to find and apply a wallpaper

1. Open the **Wallpaper Engine** tab → **Wallpapers**.
2. If the library is empty, press **"Auto detect libraries"**: Rexon finds
   the Steam folder and the Wallpaper Engine wallpapers already downloaded.
   Another folder can be added with **Library Manager** → **"Add library"**.

   ![Empty library with the "Auto detect libraries" button](../images/wallpaper-engine-empty.png)

3. Click a wallpaper card. A details panel opens on the right: preview,
   description, **Fill mode** (crop, fit, stretch, center), rotation, flip
   and the **Properties** the author provided (color, sound, speed and so on).
4. With several monitors the panel has an **"Apply to"** row — pick a monitor
   or **"All"**.
5. Press **"Apply"**. Rexon puts the wallpaper on the desktop by itself.

![Wallpaper details panel with the "Apply" button](../images/wallpaper-engine-details.png)

While nothing is shown on the desktop yet, **Displays** may be empty and say
"Wallpaper Engine wallpapers are not on the desktop yet…". That's normal: the
monitors appear after the first "Apply".

## Displays

**Displays** shows every screen and the wallpaper on it. Here you can:

- rename a monitor (**"Edit display"**) and choose its fill mode;
- pause the wallpaper on one monitor (**"Pause display"**);
- join several monitors into a **canvas** (**"Create canvas"**) so one
  wallpaper spans all of them.

![Displays before the first wallpaper is applied](../images/wallpaper-engine-displays.png)

## Playlists: wallpapers that change by themselves

1. In **Wallpapers**, select several cards: right-click them (or Ctrl-click,
   or press and hold).
2. A panel appears at the bottom — press **"New playlist"**, type a name and
   press **"Create"**. **"Add to playlist"** adds to an existing playlist.

   ![Two cards selected, panel with "New playlist" at the bottom](../images/wallpaper-engine-select.png)

3. Open the list with **Playlists** (right, above the library). Each playlist
   has the buttons **"Play playlist"**, **"Edit playlist"**, **"Edit
   wallpapers"** and **"Delete playlist"**. Above the list you choose which
   monitors to play on (**"All"** includes displays connected later).

   ![The playlist list](../images/wallpaper-engine-playlists.png)

4. In **"Edit playlist"** choose the **Mode** — **Sequential**, **Shuffle**
   or **Random** — and the **Rotation interval** (hours, minutes, seconds).
   Save with the check mark at the top.

![Editing a playlist: mode and rotation interval](../images/wallpaper-engine-playlist-edit.png)

## Steam Workshop: subscribing to new wallpapers

You need Wallpaper Engine bought on Steam and the Steam client running.

1. Open **Discover** and choose **Steam Workshop** in the first list.
2. Press **"Log in to Steam"** and scan the QR code with the Steam mobile app.
   Rexon never sees or asks for your Steam password.

   ![Discover → Steam Workshop before logging in](../images/wallpaper-engine-discover.png)

3. After logging in the catalog appears. Sort by "Trending (Today)",
   "Trending (Week)" and so on, "Recently Added" or "Top Rated"; there's
   search and **Filters**.
   **"My Subscriptions"** shows what you have already subscribed to.
4. Open a wallpaper and press **"Subscribe"**. The Steam client downloads the
   files itself — keep it running, and the wallpaper appears in
   **Wallpapers** automatically.
5. Unsubscribe in the same place (**"Unsubscribe"**) or in the details panel
   in your library.

Wallpapers marked 18+ stay hidden until you turn that filter on yourself
(Rexon asks again).

**Wallhaven** is the second source in **Discover**: free pictures, no login
needed. A picture you like can be saved with **"Download"**.

## Wallpaper settings

**Settings** at the bottom of the section panel opens the engine's own
settings:

- **Auto replay** — what to do with the wallpaper when any, focused,
  maximized or fullscreen window is open, the session is locked or the
  computer runs on battery: nothing, mute, pause or stop. This way games and
  films don't share the graphics card with the wallpaper.
- **Audio** — volume and "Mute for other active playback streams".
- **Wallpaper transition** and its duration, **Excluded windows**, and the
  cache for pictures from the network.

![Wallpaper engine settings](../images/wallpaper-engine-settings.png)

## If something goes wrong

- **"Starting the waywallen wallpaper engine…" takes long.** The first start
  after installation can take a minute or two while the engine unpacks
  itself. Please wait.
- **"The waywallen wallpaper engine did not start".** Press **"Retry
  Start"**. If that doesn't help, look at **Status** → **Logs**.
- **The library is empty although you own wallpapers.** Check that Steam has
  downloaded them (they are in `steamapps/workshop/content/431960`) and press
  **"Auto detect libraries"** or **Refresh**. If Steam lives in an unusual
  place, add its folder in **Library Manager** — the folder that contains
  `steamapps`.
- **Subscribed, but the wallpaper didn't appear.** The Steam client must be
  running — it downloads the wallpapers, not Rexon.
- **In KDE Plasma, a warning about the Plasma wallpaper plugin at the top.**
  Press **"Install Rexon's Version"** — Rexon installs a plugin that fits.
- **No wallpaper on GNOME or Cinnamon.** Turn on Rexon's extension and log in
  again.
- **The shell draws the background instead of Rexon.** Press **"Give
  Wallpaper to Rexon"** in the strip at the top of the tab.
