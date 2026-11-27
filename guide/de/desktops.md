[English](../en/desktops.md) | [Русский](../ru/desktops.md) | Deutsch

# Desktops

In diesem Reiter installieren, wechseln und entfernen Sie Desktops. Hier
können Sie auch prüfen, ob Ihrem Desktop etwas fehlt, Desktops miteinander
vergleichen und Ihren Desktop mit einem Klick wie eine bekannte Distribution
aussehen lassen.

![Der Reiter „Desktops“: links die Liste, in der Mitte „Wie in einer Distribution“ und „Werkzeuge“, rechts die Desktop-Karte](../images/desktops-overview.png)

## Was der Reiter enthält

**Die Liste der Desktops (links).** Die Desktops sind gruppiert: „Klassisch“
(Cinnamon, XFCE, MATE, GNOME, KDE Plasma, Budgie, Pantheon, Deepin, COSMIC,
LXQt), „Kachelnd“ (Hyprland, niri, Sway, River) und „Compositoren mit
Effekten“ (Wayfire). Lässt sich ein Desktop auf Ihrer Distribution nicht
installieren, steht das direkt in der Liste.

**Die Desktop-Karte (rechts)** — alles über den ausgewählten Desktop:

- Bildschirmfotos (die Pfeile blättern, die Lupe oder ein Klick auf das Bild
  öffnet es im Vollbild);
- der Zustand: „Installiert“, „Nicht installiert“, „Aktueller Desktop“;
- eine kurze Beschreibung und vier Bewertungen: „Arbeitsspeicher“ (wie viel
  er direkt nach der Anmeldung belegt), „Prozessor“, „Anpassung“ und
  „Spiele“;
- wie viele Pakete des Desktops schon installiert sind, welche fehlen und
  für welche es Updates gibt;
- die installierte Version und, falls vorhanden, eine neuere aus den
  Paketquellen;
- Schaltflächen:
  - **📦 Installieren** — bei einem Desktop, den Sie noch nicht haben;
  - **🔄 Wechseln** — abmelden und sich an diesem Desktop anmelden;
  - **🔧 Prüfen / Reparieren** — fehlende Pakete finden und nachinstallieren;
  - **🗑️ Entfernen** — den Desktop entfernen. Der aktuelle Desktop hat diese
    Schaltfläche nicht;
  - **➕ Variante Wayland hinzufügen** — bei einem installierten Desktop, der
    sowohl unter X11 als auch unter Wayland läuft, solange die
    Wayland-Variante noch fehlt.
- bei Hyprland, niri und Sway ein Block **„Shells“**: fertige Leisten mit
  Starter und Benachrichtigungen (Noctalia, DankMaterialShell). Sie lassen
  sich installieren, damit anmelden oder entfernen.

**🎨 Wie in einer Distribution (Mitte).** Das komplette Aussehen einer
bekannten Distribution auf einmal: derzeit „Wie SteamOS“ und „Wie Garuda
Linux (Dr460nized)“. Ein angewendetes Design bekommt die Schaltfläche
**↩️ Zurücksetzen**.

**🧰 Werkzeuge (darunter):**

- **⚔️ Desktops vergleichen** — eine Tabelle aller Desktops nebeneinander;
- **🔧 Desktop prüfen** — dasselbe wie „Prüfen / Reparieren“, aber für den
  Desktop, an dem Sie gerade arbeiten;
- **🛟 Leiste sichern** und **↩️ Leiste zurückholen** — eine Kopie der
  Leisteneinstellungen, falls Experimente mit Designs die Leiste beschädigen.
  Diese Schaltflächen gibt es nur in KDE Plasma, MATE und XFCE.

![Karte eines installierten Desktops mit „Wechseln“, „Prüfen / Reparieren“ und „Entfernen“](../images/desktops-installed.png)

## So geht's

### Einen Desktop installieren

1. Wählen Sie den Desktop in der Liste links.
2. Klicken Sie auf **📦 Installieren**.
3. Prüfen Sie im Fenster „Installieren?“, was installiert wird. Bietet der
   Desktop „Wie der Desktop laufen soll“ an, wählen Sie eine Variante (siehe
   unten).
