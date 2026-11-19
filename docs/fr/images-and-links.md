[English](../en/images-and-links.md)

# Images et liens

- [Images dans le terminal](#images-dans-le-terminal)
- [Liens cliquables](#liens-cliquables)

## Images dans le terminal

Les programmes peuvent afficher des images directement dans le terminal. Graphene prend
en charge les deux formats courants :

- le **protocole graphique de kitty** (kitty graphics protocol), images fixes et
  animations, y compris le mode à emplacements réservés utilisé par `tmux` et certains
  éditeurs ;
- les images **sixel**.

Pour essayer, utilisez un programme qui gère l'un de ces formats, comme `chafa` ou
`img2sixel` (libsixel) :

```bash
chafa picture.png
img2sixel picture.png
```

Comportement des images :

- Une image s'insère dans le texte comme un bloc de caractères : elle défile avec la
  sortie, reste dans l'historique, et est effacée quand le programme efface cette
  partie de l'écran.
- Du texte écrit par-dessus une image remplace cette partie de l'image.
- Zoomer la police (`Ctrl+=` / `Ctrl+-`) redimensionne les images avec le texte.
- Rétrécir la fenêtre peut couper une image ; l'élargir à nouveau la rétablit.
- Chaque panneau conserve jusqu'à 256 Mio de données d'image. Une image ou une
  animation très lourde qui dépasse cette limite est refusée.
- Sélectionner et copier une partie de l'écran qui contient une image copie aussi
  l'image (voir [Sélection et presse-papiers](selection-and-clipboard.md#ce-qui-est-copié)).

## Liens cliquables

Graphene rend deux sortes de liens cliquables :

- les **liens définis par les programmes**, qui marquent un morceau de texte comme lien
  (la séquence d'échappement OSC 8 ; `ls --hyperlink`, `gcc`, les outils de `systemd`
  et d'autres peuvent en émettre) ;
- les **URL détectées dans le texte**, qui commencent par `http://`, `https://`,
  `file:` ou `mailto:`. La ponctuation qui suit immédiatement une URL (un point final,
  un guillemet ou une parenthèse fermante) n'en fait pas partie.

Un lien sous le pointeur de la souris est souligné.

| Pour                                        | Utilisez                                    |
|---------------------------------------------|---------------------------------------------|
| Ouvrir un lien                              | Cliquez dessus (sans bouger la souris)      |
| Ouvrir un lien là où un programme utilise la souris | `Ctrl` + clic                       |
| Sélectionner du texte dans un lien          | Glisser, double ou triple clic, ou `Shift` + clic |

Les liens s'ouvrent avec l'application par défaut de votre bureau : votre navigateur
pour les liens web, votre messagerie pour `mailto:`, votre gestionnaire de fichiers ou
l'application correspondante pour `file:`.

Par sécurité, un lien `file://` qui désigne une autre machine (`file://autre-hote/...`)
n'est pas ouvert : seuls les fichiers de votre propre machine le sont. Les liens définis
par les programmes peuvent utiliser n'importe quel autre schéma (`ssh:`, `vscode:`...),
du moment que votre bureau sait l'ouvrir ; les URL détectées se limitent aux quatre
schémas ci-dessus.
