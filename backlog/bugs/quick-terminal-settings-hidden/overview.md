# Les réglages s'ouvrent sous le quick terminal

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

Quand on ouvre les réglages en mode quick terminal, ils apparaissent sous le quick
terminal et l'utilisateur ne peut pas y accéder. Constaté sous Linux X11, où le quick
terminal est maintenu au-dessus des autres fenêtres.

## Objectif

Les réglages ouverts en mode quick terminal s'affichent devant lui et sont accessibles.

## Décisions actées

- **Affichage** : le quick terminal reste affiché ; la fenêtre des réglages s'affiche
  devant lui et s'utilise normalement.
- **Fermeture des réglages** : si le quick terminal est toujours affiché, il reprend le
  focus clavier.
- **Plateformes** : le correctif cible Linux X11, où le bug a été constaté. macOS est
  vérifié pendant le correctif et corrigé de la même façon s'il est touché.

## Voir aussi

- [Quick terminal](../../features/quick-terminal/overview.md)