4. Bestätigen Sie und geben Sie das Administratorpasswort ein, wenn das
   System danach fragt.
5. Sobald „Installation abgeschlossen“ erscheint, melden Sie sich ab und
   wählen den neuen Desktop auf dem Anmeldebildschirm. Braucht der Desktop
   einen Neustart, sagt Rexon das im selben Fenster.

### X11 oder Wayland wählen

Cinnamon, XFCE, MATE und LXQt können auf zwei Arten laufen. Die Auswahl
erscheint direkt im Fenster „Installieren?“ unter „Wie der Desktop laufen
soll“:

- **X11 — bewährt** — so läuft der Desktop seit Jahren, alle Programme und
  Einstellungen verhalten sich wie erwartet;
- **Wayland — neuer** — das Bild ist flüssiger und auf hochauflösenden
  Bildschirmen schärfer. Bei manchen Desktops ist es noch experimentell:
  einzelne Programme, Leisten-Applets und Einstellungen funktionieren
  vielleicht nicht;
- **Wayland — experimentell, aus dem AUR** — so ist Wayland bei MATE
  gekennzeichnet: Es wird auf Ihrem Rechner aus dem AUR gebaut, und Rexon
  weist im selben Fenster darauf hin.

Die empfohlene Variante ist mit „(empfohlen)“ markiert. Wählen Sie Wayland,
bleibt die X11-Variante ebenfalls erhalten — auf dem Anmeldebildschirm
stehen dann beide. Am Ende sagt Rexon, welche Sitzung Sie bei der Anmeldung
wählen. Die Wayland-Variante lässt sich auch später mit
**➕ Variante Wayland hinzufügen** auf der Karte nachrüsten.

### Zu einem anderen Desktop wechseln

1. Wählen Sie einen installierten Desktop in der Liste.
2. Klicken Sie auf **🔄 Wechseln**.
3. Speichern Sie offene Dokumente und bestätigen Sie. Rexon sichert die
   Einstellungen des aktuellen Desktops und meldet Sie ab.
4. Wählen Sie den neuen Desktop auf dem Anmeldebildschirm.

### Einen Desktop entfernen

1. Wählen Sie den Desktop und klicken Sie auf **🗑️ Entfernen**.
2. Das Fenster „Entfernen?“ zeigt, welcher Desktop gerade aktuell ist — er
   bleibt unberührt. Bestätigen Sie und geben Sie das Passwort ein.

Nicht entfernen lassen sich:

- **der aktuelle Desktop** — sonst gäbe es bei der nächsten Anmeldung einen
  schwarzen Bildschirm. Wechseln Sie zuerst zu einem anderen Desktop, melden
  Sie sich dort an und entfernen Sie erst dann diesen;
- **der letzte installierte Desktop** — installieren Sie zuerst einen
  anderen.

Pakete, die auch andere Programme brauchen, bleiben: Entfernt wird nur, was
nichts anderes mehr benötigt.

### Einen Desktop prüfen und reparieren

Benimmt sich ein Desktop seltsam (die Leiste ist weg, die Einstellungen
öffnen sich nicht), klicken Sie unter „Werkzeuge“ auf **🔧 Desktop prüfen**
oder auf der Karte eines installierten Desktops auf **🔧 Prüfen /
Reparieren**. Rexon gleicht die Paketliste des Desktops ab und installiert
fehlende Pakete nach. Meldet die Karte bereits fehlende Pakete, installiert
diese Schaltfläche genau sie.

### Desktops vergleichen

Klicken Sie auf **⚔️ Desktops vergleichen**. Es öffnet sich die Tabelle
„Alle Desktops“: Arbeitsspeicher, Prozessorlast, worauf der Desktop aufbaut
und was die Fenster zeichnet. Zeilen lassen sich aufklappen. Die
„Schnellvergleiche“ stellen häufige Paare nebeneinander (zum Beispiel
XFCE ↔ KDE). Zurück geht es mit **← Desktops**.

