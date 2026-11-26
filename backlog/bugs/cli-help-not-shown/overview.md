# `graphene --help` n'affiche pas l'aide du CLI

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

`graphene --help` n'est pas reconnu : la commande ouvre une fenêtre de terminal normale
au lieu d'afficher l'aide. Plus généralement, toute option inconnue (par exemple
`graphene --foo`) est ignorée sans message et ouvre aussi une fenêtre, si bien qu'une
faute de frappe passe inaperçue.

## Objectif

`graphene --help` affiche l'aide du CLI et rend la main sans ouvrir de fenêtre. Une
option inconnue est signalée au lieu d'être ignorée.

## Décisions actées

- `--help` et `-h` affichent l'aide puis quittent avec succès, sans ouvrir de fenêtre.
- L'aide liste toutes les options du CLI, y compris celles lancées d'habitude par
  Graphene lui-même (`--quick`, `--hidden`, `--restore-session`) : elles sont
  utilisables publiquement.
- Une option inconnue, ou un argument qui n'est pas une option (`graphene htop`), affiche une erreur sur stderr qui renvoie à `--help`, quitte avec
  le code 2 et n'ouvre aucune fenêtre.
- Les arguments placés après `-e` appartiennent au programme lancé : `--help` ou une
  option inconnue à cet endroit ne sont pas interprétés par Graphene.
