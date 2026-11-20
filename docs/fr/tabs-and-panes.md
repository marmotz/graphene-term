[English](../en/tabs-and-panes.md)

# Onglets et panneaux

Une fenêtre contient des onglets, et chaque onglet peut être divisé en panneaux, chacun
avec son propre shell.

- [Onglets](#onglets)
- [Panneaux](#panneaux)
- [Barre de titre d'un panneau](#barre-de-titre-dun-panneau)
- [Déplacer les panneaux à la souris](#déplacer-les-panneaux-à-la-souris)
- [Zoom](#zoom)
- [Palette de commandes](#palette-de-commandes)
- [Fermeture et confirmation](#fermeture-et-confirmation)

Tous les raccourcis ci-dessous sont ceux par défaut ; voir
[Raccourcis clavier](keybindings.md) pour les modifier.

## Onglets

| Pour                                   | Utilisez                                         |
|----------------------------------------|--------------------------------------------------|
| Ouvrir un onglet                       | `Ctrl+Shift+T`, ou cliquez sur `+` dans la barre d'onglets |
| Fermer l'onglet                        | `Ctrl+Alt+W`, clic du milieu sur l'onglet, ou son `×` |
| Onglet suivant / précédent             | `Ctrl+PageDown` / `Ctrl+PageUp` |
| Aller à l'onglet 1 à 9                 | `Alt+1` ... `Alt+9`                              |
| Afficher un onglet                     | Cliquez dessus                                   |
| Réordonner les onglets                 | Faites glisser un onglet le long de la barre     |
| Menu de l'onglet (nouvel onglet, fermer, paramètres, thèmes) | Clic droit sur un onglet   |

La barre d'onglets apparaît dès qu'il y a deux onglets. Son bord est défini par
`position` dans la section `[tabs]` de la [configuration](configuration.md#tabs) :
`"top"` (par défaut), `"bottom"`, `"left"` ou `"right"`. À gauche ou à droite, les
onglets sont empilés.

## Panneaux

| Pour                                   | Utilisez                                         |
|----------------------------------------|--------------------------------------------------|
| Diviser, nouveau panneau à droite      | `Ctrl+Shift+E`                                   |
| Diviser, nouveau panneau en bas        | `Ctrl+Shift+O`                                   |
| Fermer le panneau                      | `Ctrl+Shift+W`, ou `×` sur sa barre de titre     |
| Donner le focus à un panneau           | Cliquez dedans, ou `Ctrl+Shift+Flèche`           |
| Panneau suivant / précédent            | `Ctrl+Tab` / `Ctrl+Shift+Tab`                  |
| Redimensionner le panneau              | Faites glisser la séparation entre deux panneaux, ou `Ctrl+Shift+Alt+Flèche` |
| Donner la même taille à tous les panneaux | **Égaliser les panneaux** dans la palette de commandes |
| Déplacer le panneau dans un nouvel onglet | **Déplacer le panneau dans un nouvel onglet** dans la palette de commandes, ou faites-le glisser (voir plus bas) |

- Sous Linux, un nouveau panneau démarre dans le dossier courant du panneau actif ; sous
  macOS, dans votre dossier personnel.
- Un panneau a besoin d'au moins 10 colonnes et 3 lignes : une division qui le rendrait
  plus petit ne fait rien.
- Les panneaux qui n'ont pas le focus sont légèrement assombris.
- À l'invite du shell, `Ctrl+Shift+Left` / `Right` sélectionnent des mots dans la ligne
  de commande (voir [Éditeur de saisie](input-editor.md)) ; quand l'onglet a plusieurs
  panneaux, ils déplacent plutôt le focus.

## Barre de titre d'un panneau

Chaque panneau a en haut une barre de titre qui affiche son titre (défini par le shell
ou le programme en cours, sinon le dossier courant) et un bouton `×`. La barre du
panneau actif est dessinée dans la couleur d'accent. Les clics sur la barre sont gérés
par Graphene et n'atteignent jamais le programme du panneau.

| Sur la barre de titre | Effet                                 |
|-----------------------|---------------------------------------|
| Clic                  | Donne le focus au panneau             |
| Double clic           | Agrandit le panneau, ou le réduit     |
| `×`                   | Ferme le panneau                      |
| Clic droit            | Menu du panneau                       |
| Glisser               | Déplace le panneau (voir plus bas)    |

Le menu du panneau, qui s'ouvre aussi par un clic droit dans le panneau, propose de
copier et coller, diviser, fermer, agrandir, ouvrir un nouvel onglet, fermer l'onglet,
les paramètres, les thèmes et quitter. Ouvert sur un [bloc de commande](command-blocks.md)
terminé, il propose aussi de copier sa commande ou sa sortie et de le replier. Là où un
programme utilise la souris (voir
[Sélection et presse-papiers](selection-and-clipboard.md#quand-un-programme-utilise-la-souris)),
maintenez `Shift` pendant le clic droit dans le panneau.

## Déplacer les panneaux à la souris

Faites glisser un panneau par sa barre de titre et déposez-le :

- près d'un bord d'un autre panneau : il est placé de ce côté de celui-ci ;
- au milieu d'un autre panneau : les deux panneaux échangent leur place ;
- sur un onglet : il passe dans cet onglet (maintenez-le une seconde au-dessus d'un
  onglet pour afficher cet onglet et le déposer précisément) ;
- sur `+` ou sur la partie vide de la barre d'onglets : il passe dans un nouvel onglet.

La barre d'onglets s'affiche pendant le glisser même s'il n'y a qu'un onglet, et une
surbrillance translucide montre où le panneau va atterrir. `Échap` annule le glisser.

## Zoom

`Ctrl+Shift+X`, ou un double clic sur la barre de titre, fait occuper tout l'onglet au
panneau ; la même action rétablit la disposition. Les autres panneaux continuent de
tourner pendant ce temps.

## Diffusion

`Ctrl+Shift+B` envoie ce que vous tapez et collez à tous les panneaux de l'onglet à la
fois ; le même raccourci l'arrête. Chaque panneau traite les touches selon son propre
état (invite, programme en cours). Les raccourcis de Graphene ne sont jamais diffusés :
ils n'agissent que sur le panneau actif. La diffusion continue pendant un zoom.

`Ctrl`+clic sur un panneau (ou sur sa barre de titre) l'ajoute à la diffusion ou l'en
retire, sans lui donner le focus ; sur un lien, `Ctrl`+clic ouvre toujours le lien.
`Ctrl`+glisser fait toujours une sélection rectangulaire, sauf là où l'application
capture la souris.

La barre de titre d'un panneau de la diffusion prend la couleur de thème `ui.pane_broadcast`
et commence par `»` suivi du nombre de panneaux.

## Palette de commandes

`Ctrl+Shift+P` ouvre la palette de commandes : toutes les actions avec leur raccourci,
puis les onglets et panneaux ouverts, pour aller directement à l'un d'eux.

- Tapez pour filtrer. La recherche est approximative (`divdr` trouve « Diviser à
  droite ») et ignore la casse et les accents, sauf si vous tapez une majuscule ou une
  lettre accentuée.
- `Up` / `Down` pour choisir, `Entrée` pour lancer, `Échap` pour fermer.
- Les actions lancées récemment viennent en premier. Elles sont mémorisées d'un
  lancement à l'autre.

Les actions sans raccourci par défaut, comme **Égaliser les panneaux** et
**Déplacer le panneau dans un nouvel onglet**, se lancent d'ici (ou attribuez-leur un
raccourci, voir [Raccourcis clavier](keybindings.md#modifier-les-raccourcis)).

## Fermeture et confirmation

Fermer un panneau, un onglet ou la fenêtre pendant qu'un programme autre que le shell
y tourne (un éditeur, `ssh`, une compilation...) demande confirmation et nomme le
programme. Quand seul le shell tourne, la fermeture est immédiate. Sous macOS, la
fermeture ne demande jamais confirmation.

Quand le shell d'un panneau se termine (`exit`, `Ctrl+D`), le panneau se ferme ; quand
le dernier panneau de la fenêtre se ferme, Graphene quitte.
