[English](../en/configuration.md)

# Configuration

Graphene fonctionne sans aucune configuration. Pour la modifier, utilisez la
[fenêtre des paramètres](#fenêtre-des-paramètres) (`Ctrl+,`) ou modifiez le fichier de
configuration à la main : les deux écrivent le même fichier, et chaque changement
s'applique aussitôt à la fenêtre ouverte.

- [Emplacement des fichiers](#emplacement-des-fichiers)
- [Le fichier de configuration](#le-fichier-de-configuration)
- [Rechargement à chaud et erreurs](#rechargement-à-chaud-et-erreurs)
- [Fenêtre des paramètres](#fenêtre-des-paramètres)
- [Retirer les paramètres inconnus](#retirer-les-paramètres-inconnus)
- [Liste des paramètres](#liste-des-paramètres)
- [Recherche de mises à jour](#recherche-de-mises-à-jour)
- [État enregistré](#état-enregistré)
- [Journaux](#journaux)

## Emplacement des fichiers

| Système | Fichier de configuration                                   | Fichier des raccourcis clavier                                |
|---------|------------------------------------------------------------|---------------------------------------------------------------|
| Linux   | `~/.config/graphene/config.toml`                           | `~/.config/graphene/keybindings.toml`                         |
| macOS   | `~/Library/Application Support/graphene/config.toml`       | `~/Library/Application Support/graphene/keybindings.toml`     |

Si `XDG_CONFIG_HOME` est défini, `$XDG_CONFIG_HOME/graphene/config.toml` et
`$XDG_CONFIG_HOME/graphene/keybindings.toml` sont utilisés à la place, sur les deux
systèmes. Vos propres [thèmes](themes.md#thèmes-personnalisés) vont dans un dossier
`themes` à côté de `config.toml`.

## Le fichier de configuration

Le fichier est au format [TOML](https://toml.io). Graphene le crée au premier
lancement, avec chaque paramètre en commentaire et à sa valeur par défaut, chacun
accompagné d'une courte explication (en anglais) :

```toml
# [font]
## Size in points, from 6 to 72.
# size = 14.0
```

Pour changer un paramètre, retirez le `#` devant lui et devant l'en-tête de sa section,
puis changez la valeur :

```toml
[font]
size = 12.0
```

Quand une nouvelle version de Graphene ajoute des paramètres, elle les ajoute en
commentaire à votre fichier existant au démarrage. Vos valeurs, vos commentaires et les
paramètres qu'elle ne connaît pas sont conservés tels quels.

## Rechargement à chaud et erreurs

Les modifications enregistrées de `config.toml`, de `keybindings.toml` et de vos
fichiers de thème s'appliquent aux fenêtres ouvertes sans redémarrer, à quelques
exceptions près :

- une nouvelle valeur de `font.size` annule aussi le zoom fait avec `Ctrl+=` / `Ctrl+-` ;
- `window.width`, `window.height` et `window.startup_mode` ne concernent que les
  fenêtres ouvertes ensuite.

Une erreur dans le fichier n'arrête pas Graphene. Un bandeau en haut de la fenêtre
décrit chaque problème (cliquez dessus pour le masquer) :

- un paramètre dont la valeur est invalide garde sa valeur par défaut, ou sa valeur
  précédente lors d'un rechargement ;
- un nombre hors limites est ramené dans les limites ;
- un paramètre inconnu est ignoré ;
- une erreur de syntaxe TOML fait ignorer tout le fichier jusqu'à sa correction.

## Fenêtre des paramètres

Ouvrez-la avec `Ctrl+,`, avec **Paramètres** dans la
[palette de commandes](tabs-and-panes.md#palette-de-commandes), ou depuis le menu du
clic droit. Elle présente les mêmes sections que le fichier (**Langue**, **Police**,
**Fenêtre**, **Historique**, **Curseur**, **Sélection**, **Onglets**, **Thème**,
**Raccourcis clavier**, **Notifications**, **Quick terminal**, **Mises à jour**), et
chaque changement est écrit aussitôt dans `config.toml`, ou dans `keybindings.toml` pour
les raccourcis clavier, en conservant les commentaires des fichiers, puis appliqué comme
une modification à la main.

- **Raccourcis clavier** : liste chaque action avec ses raccourcis. Ajoutez un raccourci
  en l'appuyant (`Échap` annule), ou supprimez-en un.
- Les problèmes du fichier s'affichent sous le paramètre concerné ; les paramètres
  inconnus sont listés en haut.
- Tant qu'un fichier contient une erreur de syntaxe, la fenêtre l'affiche et verrouille
  ce qui en dépend (les raccourcis clavier pour `keybindings.toml`, les autres sections
  pour `config.toml`) jusqu'à ce que vous corrigiez le fichier à la main.

## Retirer les paramètres inconnus

Les paramètres que Graphene ne connaît pas (une faute de frappe, ou un paramètre d'une
ancienne version) sont ignorés et conservés dans le fichier. Cela concerne `config.toml`
et `keybindings.toml`. Pour les lister et les retirer :

```bash
graphene --prune-config
```

La commande liste les paramètres inconnus des deux fichiers et demande confirmation. Hors d'un terminal
interactif, elle ne change rien sauf si vous ajoutez `--yes` :

```bash
graphene --prune-config --yes
```

Les paramètres connus sont toujours conservés, même avec une valeur invalide, et un
fichier contenant une erreur de syntaxe TOML n'est jamais modifié. Un
`keybindings.toml` absent est créé à partir de son modèle.

## Liste des paramètres

Voici la liste complète. Les valeurs indiquées sont les valeurs par défaut.

### Général

| Paramètre  | Défaut   | Valeurs                                                                |
|------------|----------|------------------------------------------------------------------------|
| `language` | `"auto"` | `"auto"`, `"en"`, `"fr"` : langue de l'interface (menus, palette, paramètres, bandeaux). `"auto"` suit la langue du système : français quand elle est française, anglais sinon. |

`language` se place en haut du fichier, avant toute section.

### `[font]`

| Paramètre | Défaut | Valeurs                                                                                    |
|-----------|--------|--------------------------------------------------------------------------------------------|
| `family`  | `""`   | Nom de la famille de police. Vide : police monospace du système. Sous Linux, `fc-list : family` liste les polices installées. |
| `size`    | `14.0` | Taille en points, de 6 à 72.                                                               |
| `ligatures` | `true` | Affiche les ligatures des polices qui en ont (Fira Code, JetBrains Mono...) : `->`, `===` et autres sont dessinés comme un seul symbole. |
| `features` | `""`  | Features OpenType du texte du terminal, séparées par des espaces ou des virgules : `"ss01"` ou `"+ss01"` active une feature, `"-calt"` la désactive, `"cv05=2"` fixe une valeur. Un tag fait 4 caractères ; si un tag est répété, le dernier l'emporte. Exemple : `"zero ss01"`. |

### `[window]`

| Paramètre            | Défaut      | Valeurs                                                                                    |
|----------------------|-------------|--------------------------------------------------------------------------------------------|
| `startup_mode`       | `"default"` | `"default"` : ouvrir à `width` × `height`. `"maximized"` : ouvrir la fenêtre maximisée. `"remember"` : ouvrir à la taille et dans l'état maximisé ou non de la dernière fenêtre fermée. |
| `width`              | `960`       | Largeur d'une nouvelle fenêtre, en pixels logiques (supérieure à zéro).                    |
| `height`             | `600`       | Hauteur d'une nouvelle fenêtre, en pixels logiques (supérieure à zéro).                    |
| `padding`            | `8`         | Espace minimal autour du texte, en pixels logiques (zéro ou plus).                         |
| `background_opacity` | `1.0`       | De `0.0` (transparent) à `1.0` (opaque). Voir [Thèmes](themes.md#fond-translucide).        |
| `progress`           | `true`      | Affiche la progression signalée par les programmes (OSC 9;4) dans les onglets, les barres de pane, les blocs et la barre des tâches. `false` la masque partout. |

### `[scrollback]`

| Paramètre | Défaut  | Valeurs                                                 |
|-----------|---------|---------------------------------------------------------|
| `lines`   | `10000` | Nombre de lignes d'historique conservées, jusqu'à 100000. |

### `[cursor]`

| Paramètre  | Défaut    | Valeurs                                                                                 |
|------------|-----------|-----------------------------------------------------------------------------------------|
| `shape`    | `"block"` | `"block"` (bloc), `"beam"` (barre verticale) ou `"underline"` (souligné).               |
| `blinking` | `false`   | `true` ou `false`. Les programmes peuvent toujours changer eux-mêmes la forme et le clignotement du curseur. |

### `[selection]`

| Paramètre         | Défaut          | Valeurs                                                     |
|-------------------|-----------------|-------------------------------------------------------------|
| `word_separators` | (voir ci-dessous) | Caractères qui terminent un mot lors d'un double clic.    |

La valeur par défaut est :

```toml
word_separators = ",│`|\"' ()[]{}<>\t"
```

`:` n'est pas dans la liste : une URL ou un `hôte:port` est donc sélectionné en entier
par un double clic.

### `[tabs]`

| Paramètre  | Défaut  | Valeurs                                                                    |
|------------|---------|----------------------------------------------------------------------------|
| `position` | `"top"` | Bord de la barre d'onglets : `"top"` (haut), `"bottom"` (bas), `"left"` (gauche) ou `"right"` (droite). Voir [Onglets et panneaux](tabs-and-panes.md#onglets). |

### `[theme]`

| Paramètre | Défaut             | Valeurs                                                                       |
|-----------|--------------------|-------------------------------------------------------------------------------|
| `name`    | `"graphene-night"` | Nom du thème : un thème intégré ou l'un de vos fichiers de thème.             |
| `light`   | non défini         | Thème utilisé quand le système est en mode clair. Nécessite aussi `dark`.     |
| `dark`    | non défini         | Thème utilisé quand le système est en mode sombre. Nécessite aussi `light`.   |

Si `light` et `dark` sont tous deux définis, Graphene suit l'apparence du système et
`name` n'est pas utilisé. Voir [Thèmes](themes.md).

### Raccourcis clavier

Les raccourcis ne se règlent pas dans `config.toml` : ils ont leur propre fichier,
`keybindings.toml` (une section `[keybindings]`, même format). La syntaxe, les noms
d'actions et les raccourcis par défaut sont sur la page
[Raccourcis clavier](keybindings.md#modifier-les-raccourcis). Une section
`[keybindings]` trouvée dans `config.toml` est déplacée vers `keybindings.toml` au
démarrage.

### `[updates]`

| Paramètre  | Défaut   | Valeurs                                                                                         |
|------------|----------|-------------------------------------------------------------------------------------------------|
| `check`    | `true`   | `true` ou `false`. Rechercher une version plus récente sur GitHub ; `false` ne fait plus aucun appel réseau. |
| `interval` | `"auto"` | `"auto"`, `"6h"`, `"12h"`, `"24h"`, `"2d"`, `"3d"` ou `"1w"`. `"auto"` vaut 24 heures pour une version publiée, 6 heures pour une version de développement. |

### `[notifications]`

| Paramètre           | Défaut  | Valeurs                                                                                    |
|---------------------|---------|--------------------------------------------------------------------------------------------|
| `program`           | `true`  | `true` ou `false`. Afficher les notifications demandées par les programmes (OSC 9, OSC 777). |
| `command_finished`  | `true`  | `true` ou `false`. Signaler la fin d'une commande qui a duré au moins `command_threshold` secondes. |
| `command_threshold` | `10`    | Nombre entier de secondes, au moins 1. Une valeur inférieure à 1 est refusée et la précédente conservée. |
| `bell`              | `false` | `true` ou `false`. Transformer la sonnerie du terminal en notification.                    |

Un changement s'applique à la notification suivante.

Sous Linux, les notifications passent par le serveur de notifications du bureau, via
D-Bus : chaque panneau en affiche au plus une, un clic dessus ramène le panneau, et le
nombre de notifications non vues de la fenêtre s'affiche sur l'icône de la barre des
tâches avec les docks qui le permettent (KDE Plasma, le dock d'Ubuntu, Dash to Dock).
Sans serveur de notifications, seuls les compteurs des onglets et des barres de panneau
s'affichent. Sous Wayland, ramener la fenêtre au premier plan après un clic dépend du
compositeur.

Sous macOS, les notifications passent par le centre de notifications, qui demande
l'autorisation à la première : chaque panneau en affiche au plus une, un clic dessus
ramène le panneau, et le nombre de notifications non vues de la fenêtre s'affiche sur
l'icône du Dock. Les notifications demandent de lancer Graphene depuis `Graphene.app` ;
lancé autrement, seuls le badge du Dock et les compteurs des onglets et des barres de
panneau s'affichent.

### `[quick_terminal]`

Un terminal qui glisse depuis un bord de l'écran et reste résident entre deux
utilisations. Disponible sous macOS et sous Linux avec X11 ; sous Wayland, il ne démarre
pas et sa section des paramètres est désactivée.

| Paramètre            | Défaut     | Valeurs                                                                                    |
|----------------------|------------|--------------------------------------------------------------------------------------------|
| `hotkey`             | `""`       | Raccourci global natif qui affiche ou masque le quick terminal, écrit comme un [raccourci clavier](#raccourcis-clavier) (`"Ctrl+F12"`). Vide : aucun. |
| `edge`               | `"top"`    | Bord de l'écran d'où il glisse : `"top"`, `"bottom"`, `"left"` ou `"right"`.               |
| `size`               | `40`       | Pourcentage de l'écran, de 10 à 100 : la hauteur pour `top` et `bottom`, la largeur pour `left` et `right`. L'autre dimension prend tout l'écran. |
| `screen`             | `"active"` | `"active"` (l'écran sous le curseur de la souris), `"primary"`, ou le nom d'un écran. Un nom qui ne correspond à aucun écran revient à `"active"`. |
| `animation_ms`       | `150`      | Durée du glissement en millisecondes, `0` pour aucun, au plus 1000.                        |
| `hide_on_focus_loss` | `true`     | `true` ou `false`. Masquer le quick terminal quand il perd le focus.                       |

`edge`, `size`, `screen` et `animation_ms` s'appliquent au prochain affichage du quick
terminal.

## Recherche de mises à jour

Tant que `updates.check` vaut `true`, Graphene demande en arrière-plan à GitHub si une
version plus récente existe, au plus une fois par `updates.interval`. C'est la seule
requête réseau que fait Graphene. Quand une version plus récente existe, un bandeau en
haut de la fenêtre propose :

- **Voir la version** : ouvre la page de la version dans votre navigateur ;
- **Ignorer cette version** : masque le bandeau définitivement, jusqu'à une version
  encore plus récente ;
- **×** : le masque jusqu'au prochain lancement.

## État enregistré

Graphene conserve quelques fichiers d'un lancement à l'autre : la taille de la dernière
fenêtre (pour `startup_mode = "remember"`), le résultat de la dernière recherche de mise
à jour, et les actions récemment lancées depuis la palette de commandes.

| Système | Dossier d'état                              |
|---------|---------------------------------------------|
| Linux   | `~/.local/state/graphene/`                  |
| macOS   | `~/Library/Application Support/graphene/`   |

Si `XDG_STATE_HOME` est défini, `$XDG_STATE_HOME/graphene/` est utilisé à la place.
Supprimer ce dossier est sans danger.

## Journaux

Graphene écrit les avertissements et les erreurs sur sa sortie d'erreur standard. La
variable d'environnement `RUST_LOG` règle le niveau (par défaut `warn`), par exemple :

```bash
RUST_LOG=info graphene
```

Lancez Graphene depuis un autre terminal pour les voir.