![Desktop-Vergleich](../images/desktops-compare.png)

### Den Desktop wie eine Distribution aussehen lassen

1. Klicken Sie im Block **🎨 Wie in einer Distribution** beim gewünschten
   Design auf **Anwenden**.
2. Rexon prüft, was dafür nötig ist, und zeigt ein Fenster: was sich ändert,
   was auf Ihrem Desktop nur teilweise greift und was gar nicht. Beim
   SteamOS-Design können Sie einen Stil wählen und bei Bedarf den Spielmodus
   einschalten.
3. Klicken Sie auf **Anwenden**. Braucht das Design Programme, installiert
   Rexon sie und fragt nach dem Passwort.
4. Doch nicht gewollt? Klicken Sie auf **↩️ Zurücksetzen**: Das vorherige
   Aussehen und die Einstellungen kommen zurück. Für das Design installierte
   Programme bleiben; Sie können sie selbst entfernen.

![Das Fenster „„Wie SteamOS“ anwenden?“ mit der Liste der Änderungen](../images/desktops-looks.png)

## Wenn etwas schiefgeht

- **Der neue Desktop steht nicht auf dem Anmeldebildschirm.** Melden Sie
  sich noch einmal ab. Hat Rexon einen Neustart verlangt, starten Sie den
  Rechner neu. Prüfen Sie bei Cinnamon, XFCE, MATE und LXQt, welche Sitzung
  (X11 oder Wayland) Sie wählen.
- **Der Desktop startet ohne Leiste oder mit Fehlern.** Melden Sie sich an
  einem anderen Desktop an oder bleiben Sie in diesem und klicken Sie auf
  **🔧 Desktop prüfen**.
- **Ein Design hat die Leiste beschädigt.** Haben Sie vorher
  **🛟 Leiste sichern** geklickt, klicken Sie auf **↩️ Leiste zurückholen**.
- **Das Distributions-Design gefällt nicht.** **↩️ Zurücksetzen** auf seiner
  Kachel.
- **Fenster „Ein Paket bekommt keine Updates mehr“.** Ein Paket, das aus den
  Paketquellen entfernt wurde, ist noch installiert. Nutzen Sie es nicht,
  kann Rexon es entfernen; sonst klicken Sie auf „Nicht jetzt“.
- **„Desktop nicht erkannt“.** Rexon hat den Desktop, an dem Sie arbeiten,
  nicht erkannt. Wählen Sie ihn in der Liste links und klicken Sie auf seiner
  Karte auf **🔧 Prüfen / Reparieren**.
- **Nach dem Wechsel ist die KDE-Plasma-Leiste weg.** Vor dem Wechsel weg von
  Plasma sichert Rexon dessen Leiste; zurückholen lässt sie sich unter
  **Einstellungen → 🛡️ Schutz der KDE-Plasma-Leiste**. Hat der neue Desktop
  Ihren Standardbrowser oder -player übernommen, sehen Sie unter
  **Einstellungen → 🔒 Schutz der Standardanwendungen** nach.
- **Das ganze System soll zurück.** Rexon selbst legt keine
  System-Snapshots an. Ist bei Ihnen snapper mit snap-pac eingerichtet (häufig
  bei CachyOS und anderen Distributionen auf Btrfs), entsteht vor jeder
  Installation und Entfernung von Paketen automatisch ein Snapshot, zu dem
  Sie mit den Werkzeugen von snapper zurückkehren können (zum Beispiel mit
  Btrfs Assistant).

## Siehe auch

- [Erscheinungsbild](appearance.md) — Designs, Symbole, Zeiger, Schriften,
  Anmeldebildschirm
- [Erweiterungen](extensions.md) — `.rxn`-Pakete und Windows-Designs
- [Wallpaper Engine](wallpaper-engine.md) — Live-Hintergründe
- [Steam](steam.md) — Startvideo und Profilhintergründe
- [Einstellungen](settings.md) — Sicherungen und Optionen von Rexon
- [Inhalt des Handbuchs](../README.md)
