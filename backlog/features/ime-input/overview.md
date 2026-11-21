# Saisie IME

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

Graphene ne gère pas les méthodes de saisie du système (IME) : aucun usage de l'API IME de la fenêtre dans `src`. Les utilisateurs qui écrivent en chinois, japonais ou coréen, ou qui passent par un IME pour les emojis et certaines compositions, ne peuvent pas taper leur texte. Kitty, WezTerm, Ghostty et Alacritty supportent l'IME.

## Objectif

Pouvoir saisir du texte via l'IME du système : texte en cours de composition affiché au curseur, fenêtre de suggestions positionnée au bon endroit, seul le texte validé envoyé au shell. Ça doit marcher dans l'éditeur de prompt comme dans les programmes plein écran.

## Décisions actées

- **Affichage du preedit** : inline au curseur, souligné. Dans l'éditeur de prompt il
  décale le texte qui suit ; dans les programmes plein écran il recouvre les cellules
  sans les modifier. La fenêtre de suggestions de l'IME est positionnée au curseur.
- **Périmètre** : le terminal (éditeur de prompt et programmes plein écran), la palette
  de commandes, les champs de recherche et la fenêtre de réglages (éditeur de thème
  compris).
- **Option de configuration** : un réglage permet de désactiver l'IME. Il est activé
  par défaut, pour que la saisie CJK marche dès l'installation ; il sert à couper un IME
  système qui gênerait des raccourcis.
- **Plateformes** : Linux (X11 et Wayland) et macOS dès la première version.

## Ordre des features

- [scrollback-search](../scrollback-search/overview.md) : son champ de recherche devra
  accepter la saisie IME, avec le même mécanisme que la palette de commandes.
