# End-to-end tests

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

The project has unit tests, but, to the best of our knowledge, no end-to-end tests.
Without them, changes can break application flows or user actions without being caught.

## Objectif

Provide end-to-end tests for every feature and user action, so changes can be checked
against the behavior of the application as a whole.

## Décisions actées

- Viser la couverture de toutes les fonctionnalités et actions de l’application, pas
  seulement des parcours principaux ou des nouvelles fonctionnalités.
- Couvrir l’ensemble de l’interface utilisateur.
- Garantir cette couverture sur X11 et Wayland sous Linux, les environnements
  graphiques actuellement pris en charge.
- Inclure les intégrations shell bash, zsh et fish.
- Les échecs des tests E2E doivent bloquer la validation d’un changement ou d’une
  livraison, dès le premier scénario E2E; la couverture s’étendra ensuite jusqu’à
  l’inventaire complet.
