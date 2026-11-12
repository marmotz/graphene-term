# Performance

**Statut** : tâches créées, voir [technical.md](technical.md)

## Contexte

Le lancement de `btop` est perceptiblement plus lent dans Graphene (build release) que
dans Terminator, alors que Terminator n'est pas réputé pour sa rapidité :

- Terminator : btop apparaît immédiatement avec tout son contenu (liste des process,
  graphes).
- Graphene : btop apparaît d'abord avec son cadre seul, puis les informations
  arrivent ensuite.

Le ressenti de vitesse est un critère central pour un terminal, et Graphene est un
terminal GPU écrit en Rust : il ne devrait pas être battu par un terminal VTE classique.

## Objectif

Amener Graphene au niveau de vitesse des terminaux GPU de référence (Alacritty, kitty)
sur les usages courants, et pouvoir le mesurer.

## Décisions actées

- **Référence** : Alacritty et kitty. Terminator n'est qu'un plancher.
- **Périmètre** : les usages courants, avec btop comme cas de départ. Au minimum :
  lancement et affichage d'une app plein écran (btop, htop, vim), sortie massive
  (`cat` d'un gros fichier), défilement de l'historique.
- **Critère** : le temps jusqu'à l'affichage complet compte. Afficher l'app « d'un
  bloc », sans état intermédiaire visible, n'est pas une exigence en soi.
- **Mesure** : un banc de mesure reproductible dans le projet, lancé en local. Pas de
  contrôle automatique en CI.
