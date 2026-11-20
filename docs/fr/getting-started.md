[English](../en/getting-started.md)

# Premiers pas

## Premier lancement

Lancez **Graphene Term** depuis le menu des applications, ou tapez `graphene` dans un
autre terminal.

Une fenêtre s'ouvre avec votre shell habituel (celui de `$SHELL`). Au premier
lancement, Graphene écrit aussi son fichier de configuration, avec tous les paramètres
en commentaire (voir [Configuration](configuration.md)) ; inutile de le modifier pour
commencer.

Si votre shell est zsh, bash ou fish, Graphene met en place son
[intégration shell](command-blocks.md#intégration-shell) tout seul : vos fichiers de
configuration du shell ne sont pas modifiés.

## Cinq choses à essayer

1. **Les blocs de commande.** Lancez quelques commandes, par exemple `ls` puis
   `false`. Chaque commande et sa sortie forment un bloc avec une barre colorée à
   gauche : verte en cas de succès, rouge en cas d'échec, jaune pendant l'exécution.
   Cliquez sur la barre pour sélectionner le bloc, puis appuyez sur `Ctrl+Shift+Alt+O`
   pour copier sa sortie, ou sur `Ctrl+Shift+F` pour le replier.
   Voir [Blocs de commande](command-blocks.md).
2. **La palette de commandes.** Appuyez sur `Ctrl+Shift+P`, tapez quelques lettres
   (`diviser`, `onglet`, `police`...) puis `Entrée`. Toutes les actions y sont, avec
   leur raccourci. Voir [Onglets et panneaux](tabs-and-panes.md#palette-de-commandes).
3. **Diviser la fenêtre.** Appuyez sur `Ctrl+Shift+E` pour diviser à droite,
   `Ctrl+Shift+O` pour diviser en bas. Cliquez dans un panneau ou utilisez
   `Ctrl+Shift+Flèche` pour passer de l'un à l'autre, et faites glisser la barre de
   titre d'un panneau pour le déplacer. Voir [Onglets et panneaux](tabs-and-panes.md).
4. **La fenêtre des paramètres.** Appuyez sur `Ctrl+,` pour changer la police, le
   curseur, les raccourcis clavier et plus encore. Les changements s'appliquent
   aussitôt. Voir [Configuration](configuration.md#fenêtre-des-paramètres).
5. **Un thème.** Faites un clic droit dans la fenêtre, ouvrez **Thèmes** et choisissez-en
   un ; ou réglez-le dans la fenêtre des paramètres. Voir [Thèmes](themes.md).

À l'invite du shell, vous pouvez aussi cliquer dans votre ligne de commande pour y
déplacer le curseur, et sélectionner du texte avec `Shift+Flèche` comme dans un
éditeur : voir [Éditeur de saisie](input-editor.md).

## Options de la ligne de commande

| Commande                           | Effet                                                                |
|------------------------------------|----------------------------------------------------------------------|
| `graphene`                         | Ouvre une fenêtre avec votre shell                                   |
| `graphene -e <programme> [args...]` | Ouvre une fenêtre dont le premier panneau lance `<programme>` au lieu du shell |
| `graphene --columns <n> --lines <n>` | Ouvre la fenêtre à la taille d'une grille de `<n>` colonnes et `<n>` lignes |
| `graphene --version`               | Affiche la version et quitte                                         |
| `graphene --prune-config [--yes]`  | Retire du fichier de configuration les paramètres que cette version ne connaît pas (voir [Configuration](configuration.md#retirer-les-paramètres-inconnus)) |

### Lancer un programme avec `-e`

`-e` prend tout ce qui le suit comme programme et arguments :

```bash
graphene -e htop
graphene -e ssh my-server
graphene -e vim --clean notes.txt
```

- Le programme est lancé directement, sans shell : il n'a donc pas de blocs de
  commande.
- Quand il se termine, son panneau se ferme ; si c'était le dernier panneau, Graphene
  quitte.
- Les panneaux et onglets ouverts ensuite lancent votre shell comme d'habitude.
- Les options placées après `-e` appartiennent au programme : `graphene -e less --version`
  lance `less --version`. `graphene -e` sans rien derrière est une erreur.

### Taille de la fenêtre en cellules

`--columns` et `--lines` dimensionnent la première fenêtre pour qu'elle contienne cette
grille, quels que soient la police et les réglages `window` :
`graphene --columns 120 --lines 40 -e htop`.

- Chacune peut être donnée seule ; l'autre dimension garde la taille configurée.
- La fenêtre s'ouvre sans être maximisée, et sa taille n'est pas retenue pour le
  prochain lancement (`window.startup_mode = "remember"`).
- Le gestionnaire de fenêtres peut tout de même refuser la taille, un gestionnaire en
  mosaïque par exemple.

## Pour aller plus loin

- [Raccourcis clavier](keybindings.md) : la liste complète, et comment les modifier.
- [Configuration](configuration.md) : tous les paramètres.
- [Sélection et presse-papiers](selection-and-clipboard.md) et
  [Images et liens](images-and-links.md).
