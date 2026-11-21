# Quick terminal : conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## Constats sur le code actuel

- **Une fenêtre = un processus.** `App` ne porte qu'une `session` ([src/app/application.rs:84](../../../src/app/application.rs)), créée par `create_session` ([src/app/application.rs:144](../../../src/app/application.rs)) : fenêtre invisible, GPU et premier shell en parallèle, puis `set_visible(true)`.
- **CLI** : `command` ([src/main.rs:47](../../../src/main.rs)) distingue `Version`, `PruneConfig` et `Run`. Aucune communication entre processus `graphene` aujourd'hui.
- **Fermeture** : `close(Target::Window)` et le dernier `close_pane` appellent `event_loop.exit()` ([src/app/panes.rs:144](../../../src/app/panes.rs), [src/app/panes.rs:176](../../../src/app/panes.rs)).
- **Focus** : `WindowEvent::Focused` ne sert qu'aux rapports de focus et à l'annulation d'un glisser ([src/app/application.rs:554](../../../src/app/application.rs)). La fenêtre de réglages est une seconde fenêtre winit du même processus ([src/app/application.rs:361](../../../src/app/application.rs)).
- **X11** : `x11_window::preset_maximized` écrit `_NET_WM_STATE` avant le premier affichage avec `x11rb` ([src/x11_window.rs:23](../../../src/x11_window.rs)). `x11rb` 0.13.2 est déjà une dépendance Linux.
- **Raccourcis** : `Shortcut::parse` ([src/keybindings.rs:163](../../../src/keybindings.rs)) analyse la syntaxe de `keybindings.toml` (`KeyId` + `ModifiersState`).
- **winit 0.30.13** expose `ActiveEventLoopExtWayland::is_wayland`, `EventLoopBuilderExtMacOS::with_activation_policy`, `WindowLevel::AlwaysOnTop`, `WindowAttributesExtX11::with_x11_window_type`. Rien pour un raccourci global, ni pour le comportement d'une `NSWindow` sur les Spaces.
- **Réglages** : une section verrouillée est affichée grisée avec son explication (`add_enabled_ui(model.locked(..).is_none())`, [src/settings/view/mod.rs:208](../../../src/settings/view/mod.rs), [src/settings/view/mod.rs:455](../../../src/settings/view/mod.rs)).

## 1. Plateformes

