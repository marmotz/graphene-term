# Multiplexage

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

Quand la fenêtre de Graphene se ferme (volontairement ou en cas de plantage), les programmes en cours dans ses panes s'arrêtent. WezTerm sépare la fenêtre d'un serveur qui garde les sessions vivantes, comme tmux mais intégré au terminal. Ce type de fonctionnalité est souvent complexe à configurer et à comprendre.

## Objectif

Pouvoir fermer puis rouvrir la fenêtre en retrouvant les panes et les programmes qui y tournaient.

## Décisions actées

- **Condition d'implémentation** : la fonctionnalité doit être simple pour l'utilisateur (pas de notion de serveur, de domaine ou de socket à configurer ou à comprendre). Si aucune conception ne respecte cette contrainte, la feature n'est pas implémentée.
- **Fermeture volontaire** : si au moins un pane exécute autre chose qu'un shell inactif, un dialogue propose "Garder en arrière-plan" ou "Tout arrêter". Sinon la fenêtre se ferme et tout s'arrête sans question.
- **Mémoriser le choix** : une case "Ne plus demander" fait du choix le comportement par défaut, modifiable dans les réglages.
- **Plantage** : un plantage de la fenêtre ne tue jamais les programmes, quel que soit le choix mémorisé ; ils sont rattachés à la réouverture.
- **Reprise** : au lancement, Graphene rattache automatiquement les sessions détachées, sans question. Chaque session détachée rouvre sa propre fenêtre, comme avant la fermeture.
- **Contenu retrouvé** : écran, historique de défilement et blocs de commande avec leur statut, comme si la fenêtre n'avait jamais été fermée.
- **Nouvelle fenêtre** : lancer Graphene alors qu'une fenêtre est déjà ouverte (et sans session détachée) ouvre une fenêtre avec un shell neuf.
- **Visibilité** : une icône dans la zone de notification signale les sessions gardées en arrière-plan et propose "Rouvrir" et "Tout arrêter".
- **Durée de vie** : les sessions détachées vivent tant que la session utilisateur est ouverte, sans limite de temps. Un programme qui se termine de lui-même ferme son pane normalement.
- **Mise à jour** : une mise à jour de Graphene ne fait jamais perdre une session détachée ; elle reste rattachable par la nouvelle version.

## Features liées

- [Restauration de session](../session-restore/overview.md) : complémentaire et indépendante, sans ordre d'implémentation. Le multiplexage garde les programmes vivants tant que la machine tourne ; après un redémarrage, la restauration de session recrée la disposition avec des shells neufs. Au lancement, s'il reste des sessions détachées, le multiplexage est prioritaire et la restauration de session ne s'applique pas.
