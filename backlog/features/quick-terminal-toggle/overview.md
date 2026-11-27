# Activation du quick terminal

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

Aujourd'hui, quand le raccourci natif du quick terminal est configuré, chaque fenêtre normale démarre le quick terminal masqué s'il ne tourne pas (voir [quick-terminal](../quick-terminal/overview.md)). Un utilisateur qui ne veut pas du quick terminal n'a aucun moyen de l'empêcher de se lancer.

## Objectif

Permettre d'activer ou de désactiver le quick terminal par un réglage, pour qu'il ne soit pas lancé automatiquement si l'utilisateur n'en veut pas. Désactivé par défaut.

## Décisions actées

- **Défaut** : le quick terminal est désactivé par défaut.
- **Effet de la désactivation** : le quick terminal n'est jamais lancé automatiquement (ni au démarrage d'une fenêtre normale, ni par le raccourci natif).
- **Commande CLI** : quand le quick terminal est désactivé, la commande échoue avec un message d'erreur qui explique comment l'activer.
- **Réglages** : tant que le quick terminal est désactivé, tous les réglages de la section sont grisés (raccourci natif compris), sauf l'interrupteur, avec une explication.
- **Démarrage avec la session** : l'option existante ne vaut que si le quick terminal est activé. Désactivé, l'entrée de démarrage de session est retirée.
- **Changement du réglage** : pris en compte au prochain démarrage de Graphene, pas à chaud. Un quick terminal déjà résident reste actif jusqu'à la fin de la session, mais la commande CLI refuse dès que le réglage est désactivé.
- **Migration** : rupture assumée. Le défaut désactivé s'applique à tous, y compris aux utilisateurs qui ont déjà un raccourci natif configuré ; ils doivent activer le réglage.

## Dépend de

- [Quick terminal](../quick-terminal/overview.md) : évolution de cette fonctionnalité, déjà livrée.
