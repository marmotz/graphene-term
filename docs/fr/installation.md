[English](../en/installation.md)

# Installation

Graphene Term fonctionne sous Linux (x86_64) et macOS (Apple Silicon et Intel).
Installez un paquet publié avec une version, ou compilez-le depuis les sources.

- [Paquets](#paquets)
  - [Debian, Ubuntu (.deb)](#debian-ubuntu-deb)
  - [Fedora, openSUSE, RHEL (.rpm)](#fedora-opensuse-rhel-rpm)
  - [AppImage](#appimage)
  - [Archive Linux (.tar.gz)](#archive-linux-targz)
  - [macOS (.dmg)](#macos-dmg)
  - [Archive macOS (.tar.gz)](#archive-macos-targz)
  - [Vérifier un téléchargement](#vérifier-un-téléchargement)
- [Compiler depuis les sources](#compiler-depuis-les-sources)
- [Prérequis](#prérequis)
- [Désinstaller](#désinstaller)

## Paquets

Chaque version publiée sur la [page des versions](https://github.com/marmotz/graphene-term/releases)
fournit ces fichiers (`<version>` est le numéro de version, par exemple `0.1.3`) :

| Système                | Fichier                                               |
|------------------------|-------------------------------------------------------|
| Debian, Ubuntu         | `graphene-term_<version>_amd64.deb`                   |
| Fedora, openSUSE, RHEL | `graphene-term-<version>-1.x86_64.rpm`                |
| Tout Linux             | `graphene-v<version>-x86_64.AppImage`                 |
| Tout Linux             | `graphene-v<version>-x86_64-unknown-linux-gnu.tar.gz` |
| macOS, Apple Silicon   | `graphene-v<version>-aarch64-apple-darwin.dmg`        |
| macOS, Intel           | `graphene-v<version>-x86_64-apple-darwin.dmg`         |
| macOS, ligne de commande | `graphene-v<version>-<arch>-apple-darwin.tar.gz`    |

Les paquets Linux sont compilés sur Ubuntu 24.04. Une distribution dont la bibliothèque
C (glibc) est plus ancienne peut ne pas les faire fonctionner :
[compilez depuis les sources](#compiler-depuis-les-sources) à la place.

### Debian, Ubuntu (.deb)

```bash
sudo apt install ./graphene-term_<version>_amd64.deb
```

Cela installe la commande `graphene` et une entrée « Graphene Term » dans le menu des
applications.

### Fedora, openSUSE, RHEL (.rpm)

```bash
sudo dnf install ./graphene-term-<version>-1.x86_64.rpm      # Fedora, RHEL
sudo zypper install ./graphene-term-<version>-1.x86_64.rpm   # openSUSE
```

Comme le `.deb`, il installe la commande `graphene` et une entrée de menu.

### AppImage

L'AppImage est un fichier unique qui se lance sans installation :

```bash
chmod +x graphene-v<version>-x86_64.AppImage
./graphene-v<version>-x86_64.AppImage
```

Placez-le où vous voulez, par exemple `~/.local/bin/graphene`.

### Archive Linux (.tar.gz)

L'archive contient le programme `graphene`, une entrée de menu (`graphene.desktop`) et
une icône (`graphene.svg`). Pour les installer pour votre utilisateur :

```bash
tar -xzf graphene-v<version>-x86_64-unknown-linux-gnu.tar.gz
cd graphene-v<version>-x86_64-unknown-linux-gnu
install -Dm755 graphene ~/.local/bin/graphene
install -Dm644 graphene.desktop ~/.local/share/applications/graphene.desktop
install -Dm644 graphene.svg ~/.local/share/icons/hicolor/scalable/apps/graphene.svg
```

`~/.local/bin` doit être dans votre `PATH` pour que l'entrée de menu trouve `graphene`.

### macOS (.dmg)

1. Téléchargez le `.dmg` correspondant à votre Mac : `aarch64` pour Apple Silicon (M1
   et suivants), `x86_64` pour Intel.
2. Ouvrez-le et faites glisser **Graphene.app** dans **Applications**.
3. Lancez Graphene depuis Applications.

L'application est signée mais pas notariée par Apple : macOS bloque donc son premier
lancement. Pour l'autoriser, au choix :

- ouvrez **Réglages Système → Confidentialité et sécurité** et cliquez sur
  **Ouvrir quand même** à côté du message concernant Graphene, puis confirmez ;
- ou lancez dans un terminal :

  ```bash
  xattr -dr com.apple.quarantine /Applications/Graphene.app
  ```

Il suffit de le faire une fois par version installée.

### Archive macOS (.tar.gz)

Pour lancer `graphene` depuis un autre terminal ou un script, l'archive contient le
programme seul :

```bash
tar -xzf graphene-v<version>-aarch64-apple-darwin.tar.gz
install -m755 graphene-v<version>-aarch64-apple-darwin/graphene /usr/local/bin/graphene
```

Prenez l'archive `x86_64` sur un Mac Intel. Si macOS refuse de lancer le programme
téléchargé, retirez-lui l'attribut de quarantaine avec `xattr -d com.apple.quarantine`.

### Vérifier un téléchargement

Chaque version fournit aussi un fichier `SHA256SUMS`. Dans le dossier qui contient
votre téléchargement :

```bash
sha256sum --ignore-missing -c SHA256SUMS     # Linux
shasum -a 256 --ignore-missing -c SHA256SUMS # macOS
```

## Compiler depuis les sources

Il vous faut :

- **Rust** (stable), installé avec [rustup](https://rustup.rs). Le dépôt fixe la
  chaîne d'outils stable, rustup la choisit donc tout seul.
- **Git** et un **compilateur C** : `build-essential` sur Debian et Ubuntu, `gcc` sur
  Fedora, les Xcode Command Line Tools sur macOS (`xcode-select --install`).

Puis :

```bash
git clone https://github.com/marmotz/graphene-term.git
cd graphene-term
cargo build --release
```

Le programme est `target/release/graphene`. Copiez-le dans votre `PATH`, par exemple :

```bash
install -Dm755 target/release/graphene ~/.local/bin/graphene
```

Sous Linux, l'entrée de menu et l'icône sont dans `packaging/` (`graphene.desktop`,
`graphene.svg`) ; installez-les comme pour l'[archive Linux](#archive-linux-targz).
Pour travailler sur Graphene lui-même, voir le
[guide de développement](../dev/development.md) (en anglais).

## Prérequis

- Une carte graphique avec des pilotes Vulkan ou OpenGL sous Linux (Metal sous macOS).
- Sous Linux, un bureau Wayland ou X11. Graphene charge les bibliothèques Wayland, X11
  et clavier (`libxkbcommon`) déjà fournies par votre bureau.
- Un fond translucide (`background_opacity`) nécessite un compositeur sous Linux.

## Désinstaller

| Installé avec   | Supprimer avec                                                  |
|-----------------|-----------------------------------------------------------------|
| `.deb`          | `sudo apt remove graphene-term`                                 |
| `.rpm`          | `sudo dnf remove graphene-term` ou `sudo zypper remove graphene-term` |
| AppImage        | supprimez le fichier                                            |
| `.tar.gz`       | supprimez les fichiers copiés                                   |
| `.dmg`          | mettez **Graphene.app** à la corbeille                          |

Vos paramètres restent dans le dossier de configuration (voir
[Configuration](configuration.md#emplacement-des-fichiers)) ; supprimez-le pour les
effacer aussi.
