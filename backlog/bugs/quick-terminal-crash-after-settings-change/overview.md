# Quick terminal crashes when closed after changing the settings

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

Dans le quick terminal (feature [quick-terminal](../../features/quick-terminal/overview.md)),
après avoir modifié le pourcentage de taille du quick terminal dans les settings, le
fermer provoque parfois un plantage. Observé avec un quick terminal configuré pour
venir de la droite. Le défaut est intermittent.

Le plantage tue tout le processus : wgpu lève une erreur de validation traitée comme
fatale (panic), car la zone de dessin (scissor rect `2560x1371`) dépasse la taille de
la cible de rendu (`1280x1400`). Le rendu utilise donc encore l'ancienne taille alors
que la fenêtre a déjà été redimensionnée selon le nouveau pourcentage.

Le quick terminal "ne peut plus se rouvrir" parce que l'application n'existe plus :
la relancer suffit à retrouver un quick terminal fonctionnel (pas de settings
corrompus).

Non vérifié : si le même défaut existe en fenêtre normale.

## Objectif

Modifier les settings (notamment le pourcentage du quick terminal) puis fermer ou
rouvrir le quick terminal ne doit jamais faire planter l'application, et le quick
terminal doit toujours pouvoir être rouvert ensuite.

## Décisions actées

- La correction couvre la cause : la taille de rendu doit rester synchronisée avec la
  taille réelle de la fenêtre après un changement de settings.
- La correction couvre aussi la robustesse : une erreur de rendu de ce type ne doit
  plus faire tomber toute l'application.
- Vérifier pendant le diagnostic si les fenêtres normales sont aussi touchées, et les
  inclure dans le correctif le cas échéant.
