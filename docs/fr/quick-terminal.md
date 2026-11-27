[English](../en/quick-terminal.md)

# Quick terminal

Le quick terminal est une fenêtre de terminal attachée à un bord de l'écran, que vous
affichez et masquez avec une seule commande. Il reste résident entre deux utilisations :
son shell continue de tourner pendant qu'il est masqué, et il revient aussitôt, tel que
vous l'avez laissé.

## Activer le quick terminal

Le quick terminal est désactivé par défaut. Activez-le avec **Activer le quick
terminal**, en tête de la section **Quick terminal** de
l'[onglet des paramètres](configuration.md#onglet-des-paramètres), ou avec la clé
`enabled` de la section [`[quick_terminal]`](configuration.md#quick_terminal) :

```toml
[quick_terminal]
enabled = true
```

Tant qu'il est désactivé, les autres réglages de cette section sont grisés, avec une
note qui explique pourquoi ; ils redeviennent modifiables dès que vous l'activez.

Tant qu'il est désactivé, Graphene ne lance jamais le quick terminal : ni la première
fenêtre que vous ouvrez, ni le raccourci global, ni l'entrée de démarrage de session, et
`graphene --quick-toggle` échoue avec un message qui explique comment l'activer. Le
changement s'applique au prochain démarrage de Graphene : un quick terminal déjà lancé
reste jusqu'à la fin de votre session, mais `graphene --quick-toggle` refuse dès que le
réglage est désactivé.

## L'afficher et le masquer

```bash
graphene --quick-toggle
```

La commande affiche le quick terminal quand il est masqué et le masque quand il est
affiché. Elle exige `enabled = true` (voir
[Activer le quick terminal](#activer-le-quick-terminal)) : sinon, elle affiche comment
l'activer et sort avec le code 1. La première fois, elle lance le processus du quick terminal, qui reste ensuite
en arrière-plan jusqu'à la fin de votre session graphique. Associez la commande à une
touche dans votre gestionnaire de fenêtres ou dans les raccourcis clavier de votre
système (par exemple `F12`).

Il n'y a jamais qu'un quick terminal par utilisateur : lancer la commande plusieurs fois
en même temps n'en ouvre jamais un second.

## Raccourci global

Au lieu d'associer la commande dans votre système, vous pouvez donner au quick terminal
son propre raccourci global avec la clé `hotkey` de la section
[`[quick_terminal]`](configuration.md#quick_terminal), écrite comme un
[raccourci clavier](configuration.md#raccourcis-clavier) :

```toml
[quick_terminal]
hotkey = "Ctrl+`"
```

Le raccourci exige `enabled = true` (voir
[Activer le quick terminal](#activer-le-quick-terminal)). Il fonctionne depuis n'importe
quelle application. Tant qu'il est défini, la
première fenêtre Graphene que vous ouvrez lance aussi le quick terminal en arrière-plan,
masqué, pour que le raccourci fonctionne tout de suite, avant tout
`graphene --quick-toggle`. Définir le raccourci dans un Graphene déjà lancé, depuis le
fichier de configuration ou la fenêtre de réglages, le lance de la même façon, tout
comme activer `enabled` alors que le raccourci est défini. Le
changer remplace le précédent sans redémarrage.

Quand le raccourci ne peut pas être enregistré, parce qu'une autre application l'utilise
déjà ou que la touche n'a pas d'équivalent sur votre système, l'erreur est journalisée et
affichée dans le bandeau de configuration à la prochaine apparition du quick terminal.
Le quick terminal reste utilisable avec `graphene --quick-toggle`.

- **Linux sous X11** : la touche est reconnue par le caractère qu'elle tape dans votre
  disposition (`Alt` avec un chiffre par la touche physique de la rangée des chiffres).
- **macOS** : la touche est reconnue par sa position sur un clavier américain. `Super`
  est la touche Commande. `F21` à `F24`, `PrintScreen`, `Pause`, `ScrollLock` et les
  touches `Copy`, `Paste` et `Cut` n'ont pas d'équivalent.

## Démarrer avec la session

Avec `autostart = true` dans la section
[`[quick_terminal]`](configuration.md#quick_terminal), ou **Démarrer avec la session**
coché dans la fenêtre de réglages, le quick terminal démarre masqué à l'ouverture de
votre session, sans ouvrir de fenêtre Graphene. Graphene crée l'entrée aussitôt et la
supprime quand le réglage est désactivé, et la remet en place à chaque démarrage si elle
a été supprimée ou si l'exécutable de Graphene a été déplacé. L'entrée n'existe que tant
que le quick terminal est activé (`enabled = true`) : le désactiver supprime l'entrée,
sans changer `autostart`.

- **Linux sous X11** : `graphene-quick-terminal.desktop` dans
  `$XDG_CONFIG_HOME/autostart/` (`~/.config/autostart/` par défaut). Pour une AppImage,
  il lance le fichier AppImage.
- **macOS** : `dev.marmotz.graphene.quick-terminal.plist` dans `~/Library/LaunchAgents/`.

Le quick terminal résident ne s'atteint que par un raccourci : définissez un
[raccourci global](#raccourci-global) ou associez `graphene --quick-toggle` dans votre
système. Sous Wayland, où le quick terminal n'est pas disponible, le réglage n'a aucun
effet.

## Glissement, focus et fermeture

Le quick terminal glisse depuis son bord quand il s'affiche et y repart quand il se
masque, en 150 ms par défaut (`animation_ms`, `0` pour l'afficher et le masquer d'un
coup). Le relancer pendant un glissement le fait repartir dans l'autre sens depuis là où
il se trouve.

Quand il perd le focus au profit d'une autre application, il se masque après un court
délai (`hide_on_focus_loss`, activé par défaut).

Le fermer, par son dernier panneau, par la sortie du shell ou par le raccourci de
fermeture de la fenêtre, ne met pas fin au quick terminal : il se masque, et revient la
fois suivante avec un seul onglet neuf, son shell dans votre dossier personnel. La
confirmation demandée quand un programme tourne encore est la même que pour une fenêtre
ordinaire.

## Restauration de session

Avec [`window.restore_session`](configuration.md#window) activé, le quick terminal
sauvegarde ses onglets comme une fenêtre ordinaire, et les rouvre au prochain démarrage
de son processus, par `graphene --quick-toggle`, par le raccourci global ou à
l'ouverture de session. Chaque panneau démarre un shell neuf dans son répertoire
sauvegardé. Sa place et sa taille viennent toujours de la section
[`[quick_terminal]`](configuration.md#quick_terminal).

Le quick terminal et les fenêtres ordinaires sont restaurés séparément : démarrer le
quick terminal ne rouvre jamais une fenêtre ordinaire, et un quick terminal résident
n'empêche pas les fenêtres ordinaires d'être rouvertes au lancement suivant. Après sa
fermeture, l'onglet neuf unique avec lequel il repart est sauvegardé aussitôt : un
redémarrage ne fait pas revenir les onglets fermés.

## Où il apparaît

Le quick terminal couvre une part de l'écran contre l'un de ses bords, réglée dans la
section [`[quick_terminal]`](configuration.md#quick_terminal) de la configuration ou dans
la section **Quick terminal** de l'onglet des paramètres :

- `edge` : le bord auquel il est attaché, `top` (haut) par défaut ;
- `size` : la part de l'écran qu'il couvre, 40 % par défaut ;
- `screen` : l'écran où il apparaît, celui sous le pointeur de la souris par défaut.
  L'onglet des paramètres propose l'écran sous le pointeur, l'écran principal et les
  écrans branchés à son ouverture.

Ces réglages s'appliquent au prochain affichage. Il n'a pas de barre de titre, reste
au-dessus des autres fenêtres et s'affiche sur tous les bureaux. Il n'apparaît pas dans
la barre des tâches sous Linux, ni dans le Dock et le sélecteur d'applications sous
macOS.

## Plateformes

- **macOS** : le quick terminal apparaît sur l'espace (Space) courant, au-dessus des
  applications en plein écran, sous la barre des menus.
- **Linux sous X11** : le quick terminal couvre tout le bord de l'écran, au-dessus des
  panneaux du bureau. C'est une fenêtre de type dock : le gestionnaire de fenêtres ne
  joue pas d'effet d'ouverture dessus et le laisse glisser depuis l'extérieur de
  l'écran.
- **Linux sous Wayland** : non disponible. `graphene --quick-toggle` affiche
  `the quick terminal is not available under Wayland` et sort avec le code 1. Une
  session Wayland qui lance Graphene par X11 (avec `WAYLAND_DISPLAY` vide) utilise le
  quick terminal X11.
