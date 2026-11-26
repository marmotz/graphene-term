# Quick terminal (mode Quake)

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

Ghostty, WezTerm, Tabby, Guake et Yakuake proposent un terminal qu'un raccourci global fait apparaître et disparaître (souvent depuis le haut de l'écran). Graphene n'a pas cette fonctionnalité.

## Objectif

Afficher ou masquer une fenêtre Graphene dédiée avec un raccourci global, sans perdre son état entre deux apparitions.

## Décisions actées

- **Plateformes** : macOS et Linux sous X11. Pas de quick terminal sous Wayland : la commande l'explique par un message d'erreur, et la section des réglages est grisée avec la même explication.
- **Déclenchement** : une commande CLI affiche ou masque le quick terminal ; l'utilisateur la lie dans son gestionnaire de fenêtres. En plus, un raccourci global natif configurable, sans valeur par défaut.
- **Démarrage** : si Graphene ne tourne pas, la commande le lance et affiche directement le quick terminal, sans ouvrir de fenêtre normale. Quand le raccourci natif est configuré, chaque fenêtre normale démarre le quick terminal masqué s'il ne tourne pas, pour que le raccourci soit actif. Le quick terminal reste ensuite résident.
- **Placement** : collé à un bord de l'écran, en haut par défaut. Bord (haut, bas, gauche, droite) et taille (en % de l'écran) configurables.
- **Multi-écran** : écran configurable (actif, principal ou un écran nommé) ; par défaut l'écran actif (focus ou curseur) au moment du déclenchement.
- **Animation** : glissement depuis le bord choisi, durée configurable, 0 pour un affichage instantané.
- **Perte de focus** : masquage automatique par défaut, désactivable.
- **Contenu** : se comporte comme une fenêtre normale (onglets, splits, palette de commandes).
- **Fermeture** : fermer le dernier onglet ou la fenêtre masque le quick terminal ; au prochain affichage, il repart avec un shell neuf.
- **Persistance** : l'état est conservé entre deux apparitions tant que Graphene tourne. Au redémarrage, il est restauré via la restauration de session, uniquement si celle-ci est activée (pas de réglage propre au quick terminal).

## Dépend de

- [Restauration de session](../session-restore/overview.md) : doit être livrée avant le quick terminal, qui s'appuie dessus pour persister son état.
