[English](../en/selection-and-clipboard.md)

# Sélection et presse-papiers

- [Sélectionner à la souris](#sélectionner-à-la-souris)
- [Copier](#copier)
- [Ce qui est copié](#ce-qui-est-copié)
- [Coller](#coller)
- [Quand un programme utilise la souris](#quand-un-programme-utilise-la-souris)

## Sélectionner à la souris

| Pour                             | Utilisez                                     |
|----------------------------------|----------------------------------------------|
| Sélectionner du texte            | Glisser avec le bouton gauche                |
| Sélectionner un mot              | Double clic                                  |
| Sélectionner une ligne entière   | Triple clic                                  |
| Sélectionner un rectangle (colonnes) | `Ctrl` + glisser avec le bouton gauche   |
| Étendre la sélection             | `Shift` + clic gauche                        |

- Glisser au-delà du haut ou du bas d'un panneau le fait défiler.
- Ce qui compte comme un mot pour un double clic est défini par `word_separators` dans
  la [configuration](configuration.md#selection). Par défaut, une URL ou un
  `hôte:port` forme un seul mot.
- Pour sélectionner tout un [bloc de commande](command-blocks.md#sélectionner-un-bloc),
  cliquez sur sa barre d'état.
- La sélection reste tant que le programme redessine le même texte ; elle est effacée
  quand vous tapez, quand vous collez, ou quand le texte sélectionné change.

À l'invite du shell, vous pouvez aussi sélectionner votre ligne de commande au clavier :
voir [Éditeur de saisie](input-editor.md).

## Copier

| Copier vers                      | Comment                                      |
|----------------------------------|----------------------------------------------|
| Le presse-papiers                | `Ctrl+Shift+C` (ou la touche `Copy`), ou **Copier** dans le menu du clic droit |
| La sélection primaire (Linux)    | Automatique : relâcher la souris après une sélection l'y copie |

`Ctrl+Shift+C` n'envoie jamais `Ctrl+C` au programme, même sans sélection : il ne peut
donc pas interrompre une commande par erreur.

Certains bureaux Wayland ne gèrent pas la sélection primaire ; le presse-papiers
fonctionne quand même.

## Ce qui est copié

Le texte copié est nettoyé pour bien se coller :

- les espaces en fin de ligne sont retirés ;
- l'indentation commune à toutes les lignes est retirée ;
- il n'y a pas de retour à la ligne final : coller une commande copiée ne la lance donc
  jamais toute seule.

Une sélection qui contient une image (voir [Images et liens](images-and-links.md))
place aussi l'image dans le presse-papiers, à côté du texte : coller dans un éditeur
d'images ou une messagerie donne l'image.

## Coller

| Coller                           | Utilisez                                     |
|----------------------------------|----------------------------------------------|
| Le presse-papiers                | `Ctrl+Shift+V`, `Shift+Insert` (ou la touche `Paste`), ou **Coller** dans le menu du clic droit |
| La sélection primaire (Linux)    | Clic du milieu                               |

Quand le programme le demande (la plupart des shells et des éditeurs le font), le texte
collé est marqué comme un collage (« bracketed paste ») : le programme n'exécute donc
pas les lignes collées une par une.

## Quand un programme utilise la souris

Certains programmes (`vim`, `htop`, `tmux`, `mc`...) utilisent eux-mêmes la souris.
Dans leur écran, les clics, les glisser et la molette vont au programme, pas à
Graphene.

- Maintenez `Shift` pour sélectionner, étendre la sélection ou faire un clic droit
  comme d'habitude malgré tout.
- Pour y ouvrir un [lien](images-and-links.md#liens-cliquables), faites `Ctrl` + clic
  dessus.
- `Ctrl` + molette change toujours la taille de police du panneau, elle ne va
  jamais au programme.
- Dans un programme plein écran qui n'utilise pas la souris (`less`, `man`...), la
  molette fait défiler le programme en lui envoyant des flèches.
- Les clics sur la barre de titre d'un panneau, sur la barre d'état ou le chevron d'un
  bloc, et sur les blocs de commande précédents sont toujours pour Graphene.
