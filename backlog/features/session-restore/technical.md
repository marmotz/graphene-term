# Restauration de session : conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## Constats sur le code actuel

- **Une fenêtre = un processus.** Il n'existe aucune action « nouvelle fenêtre » ;
  `App` porte une seule `Session` ([src/app/application.rs:89](../../../src/app/application.rs)).
  Restaurer plusieurs fenêtres veut donc dire relancer plusieurs processus `graphene`.
- **Taille de fenêtre déjà persistée** : `WindowState { width, height, maximized }` dans
  `window.json` ([src/window_state.rs](../../../src/window_state.rs)), écrit à la sortie
  propre (`exiting`, [src/app/application.rs:420](../../../src/app/application.rs)) via
  `save_window_state` ([src/app/panes.rs:106](../../../src/app/panes.rs)). La position n'est pas
  sauvegardée aujourd'hui.
- **Écriture atomique** disponible : `atomic_file::write_json` / `read_json`
  ([src/atomic_file.rs:69](../../../src/atomic_file.rs)), dossier d'état `config::state_dir()`
  ([src/config/paths.rs](../../../src/config/paths.rs)).
- **Modèle d'onglets testable sans shell** : `Workspace<P>` générique, `Tab { tree, active,
  zoomed, broadcast }`, `Node::Leaf(PaneId) | Node::Split { axis, ratio, first, second }`
  ([src/workspace/mod.rs:85](../../../src/workspace/mod.rs),
  [src/workspace/tree.rs:216](../../../src/workspace/tree.rs)).
- **Répertoire courant** : `process::current_dir` lit `/proc/<pid>/cwd`, `None` hors Linux
  ([src/process.rs:59](../../../src/process.rs)). Sur macOS, les nouveaux onglets démarrent donc
  déjà dans le dossier personnel.
- **Pas de renommage d'onglet** : le titre vient toujours de `title_for` (titre OSC, puis
  répertoire, puis nom du shell, [src/workspace/mod.rs:755](../../../src/workspace/mod.rs)).
- **Premier pane** créé dans `spawn_first_pane`, en parallèle de la création du contexte GPU
  ([src/app/application.rs:247](../../../src/app/application.rs)).

## 1. Réglage `window.restore_session`

Booléen, `false` par défaut, sous `[window]`.

- `WindowConfig` ([src/config/values.rs:114](../../../src/config/values.rs)), lecture dans
  [src/config/reader.rs:134](../../../src/config/reader.rs) sur le modèle de `startup_mode`.
- Champ `Field::RestoreSession` dans la fenêtre de paramètres
  ([src/settings/model.rs:176](../../../src/settings/model.rs)), libellés FR/EN dans `i18n.rs`.
- Documentation : tableau `[window]` de `docs/en/configuration.md` et `docs/fr/…`, plus la
  section « Saved state ».

**Alternative écartée** : une valeur `startup_mode = "session"`. La taille d'une nouvelle
fenêtre et la restauration sont indépendantes : on veut pouvoir combiner `maximized` pour
les fenêtres neuves et la restauration.

Le réglage ne s'applique qu'au lancement suivant (comme `startup_mode`, cf.
configuration.md:60). Désactivé : rien n'est lu ni écrit dans `sessions/`.

## 2. Format sauvegardé

Nouveau module `src/session.rs`. Un fichier par instance :
`<state_dir>/sessions/<instance>.json`, `<instance>` = `<pid>-<instant de démarrage en ns>`
(évite la réutilisation de pid).

```rust
#[derive(Serialize, Deserialize)]
pub struct WindowSnapshot {
    pub version: u32,                 // 1
    /// `Normal`, ou `Quick` pour le quick terminal (section 5).
    pub kind: WindowKind,
    pub window: WindowState,          // réutilise src/window_state.rs
    /// Position extérieure en pixels physiques et nom de l'écran ; `None` sous Wayland.
    pub placement: Option<Placement>,
    pub tabs: Vec<TabSnapshot>,
    pub active_tab: usize,
    /// Secondes Unix de la fermeture propre ; `None` tant que la fenêtre vit ou après un plantage.
    pub closed_at: Option<u64>,
}

#[derive(Serialize, Deserialize)]
pub struct Placement { pub x: i32, pub y: i32, pub monitor: Option<String> }

#[derive(Serialize, Deserialize)]
pub struct TabSnapshot {
    pub title: Option<String>,        // titre personnalisé (section 6)
    pub tree: NodeSnapshot,
    /// Rang de la pane active parmi les feuilles, en profondeur d'abord.
    pub active: usize,
    pub zoomed: bool,
}

