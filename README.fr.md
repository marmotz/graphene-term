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
- **Intégration shell automatique** pour zsh, bash et fish, sans modifier vos fichiers
  de configuration : [intégration shell](docs/fr/command-blocks.md#intégration-shell).
- **Éditeur d'invite enrichi** : cliquez pour déplacer le curseur, sélectionnez à la
  souris ou avec `Shift+Flèche`, raccourcis d'éditeur :
  [éditeur de saisie](docs/fr/input-editor.md).
- **Onglets et panneaux**, déplacement des panneaux à la souris, zoom sur un panneau :
  [onglets et panneaux](docs/fr/tabs-and-panes.md).
- **Palette de commandes** avec recherche approximative sur toutes les actions :
  [palette de commandes](docs/fr/tabs-and-panes.md#palette-de-commandes).
- **Images dans le terminal** (protocole graphique de kitty avec animations, sixel) et
  **liens cliquables** (OSC 8 et URL détectées) :
  [images et liens](docs/fr/images-and-links.md).
- **Sélection à la souris**, y compris rectangulaire, et transmission de la souris aux
  applications : [sélection et presse-papiers](docs/fr/selection-and-clipboard.md).
- **Raccourcis clavier modifiables** : [raccourcis clavier](docs/fr/keybindings.md).
- **Fenêtre des paramètres**, rechargement à chaud de la configuration, taille de
  fenêtre mémorisée, notification de mise à jour :
  [configuration](docs/fr/configuration.md).
- **Huit thèmes intégrés**, vos propres fichiers de thème, et des thèmes qui suivent le
  mode clair/sombre du système : [thèmes](docs/fr/themes.md).
- **Interface en anglais et en français**, selon la langue du système :
  [paramètre de langue](docs/fr/configuration.md#général).

## Comparatif

Graphene comparé à d'autres terminaux Linux, chacun dans sa configuration par défaut.

### Performances

<!-- Coller ici le tableau affiché par `scripts/benchmark-terminals.py run`, en
traduisant les en-têtes. -->

| Terminal | Débit (s) | Taux de rafraîchissement (fps) | Démarrage (ms) | Mémoire au repos (Mio) | Mémoire sous charge (Mio) |
|---|---|---|---|---|---|
| **Graphene** |  |  |  |  |  |
| Alacritty |  |  |  |  |  |
| Kitty |  |  |  |  |  |
| WezTerm |  |  |  |  |  |
| foot |  |  |  |  |  |
| Terminator |  |  |  |  |  |
| GNOME Terminal |  |  |  |  |  |
| Konsole |  |  |  |  |  |

Plus bas est meilleur, sauf pour le taux de rafraîchissement. Chaque valeur est la
médiane de plusieurs mesures. Le débit (temps pour afficher avec `cat` un fichier texte
de 11 Mo) et le taux de rafraîchissement mesurent la vitesse à laquelle un terminal
absorbe la sortie, pas ce qu'il dessine à l'écran.

### Fonctionnalités comparées

✅ pris en charge, ❌ non pris en charge, ⚠️ partiel (voir la note).

<!-- Écrire la version testée de chaque terminal dans l'en-tête, puis remplir les
cases (mêmes cases que dans README.md). -->

| Fonctionnalité | Graphene | Alacritty | Kitty | WezTerm | foot | Terminator | GNOME Terminal | Konsole |
|---|---|---|---|---|---|---|---|---|
| **Rendu** | | | | | | | | |
| Rendu GPU | | | | | | | | |
| Truecolor | | | | | | | | |
| Images dans le terminal (protocole graphique de kitty, avec animations) | | | | | | | | |
| Images sixel | | | | | | | | |
| Fond translucide | | | | | | | | |
| **Shell** | | | | | | | | |
| Blocs de commande avec code de retour | | | | | | | | |
| Replier / déplier les sorties | | | | | | | | |
| Copier une commande ou sa sortie | | | | | | | | |
| Intégration shell automatique (zsh, bash, fish) sans modifier les fichiers de configuration | | | | | | | | |
| Éditeur d'invite enrichi (curseur à la souris, sélection, raccourcis d'éditeur) | | | | | | | | |
| **Disposition** | | | | | | | | |
| Onglets | | | | | | | | |
| Panneaux | | | | | | | | |
| Déplacer les panneaux à la souris | | | | | | | | |
| Zoom sur un panneau | | | | | | | | |
| Palette de commandes (recherche approximative) | | | | | | | | |
| **Interaction** | | | | | | | | |
| Liens cliquables (OSC 8 et URL détectées) | | | | | | | | |
| Sélection rectangulaire | | | | | | | | |
| Transmission de la souris aux applications | | | | | | | | |
| Raccourcis clavier modifiables | | | | | | | | |
| **Configuration** | | | | | | | | |
| Fenêtre des paramètres (interface graphique) | | | | | | | | |
| Rechargement à chaud de la configuration | | | | | | | | |
| Thèmes qui suivent le mode clair/sombre du système | | | | | | | | |
| Thèmes intégrés | | | | | | | | |
| Interface traduite (en, fr) | | | | | | | | |
| Taille de fenêtre mémorisée | | | | | | | | |
| Notification de mise à jour | | | | | | | | |

### Machine de test et reproduction

<!-- Coller ici le bloc « Machine » affiché par le script. -->

Les tableaux viennent de [scripts/benchmark-terminals.py](scripts/benchmark-terminals.py)
(Python 3, Linux). Il lance chaque terminal installé et affiche le tableau de
performances et la machine :

```bash
cargo build --release
scripts/benchmark-terminals.py run
```

Voir [Comparing terminals](docs/dev/development.md#comparing-terminals) (en anglais)
pour ce que mesure chaque colonne.

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
