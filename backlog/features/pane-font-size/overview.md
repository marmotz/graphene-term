# Per-pane font size

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

Aujourd'hui `Ctrl+=` / `Ctrl++`, `Ctrl+-` / `Ctrl+_` et `Ctrl+0` (actions
`FontSizeIncrease`, `FontSizeDecrease`, `FontSizeReset`) changent la taille de police de
toute l'application, tous panes et onglets confondus. Il n'existe pas de raccourci souris
pour la taille de police.

À ne pas confondre avec le "zoom" existant (`Ctrl+Shift+X`, `ToggleZoom`), qui agrandit
un pane à tout l'onglet.

## Objectif

- `Ctrl+molette` agrandit / réduit la taille de police du pane courant.
- `Ctrl+=` / `Ctrl+-` (et variantes) n'agissent plus que sur le pane courant, plus sur
  toute l'application.

## Décisions actées

- **Ctrl+molette** : agit sur le pane sous la souris, pas forcément le pane actif.
  Molette vers le haut agrandit, vers le bas réduit.
- **Mouse mode** : Ctrl+molette change toujours la police, même quand le programme
  capture la souris (vim, htop, tmux). L'événement n'est jamais transmis au programme.
- **Ctrl+= / Ctrl+- (et variantes)** : agissent sur le pane actif uniquement.
- **Ctrl+0** : remet le pane actif à la taille configurée. Les autres panes ne changent
  pas.
- **Plus de changement global par raccourci** : la taille de toute l'app se change
  uniquement dans les réglages. Aucune action "toute l'app" n'est ajoutée.
- **Réglages** : chaque pane garde son écart par rapport à la taille configurée. Si le
  réglage passe de 12 à 14, un pane à +2 passe à 16 ; un pane non ajusté suit le réglage.
- **Split** : le nouveau pane hérite de la taille du pane d'origine.
- **Nouvel onglet** : part de la taille configurée.
- **Persistance** : la taille de chaque pane est restaurée au lancement, avec la session
  (voir [session-restore](../session-restore/overview.md), déjà livrée).
