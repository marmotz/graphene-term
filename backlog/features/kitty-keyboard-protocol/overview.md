# Protocole clavier kitty

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

L'encodage clavier historique est ambigu : Tab et Ctrl+I, Entrée et Ctrl+M,
Échap et Ctrl+[ envoient les mêmes octets, Ctrl+Shift+lettre n'est pas
distinguable, et le relâchement des touches n'est pas transmis. Le protocole
clavier de kitty (CSI u, « progressive enhancement ») lève ces ambiguïtés.
Neovim, Helix, Kakoune et fish 4 l'activent quand il est disponible. Kitty,
WezTerm, Ghostty, foot et Alacritty le supportent, Graphene non.

Vim et Emacs s'appuient plutôt sur le mode xterm `modifyOtherKeys`, plus
ancien, que Graphene ne supporte pas non plus.

## Objectif

Supporter le protocole clavier de kitty, ainsi que `modifyOtherKeys` d'xterm,
pour les programmes qui les demandent, sans rien changer pour ceux qui ne les
demandent pas.

## Décisions actées

- **Protocole kitty complet** : les 5 drapeaux progressifs sont supportés
  (désambiguïsation, types d'événements repeat/release, touches alternatives,
  toutes les touches en séquences, texte associé).
- **modifyOtherKeys inclus** : niveaux 1 et 2, comme xterm.
- **Priorité entre modes** : si une application active les deux, le protocole
  kitty l'emporte ; `modifyOtherKeys` reprend effet si le protocole kitty est
  désactivé.
- **Raccourcis Graphene prioritaires** : une touche liée à un raccourci
  Graphene (copie, onglets, palette...) reste capturée par Graphene, même quand
  l'application a activé un de ces modes. Les autres touches partent vers
  l'application, encodées selon le mode actif.
- **Éditeur de saisie à l'invite** : il garde la main même si le shell (fish 4)
  active le protocole. Seules les touches qu'il transmet au shell sont encodées
  selon le mode actif.
- **Pas de réglage utilisateur** : le support est toujours disponible et
  n'agit que sur demande de l'application.
- **Documentation** : une ligne dans les fonctionnalités du README (en et fr)
  et une section dans `docs/en|fr/keybindings.md` expliquant l'interaction
  avec les raccourcis Graphene.

## Hors périmètre

- Tout réglage pour forcer, limiter ou désactiver ces modes.
