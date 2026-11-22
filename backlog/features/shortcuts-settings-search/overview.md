# Recherche dans les réglages des raccourcis

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

La section Raccourcis des réglages liste toutes les actions et leurs raccourcis, avec
une capture de touche pour en ajouter ou en modifier un. Avec le nombre d'actions qui
grandit, retrouver une action précise demande de parcourir toute la liste, et il est
impossible de savoir rapidement à quelle action un raccourci donné est déjà associé.

## Objectif

Retrouver rapidement une action dans la section Raccourcis :

- en filtrant la liste des actions par du texte tapé ;
- en tapant un raccourci clavier pour retrouver la ou les actions qui l'utilisent.

## Décisions actées

### Interface

- En haut de la section Raccourcis : un champ texte pour filtrer et, à côté, un bouton
  « Rechercher un raccourci » qui capture la prochaine combinaison de touches. Les deux
  modes sont séparés, donc taper une lettre dans le champ ne lance jamais une recherche
  de raccourci.
- Le champ texte prend le focus à l'ouverture de la section. Échap vide le champ.
- Les deux critères s'excluent : capturer un raccourci vide le champ texte, et taper du
  texte annule le raccourci recherché.

### Filtre texte

- Il ne porte que sur le nom de l'action, dans la langue de l'interface.
- La recherche se fait par sous-chaîne, sans tenir compte de la casse ni des accents :
  « onglet » trouve « Nouvel onglet » et « Fermer l'onglet ».

### Recherche par raccourci

- La liste n'affiche que la ou les actions associées au raccourci capturé.
- Si aucune action ne l'utilise, la liste est vide avec le message « Aucune action
  n'utilise ce raccourci » : le raccourci est donc libre.

### Comportement

- Modifier le filtre annule l'édition en cours (capture « Ajouter » ou conflit
  affiché), comme si l'utilisateur cliquait sur « Annuler ».
- La liste filtrée est figée : ajouter, retirer ou remplacer un raccourci ne recalcule
  pas le filtre. Elle n'est mise à jour qu'au prochain changement du filtre.
- Le filtre est vidé quand on quitte la section Raccourcis.
