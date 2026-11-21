# Notifications de bureau

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

Graphene ne gère ni les notifications demandées par les programmes (OSC 9, OSC 777) ni de notification de fin de commande. Les blocs de commande connaissent déjà le statut de chaque commande (pas encore sa durée), ce qui rend possible une notification « commande longue terminée ». Kitty et Warp proposent ce type de notification.

## Objectif

Prévenir l'utilisateur via le système de notifications du bureau quand un programme le demande ou quand une commande longue se termine alors que la fenêtre ou le pane n'a pas le focus.

## Décisions actées

### Sources

- **OSC 9** (message simple) et **OSC 777** (`notify;titre;corps`) émis par les programmes.
- **Fin de commande longue** : détectée via les blocs de commande, au-delà d'un seuil de
  durée configurable (10 s par défaut). Succès et échec notifient tous les deux. Le
  message indique la commande, le statut (avec le code de sortie en cas d'échec) et la
  durée.
- **BEL** : peut aussi produire une notification, en plus de la demande d'attention
  actuelle.

### Réglages

- Un interrupteur par source. Par défaut : OSC 9/777 et fin de commande activés, BEL
  désactivé (souvent bruyant).
- Le seuil de durée de la fin de commande est configurable.

### Quand notifier

- Uniquement si le pane d'origine n'est pas sous les yeux de l'utilisateur : fenêtre
  sans focus, ou onglet/pane qui n'est pas l'actif. Rien pour le pane qu'on regarde.

### Interaction

- Cliquer sur la notification ramène la fenêtre au premier plan et active l'onglet et
  le pane d'origine.

### Plateformes et rafales

- Linux et macOS sont couverts ensemble dès cette feature.
- Une seule notification affichée par pane : une nouvelle remplace la précédente,
  le compteur du badge continue d'augmenter.

### Badge

- En plus de la notification système, un badge marque le pane d'origine, l'onglet qui
  le contient et l'entrée de la barre des tâches (selon ce que le bureau permet).
- Le badge est un compteur de notifications non vues : le pane affiche les siennes,
  l'onglet le total de ses panes, la barre des tâches le total de la fenêtre (comme un
  compteur sur l'icône de l'application).
- Le badge suit les mêmes interrupteurs que les notifications : une source désactivée
  ne produit ni notification ni badge.
- Le badge du pane s'efface quand ce pane reçoit le focus ; ceux de l'onglet et de la
  barre des tâches s'effacent quand plus aucun pane concerné n'est badgé.

## Features liées

- [Indicateur de progression OSC 9;4](../osc-progress-indicator/overview.md) : partage
  le préfixe OSC 9. Une séquence OSC 9;4 est une progression, jamais une notification.
  Les deux features touchent aussi la barre des tâches ; leurs indicateurs doivent
  pouvoir coexister.