#[derive(Serialize, Deserialize)]
pub enum NodeSnapshot {
    Pane { cwd: Option<PathBuf> },
    Split { axis: Axis, ratio: f32, first: Box<NodeSnapshot>, second: Box<NodeSnapshot> },
}
```

- Les `PaneId` ne sont pas persistés : ils sont réalloués à la restauration. La pane active est
  donc repérée par son rang dans `Node::leaves()`
  ([src/workspace/tree.rs:229](../../../src/workspace/tree.rs)).
- Le broadcast n'est pas restauré (hors périmètre produit).
- `placement` vient de `Window::outer_position()` et `current_monitor()` (winit 0.30), qui
  échouent sous Wayland : on écrit alors `None`.
- `Axis` ([src/workspace/tree.rs:25](../../../src/workspace/tree.rs)) dérive `Serialize`/`Deserialize`.
- `version` inconnue ou fichier illisible : fichier ignoré et supprimé (comme `WindowState::load`).
- Construction : `Workspace::<P: TitleSource>::snapshot()` lit `current_dir()` de chaque pane,
  donc testable avec un faux pane.

## 3. Instances vivantes

Chaque instance qui participe à la session ouvre `sessions/<instance>.lock` et y pose un
`flock(LOCK_EX | LOCK_NB)` gardé jusqu'à la fin du processus (libc 0.2.191, Unix hors Solaris).
Une instance est **vivante** si son verrou ne peut pas être pris. Le noyau libère le verrou à la
mort du processus, plantage compris.

Le **type** d'une instance vivante (`Normal` ou `Quick`) est lu dans le champ `kind` de son
`<instance>.json`. Une instance vivante dont le fichier est absent ou illisible compte comme
`Normal`. Ce cas se présente au tout début du démarrage, avant la première écriture : traiter
l'instance comme `Normal` évite une double restauration. Le format du verrou ne change donc pas.

**Alternative écartée** : `kill(pid, 0)`. Un pid réutilisé ferait croire qu'une fenêtre
plantée est toujours ouverte.

## 4. Sauvegarde au fil de l'eau

- **Quand** : à chaque `about_to_wait` ([src/app/application.rs:434](../../../src/app/application.rs)),
  au plus toutes les `SESSION_SAVE_INTERVAL = 2 s`. Le snapshot est sérialisé puis comparé au
  dernier écrit : le fichier n'est réécrit que s'il a changé. Les changements de répertoire
  (`cd`) n'émettent pas d'événement, d'où le contrôle périodique (même principe que
  `TITLE_REFRESH`, [src/pane.rs:41](../../../src/pane.rs)).
- **Battement** : si rien n'a changé depuis `HEARTBEAT = 10 s`, la date de modification du fichier
  est mise à jour (`File::set_modified`) sans le réécrire. Elle date la mort d'une instance
  plantée ou tuée.
- **Sortie propre** (`exiting`) : écrit le snapshot final avec `closed_at = now`.
- **Exclus** : lancement avec `-e`, `--columns`/`--lines` (`first_program`, `grid.is_set()`) ou
  `--no-restore` : aucun fichier, aucun verrou. Pour `--no-restore`, c'est ce qui garde intacte
  la session sauvegardée. Si cette fenêtre écrivait son fichier, sa fermeture serait la plus
  récente et ferait écarter les anciennes fenêtres par `GRACE` au lancement suivant.

## 5. Restauration au lancement

Dans `App::create_session` ([src/app/application.rs:144](../../../src/app/application.rs)),
quand `restore_session` est vrai et que le lancement n'est pas exclu :

1. Verrou global bloquant `sessions/.restore.lock` pendant le choix : deux lancements simultanés
   ne restaurent pas deux fois.
2. Une instance vivante **du même type** existe : **pas de restauration**, fenêtre neuve
   (décision produit). Un quick terminal résident n'empêche pas de restaurer les fenêtres
   normales, et une fenêtre normale ouverte n'empêche pas de restaurer le quick terminal.
3. Sinon, pour chaque snapshot, date de fin = `closed_at`, à défaut la date de modification du
   fichier (plantage). On garde ceux dont la fin est à moins de `GRACE = 15 s` de la plus récente.
   Les autres sont supprimés.
   - Une fenêtre fermée pendant que d'autres restent ouvertes finit bien avant la dernière :
     elle est écartée.
   - Déconnexion ou arrêt : toutes les fins sont proches, toutes sont gardées.
   - Limite assumée : deux fenêtres fermées volontairement à moins de 15 s d'écart reviennent
     toutes les deux.
4. Le snapshot le plus récent est adopté par ce processus : renommé en `<instance>.json`.
   Chaque autre est renommé `pending-<n>.json`, puis le processus lance
   `std::env::current_exe()` avec l'option cachée `--restore-session <fichier>`
   (stdin/stdout/stderr à `null`, sans attendre l'enfant).
5. Le processus enfant prend le verrou global, renomme le fichier reçu à son propre nom, puis
   le restaure. Il saute les étapes 2 et 3.

Options ajoutées à `Command::Run` dans [src/main.rs:47](../../../src/main.rs) :

- `--restore-session <fichier>` : interne, absente de l'aide.
- `--no-restore` : publique et documentée. Elle saute la restauration pour ce lancement, sans
  toucher au réglage ni aux fichiers de `sessions/`.

### Priorité au multiplexage

Si le [multiplexage](../multiplexing/technical.md) est livré et qu'il reste des fenêtres
détachées (`ListDetached`), la restauration ne s'applique pas : les étapes 1 à 5 sont sautées.
Ce test se place en tête de l'étape 1. Les deux features relancent un processus par fenêtre
(`--attach` et `--restore-session`) avec un blob de disposition proche. Le format
`TabSnapshot`/`NodeSnapshot` est conçu pour servir aux deux : la feuille gagnera un identifiant
de pane serveur à côté de `cwd`.

### Quick terminal

`WindowKind::Quick` est réservé à la [feature quick terminal](../quick-terminal/overview.md), qui
dépend de celle-ci. Règles fixées ici pour qu'elle s'y branche :

- Sélection et `GRACE` (étape 3) se calculent **par type** : un lancement normal ne voit que les
  snapshots `Normal`, et la commande du quick terminal ne voit que le snapshot `Quick`.
- Graphene lancé par la commande du quick terminal n'adopte que le snapshot `Quick`. Les
  snapshots `Normal` restent sur disque pour le prochain lancement normal.
- Le cycle de vie de la fenêtre quick (masquage et réaffichage) est conçu dans la feature quick
  terminal.

### Reconstruction

- Taille : `snapshot.window` est passé comme `remembered` à `initial_window`
  ([src/app/application.rs:52](../../../src/app/application.rs)), quel que soit `startup_mode`.
- Position : `with_position` sur les attributs de la fenêtre
  ([src/app/application.rs:155](../../../src/app/application.rs)), seulement si `placement` existe
  et que l'écran nommé est encore branché (`available_monitors()`). Sinon, la position est laissée
  au gestionnaire de fenêtres. Sous Wayland, `placement` vaut `None`.
- Panes : `spawn_first_pane` devient « spawn workspace ». Un nouveau
  `Workspace::restore(tabs, spawn: impl FnMut(Option<PathBuf>) -> Option<P>)` construit chaque
  arbre en conservant `axis`/`ratio`, puis applique `active_tab`, la pane active et `zoomed`.
  Un rang hors bornes, par exemple après l'échec d'un shell, retombe sur la première feuille, et le
  zoom est alors abandonné.
  Terminal::spawn prend déjà un `cwd` ([src/terminal.rs:142](../../../src/terminal.rs)).
- Échec d'un shell : la feuille est retirée (son voisin prend la place). Un onglet vide est
  retiré. Sans aucun pane : repli sur la fenêtre neuve habituelle.
- Répertoire : `nearest_existing_dir(path)` remonte jusqu'au premier parent existant, à défaut
  `$HOME`.
- Démarrage : tous les shells sont lancés dans la closure de `gpu_alongside`. Le coût est un fork
  par pane, mesuré avec les marques `perf` existantes.

## 6. Renommer un onglet

Indépendant du réglage de restauration.

- **Modèle** : `Tab.custom_title: Option<String>`. `Workspace::tab_title`
  ([src/workspace/mod.rs:730](../../../src/workspace/mod.rs)) le renvoie en priorité, et le titre
  de la fenêtre suit via `refresh_title` ([src/app/refresh.rs:182](../../../src/app/refresh.rs)).
  Le titre reste attaché à l'onglet quand on le déplace. Un onglet créé par détachement de pane
  n'en a pas. Le titre de la barre de pane ne change pas.
- **Action** `Action::RenameTab` ([src/input.rs:11](../../../src/input.rs), ajoutée à `ALL`),
  sans raccourci par défaut, remappable. Dans la palette, elle agit sur l'onglet actif.
- **Menu contextuel** : entrée « Renommer l'onglet » dans `tab_menu`
  ([src/chrome/context_menu.rs:228](../../../src/chrome/context_menu.rs)), commande
  `Command::RenameTab(index)`.
- **Double-clic** sur un onglet : `tab_bar_press`
  ([src/app/overlays.rs:389](../../../src/app/overlays.rs)) compte les clics avec un
  `ClickTracker<usize>` (comme `pane_bar_clicks`, [src/app/mod.rs:224](../../../src/app/mod.rs)).
  Au second clic, le glisser d'onglet est annulé et la boîte de renommage s'ouvre.
- **Boîte de saisie** : nouvel `Overlay::RenameTab` ([src/app/mod.rs:151](../../../src/app/mod.rs),
  [src/chrome/scene.rs:57](../../../src/chrome/scene.rs)), centrée comme la palette, préremplie
  avec le titre affiché. L'édition de texte sur une ligne de la palette (`CommandPalette::key`,
  [src/chrome/command_palette.rs:349](../../../src/chrome/command_palette.rs)) est extraite dans
  un `chrome/text_field.rs` partagé. Entrée valide après `trim`. Une saisie vide donne `None`
  (retour au titre automatique). Échap annule.

**Alternative écartée** : édition directement dans la barre d'onglets. Elle est difficile avec
des onglets étroits et une barre verticale.

## 7. Répertoire courant sur macOS

Branche `#[cfg(target_os = "macos")]` dans `process::current_dir`
([src/process.rs:59](../../../src/process.rs)) : `libc::proc_pidinfo(pid, PROC_PIDVNODEPATHINFO,
0, &mut proc_vnodepathinfo, size)` puis lecture de `pvi_cdir.vip_path`, un tableau
`[[c_char; 32]; 32]` à aplatir en chaîne C (libc 0.2.191, `src/unix/bsd/apple/mod.rs`). Cela
corrige aussi le répertoire des nouveaux onglets et panes sur macOS.

