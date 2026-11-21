# Easter eggs

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

Graphene n'a aucun easter egg. L'idée est d'en cacher quelques-uns dans l'application,
pour le plaisir de ceux qui les trouvent. Ils restent visibles dans le code et les
commits : ils sont discrets, pas secrets.

## Objectif

Ajouter quelques surprises légères, découvrables par curiosité, qui ne gênent jamais
l'usage normal du terminal (pas de déclenchement accidentel, pas d'impact sur les
performances ni sur la sortie des programmes).

## Décisions actées

- **Règles communes** : aucun easter egg ne peut se déclencher par accident, aucun ne
  touche au flux PTY ni à la sortie des programmes.
- **Konami code** : la séquence ↑↑↓↓←→←→BA lance une courte animation sur la grille,
  des hexagones de graphène qui se dessinent puis s'effacent.
- **Entrées cachées de la palette de commandes** : absentes de la liste, elles
  n'apparaissent que si la recherche correspond exactement au mot-clé.
  - `42` ouvre la page Wikipédia sur la grande question sur la vie, l'univers et le
    reste, dans la langue de l'interface.
  - `marmotz` ouvre le profil GitHub de l'auteur.
  - `graphene` affiche un dessin ASCII du graphène dans une petite fenêtre qui se
    ferme avec Échap (rien n'est écrit dans le terminal).
- **Version dans les réglages** : la fenêtre de réglages affiche la version de Graphene
  (elle n'est affichée nulle part aujourd'hui). C'est un prérequis du thème caché.
- **Thème caché "phosphore CRT"** : un thème vert phosphore avec un effet de scanlines,
  absent de la liste des thèmes, débloqué en cliquant plusieurs fois sur la version
  dans les réglages : 7 clics rapides. Le thème est appliqué immédiatement, puis reste
  disponible dans la liste des thèmes de façon permanente (déblocage enregistré dans la
  config).
- **Dates spéciales** : certains jours, le titre de la fenêtre est décoré toute la
  journée, en général par un symbole en tête.
  - 14 février : un cœur.
  - 14 mars : le symbole π.
  - 4 mai : le texte « May the fourth... » ajouté à la fin du titre, à la place d'un
    symbole (le titre ne peut pas contenir d'image et aucun caractère ne représente un
    sabre laser).
  - 31 octobre : une citrouille.
  - 25 décembre : un sapin de Noël.
  - L'anniversaire de la v1 sera ajouté quand elle sortira.
- **Pas d'interrupteur** : aucune option pour désactiver les easter eggs.
- **Écarté** : les badges humoristiques sur les blocs de commandes (`sl`, `cowsay`, etc.),
  jugés trop intrusifs.
