[English](../en/extensions.md) | [Русский](../ru/extensions.md) | [Deutsch](../de/extensions.md) | [简体中文](../zh-CN/extensions.md) | Français

# Extensions

Cet onglet accueille les paquets `.rxn`, le format d'extensions propre à
Rexon : thèmes, collections de fonds d'écran, polices, outils pour le bureau
et greffons. Vous pouvez aussi y déposer un thème ou des curseurs de Windows :
Rexon les convertit pour Linux.

Les extensions GNOME Shell et Cinnamon ne s'installent pas ici, mais dans
l'onglet [Apparence](appearance.md), dans les sections « Extensions GNOME »
et « Extensions Cinnamon ».

![L'onglet « Extensions », rien n'est encore installé](../images/extensions-overview.png)

## Ce que contient l'onglet

- **Les sélecteurs d'affichage** « Grille » (⊞) et « Liste » (☰) : des
  vignettes avec image ou des lignes compactes.
- **＋ Installer depuis un fichier** — choisir un fichier sur le disque.
- **Rechercher** — par nom, auteur et étiquettes.
- **Les filtres par type** — « Tous » et un bouton pour chaque type que vous
  avez déjà installé (par exemple seulement les thèmes GTK ou seulement les
  outils).
- **La ligne de compteurs** — « Installé » et « Actifs ».
- **Mes extensions** — les paquets installés. Chacun a :
  - **▶️ Activer** / **⏸️ Désactiver** — pour les outils, les greffons et les
    compléments de bureau (scripts et effets KWin, extensions GNOME, applets
    et desklets Cinnamon, actions Nemo). Après l'installation, ils sont
    désactivés : c'est vous qui les activez ;
  - **Appliquer** — pour les collections de fonds d'écran ;
  - **🗑️ Supprimer**.

  Un clic sur la vignette ouvre la fenêtre « Extension » avec la description
  et le bouton **📁 Ouvrir le dossier**.
- **⬇️ Installer** — trois façons :
  - **Depuis un fichier** — ouvrir un `.rxn`, un `.themepack` ou un thème
    Windows ;
  - **Workshop Steam — bientôt** — ne fonctionne pas encore, arrivera plus
    tard ;
  - **Créer le vôtre** — un aide-mémoire pour assembler votre propre
    extension.

## Comment faire

### Installer une extension

1. Cliquez sur **＋ Installer depuis un fichier** (ou sur **Depuis un
   fichier** dans la section « ⬇️ Installer »).
2. Choisissez un fichier `.rxn`.
3. Attendez la ligne « ✅ Installé : … » en bas de la fenêtre.

Plus simple encore : faites glisser le fichier directement dans la fenêtre de
Rexon.

### Installer un thème ou des curseurs de Windows

Faites-les glisser dans la fenêtre ou ouvrez-les avec **Depuis un fichier** :

- thèmes : `.themepack`, `.deskthemepack`, `.theme`, `.msstyles` ;
- curseurs : `.cur`, `.ani`, `.inf`, `.curxptheme` ;
- jeux d'icônes : `.7tsp`.

Rexon les convertit en paquet `.rxn` et l'installe : le paquet apparaît dans
« Mes extensions », et le thème lui-même dans l'onglet
[Apparence](appearance.md), dans la section de son type. C'est de là qu'on
l'applique.

### Trouver une extension

Commencez à taper un nom, un nom d'auteur ou une étiquette dans la barre de
recherche. Pour ne garder qu'un seul type de paquets, cliquez sur le bouton
du type voulu sous la barre de recherche ; « Tous » retire le filtre.

### Activer ou désactiver

Cliquez sur **▶️ Activer** sur la vignette : « ▶️ Activé : … » s'affiche en
bas. Pour désactiver : **⏸️ Désactiver**. Les thèmes et les polices n'ont pas
ce bouton : ils restent simplement installés et s'appliquent dans l'onglet
[Apparence](appearance.md).

### Supprimer

1. Cliquez sur **🗑️ Supprimer** sur la vignette ou dans la fenêtre
   « Extension ».
2. Confirmez dans la fenêtre « Supprimer ? ». Les fichiers de l'extension sont
   supprimés définitivement.

### Créer votre propre extension `.rxn`

Cliquez sur **Créer le vôtre**. Rexon affiche un aide-mémoire : un `.rxn` est
une simple archive ZIP contenant un fichier `manifest.json` (nom, version,
auteur, type), une image pour le catalogue et les fichiers de l'extension
elle-même. L'aide-mémoire donne un exemple de `manifest.json` et la liste des
types.

![L'aide-mémoire « Créer votre propre extension .rxn »](../images/extensions-create.png)

## En cas de problème

- **« ⚠️ Une installation est déjà en cours ».** Attendez la fin de
  l'installation ou de la suppression précédente, puis recommencez.
- **« ⚠️ Erreur : … ».** Le fichier est endommagé ou ce n'est pas une
  extension Rexon. Le texte après les deux-points indique la cause. Si la
  ligne « Installé » contient des réserves (par exemple, le programme
  nécessaire pour convertir un thème Windows est introuvable), l'extension
  s'est installée, mais pas entièrement.
- **« ⚠️ Impossible d'activer ou de désactiver : … ».** L'extension n'a pas
  pu être activée ou désactivée. Fermer la session et la rouvrir aide
  souvent, surtout pour les extensions GNOME et Cinnamon.
- **Après l'activation d'une extension, le bureau se comporte bizarrement.**
  Désactivez-la avec **⏸️ Désactiver** ou supprimez-la. Si le bureau est
  complètement cassé, ouvrez une session dans un autre bureau et cliquez sur
  **🔧 Vérifier / Réparer** sur la fiche du bureau en cause dans l'onglet
  [Bureaux](desktops.md).
- **Prudence avec les outils et les greffons.** Les paquets de ces deux types
  lancent leurs propres scripts d'installation et de suppression (en votre
  nom, sans droits d'administrateur). N'installez que ceux qui viennent de
  personnes de confiance.

## Voir aussi

- [Bureaux](desktops.md)
- [Apparence](appearance.md) — où s'appliquent les thèmes et où se trouvent
  les extensions GNOME et Cinnamon
- [Wallpaper Engine](wallpaper-engine.md)
- [Steam](steam.md)
- [Paramètres](settings.md)
- [Sommaire du guide](../README.md)
