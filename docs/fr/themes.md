[English](../en/themes.md)

# Thèmes

Un thème définit les couleurs du terminal (texte, fond, curseur, sélection, les 16
couleurs ANSI) et de l'interface de Graphene (barre d'onglets, bordures des panneaux,
palette de commandes, bandeaux).

- [Choisir un thème](#choisir-un-thème)
- [Thèmes intégrés](#thèmes-intégrés)
- [Suivre le mode clair/sombre du système](#suivre-le-mode-clairsombre-du-système)
- [Thèmes personnalisés](#thèmes-personnalisés)
- [Format d'un fichier de thème](#format-dun-fichier-de-thème)
- [Fond translucide](#fond-translucide)

## Choisir un thème

Chacun de ces moyens change le thème aussitôt :

- clic droit dans la fenêtre, puis **Thèmes** ;
- la section **Thème** de la fenêtre des paramètres (`Ctrl+,`) ;
- `name` dans la section `[theme]` du [fichier de configuration](configuration.md) :

  ```toml
  [theme]
  name = "gruvbox-dark"
  ```

Le menu et la fenêtre des paramètres écrivent ce même paramètre dans le fichier de
configuration.

## Thèmes intégrés

| Nom                | Type   |
|--------------------|--------|
| `graphene-night`   | sombre (par défaut) |
| `graphene-day`     | clair  |
| `solarized-dark`   | sombre |
| `solarized-light`  | clair  |
| `gruvbox-dark`     | sombre |
| `gruvbox-light`    | clair  |
| `catppuccin-mocha` | sombre |
| `catppuccin-latte` | clair  |

Leurs fichiers sont dans le dossier [themes/](../../themes/) du dépôt, un bon point de
départ pour créer le vôtre.

## Suivre le mode clair/sombre du système

Définissez à la fois `light` et `dark` dans la section `[theme]` :

```toml
[theme]
light = "graphene-day"
dark = "graphene-night"
```

Graphene utilise alors le thème `light` quand votre système est en mode clair et le
thème `dark` sinon, et bascule dès que le mode du système change. `name` n'est pas
utilisé tant que les deux sont définis ; si un seul des deux l'est, c'est `name` qui
s'applique.

Graphene lit le mode du système via le portail de paramètres du bureau
(`xdg-desktop-portal`), fourni par la plupart des bureaux Linux. Quand il n'est pas
disponible, le thème `dark` est utilisé.

## Thèmes personnalisés

Un thème est un fichier `.toml` placé dans le dossier `themes` à côté de votre fichier
de configuration :

| Système | Dossier des thèmes                                    |
|---------|-------------------------------------------------------|
| Linux   | `~/.config/graphene/themes/`                          |
| macOS   | `~/Library/Application Support/graphene/themes/`      |

Le nom du fichier, sans `.toml`, est le nom du thème : `themes/mon-theme.toml`
s'utilise avec `name = "mon-theme"`, et il apparaît dans le menu des thèmes et dans la
fenêtre des paramètres.

- Un fichier de thème qui porte le nom d'un thème intégré le remplace.
- Un fichier de thème n'a besoin que des couleurs qu'il change : chaque couleur absente
  vient de `graphene-night`.
- Enregistrer le fichier du thème en cours l'applique aussitôt.
- Une couleur invalide ou une clé inconnue est signalée dans le bandeau d'erreur et
  ignorée ; un nom qui ne correspond à aucun thème revient à `graphene-night`.

## Format d'un fichier de thème

Les couleurs s'écrivent `"#rrggbb"`. Un thème complet a ces clés :

```toml
[colors]
foreground = "#d7dae0"
background = "#14161b"
cursor = "#82aaff"
cursor_text = "#14161b"          # text under a block cursor
selection_background = "#33508a"
selection_foreground = ""        # empty: selected text keeps its own color

[colors.normal]                  # ANSI colors 0 to 7
black = "#1c1f26"
red = "#f0717b"
green = "#9ed072"
yellow = "#e7c664"
blue = "#6cb6ff"
magenta = "#c792ea"
cyan = "#5fd7d7"
white = "#c8ccd4"

[colors.bright]                  # ANSI colors 8 to 15
black = "#5c6370"
red = "#ff8b92"
green = "#b5e890"
yellow = "#ffd982"
blue = "#8ecaff"
magenta = "#dcb0ff"
cyan = "#82eaea"
white = "#f2f4f8"

[ui]
tab_bar_background = "#0f1115"
tab_active_background = "#1c1f26"
tab_active_text = "#d7dae0"
tab_inactive_text = "#7f8590"
pane_border = "#2a2e37"
palette_background = "#1c1f26"
palette_text = "#d7dae0"
palette_match = "#82aaff"        # matched letters in the command palette
palette_selection_background = "#2e3440"
banner_background = "#5c2a30"
banner_text = "#f2f4f8"
```

Ce sont les valeurs de `graphene-night`. Quelques précisions sur les clés :

- `cursor_text` : couleur du texte sous un curseur en bloc ;
- `selection_foreground` vide : le texte sélectionné garde sa propre couleur ;
- `[colors.normal]` et `[colors.bright]` : couleurs ANSI 0 à 7 et 8 à 15 ;
- `palette_match` : lettres trouvées dans la palette de commandes.

Un thème qui ne change que le fond :

```toml
[colors]
background = "#000000"
```

## Fond translucide

`background_opacity` dans la section `[window]` rend le fond transparent, de `0.0`
(transparent) à `1.0` (opaque, la valeur par défaut) :

```toml
[window]
background_opacity = 0.9
```

Seul le fond par défaut devient translucide : les fonds colorés dessinés par les
programmes, la sélection et le curseur restent opaques. Sous Linux, cela nécessite un
compositeur (intégré à la plupart des bureaux).
