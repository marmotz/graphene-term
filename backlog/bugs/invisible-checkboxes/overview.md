# Invisible checkboxes

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

Les cases à cocher ne se voient pas. Constaté dans les paramètres, section "Thème" :
la case de l'option "Suivre l'apparence du système" est quasi invisible sur le fond
sombre.

- Les cases à cocher n'existent que dans l'écran des paramètres.
- Seule la case décochée pose problème : une fois cochée, elle est visible.
- Le défaut est marqué avec les thèmes sombres, et le contraste reste faible avec les
  thèmes clairs.

## Objectif

Une case à cocher décochée doit se distinguer clairement du fond, quel que soit le
thème (clair ou sombre).

## Décisions actées

- Le correctif s'applique à toutes les cases à cocher des paramètres, y compris celles
  qu'ajoute l'éditeur de thèmes ([custom-themes](../../features/custom-themes/overview.md)).
- La case reprend des couleurs existantes du thème actif. Aucune nouvelle couleur de
  thème n'est créée pour elle.
- Tous les contrôles des paramètres au repos (cases, boutons, listes, champs) reçoivent
  un contour visible, couleur du texte atténuée vers le fond.
- Le cas de la case cochée ne change pas.
