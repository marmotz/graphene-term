# Restauration de session

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

À la fermeture, Graphene ne retient que la taille de la fenêtre. Les onglets, la disposition des panes et les répertoires de travail sont perdus. WezTerm, Warp et iTerm2 savent restaurer une session.

## Objectif

Retrouver au redémarrage les onglets, les panes et leurs répertoires de travail tels qu'ils étaient à la fermeture.

## Décisions actées

- **Activation** : réglage dans les paramètres, désactivé par défaut. Sans activation, le comportement actuel est inchangé.
- **Contenu restauré** : onglets, disposition des panes, répertoire de travail de chaque pane, titres d'onglets renommés à la main. Chaque pane démarre un shell neuf, écran vide : ni scrollback, ni blocs de commande, ni relance du programme qui tournait.
- **Focus** : l'onglet actif, le pane qui a le focus et le pane zoomé sont restaurés.
- **Renommer un onglet** : inclus dans cette feature (il n'existe pas encore). Depuis le menu contextuel de l'onglet, la palette de commandes ou un double-clic sur l'onglet ; saisie dans une petite boîte au centre de la fenêtre, préremplie. Un nom vide rend le titre automatique.
- **Fenêtres** : toutes les fenêtres ouvertes au moment de quitter sont restaurées, chacune avec ses onglets et sa taille. Leur position et leur écran sont aussi restaurés là où l'OS le permet (macOS, X11) ; sous Wayland, seule la taille l'est. Une fenêtre fermée alors que d'autres restent ouvertes sort de la session ; des fenêtres fermées à quelques secondes d'écart (déconnexion, arrêt de la machine) sont toutes gardées.
- **Pas de restauration** quand une autre fenêtre Graphene est déjà ouverte, ou au lancement avec `-e`, `--columns` ou `--lines` (et une telle fenêtre n'entre pas dans la session).
- **Démarrer sans restaurer** : une option de ligne de commande ignore la session une fois, sans changer le réglage ni effacer la session sauvegardée.
- **Lancement par le quick terminal** : si la commande du quick terminal démarre Graphene, seul l'état du quick terminal est restauré ; les fenêtres normales le seront au prochain lancement normal.
- **macOS** : le répertoire de travail des panes est aussi lu sur macOS (aujourd'hui Linux seulement).
- **Plantage** : la session est sauvegardée au fil de l'eau ; après un arrêt brutal, on retrouve le dernier état connu.
- **Répertoire disparu** : le pane s'ouvre dans le parent existant le plus proche.

## Features liées

- [Multiplexage](../multiplexing/overview.md) : indépendante. La restauration recrée la disposition avec des shells neufs ; garder les programmes vivants reste du ressort du multiplexage. Si les deux sont actifs et qu'il reste des sessions détachées au lancement, le multiplexage est prioritaire : les sessions sont rattachées et la restauration ne s'applique pas.

## Ordre des features

- Prérequis de [Quick terminal](../quick-terminal/overview.md) : la restauration de session doit couvrir aussi l'état du quick terminal (ses onglets et panes), selon le même réglage d'activation.
