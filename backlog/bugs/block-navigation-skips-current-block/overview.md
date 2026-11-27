# La navigation entre blocs ne peut pas sélectionner le bloc en cours

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

`Ctrl+Shift+PageDown` (sélectionner le bloc de commande suivant) ne permet pas de
sélectionner le dernier bloc, celui de la commande en cours. Une fois qu'on a navigué
vers des blocs plus anciens avec `Ctrl+Shift+PageUp`, on ne peut donc plus revenir au
bloc en cours par la navigation entre blocs.

## Objectif

`Ctrl+Shift+PageDown` doit pouvoir atteindre le bloc en cours, pour qu'on puisse
toujours revenir au bloc actif après avoir parcouru les blocs précédents.

## Décisions actées

- Le « bloc en cours » est le dernier bloc du pane, dans les deux cas : une commande en
  cours d'exécution (barre jaune) ou l'invite en attente de saisie.
- `Ctrl+Shift+PageDown` sélectionne ce bloc comme n'importe quel autre, et la vue défile
  pour l'amener à l'écran, comme pour les autres blocs.
- Quand le bloc en cours est déjà sélectionné, `Ctrl+Shift+PageDown` ne fait rien : la
  sélection reste dessus.
- Sans bloc sélectionné, `Ctrl+Shift+PageDown` sélectionne le bloc en cours et
  `Ctrl+Shift+PageUp` le dernier bloc terminé.
- Un clic sur la barre d'état du bloc en cours le sélectionne aussi.
- Sur le bloc en cours sélectionné, les raccourcis agissent selon son contenu : le repli
  marche, la copie de la commande marche pendant son exécution, la copie de la sortie ne
  fait rien tant que la commande n'est pas terminée. Sur l'invite vide, seul le repli
  réagit.
