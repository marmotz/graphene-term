# Indicateur de progression OSC 9;4 : conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## Séquence prise en charge

```text
ESC ] 9 ; 4 ; <état> [; <valeur>] ST      (ST = ESC \ ou BEL)
```

| État | Sens | Valeur |
| ---- | ---- | ------ |
| `0` | effacer | ignorée |
| `1` | pourcentage | 0 à 100, absente = 0 |
| `2` | erreur | optionnelle : absente = garde la valeur précédente |
| `3` | indéterminé | ignorée |
| `4` | pause | optionnelle : absente = garde la valeur précédente |

Règles de lecture : valeur bornée à 100, valeur non numérique traitée comme absente,
état inconnu ou absent : séquence ignorée. Une séquence `9;4` est toujours une
progression, jamais un message OSC 9 (même règle que Ghostty), ce que la feature
[desktop-notifications](../desktop-notifications/overview.md) devra respecter.

## Captation dans le flux PTY

`vte` 0.15 n'a pas de branche pour OSC 9 (`osc_dispatch`, `vte-0.15.0/src/ansi.rs:1329`,
branches `0|2`, `4`, `8`, `10-12`, `22`, `50`, `52`, `104`, `110-112`) : la séquence
est perdue avant d'atteindre `Term`. Le scanner des signaux privés
[signals.rs](../../../src/signals.rs) existe précisément pour ça (commentaire
`signals.rs:16`) : il voit tout le flux, ne consomme aucun octet et rend des
`Chunk::Signal` / `Chunk::Clear` intercalés.

**Retenu** : étendre ce scanner.

- Nouveau `Chunk::Progress(ProgressReport)` à côté de `Chunk::Signal` et `Chunk::Clear`
  (`signals.rs:61`), et `Event::Progress` interne (`signals.rs:72`).
- `Scanner::finish` (`signals.rs:300`) : si la charge utile commence par `9;4;` (ou vaut
  `9;4`), la parser en `ProgressReport`, **indépendamment du jeton** de session. Le
  préfixe du jeton reste réservé aux signaux 6973. Un `Scanner::new("")` (shell sans
  intégration, programme lancé directement) reconnaît donc toujours la progression.
- La charge utile est déjà bornée à `MAX_OSC_LEN` (4096), largement suffisant.
- Le parseur pur (`parse_progress(&[u8]) -> Option<ProgressReport>`) vit dans un nouveau
  module [src/progress.rs](../../../src/progress.rs) avec le modèle d'état.

Alternatives écartées :

- *Passer par un `Handler` vte personnalisé* : `Term` est le handler, OSC 9 est filtré
  avant tout appel ; impossible sans forker vte.
- *Un second scanner dédié* : double parcours de chaque octet, alors que le scanner
  existant fait déjà la machine à états OSC complète (BEL, ST, CAN/SUB, abandon).

## Modèle d'état (`src/progress.rs`)

```rust
pub enum ProgressState { Normal, Error, Indeterminate, Paused }

pub struct Progress {
    pub state: ProgressState,
    /// 0 à 100 ; sans objet pour `Indeterminate`.
    pub value: u8,
    /// Dernière séquence reçue, pour l'expiration.
    pub updated: Instant,
}

pub const PROGRESS_TIMEOUT: Duration = Duration::from_secs(15);
```

- `Progress::apply(current: Option<Progress>, report, now) -> Option<Progress>` : `0`
  efface, les autres remplacent (en gardant la valeur pour `2`/`4` sans valeur).
- `Progress::live(&self, now) -> bool` : `now - updated < PROGRESS_TIMEOUT`.
- Tout est pur et testé unitairement (parse + transitions + expiration).

### Où vit l'état

**Retenu** : un champ `pub progress: Option<Progress>` dans `Timeline`
(`timeline.rs:151`), déjà partagé `Arc<Mutex<Timeline>>` entre le thread PTY et le
thread UI et déjà verrouillé par `pty::process` (`pty.rs:75`) à chaque chunk.

- `Timeline::apply` (`timeline.rs:307`) traite `Chunk::Progress` **même en écran
  alternatif**, contrairement aux signaux mis en attente dans `pending`
  (`timeline.rs:316`) : la progression des programmes plein écran doit s'afficher tout
  de suite.
