[English](../en/input-editor.md)

# Éditeur de saisie

À l'invite du shell, Graphene vous permet de modifier votre ligne de commande comme dans
un éditeur de texte : cliquer pour déplacer le curseur, sélectionner au clavier,
remplacer une sélection en tapant.

- [Quand est-il disponible](#quand-est-il-disponible)
- [Raccourcis à l'invite](#raccourcis-à-linvite)
- [Sélectionner et remplacer](#sélectionner-et-remplacer)
- [Méthodes de saisie (IME)](#méthodes-de-saisie-ime)
- [Limites](#limites)

## Quand est-il disponible

L'éditeur de saisie fonctionne à l'invite de zsh, bash ou fish lancés par Graphene avec
son [intégration shell](command-blocks.md#intégration-shell). Il est désactivé pendant
l'exécution d'une commande, dans les programmes plein écran et dans les shells sans
l'intégration : là, chaque touche part vers le programme comme dans n'importe quel
terminal.

Votre shell garde le contrôle total de la ligne de commande : la complétion (`Tab`),
l'historique (`Up` / `Down`) et vos propres raccourcis du shell fonctionnent comme
d'habitude.

## Raccourcis à l'invite

| Touches                         | Action                                        |
|---------------------------------|-----------------------------------------------|
| Clic gauche dans la ligne de commande | Y déplacer le curseur                   |
| `Ctrl+Left` / `Ctrl+Right`      | Début du mot précédent / suivant              |
| `Ctrl+Backspace` / `Ctrl+Delete` | Effacer le mot avant / après le curseur      |
| `Home` / `End`                  | Début / fin de la ligne                       |
| `Ctrl+Home` / `Ctrl+End`        | Début / fin d'une commande sur plusieurs lignes |
| `Shift+Left` / `Shift+Right`    | Étendre la sélection d'un caractère           |
| `Shift+Up` / `Shift+Down`       | Étendre la sélection d'une ligne              |
| `Shift+Home` / `Shift+End`      | Étendre la sélection jusqu'au début / à la fin de la ligne |
| `Ctrl+Shift+Left` / `Ctrl+Shift+Right` | Étendre la sélection jusqu'au début / à la fin d'un mot |
| `Ctrl+A`                        | Sélectionner toute la ligne de commande       |
| `Shift+Enter`                   | Nouvelle ligne, sans lancer la commande       |

Quand l'onglet a plusieurs panneaux, `Ctrl+Shift+Left` / `Right` déplacent plutôt le
focus entre les panneaux (voir [Onglets et panneaux](tabs-and-panes.md#panneaux)).

À l'invite, `Ctrl+A` sélectionne la ligne de commande au lieu d'aller à son début comme
le font habituellement les shells : utilisez `Home` pour cela. Ces raccourcis de
l'invite ne sont pas modifiables dans la configuration.

## Sélectionner et remplacer

Une sélection dans la ligne de commande est une sélection ordinaire : `Ctrl+Shift+C` la
copie, et sous Linux elle remplit aussi la sélection primaire (collage au clic du
milieu).

Taper, ou `Backspace`, `Delete`, `Ctrl+Backspace`, `Ctrl+Delete` ou `Shift+Enter` sur
une sélection dans la ligne de commande la remplace, comme dans un éditeur. Une
sélection qui déborde sur la sortie au-dessus de l'invite n'est pas remplacée : elle
est effacée et la touche part vers le shell comme d'habitude.

## Méthodes de saisie (IME)

Graphene saisit le texte par la méthode de saisie de votre système (fcitx5 ou IBus sous
Linux, sources de saisie sous macOS) : texte chinois, japonais et coréen, emoji,
caractères composés.

- **Dans un panneau**, le texte en cours de composition s'affiche au curseur, souligné.
  À l'invite, le texte à sa droite se décale pour lui faire de la place ; dans les
  programmes plein écran, il s'affiche par-dessus l'écran. La fenêtre de suggestions
  s'ouvre là où la composition commence.
- **Le texte validé** part vers le panneau actif, ou vers tous les panneaux pendant la
  [diffusion](tabs-and-panes.md#diffusion). À l'invite, il remplace une sélection dans
  la ligne de commande, comme une frappe. Il ne déclenche jamais de raccourci clavier.
- **Dans la palette de commandes**, le texte en cours de composition s'affiche après
  votre recherche, souligné, et ne filtre la liste qu'une fois validé.
- **Dans le champ de recherche de l'historique**, le texte validé va dans le champ.
- **Dans l'onglet des paramètres**, les champs texte utilisent aussi la méthode de
  saisie.

Déplacer le focus (un autre panneau, la palette, la barre de recherche, une autre
fenêtre) abandonne le texte en cours de composition. Pour taper sans la méthode de
saisie, désactivez **Méthode de saisie (IME)** dans la section **Clavier** de l'onglet
des paramètres, ou mettez `ime = false` dans [`[keyboard]`](configuration.md#keyboard).

## Limites

- `Ctrl+A` sélectionne aussi le texte de l'invite devant votre commande, car Graphene ne
  sait pas où l'invite se termine. Taper par-dessus n'efface que votre commande.
- Une suggestion grisée (fish, zsh-autosuggestions) ne fait pas encore partie de votre
  ligne de commande : une sélection glissée par-dessus peut effacer plus que ce qui
  était sélectionné. L'annulation de votre shell restaure la ligne.
- Sous macOS, le Bash du système (3.2) ne peut pas faire fonctionner l'éditeur de
  saisie : l'invite se comporte comme dans un terminal ordinaire. Utilisez zsh (le shell
  par défaut de macOS) ou un Bash plus récent.
- Le texte en cours de composition ne s'affiche pas dans le champ de recherche de
  l'historique, seul le texte validé y arrive. La zone de renommage d'onglet, les
  boîtes de dialogue et les menus contextuels ignorent la méthode de saisie.
- Dans la palette de commandes, le curseur reste après le texte en cours de
  composition.
