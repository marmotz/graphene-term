[Français](../fr/installation.md)

# Installation

Graphene Term runs on Linux (x86_64) and macOS (Apple Silicon and Intel). Install a
package from a release, or build it from source.

- [Packages](#packages)
  - [Debian, Ubuntu (.deb)](#debian-ubuntu-deb)
  - [Fedora, openSUSE, RHEL (.rpm)](#fedora-opensuse-rhel-rpm)
  - [AppImage](#appimage)
  - [Linux archive (.tar.gz)](#linux-archive-targz)
  - [macOS (.dmg)](#macos-dmg)
  - [macOS archive (.tar.gz)](#macos-archive-targz)
  - [Check a download](#check-a-download)
- [Build from source](#build-from-source)
- [Requirements](#requirements)
- [Uninstall](#uninstall)

## Packages

Every release on the [releases page](https://github.com/marmotz/graphene-term/releases)
ships these files (`<version>` is the release number, such as `0.1.3`):

| System                 | File                                                  |
|------------------------|-------------------------------------------------------|
| Debian, Ubuntu         | `graphene-term_<version>_amd64.deb`                   |
| Fedora, openSUSE, RHEL | `graphene-term-<version>-1.x86_64.rpm`                |
| Any Linux              | `graphene-v<version>-x86_64.AppImage`                 |
| Any Linux              | `graphene-v<version>-x86_64-unknown-linux-gnu.tar.gz` |
| macOS, Apple Silicon   | `graphene-v<version>-aarch64-apple-darwin.dmg`        |
| macOS, Intel           | `graphene-v<version>-x86_64-apple-darwin.dmg`         |
| macOS, command line    | `graphene-v<version>-<arch>-apple-darwin.tar.gz`      |

The Linux packages are built on Ubuntu 24.04. A distribution with an older C library
(glibc) may not run them: [build from source](#build-from-source) instead.

### Debian, Ubuntu (.deb)

```bash
sudo apt install ./graphene-term_<version>_amd64.deb
```

This installs the `graphene` command and a "Graphene Term" entry in your applications
menu.

### Fedora, openSUSE, RHEL (.rpm)

```bash
sudo dnf install ./graphene-term-<version>-1.x86_64.rpm      # Fedora, RHEL
sudo zypper install ./graphene-term-<version>-1.x86_64.rpm   # openSUSE
```

Like the `.deb`, it installs the `graphene` command and a menu entry.

### AppImage

The AppImage is a single file that runs without installation:

```bash
chmod +x graphene-v<version>-x86_64.AppImage
./graphene-v<version>-x86_64.AppImage
```

Move it anywhere you like, for example `~/.local/bin/graphene`.

### Linux archive (.tar.gz)

The archive holds the `graphene` program, a menu entry (`graphene.desktop`) and an icon
(`graphene.svg`). To install them for your user:

```bash
tar -xzf graphene-v<version>-x86_64-unknown-linux-gnu.tar.gz
cd graphene-v<version>-x86_64-unknown-linux-gnu
install -Dm755 graphene ~/.local/bin/graphene
install -Dm644 graphene.desktop ~/.local/share/applications/graphene.desktop
install -Dm644 graphene.svg ~/.local/share/icons/hicolor/scalable/apps/graphene.svg
```

`~/.local/bin` must be in your `PATH` for the menu entry to find `graphene`.

### macOS (.dmg)

1. Download the `.dmg` for your Mac: `aarch64` for Apple Silicon (M1 and later),
   `x86_64` for Intel.
2. Open it and drag **Graphene.app** to **Applications**.
3. Start Graphene from Applications.

The app is signed but not notarized by Apple, so macOS blocks its first launch. To
allow it, either:

- open **System Settings → Privacy & Security** and click **Open Anyway** next to the
  message about Graphene, then confirm; or
- run in a terminal:

  ```bash
  xattr -dr com.apple.quarantine /Applications/Graphene.app
  ```

You only need to do this once per installed version.

### macOS archive (.tar.gz)

For running `graphene` from another terminal or a script, the archive holds the bare
program:

```bash
tar -xzf graphene-v<version>-aarch64-apple-darwin.tar.gz
install -m755 graphene-v<version>-aarch64-apple-darwin/graphene /usr/local/bin/graphene
```

Use the `x86_64` archive on an Intel Mac. If macOS refuses to run the downloaded
program, remove the quarantine flag with `xattr -d com.apple.quarantine` on it.

### Check a download

Each release also has a `SHA256SUMS` file. In the folder holding your download:

```bash
sha256sum --ignore-missing -c SHA256SUMS     # Linux
shasum -a 256 --ignore-missing -c SHA256SUMS # macOS
```

## Build from source

You need:

- **Rust** (stable), installed with [rustup](https://rustup.rs). The repository pins
  the stable toolchain, so rustup picks it automatically.
- **Git** and a **C compiler**: `build-essential` on Debian and Ubuntu, `gcc` on
  Fedora, the Xcode Command Line Tools on macOS (`xcode-select --install`).

Then:

```bash
git clone https://github.com/marmotz/graphene-term.git
cd graphene-term
cargo build --release
```

The program is `target/release/graphene`. Copy it into your `PATH`, for example:

```bash
install -Dm755 target/release/graphene ~/.local/bin/graphene
```

On Linux, the menu entry and icon are in `packaging/` (`graphene.desktop`,
`graphene.svg`); install them as shown for the [Linux archive](#linux-archive-targz).
To work on Graphene itself, see the [development guide](../dev/development.md).

## Requirements

- A graphics card with Vulkan or OpenGL drivers on Linux (Metal on macOS).
- On Linux, a Wayland or X11 desktop. Graphene loads the Wayland, X11 and keyboard
  (`libxkbcommon`) libraries your desktop already provides.
- A translucent background (`background_opacity`) needs a compositor on Linux.

## Uninstall

| Installed with  | Remove with                                                     |
|-----------------|-----------------------------------------------------------------|
| `.deb`          | `sudo apt remove graphene-term`                                 |
| `.rpm`          | `sudo dnf remove graphene-term` or `sudo zypper remove graphene-term` |
| AppImage        | delete the file                                                 |
| `.tar.gz`       | delete the files you copied                                     |
| `.dmg`          | move **Graphene.app** to the Trash                              |

Your settings stay in the configuration folder (see
[Configuration](configuration.md#file-locations)); delete it to remove them too.
