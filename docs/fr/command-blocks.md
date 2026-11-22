[English](../en/command-blocks.md)

# Blocs de commande

Avec zsh, bash ou fish, Graphene regroupe chaque commande et sa sortie dans un
**bloc** : une carte avec une barre colorée qui indique si la commande a réussi, que
vous pouvez sélectionner, copier et replier.

- [Intégration shell](#intégration-shell)
- [Blocs](#blocs)
- [Sélectionner un bloc](#sélectionner-un-bloc)
- [Copier une commande ou sa sortie](#copier-une-commande-ou-sa-sortie)
- [Replier](#replier)
- [Défilement](#défilement)

## Intégration shell

Les blocs ont besoin de savoir où chaque commande commence et se termine. Graphene
l'apprend grâce à un petit script d'intégration qu'il charge dans votre shell au
moment de le lancer.

- **Shells pris en charge** : zsh, bash et fish, lus dans la variable d'environnement
  `SHELL`.
- **Rien à installer** : vos fichiers de configuration (`.zshrc`, `.bashrc`,
  `config.fish`...) ne sont pas modifiés, et ils sont chargés comme d'habitude. Les
  thèmes d'invite comme powerlevel10k continuent de fonctionner.
- **Seul le shell lancé par Graphene** est intégré. Un shell que vous lancez à la main
  dans Graphene (en tapant `bash`, ou `ssh` vers une autre machine) se comporte comme
  dans n'importe quel terminal : ses commandes ne sont pas découpées en blocs.
- **macOS** : le shell démarre comme shell de connexion, comme dans Terminal.app. Le
  Bash du système (3.2) a les blocs mais pas l'[éditeur de saisie](input-editor.md) ;
  zsh, le shell par défaut de macOS, est entièrement pris en charge.
- Les autres shells (nushell, xonsh...) fonctionnent normalement, sans blocs.

Pour lancer le shell sans l'intégration, définissez `GRAPHENE_SHELL_INTEGRATION=0` dans
l'environnement de Graphene :

```bash
GRAPHENE_SHELL_INTEGRATION=0 graphene
```

## Blocs

Chaque bloc est une carte qui contient l'invite et la commande que vous avez tapée, puis
sa sortie. La barre dans sa marge gauche indique son état :

| Couleur de la barre | Signification                          |
|---------------------|----------------------------------------|
| Jaune               | La commande est en cours d'exécution   |
| Verte               | Elle s'est terminée avec succès (code de sortie 0) |
| Rouge               | Elle a échoué (tout autre code de sortie) |

Pendant l'exécution de la commande, la progression qu'elle signale (voir
[Onglets et panneaux](tabs-and-panes.md#progression)) est aussi dessinée sur le bord
inférieur de sa carte.

Les programmes plein écran (`vim`, `htop`, `less`...) fonctionnent comme d'habitude ;
leur écran n'est pas découpé en blocs.

## Sélectionner un bloc

| Pour                                   | Utilisez                                            |
|----------------------------------------|-----------------------------------------------------|
| Sélectionner un bloc                   | Cliquez sur sa barre d'état                         |
| Sélectionner le bloc précédent / suivant | `Ctrl+Shift+PageUp` / `Ctrl+Shift+PageDown`       |

Le bloc sélectionné est celui sur lequel agissent les raccourcis de copie et de repli.
Sans bloc sélectionné, ils agissent sur le dernier bloc terminé.

## Copier une commande ou sa sortie

| Copier                                 | Utilisez                                            |
|----------------------------------------|-----------------------------------------------------|
| La commande du bloc                    | `Ctrl+Shift+Alt+C`, ou **Copier la commande du bloc** dans le menu du clic droit |
| La sortie du bloc                      | `Ctrl+Shift+Alt+O`, ou **Copier la sortie du bloc** dans le menu du clic droit  |

Le texte part dans le presse-papiers, nettoyé comme pour toute copie (voir
[Sélection et presse-papiers](selection-and-clipboard.md#ce-qui-est-copié)).

## Replier

Un bloc replié cache sa sortie derrière une seule ligne « n lignes masquées ».

| Pour                                   | Utilisez                                            |
|----------------------------------------|-----------------------------------------------------|
| Replier / déplier un bloc              | Cliquez sur le chevron en haut à droite de sa carte, `Ctrl+Shift+F`, ou **Replier le bloc** / **Déplier le bloc** dans le menu du clic droit |
| Déplier un bloc replié                 | Cliquez sur sa ligne « n lignes masquées »          |
| Replier tous les blocs terminés, ou tous les déplier | `Ctrl+Shift+Alt+F`                    |

Une sortie plus haute que la fenêtre est repliée automatiquement à la fin de sa
commande. Dépliez-la pour la lire.

## Défilement

Un bloc affiche au plus la hauteur d'une fenêtre ; une sortie plus longue défile à
l'intérieur de sa carte.

| Pour                                            | Utilisez                                 |
|-------------------------------------------------|------------------------------------------|
| Faire défiler le bloc sous le pointeur, ou l'historique | Molette de la souris              |
| Faire défiler le bloc sélectionné, ou l'historique | `Shift+PageUp` / `Shift+PageDown`     |
| Haut / bas de l'historique                      | `Shift+Home` / `Shift+End` (hors de l'invite) |

Les raccourcis sont modifiables : voir [Raccourcis clavier](keybindings.md).
