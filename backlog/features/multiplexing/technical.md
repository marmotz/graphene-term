# Multiplexage : conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## Existant

- **Un processus par fenêtre.** `App` ne porte qu'une `session` ([src/app/application.rs:84](../../../src/app/application.rs#L84)). Relancer `graphene` ouvre un autre processus.
- **Un pane = un `Terminal`** ([src/terminal.rs:125](../../../src/terminal.rs#L125)) : `term`, `timeline` et `images` sont des `Arc<Mutex<…>>` que le rendu lit directement. `Terminal::spawn` ([src/terminal.rs:142](../../../src/terminal.rs#L142)) crée le PTY avec `alacritty_terminal::tty::new` et lance le thread `PtyLoop` ([src/pty.rs:191](../../../src/pty.rs#L191)).
- **Le parsing est une fonction pure** : `pty::handle_output` ([src/pty.rs:105](../../../src/pty.rs#L105)) applique des octets à `Term`, `Timeline` et `ImageStore`. Rejouer le même flux redonne le même état.
- **Le shell possède la ligne de commande** ([src/input_editor.rs:1](../../../src/input_editor.rs#L1)) : la fenêtre ne garde aucun texte tapé qui serait perdu.
- **Blocs** : `Timeline` garde des `Term` figés par bloc. Elle évince les plus anciens, entiers, au-delà de `scrollback.lines` ([src/timeline.rs:418](../../../src/timeline.rs#L418)).
- **Fermeture** : `CloseRequested` ([src/app/application.rs:542](../../../src/app/application.rs#L542)) appelle `request_close` ([src/app/panes.rs:120](../../../src/app/panes.rs#L120)). Celui-ci ouvre `ConfirmClose` ([src/chrome/dialog.rs:74](../../../src/chrome/dialog.rs#L74)) si un pane a un programme au premier plan. Les réponses actuelles sont `Close` et `Cancel` ([src/chrome/dialog.rs:105](../../../src/chrome/dialog.rs#L105)).
- **Programme au premier plan** : il est lu dans `/proc` ([src/process.rs:28](../../../src/process.rs#L28)), donc sous Linux seulement. Sur macOS, le dialogue n'apparaît jamais aujourd'hui.
- **Effets de bord des événements du terminal** ([src/app/terminal_events.rs:68](../../../src/app/terminal_events.rs#L68)) : `PtyWrite` (réponses DA/DSR), `ClipboardStore` (OSC 52), `Bell`.
- **Disponible** : `zbus` (D-Bus, déjà utilisé par [src/appearance.rs](../../../src/appearance.rs)), `libc`, `serde_json`. `objc2` et `objc2-app-kit` sont déjà des dépendances transitives de winit sur macOS.

## Architecture retenue

Un **serveur minimal** garde les PTY et le flux brut de leurs sorties. La fenêtre reste le seul endroit où le flux est parsé, et elle **rejoue** ce flux au rattachement.

```
graphene (fenêtre A) ─┐                    ┌─ PTY pane 1 ─ shell
graphene (fenêtre B) ─┼── socket Unix ── graphene --mux ─┼─ PTY pane 2 ─ vim
icône / notification ◄┘                    └─ journal d'octets par pane
```

- **Processus** : `graphene --mux` est une sous-commande cachée de [src/main.rs](../../../src/main.rs), sans GPU ni winit. Il y en a un par utilisateur.
- **Socket** : `$XDG_RUNTIME_DIR/graphene/mux.sock` sous Linux, `$TMPDIR/graphene-<uid>/mux.sock` sous macOS. Le répertoire est en `0700`, et le serveur vérifie l'uid du pair (`SO_PEERCRED` / `getpeereid`).
- **Démarrage** : la fenêtre tente de se connecter. En cas d'échec, elle lance `current_exe() --mux` détaché (double fork + `setsid`, stdio vers `/dev/null`) puis réessaie jusqu'à 1 s. Deux fenêtres qui démarrent en même temps : un verrou `flock` sur `mux.lock` garantit un seul serveur.
- **Toutes les fenêtres passent par le serveur**, `-e` compris (décision produit). C'est la condition pour qu'un plantage préserve toujours les programmes : les PTY ne vivent jamais dans la fenêtre.
- **Rien à configurer** : pas de nom de serveur ni de domaine. Le socket est un détail interne.

### Seam côté fenêtre

`Terminal` garde son API publique (`term`, `timeline`, `images`, `write`, `resize`, `current_dir`, `foreground_program`). Seul son backend change :

- `Backend::Remote` : un thread lit le socket, applique `handle_output` aux octets reçus et envoie `Wakeup` comme `PtyLoop` aujourd'hui. `write` et `resize` deviennent des messages vers le serveur.
- `Backend::Local` : le `PtyLoop` actuel, inchangé. C'est le repli (voir plus bas), et il reste utilisé par les tests existants.

La boucle de lecture de `PtyLoop::read` ([src/pty.rs](../../../src/pty.rs)) est factorisée pour accepter n'importe quelle source d'octets : `Read` du PTY ou trames du socket.

## Serveur

### PTY

- Le serveur crée ses PTY lui-même avec `libc::openpty` + `fork`/`setsid`/`TIOCSCTTY`/`execve`, sans passer par `alacritty_terminal::tty`. Une `Pty` d'alacritty ne peut pas être reconstruite à partir d'un fd brut, or la passation entre versions en a besoin (voir plus bas).
- La fenêtre envoie `Spawn { program, args, env, cwd, size }`. `shell_integration::prepare` et le jeton de session ([src/terminal.rs](../../../src/terminal.rs)) restent calculés côté fenêtre.
- Le premier plan est lu avec `tcgetpgrp(master)` sur le fd maître, sans `/proc`. `foreground_program` fonctionne donc aussi sur macOS (nom du processus et répertoire courant via libproc, `proc_name`/`proc_pidinfo` : leur exposition par le crate `libc` reste à confirmer à l'implémentation). Le répertoire courant reste lu dans `/proc` sous Linux.
- Le serveur récupère les fins de processus (`SIGCHLD` + `waitpid`), ferme le pane et le notifie aux fenêtres rattachées.

### Journal par pane

- Le journal est un tampon mémoire en anneau d'octets bruts. Il est compté en **offsets absolus** depuis la création du pane.
- **Élagage piloté par la fenêtre** : quand `Timeline::evict` retire un bloc, la fenêtre envoie `Trim { pane, offset }`, où `offset` est l'offset absolu du signal qui a ouvert le plus ancien bloc restant. À ajouter : le lecteur compte les octets reçus, et `Current`/`Block` retiennent l'offset de leur signal d'ouverture. Rejouer à partir de cet offset redonne exactement les blocs gardés.
- **Plafond dur de 64 Mio par pane**, attaché ou non (décision produit). Il couvre les panes sans intégration shell, qui n'ont pas de point de coupe, et les fenêtres détachées. Au-delà, le serveur coupe au premier `\n` après l'excédent et marque le pane comme `truncated`.

### Fenêtres

- Le serveur regroupe les panes en **fenêtres serveur** (`WindowId`). Chaque fenêtre serveur a au plus un client rattaché, et elle porte un **blob de disposition opaque** (JSON) écrit par la fenêtre : onglets, arbre et ratios, onglet actif, titres renommés, blocs pliés, taille de la fenêtre.
- La fenêtre renvoie `SetLayout` à chaque changement. Le serveur ne l'interprète pas.
- **Détachement** : sur `Detach` ou sur une coupure du socket (plantage), la fenêtre serveur passe à l'état `detached`. Les PTY gardent leur dernière taille.

### Durée de vie

- Le serveur s'arrête quand il ne reste aucun pane.
- **Déconnexion** : le serveur garde une connexion au bus de session D-Bus sous Linux, et s'arrête quand celle-ci se ferme. Le bus de session dure tant que l'utilisateur a une session ouverte. Il envoie alors `SIGHUP` aux groupes de processus des shells, comme une fermeture de terminal. Sous macOS, la fermeture de session termine les processus de la session Aqua.
- **Tout arrêter** : `SIGHUP` à chaque pane, fermeture des maîtres, puis arrêt du serveur.

## Protocole

- **Trames** : `[u32 longueur][u8 type][charge]`.
  - Messages de contrôle en JSON (`serde`, champs inconnus ignorés, `#[serde(default)]`).
  - `Output`/`Input` en octets bruts, pour ne pas encoder le flux.
- **Messages client vers serveur** : `Hello`, `ListDetached`, `CreateWindow`, `Attach { window }`, `Spawn`, `Input`, `Resize`, `Trim`, `SetLayout`, `Detach`, `CloseWindow` (tout arrêter), `ClosePane`, `Upgrade { exe }`.
- **Messages serveur vers client** : `Hello`, `Windows`, `Replay { pane, from_offset, bytes, truncated }`, `Output`, `PaneExited`, `Foreground` (réponse à une requête), `Cwd`.
- **Rattachement** : pour chaque pane, le serveur envoie `Replay` (le journal en entier), puis bascule en `Output` sans trou. Le flux du pane est suspendu pendant l'envoi.

## Fenêtre : rejeu

- Le rejeu passe par le même `handle_output` dans un `Term`, une `Timeline` et un `ImageStore` neufs, avec un `EventListener` de rejeu :
  - Il **ignore** `PtyWrite`, `ClipboardStore`, `ClipboardLoad`, `Bell`, `ColorRequest` et `TextAreaSizeRequest`. Ces requêtes ont déjà reçu leur réponse à l'époque.
  - Il **garde** `Title` (seul le dernier compte) et `CursorBlinkingChange`.
  - Les réponses graphiques kitty (`take_replies`) sont jetées de la même façon.
- Si `truncated` est vrai, le rejeu commence par un RIS (`ESC c`) pour partir d'un état propre.
- Après le rejeu, la fenêtre envoie `Resize` à sa taille réelle. Le serveur envoie en plus `SIGWINCH` au groupe au premier plan, même si la taille n'a pas changé : vim, htop ou less se redessinent même si leur écran a été tronqué.
- **Ordre de grandeur** : le rejeu de 64 Mio est borné par le débit du parseur. Le bench [benches/render.rs](../../../benches/render.rs) servira à le mesurer, et l'objectif est sous la seconde. Le blob de disposition est appliqué d'abord, pour que la fenêtre ait sa forme finale pendant le rejeu des panes.
- **Images kitty `t=t` et `t=s`** (fichier temporaire, mémoire partagée), qu'utilise `icat` en local : la source a disparu au moment du rejeu. Quand la fenêtre parse une telle transmission, elle envoie `Patch { pane, range, bytes }` pour que le serveur remplace la séquence dans le journal par son équivalent `t=d` (données en ligne). Une image transmise pendant que la fenêtre est détachée reste perdue au rejeu.

## Fermeture

- Nouvelle clé `window.on_close = "ask" | "keep" | "stop"`, `"ask"` par défaut, dans `WindowConfig` ([src/config/values.rs:114](../../../src/config/values.rs#L114)). Elle est exposée dans la fenêtre de réglages et documentée dans [docs/en/configuration.md](../../../docs/en/configuration.md) et sa version française.
- `request_close(Target::Window)` :
  - Si aucun pane n'a de programme au premier plan, la fenêtre envoie `CloseWindow` et quitte, quel que soit `on_close`.
  - Sinon, avec `ask`, `ConfirmClose` gagne les boutons "Garder en arrière-plan", "Tout arrêter" et "Annuler", plus une case "Ne plus demander". L'enum `Answer` devient `Keep | Stop | Cancel`.
  - Si la case est cochée, le choix est écrit dans `config.toml` via `config_edit` (`toml_edit`).
  - Avec `keep` ou `stop`, la fenêtre agit sans dialogue.
- Fermer un pane ou un onglet ne change pas : il n'y a pas d'option "garder", et `ClosePane` arrête son programme.
- **Plantage** : il n'y a pas de dialogue. Le socket se ferme et le serveur détache la fenêtre. Ce comportement ne dépend pas de `on_close` (décision produit).

## Lancement et rattachement

1. `Hello`, puis passation si les versions diffèrent (voir plus bas).
2. Sans `-e`, la fenêtre envoie `ListDetached` :
   - S'il y a des fenêtres détachées, ce processus rattache la plus récente et lance `current_exe() --attach <window>` pour chacune des autres. Chaque fenêtre rouvre ainsi la sienne (décision produit). `--attach` est une option interne de [src/main.rs](../../../src/main.rs).
   - Sinon, `CreateWindow` puis `Spawn` du premier shell.
   - S'il y a des fenêtres détachées, la restauration de session ne s'applique pas : le multiplexage est prioritaire (voir [overview.md](overview.md#features-liées)).
3. Avec `-e`, la fenêtre fait toujours `CreateWindow`, sans rien rattacher.
4. Au rattachement, la disposition est appliquée à partir du blob. Un pane du blob que le serveur ne connaît plus (programme terminé entre-temps) est retiré de l'arbre. La taille de la fenêtre est celle du blob.

Les `PaneId` de la fenêtre restent locaux ([src/workspace/mod.rs](../../../src/workspace/mod.rs) `allocate_id`). La fenêtre garde une table `PaneId → ServerPaneId`, et le blob ne contient que des `ServerPaneId`.

## Mises à jour : passation au nouveau binaire

- `Hello` contient `version` et `handoff_format` (entier). **`Hello` et `Upgrade` sont figés à vie.**
- Si le serveur est plus ancien que la fenêtre, celle-ci envoie `Upgrade { exe: current_exe() }`. Le serveur :
  1. suspend la lecture des PTY ;
  2. écrit son état dans un `memfd` (Linux) ou un fichier temporaire `0600` (macOS) : panes, fd maîtres, pid, offsets, journaux, fenêtres, blobs, `handoff_format` ;
  3. retire `FD_CLOEXEC` des fd maîtres et de l'état ;
  4. fait `execve(exe, ["graphene", "--mux", "--resume", <fd>])`.

  Le pid ne change pas, donc les shells restent ses enfants et `waitpid` continue de marcher. Les clients se reconnectent sur le socket, que le verrou garde au même chemin.
- **Compatibilité** : le nouveau binaire sait lire tous les `handoff_format` antérieurs. Les tests figent un exemple de chaque format. Le protocole entre une fenêtre et un serveur de la même version peut donc évoluer librement.
- **Serveur plus récent que la fenêtre** (retour à une version antérieure, rare) : la fenêtre ne fait pas de passation. Elle passe en mode local avec le bandeau de repli.

## Icône et notification

L'icône est portée par le serveur, qui est le seul processus vivant quand aucune fenêtre n'est ouverte. Elle n'apparaît que s'il existe au moins une fenêtre détachée. Son menu propose "Rouvrir" (lance `graphene`, qui rattache tout) et "Tout arrêter".

- **Linux** : `StatusNotifierItem` exposé avec `zbus`, sans GTK. L'hôte est détecté avec la propriété `IsStatusNotifierHostRegistered` de `org.kde.StatusNotifierWatcher`.
- **Linux sans hôte** (GNOME sans extension) : au détachement, une notification `org.freedesktop.Notifications` est envoyée avec l'action "Rouvrir". Le serveur écoute `ActionInvoked`.
- **macOS** : `NSStatusItem` via `objc2-app-kit`. Le serveur passe en `NSApplicationActivationPolicyAccessory` (pas d'icône dans le Dock). La boucle AppKit tourne sur le thread principal et les E/S sur un autre thread.
- Le code de l'icône tourne dans son propre thread sous `catch_unwind` (sauf sur macOS, où il est sur le thread principal). Une erreur d'icône ne doit jamais tuer le serveur ni les sessions.

## Repli local

Si le serveur reste injoignable après 1 s (pas de répertoire runtime, socket refusé, version plus récente), la fenêtre utilise `Backend::Local`. Elle affiche un bandeau ([src/banner.rs](../../../src/banner.rs)) : "Les programmes de cette fenêtre s'arrêteront à sa fermeture". Le dialogue de fermeture garde alors son comportement actuel.

## Alternatives écartées

- **Serveur qui parse et envoie un instantané** : `Term` d'alacritty n'est pas sérialisable. Seuls `Grid<Cell>` et quelques types le sont, avec la feature `serde` ; les modes, les jeux de caractères, les tabulations et les couleurs OSC 4 sont des champs privés. Il faudrait en plus sérialiser la `Timeline` et l'`ImageStore`, puis maintenir ce format entre versions. Le rejeu réutilise le code existant sans le modifier.
- **Passer le fd maître à la fenêtre (`SCM_RIGHTS`) pendant le rattachement** : le serveur ne verrait plus le flux et ne pourrait plus tenir le journal, à moins que la fenêtre le lui renvoie (double copie). Le relais par socket coûte une copie en mémoire, négligeable face au parsing.
- **Démarrer le serveur seulement à la fermeture** : un plantage tuerait les programmes, contraire à la décision produit.
- **Protocole compatible à vie** : chaque évolution du protocole devrait rester compatible avec tous les anciens serveurs. La passation limite le contrat figé à `Hello`, `Upgrade` et la lecture des anciens formats d'état.
- **`tray-icon` / libappindicator** : la dépendance GTK est lourde, et GNOME demande quand même l'extension.
- **Lancement par systemd ou launchd** : demanderait d'installer des fichiers d'unité, contraire à "rien à configurer". Le double fork suffit.

## Conséquences vérifiées

- **Démarrage** : un aller-retour sur le socket remplace `tty::new` dans `create_session` ([src/app/application.rs:290](../../../src/app/application.rs#L290)). Le premier lancement paie en plus le démarrage du serveur. Il faut le mesurer avec `perf::mark` ("shell spawned"), car le démarrage a été optimisé récemment (`5180b78`).
- **Les deux sites de `Terminal::spawn`** ([src/app/application.rs:290](../../../src/app/application.rs#L290), [src/app/panes.rs:50](../../../src/app/panes.rs#L50)) passent par le backend choisi à la connexion.
- **Panes sans intégration shell** : il n'y a pas d'élagage par blocs, seulement le plafond de 64 Mio. Le rejeu redonne un historique plus long que `scrollback.lines`, que `Term` retaille de lui-même.
- **Modes posés avant l'offset d'élagage** (titre, couleurs OSC 4, protocole clavier) : ils sont perdus au rejeu. Les shells intégrés reposent leurs modes à chaque invite, et `SIGWINCH` fait redessiner les applications plein écran.
- **Scripts d'intégration** dans `temp_dir` ([src/shell_integration.rs](../../../src/shell_integration.rs)) : ils sont chargés au démarrage du shell, donc une mise à jour qui les change n'affecte pas les shells déjà lancés.
- **macOS** : le dialogue de fermeture fonctionne enfin, puisque le premier plan est lu avec `tcgetpgrp` côté serveur.
- **Session-restore** : les deux features sont indépendantes. Le format de disposition du blob (onglets, arbre, titres) est le même besoin que la restauration de session. La première implémentée le crée, et l'autre le réutilise.

## Tests

- **Protocole** : encodage et décodage des trames, champs inconnus ignorés, `Hello`/`Upgrade` figés (exemples d'octets en dur).
- **Journal** : anneau, offsets absolus, `Trim`, plafond avec coupe au `\n`, `Patch`.
- **Rejeu** : un flux avec blocs, images `t=d`, écran alternatif et titre, rejoué dans une `Timeline` neuve, redonne les mêmes textes (aides `timeline::testing`). Aucun `PtyWrite` n'est émis.
- **Élagage** : après éviction d'un bloc, rejouer depuis `Trim.offset` redonne les blocs restants.
- **Serveur** (intégration, `tests/`) :
  - lancer `graphene --mux` sur un socket temporaire ;
  - `Spawn` d'un `sh`, `Input`, lecture de `Output` ;
  - `Detach`, nouvelle connexion, `Replay` identique ;
  - un client tué sans `Detach` provoque un détachement ;
  - `CloseWindow` arrête le processus.
- **Passation** : un état écrit par le format N-1 (fixture) est relu par N. Test d'intégration `Upgrade` sur le même binaire : le pid et le shell survivent.
- **Dialogue** : `Answer::{Keep, Stop, Cancel}`, case "Ne plus demander" écrite dans la config, pas de dialogue sans programme au premier plan.
- **Repli** : un socket injoignable donne `Backend::Local` et le bandeau.

## Découpage en tâches d'implémentation

1. [#197](https://github.com/marmotz/graphene-term/issues/197) Protocole client/serveur.
2. [#205](https://github.com/marmotz/graphene-term/issues/205) Journal d'octets par pane.
3. [#206](https://github.com/marmotz/graphene-term/issues/206) Serveur `graphene --mux` (dépend de #197, #205).
4. [#207](https://github.com/marmotz/graphene-term/issues/207) Backend distant de `Terminal` et repli local (dépend de #197, #206).
5. [#208](https://github.com/marmotz/graphene-term/issues/208) Rejeu et rattachement des fenêtres détachées (dépend de #207).
6. [#209](https://github.com/marmotz/graphene-term/issues/209) Dialogue de fermeture et `window.on_close` (dépend de #208).
7. [#210](https://github.com/marmotz/graphene-term/issues/210) Passation du serveur au nouveau binaire (dépend de #206, #207).
8. [#211](https://github.com/marmotz/graphene-term/issues/211) Icône et notification sous Linux (dépend de #206).
9. [#213](https://github.com/marmotz/graphene-term/issues/213) Icône de barre de menus sous macOS (dépend de #206).
10. [#215](https://github.com/marmotz/graphene-term/issues/215) Images kitty par fichier temporaire ou mémoire partagée (dépend de #205, #208).
