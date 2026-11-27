# Graphene Term

Un émulateur de terminal moderne, accéléré par le GPU, pour Linux et macOS, écrit en
Rust, avec des blocs de commande, une invite utilisable à la souris, des onglets et des
panneaux.

[English](README.md)

<!-- Capture : mettre l'image dans docs/images/ (voir docs/images/README.md), puis
décommenter la ligne ci-dessous.
![Graphene Term](docs/images/screenshot.png)
-->

- [Fonctionnalités](#fonctionnalités)
- [Comparatif](#comparatif)
- [Installation](#installation)
- [Configuration](#configuration)
- [Compiler depuis les sources](#compiler-depuis-les-sources)
- [Contribuer](#contribuer)
- [Documentation](#documentation)
- [Licence](#licence)

## Fonctionnalités

- **Rendu GPU** en truecolor, avec un fond translucide :
  [thèmes](docs/fr/themes.md#fond-translucide).
- **Blocs de commande** : chaque commande et sa sortie forment un bloc avec son code de
  retour ; sélectionnez-le, copiez-le, repliez-le et dépliez-le :
  [blocs de commande](docs/fr/command-blocks.md).
- **Recherche dans l'historique** avec `Ctrl+Shift+F` : toutes les occurrences
  surlignées, bascules casse, regex et mot entier, filtres commande / sortie et code de
  retour : [recherche dans l'historique](docs/fr/command-blocks.md#rechercher-dans-lhistorique).
- **Intégration shell automatique** pour zsh, bash et fish, sans modifier vos fichiers
  de configuration : [intégration shell](docs/fr/command-blocks.md#intégration-shell).
- **Éditeur d'invite enrichi** : cliquez pour déplacer le curseur, sélectionnez à la
  souris ou avec `Shift+Flèche`, raccourcis d'éditeur :
  [éditeur de saisie](docs/fr/input-editor.md).
- **Onglets et panneaux**, déplacement des panneaux à la souris, zoom sur un panneau,
  saisie dans plusieurs panneaux à la fois : [onglets et panneaux](docs/fr/tabs-and-panes.md),
  [diffusion](docs/fr/tabs-and-panes.md#diffusion).
- **Restauration de session** : onglets, disposition des panneaux, répertoires et tailles
  de police reviennent au lancement suivant : [configuration](docs/fr/configuration.md#window).
- **Taille de police par panneau**, avec `Ctrl+=` / `Ctrl+-` ou `Ctrl` + molette :
  [raccourcis clavier](docs/fr/keybindings.md#défilement-et-taille-de-police).
- **Ligatures** et fonctionnalités OpenType des polices :
  [paramètres de police](docs/fr/configuration.md#font).
- **Palette de commandes** avec recherche approximative sur toutes les actions :
  [palette de commandes](docs/fr/tabs-and-panes.md#palette-de-commandes).
- **Images dans le terminal** (protocole graphique de kitty avec animations, sixel) et **liens cliquables** (OSC 8 et
  URL détectées) :
  [images et liens](docs/fr/images-and-links.md).
- **Sélection à la souris**, y compris rectangulaire, et transmission de la souris aux
  applications : [sélection et presse-papiers](docs/fr/selection-and-clipboard.md).
- **Raccourcis clavier modifiables** : [raccourcis clavier](docs/fr/keybindings.md).
- **Notifications de bureau** pour les commandes longues, les programmes (OSC 9, OSC 777)
  et la cloche, et un **indicateur de progression** (OSC 9;4) :
  [notifications](docs/fr/configuration.md#notifications),
  [progression](docs/fr/tabs-and-panes.md#progression).
- **Quick terminal** attaché à un bord de l'écran, à activer (`enabled`), affiché et
  masqué par `graphene --quick-toggle` (macOS et X11), démarré et restauré avec la session :
  [quick terminal](docs/fr/quick-terminal.md).
- **Fenêtre des paramètres**, rechargement à chaud de la configuration, taille de
  fenêtre mémorisée, notification de mise à jour :
  [configuration](docs/fr/configuration.md).
- **Huit thèmes intégrés**, vos propres fichiers de thème, un éditeur de thème, et des
  thèmes qui suivent le mode clair/sombre du système : [thèmes](docs/fr/themes.md).
- **Interface en anglais et en français**, avec leurs variantes britannique et
  canadienne, selon la langue et la région du système, nombres et erreurs de
  configuration compris :
  [paramètre de langue](docs/fr/configuration.md#général).

## Comparatif

Graphene comparé à d'autres terminaux Linux, chacun dans sa configuration par défaut.

**Performances** : Graphene est dans la moyenne des autres terminaux : il met 0,22 s à
afficher avec `cat` un fichier texte de 11 Mo (mesuré sur la 0.1.3), contre 0,16 s à 0,48 s pour les autres.
Il démarre plus lentement et consomme plus de mémoire qu'Alacritty. Les mesures, la
machine de test et la façon de les reproduire sont dans
[Comparing terminals](docs/dev/development.md#comparing-terminals) (en anglais).

✅ pris en charge, ❌ non pris en charge, ⚠️ partiel (voir les notes sous le tableau).

| Fonctionnalité                                                                              | Graphene 0.4.1 | Alacritty 0.17.0 | Kitty 0.50.0 | WezTerm 20260716 | Terminator 2.1.6 | GNOME Terminal 3.60.0 |
|---------------------------------------------------------------------------------------------|----------------|------------------|--------------|------------------|------------------|-----------------------|
| **Rendu**                                                                                   |                |                  |              |                  |                  |                       |
| Rendu GPU                                                                                   | ✅             | ✅               | ✅           | ✅               | ❌               | ❌                    |
| Truecolor                                                                                   | ✅             | ✅               | ✅           | ✅               | ✅               | ✅                    |
| Images dans le terminal (protocole graphique de kitty, avec animations)                     | ✅             | ❌               | ✅           | ⚠️¹              | ❌               | ❌                    |
| Images sixel                                                                                | ✅             | ❌               | ❌           | ✅               | ✅               | ⚠️²                   |
| Fond transparent                                                                            | ✅             | ✅               | ✅           | ✅               | ✅               | ❌                    |
| **Shell**                                                                                   |                |                  |              |                  |                  |                       |
| Blocs de commande avec code de retour                                                       | ✅             | ❌               | ❌           | ❌               | ❌               | ❌                    |
| Replier / déplier les sorties                                                               | ✅             | ❌               | ❌           | ❌               | ❌               | ❌                    |
| Copier une commande ou sa sortie                                                            | ✅             | ❌               | ⚠️³          | ⚠️⁴              | ❌               | ❌                    |
| Intégration shell automatique (zsh, bash, fish) sans modifier les fichiers de configuration | ✅             | ❌               | ✅           | ❌               | ❌               | ❌                    |
| Éditeur d'invite enrichi (curseur à la souris, sélection, raccourcis d'éditeur)             | ✅             | ❌               | ⚠️⁵          | ❌               | ❌               | ❌                    |
| **Disposition**                                                                             |                |                  |              |                  |                  |                       |
| Onglets                                                                                     | ✅             | ❌               | ✅           | ✅               | ✅               | ✅                    |
| Panneaux                                                                                    | ✅             | ❌               | ✅           | ✅               | ✅               | ❌                    |
| Déplacer les panneaux à la souris                                                           | ✅             | ❌               | ✅           | ❌               | ✅               | ❌                    |
| Zoom sur un panneau                                                                         | ✅             | ❌               | ✅           | ✅               | ✅               | ❌                    |
| Palette de commandes (recherche approximative)                                              | ✅             | ❌               | ✅           | ✅               | ❌               | ❌                    |
| **Interaction**                                                                             |                |                  |              |                  |                  |                       |
| Liens cliquables (OSC 8 et URL détectées)                                                   | ✅             | ✅               | ✅           | ✅               | ✅               | ✅                    |
| Sélection rectangulaire                                                                     | ✅             | ✅               | ✅           | ✅               | ✅               | ✅                    |
| Transmission de la souris aux applications                                                  | ✅             | ✅               | ✅           | ✅               | ✅               | ✅                    |
| Raccourcis clavier modifiables                                                              | ✅             | ✅               | ✅           | ✅               | ✅               | ✅                    |
| **Configuration**                                                                           |                |                  |              |                  |                  |                       |
| Fenêtre des paramètres (interface graphique)                                                | ✅             | ❌               | ❌           | ❌               | ✅               | ✅                    |
| Rechargement à chaud de la configuration                                                    | ✅             | ✅               | ⚠️⁶          | ✅               | ✅               | ✅                    |
| Thèmes qui suivent le mode clair/sombre du système                                          | ✅             | ❌               | ✅           | ⚠️⁷              | ❌               | ⚠️⁸                   |
| Thèmes intégrés                                                                             | ✅             | ❌               | ✅           | ✅               | ✅               | ✅                    |
| Interface traduite (en, fr)                                                                 | ✅             | ❌               | ❌           | ❌               | ✅               | ✅                    |
| Taille de fenêtre mémorisée                                                                 | ✅             | ❌               | ✅           | ❌               | ❌               | ❌                    |
| Notification de mise à jour                                                                 | ✅             | ❌               | ⚠️⁹          | ✅               | ❌               | ❌                    |

1. WezTerm implémente une partie du protocole graphique de kitty.
2. GNOME Terminal : désactivé par défaut, à activer dans les préférences du profil.
3. Kitty copie ou affiche la sortie de la dernière commande avec un raccourci, pas la
   commande.
4. WezTerm sélectionne une commande ou sa sortie une fois son intégration shell ajoutée
   à vos fichiers de configuration.
5. Kitty déplace le curseur au clic dans l'invite, sans sélection à la souris.
6. Kitty recharge sa configuration avec un raccourci (`Ctrl+Shift+F5`), pas tout seul.
7. WezTerm demande quelques lignes de Lua dans la configuration.
8. GNOME Terminal suit le mode du système uniquement avec les couleurs du thème système.
9. Kitty ne vérifie les mises à jour que dans ses versions officielles, pas dans les
   paquets des distributions.

## Installation

Téléchargez un paquet depuis la
[page des releases](https://github.com/marmotz/graphene-term/releases) :

- **Debian, Ubuntu** : `.deb`
- **Fedora, openSUSE, RHEL** : `.rpm`
- **Toute distribution Linux** : `.AppImage` ou `.tar.gz`
- **macOS** (Apple Silicon ou Intel) : `.dmg`, ou `.tar.gz` pour la ligne de commande

Les commandes d'installation, le premier lancement sur macOS et la désinstallation sont
dans le [guide d'installation](docs/fr/installation.md).

## Configuration

Les paramètres sont dans `~/.config/graphene/config.toml` sous Linux, ou
`~/Library/Application Support/graphene/config.toml` sous macOS. Le fichier est créé au
premier lancement avec tous les paramètres en commentaire, et les modifications
s'appliquent dès l'enregistrement. La fenêtre des paramètres (`Ctrl+,`) modifie le même
fichier.

Chaque paramètre est décrit dans le [guide de configuration](docs/fr/configuration.md).

## Compiler depuis les sources

Il faut Rust (stable, via [rustup](https://rustup.rs)), Git et un compilateur C.

```bash
git clone https://github.com/marmotz/graphene-term.git
cd graphene-term
cargo build --release
```

Le programme est `target/release/graphene`. Les détails par système sont dans le
[guide d'installation](docs/fr/installation.md#compiler-depuis-les-sources) ; pour
travailler sur Graphene lui-même, voir [CONTRIBUTING.md](CONTRIBUTING.md) (en anglais).

## Contribuer

Les rapports de bug et les pull requests sont les bienvenus : lisez
[CONTRIBUTING.md](CONTRIBUTING.md) (en anglais).

## Documentation

- [Guide d'utilisation](docs/fr/README.md) ([in English](docs/en/README.md)) :
  installation, configuration, thèmes, raccourcis, onglets et panneaux, blocs de
  commande et plus encore.
- [Documentation contributeur](docs/dev/README.md) (en anglais) : vision, architecture,
  décisions techniques, guide de développement.

## Licence

Graphene Term est distribué sous la licence [MIT](LICENSE-MIT) ou
[Apache-2.0](LICENSE-APACHE), au choix.
