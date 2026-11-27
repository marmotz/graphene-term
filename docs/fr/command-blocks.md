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
- [Rechercher dans l'historique](#rechercher-dans-lhistorique)

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

Le bloc en cours, le dernier du pane (la commande qui tourne ou l'invite en attente de
saisie), se sélectionne aussi : `Ctrl+Shift+PageDown` y mène depuis le dernier bloc
terminé, et le sélectionne directement quand aucun bloc ne l'est. Sur le bloc en cours,
`Ctrl+Shift+PageDown` ne fait plus rien. Pendant que sa commande tourne, sa commande se
copie, mais sa sortie seulement une fois la commande terminée. À l'invite, il n'a rien à
copier.

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
| Replier / déplier un bloc              | Cliquez sur le chevron en haut à droite de sa carte, `Ctrl+Shift+Z`, ou **Replier le bloc** / **Déplier le bloc** dans le menu du clic droit |
| Déplier un bloc replié                 | Cliquez sur sa ligne « n lignes masquées »          |
| Replier tous les blocs terminés, ou tous les déplier | `Ctrl+Shift+Alt+Z`                    |

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

## Rechercher dans l'historique

`Ctrl+Shift+F` ouvre une barre de recherche en haut à droite du pane actif. Elle
cherche dans tout l'historique du pane pendant la frappe et surligne chaque occurrence ;
l'occurrence courante ressort dans une autre couleur (deux [couleurs de thème](themes.md),
**Occurrences de la recherche** et **Occurrence courante de la recherche**).

| Pour                                   | Utilisez                                            |
|----------------------------------------|-----------------------------------------------------|
| Ouvrir la barre de recherche, ou lui redonner le clavier | `Ctrl+Shift+F`, ou **Rechercher dans l'historique** dans la palette de commandes |
| Aller à l'occurrence plus ancienne / plus récente | `Enter` / `Shift+Enter`, ou les boutons `↑` / `↓` |
| Fermer la barre                        | `Escape`, ou le bouton `×`                           |
| Rendre le clavier au shell, barre ouverte | Cliquez dans le contenu du pane                  |
| Redonner le clavier à la barre         | Cliquez sur la barre, ou `Ctrl+Shift+F`             |

**Correspondance.** La recherche ignore la casse par défaut. Trois bascules à côté du
champ la modifient :

| Bascule | Activée                                                        |
|---------|----------------------------------------------------------------|
| `Aa`    | Majuscules et minuscules sont distinguées                      |
| `.*`    | Le texte est une expression régulière (`Regex invalide` s'affiche tant qu'elle n'est pas valide) |
| `ab`    | Seuls les mots entiers correspondent                           |

Une occurrence peut s'étendre sur une ligne que le terminal a renvoyée à la ligne.

**Filtres.** Deux sélecteurs gardent une partie de l'historique ; cliquez sur l'un pour
passer à sa valeur suivante :

| Sélecteur | Valeurs                                                      |
|-----------|--------------------------------------------------------------|
| Zone      | **Tout**, **Commandes** (le texte de la commande, sans l'invite), **Sorties** |
| Statut    | **Tous**, **Succès**, **Échecs** (les blocs encore en cours sont exclus des deux derniers) |

Ils sont grisés sans [intégration shell](#intégration-shell) : la recherche porte alors
sur tout l'historique, qui n'est pas découpé en blocs.

**Compteur.** La barre affiche l'occurrence courante et le nombre d'occurrences, par
exemple `2/7`. Les occurrences sont comptées depuis le bas : `1` est la plus récente.
`Enter` remonte vers les occurrences plus anciennes et repart du bas après la plus
ancienne. Au-delà de 10 000 occurrences, le compteur affiche `10000+`.

**Aller à une occurrence.** À l'ouverture de la barre, l'occurrence courante est la plus
récente au-dessus du bas de la vue. Aller à une occurrence fait défiler l'historique, ou
la sortie de son bloc, pour la montrer ; un bloc replié se déplie quand une occurrence
de sa sortie devient la courante. La sortie ajoutée pendant que la barre est ouverte
rejoint les occurrences sans déplacer la vue. `Escape` laisse la vue sur la dernière
occurrence montrée.

**Le champ.** `Left` / `Right`, `Home` / `End`, `Backspace`, `Delete` et
`Ctrl+Backspace` (effacer un mot) modifient le texte ; `Ctrl+Shift+V` colle, les
retours à la ligne devenant des espaces. Tant que la barre a le clavier, rien de ce que
vous tapez ne part au shell, mais les autres raccourcis de Graphene marchent toujours.
Un pane étroit masque d'abord les sélecteurs, puis les bascules.

**Non cherchés :** la ligne de commande en cours de saisie, et l'écran d'un programme
plein écran (`vim`, `less`, `htop`...). Pendant qu'un tel programme tourne,
`Ctrl+Shift+F` ne fait rien, et une barre ouverte se ferme.

Les raccourcis sont modifiables : voir [Raccourcis clavier](keybindings.md).