- **Pris en charge** : macOS et Linux sous X11.
- **Wayland** (décision produit) : non pris en charge. La détection fiable est `is_wayland()` sur la boucle d'événements, donc dans le processus quick lui-même (section 3). Une session Wayland qui force X11 (`WAYLAND_DISPLAY` vide, XWayland) passe par X11 et fonctionne.
- **Alternatives écartées** : XWayland forcé (flou en mise à l'échelle fractionnaire), surface layer-shell via smithay-client-toolkit (deuxième chemin de fenêtre hors winit, absent de GNOME), fenêtre Wayland simple (ni bord ni animation garantis).

## 2. Configuration `[quick_terminal]`

Nouvelle `QuickTerminalConfig` dans [src/config/values.rs](../../../src/config/values.rs), lue dans [src/config/reader.rs](../../../src/config/reader.rs) comme `WindowConfig`.

| Clé | Défaut | Valeurs |
|-----|--------|---------|
| `hotkey` | `""` | Raccourci global natif, syntaxe de `keybindings.toml`. Vide : aucun. |
| `edge` | `"top"` | `"top"`, `"bottom"`, `"left"`, `"right"`. |
| `size` | `40` | Pourcentage de l'écran (10 à 100) : hauteur pour `top`/`bottom`, largeur pour `left`/`right`. L'autre dimension prend tout l'écran. |
| `screen` | `"active"` | `"active"` (écran sous le curseur), `"primary"`, ou le nom d'un écran (`MonitorHandle::name`). Nom absent : repli sur `"active"`. |
| `animation_ms` | `150` | Durée du glissement, `0` pour instantané, plafonnée à 1000. |
| `hide_on_focus_loss` | `true` | Masquage quand la fenêtre perd le focus. |

- Rechargement à chaud : `edge`, `size`, `screen` et `animation_ms` s'appliquent au prochain affichage, `hotkey` est ré-enregistré tout de suite (section 5).
- Fenêtre de réglages : nouvelle section `QuickTerminal` ([src/settings/model.rs:131](../../../src/settings/model.rs)). Sous Wayland, elle est grisée avec le message « Le quick terminal n'est pas disponible sous Wayland », sur le modèle de `locked`.
- « Écran actif » = écran sous le curseur, pas l'écran de la fenêtre focalisée : un processus n'a pas accès à la fenêtre active d'une autre application sous macOS, et le curseur est lisible partout (X11 `QueryPointer` sur la racine, macOS `NSEvent::mouseLocation`).

## 3. Processus et commande

### Processus quick

`graphene --quick [--hidden]` (option interne, absente de l'aide) lance une instance `App` en mode `WindowMode::Quick`. Il y en a au plus une par utilisateur.

- **Instance unique** : `flock(LOCK_EX | LOCK_NB)` sur `<runtime>/quick.lock`. Verrou pris par un autre : le processus envoie `toggle` à celui-ci (sauf avec `--hidden`) puis sort. Deux déclenchements simultanés n'ouvrent donc jamais deux quick terminals.
- **Répertoire runtime** : `$XDG_RUNTIME_DIR/graphene/` sous Linux, `$TMPDIR/graphene-<uid>/` sous macOS, en `0700`. Même emplacement que le socket du [multiplexage](../multiplexing/technical.md), via une fonction commune `config::runtime_dir()` ([src/config/paths.rs](../../../src/config/paths.rs)).
- **Socket de commande** : `UnixListener` sur `<runtime>/quick.sock`, dans un thread. Une connexion = une ligne `toggle`, `show` ou `hide`, transmise par `UserEvent::Quick(QuickCommand)` ([src/terminal.rs:32](../../../src/terminal.rs)). Seul `toggle` est utilisé par la commande publique, `show` sert au démarrage. La protection est le `0700` du répertoire.
- **Wayland** : si `is_wayland()` au `resumed`, le processus sort avec le code `3` sans rien créer.
- **macOS** : `with_activation_policy(ActivationPolicy::Accessory)` : pas d'icône dans le Dock ni dans le sélecteur d'applications.
- **Fenêtre préchauffée** : la session est créée au démarrage, invisible (même avec `--hidden`), shell compris. La première apparition est ainsi immédiate. Le coût est un shell en attente.
- **Durée de vie** : le processus reste résident jusqu'à la fin de la session graphique (fermeture de la connexion X11, ou de la session macOS). Il n'a pas d'autre moyen d'arrêt.

### Commande publique

`graphene --quick-toggle` ([src/main.rs](../../../src/main.rs), `Command::QuickToggle`) :

1. Se connecte à `quick.sock` et envoie `toggle`. Succès : sort avec `0`.
2. Sinon, lance `current_exe() --quick` (`setsid`, stdio vers `/dev/null`) qui démarre visible.
3. Attend jusqu'à 5 s que le socket réponde ou que l'enfant sorte. Code `3` : affiche « graphene: le quick terminal n'est pas disponible sous Wayland » et sort avec `1`. Autre échec : message générique, code `1`.

L'utilisateur lie `graphene --quick-toggle` dans son gestionnaire de fenêtres ou dans les raccourcis système.

**Alternative écartée** : D-Bus (`zbus` est déjà là), mais inexistant sous macOS. Le socket Unix couvre les deux plateformes avec le même code.

### Démarrage caché pour le raccourci natif

Si `quick_terminal.hotkey` n'est pas vide, une fenêtre normale lance `current_exe() --quick --hidden` après son premier affichage, sur plateforme prise en charge, si `quick.sock` ne répond pas. Elle le refait si un rechargement de la configuration renseigne `hotkey`. Le verrou rend ce lancement sans risque quand plusieurs fenêtres le font en même temps.

## 4. Fenêtre quick

### Attributs

- `with_decorations(false)`, `with_window_level(WindowLevel::AlwaysOnTop)`, `with_visible(false)`, taille et position calculées (section 6).
- **X11** : avant le premier affichage, `_NET_WM_STATE` = `SKIP_TASKBAR`, `SKIP_PAGER`, `ABOVE`, `STICKY` (visible sur tous les bureaux). `x11_window::preset_maximized` est généralisé en `preset_states(window, &[atoms])`, réutilisé pour `MAXIMIZED_*`.
- **macOS** : `setCollectionBehavior(CanJoinAllSpaces | FullScreenAuxiliary)` sur la `NSWindow` (lue par `window_handle()`), pour apparaître sur l'espace courant et au-dessus d'une application plein écran. `objc2-app-kit` 0.3.2, déjà transitive par winit, devient une dépendance directe macOS.

### Afficher et masquer

Nouvel état `QuickState { shown, slide: Option<Slide>, blur_deadline: Option<Instant> }` dans `App` ([src/app/application.rs:84](../../../src/app/application.rs)).

- **Afficher** : choix de l'écran, calcul du cadre, positionnement hors écran au bord choisi, `set_visible(true)`, `focus_window()`, puis glissement jusqu'au cadre. Avec `animation_ms = 0`, positionnement direct.
- **Masquer** : glissement inverse puis `set_visible(false)`. Sur macOS, `hide_application()` en plus, pour rendre le focus à l'application précédente.
- **Glissement** : à chaque `about_to_wait` ([src/app/application.rs:429](../../../src/app/application.rs)), `set_outer_position` selon `ease_out_cubic(t)`, avec `ControlFlow::WaitUntil` sur la frame suivante (16 ms). On déplace la fenêtre sans la redimensionner, pour éviter de reformater la grille à chaque frame. Un `toggle` reçu pendant un glissement l'inverse depuis la position courante.
- **Limite assumée** : sur un bord partagé avec un autre écran (par exemple `left` avec un écran à gauche), le glissement est visible sur l'écran voisin pendant l'animation.
- **Changement d'écran ou de taille** entre deux apparitions : `request_inner_size` avant l'affichage, puis la grille se met en page au `Resized` habituel.

### Perte de focus

Si `hide_on_focus_loss` est activé, `Focused(false)` arme `blur_deadline = now + 150 ms`, et le masquage se fait à l'échéance. L'échéance est annulée si la fenêtre quick reprend le focus, ou si la fenêtre de réglages du même processus le prend. Ce délai absorbe aussi les pertes de focus transitoires au moment de l'affichage.

### Fermeture

En mode quick, `close(Target::Window)`, `CloseRequested` et la fermeture du dernier pane ([src/app/panes.rs:144](../../../src/app/panes.rs), [src/app/panes.rs:176](../../../src/app/panes.rs)) ne quittent plus : la fenêtre est masquée, puis le workspace est remplacé par un onglet neuf (shell dans le dossier personnel). Le dialogue `ConfirmClose` reste inchangé. `event_loop.exit()` ne sert plus qu'en mode normal.

## 5. Raccourci global natif

Code maison, sans nouvelle dépendance. Une erreur d'enregistrement (raccourci déjà pris, touche inconnue) est journalisée et affichée dans le bandeau de configuration ([src/banner.rs](../../../src/banner.rs)) à la prochaine apparition. Elle ne bloque pas le processus.

- **Syntaxe** : `Shortcut::parse` ([src/keybindings.rs:163](../../../src/keybindings.rs)). Le module `src/hotkey/` traduit `Shortcut` en code de touche natif (X11 : keysym via la table de `NAMED_KEYS` et les caractères Latin-1, puis keycode par `GetKeyboardMapping` ; macOS : table des codes virtuels `kVK_*`). Une touche sans équivalent est une erreur.
- **X11** (`src/hotkey/x11.rs`) : connexion `x11rb` dédiée dans un thread, `GrabKey` sur la racine pour le keycode avec les modificateurs, décliné avec et sans `Lock` et `Mod2` (Verr. Maj et Verr. Num). Chaque `KeyPress` envoie `UserEvent::Quick(Toggle)`. `BadAccess` indique un raccourci déjà pris. Au rechargement, `UngrabKey` puis nouveau `GrabKey`.
- **macOS** (`src/hotkey/macos.rs`) : `RegisterEventHotKey` et `InstallEventHandler` de Carbon par FFI (`#[link(name = "Carbon", kind = "framework")]`), sur le thread principal au `resumed`. La boucle AppKit de winit distribue les événements Carbon. Le gestionnaire envoie le même `UserEvent`.
- **Alternative écartée** : la crate `global-hotkey`, une dépendance de plus pour deux implémentations courtes.

## 6. Géométrie

Fonctions pures dans `src/quick_terminal.rs`, testables sans fenêtre :

- `pick_monitor(screen, monitors, primary, cursor) -> Option<MonitorRect>`.
- `frame(monitor: Rect, edge, size_percent) -> Rect` : cadre en pixels physiques, collé au bord, l'autre dimension prenant tout l'écran.
- `hidden_origin(frame, edge) -> Position` : point de départ du glissement, juste hors du bord.
- `slide_position(from, to, elapsed, duration) -> Position`.

- **Linux** : le rectangle vient de `MonitorHandle::position()`/`size()`, la surface entière de l'écran. Les panneaux du bureau ne sont pas exclus : `_NET_WORKAREA` est global à tous les écrans, donc faux en multi-écran. La fenêtre `ABOVE` passe par-dessus les panneaux.
- **macOS** : le rectangle est réduit par `NSScreen::visibleFrame` de l'écran choisi, pour que le cadre `top` démarre sous la barre de menus et qu'un cadre `bottom` ne passe pas sous le Dock.

## 7. Restauration de session

Branchement sur [session-restore](../session-restore/technical.md), qui réserve déjà `WindowKind::Quick` et la sélection par type :

- Le processus quick écrit son snapshot avec `kind: Quick`, aux mêmes conditions qu'une fenêtre normale (réglage `window.restore_session`). `--quick` n'est pas un lancement exclu.
- Au démarrage, `--quick` (avec ou sans `--hidden`) n'adopte que le snapshot `Quick`. Il ne lance aucun processus pour les snapshots `Normal`.
- **Instance vivante par type** : le test « une instance vivante existe » de session-restore (§3 et §5, étape 2) se fait par type. Un processus quick résident n'empêche donc pas la restauration des fenêtres normales, et inversement. C'est couvert par [#214](https://github.com/marmotz/graphene-term/issues/214).
- La réinitialisation après fermeture (section 4) écrit un snapshot à un seul onglet : un redémarrage ne fait pas revenir les onglets fermés.

## Conséquences vérifiées

- Mode normal inchangé : sans `--quick`, `WindowMode::Normal` garde les chemins actuels, y compris `event_loop.exit()`.
- `hotkey` vide (défaut) : aucun processus lancé en arrière-plan, aucun raccourci enregistré.
- Sous Wayland : `--quick-toggle` échoue proprement, aucune fenêtre ne s'ouvre, la section des réglages est grisée.
- `x11_window::preset_maximized` devient un cas de `preset_states`. Son test actuel ([src/x11_window.rs:61](../../../src/x11_window.rs)) est adapté.
- Documentation : nouvelle page `docs/en/quick-terminal.md` et sa version française, ligne dans le README (EN/FR), tableau `[quick_terminal]` dans `docs/en/configuration.md` et `docs/fr/…`.

## Tests

- `main.rs` : analyse de `--quick-toggle`, `--quick`, `--quick --hidden`.
- `quick_terminal.rs` : `pick_monitor` (actif, principal, nommé, nom absent), `frame` pour les quatre bords et des tailles limites, `hidden_origin`, `slide_position` (début, fin, inversion en cours de route).
- Configuration : valeurs par défaut, bornes de `size` et `animation_ms`, `edge` et `screen` invalides signalés comme les autres clés.
- Socket : un serveur de test reçoit `toggle` ; la commande lance l'enfant quand le socket est absent ; code `3` traduit en message Wayland.
- Instance unique : un second `--quick` avec le verrou pris envoie `toggle` et sort.
- Perte de focus : échéance armée, annulée par le retour du focus ou par la fenêtre de réglages (logique isolée de winit).
- Fermeture en mode quick : le dernier pane fermé masque et réinitialise au lieu de quitter (faux panes du workspace).
- Raccourci : traduction `Shortcut` vers keysym X11 et code virtuel macOS, touche sans équivalent refusée. `GrabKey` testé sur un serveur X11 s'il y en a un, sinon ignoré, comme [src/x11_window.rs:62](../../../src/x11_window.rs).
- `preset_states` écrit les quatre états quick.

## Découpage en tâches d'implémentation

1. [#229](https://github.com/marmotz/graphene-term/issues/229) : configuration `[quick_terminal]` et section des réglages, grisée sous Wayland (§1, §2). Aucune dépendance.
2. [#230](https://github.com/marmotz/graphene-term/issues/230) : processus quick, commande `--quick-toggle`, attributs et placement de la fenêtre, sans animation (§3, §4 « Attributs », §6). Dépend de #229.
3. [#231](https://github.com/marmotz/graphene-term/issues/231) : glissement, masquage à la perte de focus, réinitialisation à la fermeture (§4). Dépend de #230.
4. [#232](https://github.com/marmotz/graphene-term/issues/232) : raccourci global natif et démarrage caché (§3, §5). Dépend de #229 et #230.
5. [#233](https://github.com/marmotz/graphene-term/issues/233) : restauration du quick terminal avec la session (§7). Dépend de #230, #231 et [#214](https://github.com/marmotz/graphene-term/issues/214) (session-restore).