**Alternative écartée** : OSC 7 émis par l'intégration shell. Il ne couvre que les shells
intégrés et ajoute un protocole, alors que l'appel système suffit.

## Conséquences vérifiées

- `window.json` et `startup_mode = "remember"` restent inchangés pour les fenêtres neuves.
- Réglage désactivé (défaut) : aucun changement de comportement, aucun fichier nouveau.
- Effacer `sessions/` est sans danger : c'est la règle de la section « Saved state » de la doc.
- `Action::ALL` passe de 38 à 39 entrées : les tests de la palette et des keybindings qui comptent
  les actions sont à mettre à jour.

## Tests

- `session.rs` : aller-retour JSON, version inconnue, sélection par `GRACE` (fenêtre fermée
  tôt écartée, fermetures proches gardées, plantage daté par la date de modification),
  `nearest_existing_dir`.
- `workspace` : `snapshot` puis `restore` sur des faux panes (arbre, ratios, titres, onglet actif,
  pane active, zoom), échec de spawn d'une feuille.
- Sélection par type : un snapshot `Quick` n'est jamais adopté par un lancement normal, et
  inversement. `--no-restore` ne crée ni fichier ni verrou.
- Placement : écran absent, donc pas de position imposée.
- Instance vivante par type : une instance `Quick` vivante ne bloque pas un lancement normal,
  une instance `Normal` vivante le bloque, une instance vivante sans fichier compte comme `Normal`.
