# Garder la fenêtre ouverte après `-e`

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

`graphene -e <programme>` ouvre une fenêtre dont le premier pane lance le programme au
lieu du shell. Quand le programme se termine, le pane se ferme et, s'il était le
dernier, la fenêtre aussi. Pour un programme interactif (`htop`, `vim`), c'est le
comportement attendu. Pour un programme qui affiche un résultat puis se termine
(`ls`, un script, un build), la fenêtre disparaît avant qu'on ait pu lire la sortie.

Les autres terminaux gardent tous `-e` = « ferme à la fin » et ajoutent une option
pour garder la fenêtre : `--hold` (alacritty, foot, kitty, konsole), `-hold` (xterm).
Des outils tiers appellent `<terminal> -e <programme>` en comptant sur la fermeture :
lanceurs d'applications (`.desktop` avec `Terminal=true`), `xdg-terminal-exec`,
`i3-sensible-terminal`, gestionnaires de fichiers, `$TERMINAL -e` dans des scripts.

Note : graphene ne relaie ni la sortie ni le code de retour du programme
(`graphene::run` ne renvoie pas le code du programme). Le cas « programmatique » actuel
sert donc surtout aux lanceurs, pas à récupérer un résultat.

## Objectif

Pouvoir lancer un programme dans une fenêtre graphene et lire sa sortie une fois qu'il
s'est terminé : la fenêtre reste ouverte, un message indique que le programme est
terminé et qu'il suffit d'appuyer sur une touche pour fermer.

## Décisions actées

### Option

- `-e` garde son comportement actuel : la fenêtre se ferme quand le programme se
  termine. Pas d'inversion, pour rester compatible avec la convention des autres
  terminaux et les outils qui appellent `<terminal> -e`.
- Nouvelle option `-E <programme> [args...]`, sans forme longue, qui garde la fenêtre
  ouverte après la fin du programme. Elle se comporte comme `-e` : elle se place en
  dernier et tous les arguments qui la suivent vont au programme.
- Pas de réglage dans la configuration : le comportement se choisit uniquement par
  l'option passée en ligne de commande.

### Fin du programme

- La sortie du programme reste affichée, suivie d'un message qui indique que le
  programme est terminé, avec son code de retour, et qu'il faut appuyer sur une touche
  pour fermer.
- N'importe quelle touche ferme, raccourcis de graphene compris. Les touches de
  modification seules (`Shift`, `Ctrl`, `Alt`...) ne ferment pas.
- La souris reste utilisable (défilement, sélection) pendant l'attente.