- **Fin de commande** : dans `Timeline::signal`, la branche `(Signal::End, Running)`
  (`timeline.rs:332`) remet `progress` à `None`. C'est la règle « effacé à la fin du
  bloc ». Sans intégration shell, seul le délai de 15 s s'applique.

Alternative écartée : un `Arc<Mutex<Option<Progress>>>` séparé sur `Terminal`. Un verrou
de plus sur le chemin de lecture, et l'effacement en fin de commande devrait être
synchronisé à la main avec la timeline.

## Thread UI

### Réception

`TermEvent::Wakeup` (`app/terminal_events.rs:22`) arrive après chaque lecture PTY. On y
ajoute une synchronisation :

- `Pane` (`pane.rs:43`) garde `progress_seen: Option<Progress>`, la dernière valeur lue.
- Au `Wakeup`, lire `timeline.progress` ; s'il diffère de `progress_seen`, le copier et :
  - demander un redessin si le pane est visible **ou** si la barre d'onglets est
    affichée et que ce pane est le pane actif de son onglet (un onglet caché doit mettre
    à jour son indicateur, ce que la règle actuelle `if visible` ne couvre pas) ;
  - mettre à jour la barre des tâches (voir plus bas).

### Expiration et animation

Nouvelle méthode `Session::progress_tick(now) -> Option<Instant>` (dans
[app/refresh.rs](../../../src/app/refresh.rs), à côté de `animate`, `refresh.rs:90`),
appelée dans `about_to_wait` (`app/application.rs:428`) avec les autres échéances :

- Une progression expirée (`!live(now)`) est effacée de `progress_seen` et de la
  timeline, puis redessin + mise à jour de la barre des tâches. Échéance retournée : la
  plus proche expiration à venir.
- Tant qu'un état `Indeterminate` est **affiché** (pane visible, onglet visible dans la
  barre), retourne une échéance de frame à ~30 fps (`INDETERMINATE_FRAME = 33 ms`) et
  demande un redessin. Rien n'est animé quand aucun indéterminé n'est à l'écran.
- Position du segment animé : fonction pure du temps (`indeterminate_offset(now)`),
  sans état, testable.

### Quelle progression montrer

- **Pane et bloc** : `progress_seen` du pane lui-même.
- **Onglet** : celle du pane actif de l'onglet, sur le modèle de `Workspace::tab_title`
  (`workspace/mod.rs:730`) : nouveau `Workspace::tab_progress(index)`.
- **Barre des tâches** : `tab_progress(active_index())`.
- Réglage `window.progress = false` : toutes ces lectures retournent `None` (le parsing
  continue, sans coût notable, pour qu'activer le réglage à chaud reprenne l'état
  courant).

## Rendu

Couleurs, tirées de la palette du thème (aucune nouvelle couleur de thème) :

| État | Couleur |
| ---- | ------- |
| Normal, Indéterminé | `palette_match` (accent, déjà utilisé pour la bordure du pane actif, `chrome/scene.rs:466`) |
| Erreur | rouge du terminal (`BlockColors::failure`, `frame.rs:169`) |
| Pause | jaune du terminal (`BlockColors::running`, `frame.rs:171`) |

Une piste (fond) en `tab_inactive_text` translucide sous la barre. Épaisseur : 2 px
logiques, arrondie à l'échelle (`UiMetrics::scale`).

- **Onglet** : `TabSlot` (`chrome/tab_bar.rs:71`) gagne `progress: Option<ProgressView>`
  ; `tabs()` (`chrome/scene.rs:384`) dessine la barre sur le bord inférieur du
  rectangle de l'onglet (barre horizontale comme latérale). La barre d'onglets n'existe
  qu'à partir de deux onglets (`tab_bar::shown`, `tab_bar.rs:44`) : avec un seul
  onglet, le pane et la barre des tâches suffisent ; ce comportement n'est pas modifié.
