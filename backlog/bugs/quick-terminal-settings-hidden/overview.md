# Les réglages s'ouvrent sous le quick terminal

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

Quand on ouvre les réglages en mode quick terminal, ils apparaissent sous le quick
terminal et l'utilisateur ne peut pas y accéder. Constaté sous Linux X11, où le quick
terminal est maintenu au-dessus des autres fenêtres.

## Objectif

Les réglages ouverts en mode quick terminal s'affichent devant lui et sont accessib## Décisions actées

- **Réglages en onglet** : une fenêtre de réglages séparée reste sous le quick terminal
  sous Cinnamon X11 malgré les contournements essayés (niveau au-dessus, activation,
  fenêtre liée). Les réglages quittent donc leur fenêtre et s'affichent dans un onglet,
  partout, en mode normal comme en quick terminal.
- **Barre d'onglets** : toujours visible, même avec un seul onglet de terminal, y
  compris en quick terminal.
- **Onglet réglages** : un onglet permanent, toujours en première position, affiché
  comme une simple icône de paramètres. Un seul par fenêtre.
- **Ouverture** : le menu contextuel et le raccourci des réglages activent cet onglet.
- **Fermeture** : l'onglet réglages ne se ferme jamais. On le quitte en activant un autre
  onglet ; Échap revient à l'onglet de terminal précédent.
- **Splits** : les réglages occupent l'onglet entier, on ne peut pas splitter à côté.
- **Navigation** : les raccourcis entre onglets (suivant, précédent, aller à l'onglet N)
  ne concernent que les onglets de terminal : l'onglet 1 est le premier terminal.
- **Dernier terminal fermé** : comme aujourd'hui, la fenêtre se ferme (le quick terminal
  repart sur un onglet neuf) ; l'onglet réglages ne la retient pas.
- **Restauration de session** : sans objet, l'onglet réglages est toujours présent.

ché.

## Voir aussi

- [Quick terminal](../../features/quick-terminal/overview.md)
