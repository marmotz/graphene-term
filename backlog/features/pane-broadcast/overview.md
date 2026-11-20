# Broadcast de la saisie sur plusieurs panes

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

Quand on travaille sur plusieurs machines ou sessions en parallèle (un pane par serveur,
par exemple), il faut aujourd'hui retaper la même commande dans chaque pane. Un mode
broadcast permet d'envoyer la même saisie à plusieurs panes en une fois.

## Objectif

Pouvoir taper une seule fois et que la saisie soit envoyée simultanément à un ensemble
de panes choisis de l'onglet courant.

## Décisions actées

### Périmètre

- Le broadcast est limité à un onglet : chaque onglet a son propre ensemble de
  diffusion, et aucun pane d'un autre onglet ne peut en faire partie.
- L'ensemble de diffusion d'un onglet est conservé quand on passe à un autre onglet et
  qu'on y revient.
- Un pane déplacé vers un autre onglet (glisser ou "Move pane to a new tab") sort de
  l'ensemble de son onglet d'origine et arrive sans diffusion dans le nouvel onglet.
- Un pane fermé sort de l'ensemble.

### Sélection des panes

- Ctrl+clic n'importe où sur un pane (barre de titre ou contenu) l'ajoute à l'ensemble,
  ou l'en retire s'il en fait déjà partie. Ctrl+glisser dans le contenu reste la
  sélection rectangulaire.
- Sur un lien, Ctrl+clic ouvre toujours le lien.
- Dans un pane où une application capture la souris (vim, tmux, htop...), Ctrl+clic
  appartient à Graphene et bascule le pane ; l'application ne le reçoit plus.
- Ctrl+clic ne change pas le pane actif.
- Le raccourci `Ctrl+Shift+B` équivaut à un Ctrl+clic sur tous les panes de l'onglet :
  sans diffusion active, il ajoute tous les panes de l'onglet ; avec une diffusion
  active, il vide l'ensemble. Il n'y a pas de "mode tous" : un pane créé ensuite n'est
  jamais ajouté automatiquement.
- Comme toute action, le raccourci est remappable dans `[keybindings]` et disponible
  dans la palette de commandes.

### Ce qui est diffusé

- Les touches tapées et les collages (`Ctrl+Shift+V`, `Shift+Insert`, clic milieu)
  sont envoyés à tous les panes de l'ensemble.
- Les raccourcis propres à Graphene (onglets, panes, copie, palette...) ne sont pas
  diffusés.
- La diffusion ne s'applique que si le pane actif fait partie de l'ensemble. Taper dans
  un pane hors de l'ensemble n'envoie la saisie qu'à ce pane.

### Indicateur visuel

- La barre de titre d'un pane de l'ensemble prend une couleur dédiée et affiche une
  icône de diffusion suivie du nombre de panes de l'ensemble, distincte de la couleur
  d'accent du pane actif.
- Quand l'onglet est zoomé sur un pane de l'ensemble, la diffusion continue vers les
  panes cachés ; la barre du pane zoomé montre l'icône et le nombre de panes concernés.

### Raccourcis par défaut modifiés

Pour libérer `Ctrl+Shift+B`, plusieurs raccourcis par défaut changent :

| Action              | Avant                       | Après                       |
|---------------------|-----------------------------|-----------------------------|
| Pane suivant        | `Ctrl+Shift+N`              | `Ctrl+Tab`                  |
| Pane précédent      | `Ctrl+Shift+B`              | `Ctrl+Shift+Tab`            |
| Onglet suivant      | `Ctrl+Tab`, `Ctrl+PageDown` | `Ctrl+PageDown`             |
| Onglet précédent    | `Ctrl+Shift+Tab`, `Ctrl+PageUp` | `Ctrl+PageUp`           |
| Zoom du pane        | `Ctrl+Shift+Z`              | `Ctrl+Shift+X`              |
| Broadcast (nouveau) | -                           | `Ctrl+Shift+B`              |

`Ctrl+Shift+N` et `Ctrl+Shift+Z` redeviennent libres.