- Verrous : une instance vivante est détectée, un verrou relâché ne l'est plus (deux descripteurs
  dans le même test).
- `main.rs` : analyse de `--restore-session` et `--no-restore`.
- Renommage : `tab_title` avec `custom_title`, saisie vide, double-clic, entrées de menu et de
  palette.
- `process::current_dir` sur macOS : test du processus courant (`#[cfg(target_os = "macos")]`).

## Découpage en tâches d'implémentation

1. [#198](https://github.com/marmotz/graphene-term/issues/198) : lire le répertoire courant du shell sur macOS (§7). Aucune dépendance.
2. [#199](https://github.com/marmotz/graphene-term/issues/199) : renommer un onglet (§6). Aucune dépendance.
3. [#204](https://github.com/marmotz/graphene-term/issues/204) : réglage, format, verrous et sauvegarde au fil de l'eau, `--no-restore` (§1 à 4). Dépend de #199.
4. [#214](https://github.com/marmotz/graphene-term/issues/214) : restaurer la dernière fenêtre au lancement (§5, étapes 1 à 3, « Reconstruction »). Dépend de #204.
5. [#217](https://github.com/marmotz/graphene-term/issues/217) : restaurer toutes les fenêtres sauvegardées, `--restore-session` (§5, étapes 4 et 5). Dépend de #214.
