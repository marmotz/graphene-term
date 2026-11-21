# Indicateur de progression OSC 9;4

**Statut** : tâches créées, voir [technical.md](technical.md)

## Contexte

La séquence OSC 9;4 permet à un programme de signaler sa progression (pourcentage, erreur, indéterminée). Ghostty et Windows Terminal l'affichent, et des outils comme systemd, winget ou certaines barres de progression l'émettent. Graphene l'ignore.

## Objectif

Afficher la progression signalée par un programme dans l'interface : onglet, pane, bloc de commande et barre des tâches du système.

## Décisions actées

### États

- Les cinq états de la séquence sont pris en charge : effacer, pourcentage, erreur,
  indéterminé, pause. Chacun a un rendu distinct, tiré des couleurs du thème.

### Emplacements

- **Onglet** : indicateur visible même quand l'onglet n'est pas actif.
- **Pane** : barre en bord du pane concerné.
- **Bloc de commande** : progression dans le bloc de la commande en cours.
- **Barre des tâches système** : progression sur l'icône de l'application (Dock macOS,
  lanceurs Linux compatibles).
- Le pane et le bloc affichent la progression en même temps, sans exclusion.
- Dans le bloc, la barre est sur le bord inférieur de la carte.
- L'état indéterminé est un segment animé (statique dans le Dock macOS).

### Agrégation

- **Onglet** : reflète uniquement le pane actif de l'onglet ; rien si ce pane ne signale
  pas de progression.
- **Barre des tâches** : reflète uniquement l'onglet actif de la fenêtre.

### Effacement automatique

- L'indicateur disparaît à la fin de la commande (bloc terminé) ou, à défaut, après
  environ 15 secondes sans mise à jour (programmes plein écran, hors bloc).

### Réglage

- Un seul interrupteur, activé par défaut, dans la configuration (`[window] progress`)
  et la fenêtre de réglages.

## Features liées

- [Notifications de bureau](../desktop-notifications/overview.md) : partage le préfixe
  OSC 9. Une séquence OSC 9;4 est une progression, jamais une notification. Les deux
  features touchent l'onglet, le pane et la barre des tâches (badge contre progression) ;
  leurs indicateurs doivent pouvoir coexister. Pas d'ordre imposé entre les deux.