- **Pane** : `PaneBarLayout` (`chrome/pane_bar.rs:55`) gagne `progress`, sur le modèle
  du champ `broadcast` rempli dans `Session::pane_bars` (`app/geometry.rs:120`) ;
  `pane_bar()` (`chrome/scene.rs:470`) dessine la barre sur le bord inférieur de la
  barre de titre. Les barres de titre sont affichées pour chaque pane, même seul.
- **Bloc** : `Card` (`frame.rs:92`) gagne `progress: Option<ProgressView>`, rempli
  seulement pour la carte du bloc courant en phase `Running` (même condition que
  `card_status`, `frame.rs:535`). `cards_under` (`renderer/scene.rs:112`) dessine la
  barre sur le bord inférieur de la carte (bord calculé par `card_edges`,
  `renderer/scene.rs:105`). Pane et bloc s'affichent tous les deux, comme décidé.

`ProgressView { state, value, offset }` est le résultat pur (avec la position animée)
calculé une fois par frame ; le dessin d'une barre est une fonction commune
`progress_rects(rect, view, colors) -> [RectInstance; 2]`, testée sur ses rectangles.

## Barre des tâches système

Un module `src/taskbar/` avec `fn set_progress(Option<Progress>)`, appelé seulement
quand la valeur change (dédoublonnage côté module), implémentations par plateforme :

### Linux : API Unity LauncherEntry (D-Bus)

Signal `com.canonical.Unity.LauncherEntry.Update(app_uri: s, properties: a{sv})` sur le
bus de session, avec `app_uri = "application://graphene.desktop"` (fichier
[packaging/graphene.desktop](../../../packaging/graphene.desktop)) et les propriétés
`progress` (`d`, 0.0 à 1.0) et `progress-visible` (`b`). Compris par KDE Plasma, Ubuntu
Dock / Dash to Dock et Plank ; ignoré sans erreur ailleurs.

- `zbus` 6.3 est déjà une dépendance, utilisée en thread dédié par
  [appearance.rs](../../../src/appearance.rs) (`zbus::block_on`, `appearance.rs:52`).
  Même schéma : un thread « taskbar » qui reçoit les valeurs par canal et émet le
  signal, pour ne jamais bloquer le thread UI.
- Pas d'équivalent pour les états : erreur et pause affichent la valeur, indéterminé
  masque la barre (`progress-visible = false`). `urgent` n'est pas utilisé : il est
  laissé au badge de [desktop-notifications](../desktop-notifications/overview.md), qui
  devra passer par le même module pour que les deux ne s'écrasent pas.
- Échec de connexion au bus : log `debug`, le module devient inerte.

### Linux X11 : indices XApp (Cinnamon)

Cinnamon ne lit pas l'API Unity : ses listes de fenêtres (`window-list`,
`grouped-window-list`) lisent `metaWindow.progress`, que Muffin tire des propriétés X11
de la fenêtre `_NET_WM_XAPP_PROGRESS` (`CARDINAL`, 0 à 100) et
`_NET_WM_XAPP_PROGRESS_PULSE` (`CARDINAL`, 1 pour un indéterminé), celles de
`xapp_gtk_window_set_progress`. Le même thread « taskbar » les écrit sur la fenêtre du
terminal par une connexion `x11rb` à lui (déjà une dépendance Linux), en plus du signal
Unity ; chaque canal indisponible est ignoré (log `debug`), le module n'est inerte que
sans aucun des deux.

- Valeurs : la valeur pour normal, erreur et pause ; `PROGRESS = 0`, `PULSE = 1` pour
  l'indéterminé ; les deux à 0 sans progression, comme le recommande XApp.
- Les applets Cinnamon ignorent `PULSE` et toute valeur sous 1 : l'indéterminé et 0 %
  n'y affichent rien, comme sous Unity.
- Wayland : pas d'équivalent, seul le signal Unity est émis.

### macOS : `NSDockTile`

Vue personnalisée `contentView` sur `NSApp.dockTile` (icône de l'app + barre dessinée)
puis `display()`. Dépendances `objc2` 0.6.5 et `objc2-app-kit` 0.3.2, déjà présentes
dans `Cargo.lock` via winit/wgpu : à déclarer en dépendances directes sous
`[target.'cfg(target_os = "macos")'.dependencies]` aux mêmes versions. Appels sur le
thread principal (le thread UI de winit). Erreur et pause colorées comme dans
l'interface, indéterminé : barre pleine atténuée (pas d'animation dans le Dock).

