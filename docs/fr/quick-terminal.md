[English](../en/quick-terminal.md)

# Quick terminal

Le quick terminal est une fenêtre de terminal attachée à un bord de l'écran, que vous
affichez et masquez avec une seule commande. Il reste résident entre deux utilisations :
son shell continue de tourner pendant qu'il est masqué, et il revient aussitôt, tel que
vous l'avez laissé.

## L'afficher et le masquer

```bash
graphene --quick-toggle
```

La commande affiche le quick terminal quand il est masqué et le masque quand il est
affiché. La première fois, elle lance le processus du quick terminal, qui reste ensuite
en arrière-plan jusqu'à la fin de votre session graphique. Associez la commande à une
touche dans votre gestionnaire de fenêtres ou dans les raccourcis clavier de votre
système (par exemple `F12`).

Il n'y a jamais qu'un quick terminal par utilisateur : lancer la commande plusieurs fois
en même temps n'en ouvre jamais un second.

## Où il apparaît

Le quick terminal couvre une part de l'écran contre l'un de ses bords, réglée dans la
section [`[quick_terminal]`](configuration.md#quick_terminal) de la configuration ou dans
la section **Quick terminal** de la fenêtre des paramètres :

- `edge` : le bord auquel il est attaché, `top` (haut) par défaut ;
- `size` : la part de l'écran qu'il couvre, 40 % par défaut ;
- `screen` : l'écran où il apparaît, celui sous le pointeur de la souris par défaut.

Ces réglages s'appliquent au prochain affichage. Il n'a pas de barre de titre, reste
au-dessus des autres fenêtres et s'affiche sur tous les bureaux. Il n'apparaît pas dans
la barre des tâches sous Linux, ni dans le Dock et le sélecteur d'applications sous
macOS.

## Plateformes

- **macOS** : le quick terminal apparaît sur l'espace (Space) courant, au-dessus des
  applications en plein écran, sous la barre des menus.
- **Linux sous X11** : le quick terminal couvre tout le bord de l'écran, au-dessus des
  panneaux du bureau.
- **Linux sous Wayland** : non disponible. `graphene --quick-toggle` affiche
  `the quick terminal is not available under Wayland` et sort avec le code 1. Une
  session Wayland qui lance Graphene par X11 (avec `WAYLAND_DISPLAY` vide) utilise le
  quick terminal X11.
