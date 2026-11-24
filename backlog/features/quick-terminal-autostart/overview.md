# Quick terminal autostart

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

Le raccourci global du quick terminal (`hotkey`) n'est enregistré que par le process du quick terminal. Ce process n'est démarré que par la première fenêtre Graphene ouverte (si `hotkey` est défini) ou par `graphene --quick-toggle`. Après une ouverture de session, tant que l'utilisateur n'a rien lancé, le raccourci ne fait rien. Aujourd'hui il faut ajouter soi-même `graphene --quick --hidden` à l'autostart de son environnement.

## Objectif

Le raccourci global du quick terminal fonctionne dès l'ouverture de la session graphique, sans lancer Graphene manuellement, en faisant démarrer le quick terminal masqué par Graphene lui-même (par exemple une entrée XDG autostart sous Linux, un élément de connexion sous macOS).

## Décisions actées

- **Activation** : opt-in, désactivé par défaut. Rien n'est écrit hors de la configuration de Graphene sans action explicite de l'utilisateur.
- **Réglage** : une clé dédiée `autostart` dans la section `[quick_terminal]`, avec une case à cocher dans la section Quick terminal de la fenêtre des réglages. Pas de commande CLI.
- **Effet** : activer le réglage fait créer par Graphene une entrée de démarrage de session qui lance le quick terminal masqué. Le raccourci global fonctionne alors dès l'ouverture de session.
- **Plateformes** : macOS et Linux sous X11, comme le quick terminal. Sous Wayland, le réglage est grisé avec la même explication que le reste de la section.
- **Nettoyage** : l'entrée suit le réglage. Désactiver l'option supprime l'entrée de démarrage de session.