Alternative écartée : `NSProgressIndicator` comme sous-vue du Dock, qui ne redessine pas
sans `display()` explicite et n'a pas de couleur d'erreur ; une vue dessinée à la main
fait la même chose avec plus de contrôle.

### Fenêtres multiples

Il n'y a qu'une fenêtre de terminal par processus (plus la fenêtre de réglages) : la
barre des tâches reflète l'onglet actif de cette fenêtre, sans agrégation.

## Configuration

- `WindowConfig` (`config/values.rs:114`) gagne `progress: bool`, défaut `true`, clé
  `[window] progress`, lu comme `updates.check`.
- Modèle de réglages : nouveau `Field::WindowProgress` dans `Section::Window`
  (`settings/model.rs:147`), chemin `["window", "progress"]`, widget `Toggle`
  (`settings/model.rs:237`), libellé traduit dans [i18n.rs](../../../src/i18n.rs) (en,
  fr).
- Rechargement à chaud : le changement passe par `reload_config` ; passer à `false`
  efface la barre des tâches et redessine.
- Documentation : `docs/en/configuration.md` et `docs/fr/configuration.md` (section
  `[window]`), et une mention dans `docs/*/tabs-and-panes.md`.

## Coordination avec desktop-notifications

Sa [conception](../desktop-notifications/technical.md) est compatible : elle reconnaît le
préfixe `9;` dans le même `Scanner::finish` et laisse `9;4` à cette feature.

- #174 et la détection OSC 9/777 des notifications modifient toutes deux
  `Scanner::finish` et l'énumération `Chunk` : les implémenter l'une après l'autre, la
  seconde se rebasant sur la première.
- Les deux émettent `com.canonical.Unity.LauncherEntry.Update` sur le même `app_uri`
  (`progress` ici, `count` là-bas). Le signal ne porte que les propriétés modifiées, les
  deux coexistent donc. La première feature livrée crée la connexion D-Bus, et la
  seconde la réutilise plutôt que d'en ouvrir une autre.

## Tests

- `signals.rs` : `9;4;1;42` en BEL et ST, séquence coupée entre deux lectures, sans
  jeton (`Scanner::new("")`), octets rendus inchangés, `9;4` invalides ignorés, OSC 9
  simple non pris pour une progression.
- `progress.rs` : parsing de chaque état, bornes, transitions (`2`/`4` sans valeur),
  expiration à 15 s, offset du segment animé.
- `timeline.rs` : progression appliquée en écran alternatif, effacée par `end`.
- `frame.rs` / `renderer/scene.rs` / `chrome/scene.rs` : carte, barre de pane et onglet
  avec leur rectangle et leur couleur par état.
- `workspace` : `tab_progress` suit le pane actif.
- `settings/model.rs` et `config` : lecture, défaut, écriture de `window.progress`.
- Barre des tâches : la construction des propriétés Unity et des valeurs XApp (fonctions
  pures) est testée, ainsi que l'écriture des indices XApp sur une fenêtre X11 quand un
  serveur X est disponible ; l'émission D-Bus, Cinnamon et le Dock sont vérifiés à la
  main.

## Découpage en tâches d'implémentation

1. [#174](https://github.com/marmotz/graphene-term/issues/174) : captation OSC 9;4 et
   état de progression (scanner, `progress.rs`, timeline).
2. [#176](https://github.com/marmotz/graphene-term/issues/176) : réglage
   `[window] progress`.
3. [#178](https://github.com/marmotz/graphene-term/issues/178) : synchronisation,
   expiration et animation côté interface (dépend de #174, #176).
4. [#179](https://github.com/marmotz/graphene-term/issues/179) : barres de progression
   dans l'onglet, le pane et le bloc (dépend de #178).
5. [#180](https://github.com/marmotz/graphene-term/issues/180) : module `taskbar` et
   barre des tâches Linux (dépend de #178).
6. [#181](https://github.com/marmotz/graphene-term/issues/181) : Dock macOS (dépend de
   #180).
