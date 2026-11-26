[English](../en/keybindings.md)

# Raccourcis clavier

Chaque action ci-dessous a un raccourci par défaut, et chaque raccourci peut être
modifié. La [palette de commandes](tabs-and-panes.md#palette-de-commandes)
(`Ctrl+Shift+P`) liste toutes les actions avec leur raccourci actuel. Les raccourcis
sont les mêmes sous Linux et macOS (avec `Ctrl`, pas `Cmd`).

- [Raccourcis par défaut](#raccourcis-par-défaut)
- [Modifier les raccourcis](#modifier-les-raccourcis)
- [Noms des actions](#noms-des-actions)

Les raccourcis qui ne servent qu'à modifier la ligne de commande sont sur la page
[Éditeur de saisie](input-editor.md) ; les gestes à la souris sont sur la page de
chaque fonctionnalité.

Les touches sont écrites comme dans le fichier de configuration : `Shift` pour Maj,
`PageUp` / `PageDown` pour Page préc. / Page suiv., `Home` / `End` pour Début / Fin,
`Insert` pour Inser, `Escape` pour Échap, `Enter` pour Entrée.

## Raccourcis par défaut

### Presse-papiers

| Touches                               | Action                                     |
|---------------------------------------|--------------------------------------------|
| `Ctrl+Shift+C`, touche `Copy`         | Copier la sélection dans le presse-papiers |
| `Ctrl+Shift+V`, `Shift+Insert`, touche `Paste` | Coller                            |

### Défilement et taille de police

| Touches                             | Action                                                     |
|-------------------------------------|------------------------------------------------------------|
| `Shift+PageUp` / `Shift+PageDown`   | Remonter / descendre d'une page (le bloc sélectionné, ou l'historique) |
| `Shift+Home` / `Shift+End`          | Haut / bas de l'historique                                 |
| `Ctrl+=` ou `Ctrl++`                | Agrandir la police du pane actif                           |
| `Ctrl+-` ou `Ctrl+_`                | Réduire la police du pane actif                            |
| `Ctrl+0`                            | Remettre le pane actif à la taille de police configurée    |

Chaque pane a sa propre taille de police. Un split démarre à la taille du pane dont il
vient, un nouvel onglet à la taille configurée.

À l'invite du shell, `Shift+Home` et `Shift+End` sélectionnent plutôt dans la ligne de
commande (voir [Éditeur de saisie](input-editor.md)).

### Blocs de commande

| Touches                                     | Action                                                  |
|---------------------------------------------|---------------------------------------------------------|
| `Ctrl+Shift+PageUp` / `Ctrl+Shift+PageDown` | Sélectionner le bloc de commande précédent / suivant    |
| `Ctrl+Shift+Alt+C`                          | Copier la commande du bloc sélectionné (ou du dernier)  |
| `Ctrl+Shift+Alt+O`                          | Copier la sortie du bloc sélectionné (ou du dernier)    |
| `Ctrl+Shift+F`                              | Replier / déplier le bloc sélectionné (ou le dernier)   |
| `Ctrl+Shift+Alt+F`                          | Replier tous les blocs terminés, ou tous les déplier    |

Voir [Blocs de commande](command-blocks.md).

### Panneaux

| Touches                                      | Action                                     |
|----------------------------------------------|--------------------------------------------|
| `Ctrl+Shift+E`                               | Diviser le panneau, nouveau panneau à droite |
| `Ctrl+Shift+O`                               | Diviser le panneau, nouveau panneau en bas |
| `Ctrl+Shift+W`                               | Fermer le panneau                          |
| `Ctrl+Shift+Left` / `Right` / `Up` / `Down`  | Aller au panneau dans cette direction      |
| `Ctrl+Shift+Alt+Left` / `Right`              | Redimensionner le panneau vers la gauche / la droite |
| `Ctrl+Shift+Alt+Up` / `Down`, ou `Ctrl+Shift+Alt+PageUp` / `PageDown` | Redimensionner le panneau vers le haut / le bas |
| `Ctrl+Tab` / `Ctrl+Shift+Tab`              | Panneau suivant / précédent                |
| `Ctrl+Shift+X`                               | Agrandir le panneau à tout l'onglet, ou le réduire |
| `Ctrl+Shift+B`                               | Diffuser la saisie à tous les panneaux de l'onglet, ou arrêter |

Certains bureaux (Cinnamon, par exemple) se réservent `Ctrl+Shift+Alt+Up` / `Down` :
les variantes `PageUp` / `PageDown` font la même chose.

Voir [Onglets et panneaux](tabs-and-panes.md).

### Onglets

| Touches                                 | Action                  |
|-----------------------------------------|-------------------------|
| `Ctrl+Shift+T`                          | Nouvel onglet           |
| `Ctrl+Alt+W`                            | Fermer l'onglet         |
| `Ctrl+PageDown`                         | Onglet suivant          |
| `Ctrl+PageUp`                         | Onglet précédent        |
| `Alt+1` ... `Alt+9`                     | Aller à l'onglet 1 à 9  |

`Alt+1` à `Alt+9` utilisent la rangée des chiffres du clavier quelle que soit la
disposition (en AZERTY aussi, sans `Shift`).

### Application

| Touches         | Action                     |
|-----------------|----------------------------|
| `Ctrl+Shift+P`  | Palette de commandes       |
| `Ctrl+,`        | Onglet des paramètres      |
| `Ctrl+Shift+Q`  | Quitter Graphene           |

## Modifier les raccourcis

Utilisez la section **Raccourcis clavier** de l'onglet des paramètres (`Ctrl+,`) :
appuyez sur un nouveau raccourci pour l'ajouter, ou supprimez-en un. Tapez dans le
champ au-dessus de la liste pour ne garder que les actions dont le nom contient le
texte, ou cliquez sur **Rechercher un raccourci** puis appuyez sur un raccourci pour voir
les actions qui l'utilisent. Ou modifiez
`keybindings.toml`, situé à côté du
[fichier de configuration](configuration.md#emplacement-des-fichiers) :

```toml
[keybindings]
"Ctrl+Alt+T" = "new_tab"        # adds a shortcut; Ctrl+Shift+T still works
"Ctrl+Shift+W" = "none"         # removes a default shortcut
"F11" = "toggle_zoom"
```

Dans cet exemple, la première ligne ajoute un raccourci (`Ctrl+Shift+T` fonctionne
toujours), la deuxième supprime un raccourci par défaut.

Le fichier a le même format que `config.toml`, avec un en-tête `[keybindings]`.
Graphene le crée au premier lancement, avec les valeurs par défaut documentées en
commentaires. Les modifications enregistrées s'appliquent aux fenêtres ouvertes sans
redémarrer. Les raccourcis qui se trouvaient dans la section `[keybindings]` de
`config.toml` sont déplacés automatiquement vers `keybindings.toml` au démarrage ; les
entrées déjà présentes dans `keybindings.toml` l'emportent sur celles déplacées.

- Chaque ligne associe **un raccourci** à **une action**. Vos lignes s'ajoutent aux
  raccourcis par défaut : un raccourci par défaut reste actif sauf si vous l'associez à
  autre chose ou à `"none"`.
- `"none"` supprime un raccourci : la touche part vers le programme qui tourne dans le
  terminal, comme si Graphene n'avait aucun raccourci pour elle. Servez-vous-en pour
  rendre une touche à votre shell ou à une application.
- Une action peut avoir plusieurs raccourcis.
- Un raccourci ou un nom d'action invalide est signalé dans le bandeau d'erreur et
  ignoré.

### Écrire un raccourci

Un raccourci est composé de zéro, un ou plusieurs modificateurs reliés par `+`, puis
d'une touche :

- **Modificateurs** : `Ctrl`, `Shift`, `Alt`, `Super`.
- **Touche** : un caractère (`T`, `5`, `,`, `+`), ou un nom de touche :
  `Left`, `Right`, `Up`, `Down`, `PageUp`, `PageDown`, `Home`, `End`, `Insert`,
  `Delete`, `Backspace`, `Tab`, `Enter`, `Escape`, `Space`, `F1` à `F24`, `Copy`,
  `Paste`, `Cut`, `ContextMenu`, `PrintScreen`, `Pause`, `ScrollLock`.

Règles :

- Les noms ne tiennent pas compte de la casse, et les lettres correspondent aux deux
  casses : `"Ctrl+Shift+t"` équivaut à `Ctrl+Shift+T`.
- Pour un caractère autre qu'une lettre, `Shift` est ignoré, car votre disposition de
  clavier l'utilise déjà pour taper ce caractère : écrivez `"Ctrl++"`, pas
  `"Ctrl+Shift+="`.
- `Alt` + un chiffre correspond à la rangée des chiffres quelle que soit la disposition
  du clavier.
- Avec une disposition sans lettres latines (cyrillique, grec...), un raccourci avec
  une lettre correspond à la touche où se trouve cette lettre sur un clavier américain.

## Noms des actions

| Action                                   | Effet                                             |
|------------------------------------------|---------------------------------------------------|
| `copy`                                   | Copier la sélection dans le presse-papiers        |
| `paste`                                  | Coller depuis le presse-papiers                   |
| `scroll_page_up`, `scroll_page_down`     | Défiler d'une page                                |
| `scroll_to_top`, `scroll_to_bottom`      | Haut / bas de l'historique                        |
| `font_size_increase`, `font_size_decrease`, `font_size_reset` | Taille de police             |
| `previous_block`, `next_block`           | Sélectionner le bloc de commande précédent / suivant |
| `copy_command`, `copy_output`            | Copier la commande / la sortie d'un bloc          |
| `toggle_fold`, `toggle_fold_all`         | Replier un bloc / tous les blocs                  |
| `split_right`, `split_down`              | Diviser le panneau                                |
| `close_pane`                             | Fermer le panneau                                 |
| `focus_pane_left`, `focus_pane_right`, `focus_pane_up`, `focus_pane_down` | Aller à un panneau voisin |
| `resize_pane_left`, `resize_pane_right`, `resize_pane_up`, `resize_pane_down` | Redimensionner le panneau |
| `next_pane`, `previous_pane`             | Parcourir les panneaux de l'onglet                |
| `toggle_zoom`                            | Agrandir le panneau, ou le réduire                |
| `equalise_panes`                         | Donner la même taille à tous les panneaux de l'onglet (pas de raccourci par défaut) |
| `detach_pane`                            | Déplacer le panneau dans un nouvel onglet (pas de raccourci par défaut) |
| `new_tab`, `close_tab`                   | Ouvrir / fermer un onglet                         |
| `rename_tab`                             | Renommer l'onglet (pas de raccourci par défaut)   |
| `next_tab`, `previous_tab`               | Parcourir les onglets                             |
| `go_to_tab_1` ... `go_to_tab_255`        | Aller à un onglet par son numéro                  |
| `command_palette`                        | Ouvrir la palette de commandes                    |
| `open_settings`                          | Afficher l'onglet des paramètres                  |
| `quit`                                   | Quitter Graphene                                  |
| `none`                                   | Supprimer le raccourci                            |
