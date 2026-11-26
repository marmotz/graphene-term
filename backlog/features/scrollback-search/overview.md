# Recherche dans l'historique

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

Graphene n'offre aucune recherche de texte dans l'historique du terminal : le seul champ de recherche est le filtre de la palette de commandes. Kitty, WezTerm, Ghostty et Warp proposent tous une recherche (Ctrl+Shift+F ou équivalent), c'est une attente de base.

## Objectif

Permettre de retrouver un texte dans l'historique d'un pane, en tirant parti des blocs de commande (filtrer par commande ou sortie, par statut).

## Décisions actées

### Ouverture et raccourcis

- `Ctrl+Shift+F` ouvre la recherche dans le pane actif, comme dans les autres terminaux.
- Le pliage de bloc quitte `Ctrl+Shift+F` pour `Ctrl+Shift+Z`, et « tout plier / tout déplier » quitte `Ctrl+Shift+Alt+F` pour `Ctrl+Shift+Alt+Z`. Changement cassant à signaler dans le changelog et la documentation.
- La recherche est désactivée pendant une application plein écran (vim, less, htop) : elle ne porte que sur l'historique des blocs.

### Barre de recherche

- Barre flottante compacte en haut à droite du pane, qui ne décale pas le contenu.
- Recherche pendant la frappe.
- `Échap` ferme la barre : la vue reste sur la dernière occurrence vue, le surlignage disparaît.
- Champ avec curseur (flèches, Origine/Fin, Suppr, Ctrl+Retour arrière) et collage `Ctrl+Shift+V`, sans historique des recherches.
- Un clic dans le contenu du pane rend le clavier au shell sans fermer la barre ; `Ctrl+Shift+F` ou un clic sur la barre lui redonne le clavier.
- Couleurs des occurrences : deux nouvelles clés de thème, renseignées dans les thèmes intégrés et l'éditeur de thème.

### Portée et filtres

- Portée par défaut : tout l'historique du pane.
- Filtre zone : commandes seulement, sorties seulement, ou les deux. « Commandes » = le texte de la commande seul, sans le prompt (repli sur tout l'en-tête quand le shell n'a pas envoyé la commande).
- Filtre statut : blocs en succès seulement, en échec seulement, ou tous.
- Pas de filtre « bloc sélectionné » en v1.

### Correspondance

- Insensible à la casse par défaut.
- Trois bascules : respect de la casse, expression régulière, mot entier.

### Résultats et navigation

- Toutes les occurrences sont surlignées, l'occurrence courante est mise en avant.
- Compteur « courante / total » dans la barre.
- `Entrée` / `Maj+Entrée` : occurrence suivante / précédente. « Suivante » remonte vers le plus ancien ; le compteur part du bas (1 = la plus récente).
- Les blocs pliés sont inclus : un bloc se déplie quand la navigation arrive sur une de ses occurrences.
- La sortie ajoutée pendant que la barre est ouverte est intégrée aux résultats (compteur, surlignage), sans déplacer la vue.

### Hors périmètre v1

- Liste ou panneau des blocs trouvés.
- Recherche dans plusieurs panes à la fois.
- Recherche dans l'écran d'une application plein écran.

## Ordre des features

- [ime-input](../ime-input/overview.md) : le champ de recherche doit accepter la saisie
  IME (texte en composition affiché, texte validé ajouté à la recherche). Si ime-input
  est livrée avant, réutiliser son mécanisme ; sinon, ime-input l'ajoutera.
