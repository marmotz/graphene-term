# Ligatures de police

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

Le rendu des glyphes ne garde que le premier caractère d'un cluster
(src/renderer/glyphs.rs), donc les ligatures des polices de programmation
(Fira Code, JetBrains Mono, Cascadia Code) ne s'affichent pas. Kitty,
WezTerm et Ghostty les supportent.

## Objectif

Afficher les ligatures des polices qui en proposent, avec la possibilité de
les désactiver et d'activer les variantes stylistiques OpenType de la police.

## Décisions actées

- **Activées par défaut** : une police qui propose des ligatures les affiche
  sans configuration. Une police sans ligatures n'est pas affectée.
- **Interrupteur on/off** : un réglage permet de désactiver les ligatures.
- **Features OpenType** : en plus de l'interrupteur, une liste libre de
  features OpenType (ex. `ss01`, `cv05`, `zero`) permet d'activer les
  variantes stylistiques de la police.
- **Où se règle** : l'interrupteur et la liste de features sont disponibles
  dans la section `[font]` du fichier de config et dans la fenêtre Réglages.
- **Curseur** : la ligature sous le curseur s'affiche en caractères séparés,
  pour voir ce qu'on édite. Non configurable.
- **Styles mixtes** : une ligature ne se forme qu'entre cellules de même
  style (couleurs, attributs). Une sélection ou une coloration qui couvre
  une partie seulement de la séquence casse la ligature.
- **Portée** : tout texte rendu avec la police du terminal, c'est-à-dire la
  grille et l'éditeur de saisie. Le reste de l'interface n'est pas concerné.
