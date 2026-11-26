# Garder la fenêtre ouverte après `-e` : conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## État actuel

### Ligne de commande

- `command` ([src/main.rs:127](../../../src/main.rs)) cherche la position de `-e` ;
  tout ce qui suit devient `program: Option<Vec<String>>` dans `Command::Run`
  ([src/main.rs:78](../../../src/main.rs)).
- `HELP` ([src/main.rs:87](../../../src/main.rs)) décrit `-e` dans la section
  « Running a program ».
- `graphene::run(program, grid, no_restore, restore_session)`
  ([src/lib.rs:68](../../../src/lib.rs)) → `app::run`
  ([src/app/application.rs:827](../../../src/app/application.rs)) → `run_app`, qui
  range le programme dans `App::first_program`
  ([src/app/application.rs:131](../../../src/app/application.rs)).
- `session::participates` ([src/session.rs:388](../../../src/session.rs)) exclut déjà
  de la session sauvegardée une fenêtre lancée avec un programme.

### Lancement du premier pane

`App::first_program.take()` est passé à `Terminal::spawn`
([src/app/application.rs:412-419](../../../src/app/application.rs)), puis le pane est
créé par `Pane::new(id, terminal, layout)`
([src/pane.rs:103](../../../src/pane.rs)). `terminal::launch`
([src/terminal.rs:112](../../../src/terminal.rs)) lance le programme tel quel, sans
shell ni intégration : pas de blocs, la sortie reste dans la grille vivante.

### Fin du programme

Dans la boucle PTY ([src/pty.rs:330-337](../../../src/pty.rs)), à la fin du processus
enfant :

```rust
if let Some(tty::ChildEvent::Exited(status)) = self.pty.next_child_event() {
    if let Some(status) = status {
        self.event_proxy.send_event(Event::ChildExit(status));
    }
    self.term.lock().exit();          // envoie Event::Exit
    self.event_proxy.send_event(Event::Wakeup);
    break 'event_loop;
}
```

Le pane reçoit donc `ChildExit(ExitStatus)` (quand le statut est connu), puis toujours
`Exit`. `ExitStatus` est `std::process::ExitStatus` (alacritty_terminal 0.26.0,
`src/event.rs:58`). Les deux événements ferment le pane
([src/app/terminal_events.rs:103](../../../src/app/terminal_events.rs)) :

```rust
TermEvent::Exit | TermEvent::ChildExit(_) => self.close_pane(id, event_loop),
```

Le second est ignoré parce que le pane n'existe plus
([src/app/terminal_events.rs:16](../../../src/app/terminal_events.rs)). `close_pane`
([src/app/panes.rs:196](../../../src/app/panes.rs)) ferme la fenêtre avec son dernier
pane.

### Clavier

`Session::key_press` ([src/app/keyboard.rs:107](../../../src/app/keyboard.rs)) donne
les touches d'abord aux overlays, puis bloque tout pendant un glisser de pane, puis
résout le raccourci (`keybindings.action`) et livre la touche aux panes cibles.

## Conception retenue

### 1. Ligne de commande : `-E`

- `command` cherche le premier argument qui vaut `-e` **ou** `-E`. Ce qui suit est le
  programme, comme pour `-e`. Message d'erreur sans programme : `-E requires a program
  to run` (le nom de l'option trouvée).
- `Command::Run` remplace `program: Option<Vec<String>>` par
  `program: Option<Program>` :

  ```rust
  /// The program run by the first pane instead of the shell (`-e`, `-E`).
  pub struct Program {
      /// The program then its arguments.
      pub argv: Vec<String>,
      /// `-E`: the pane stays open once the program ends, until a key is pressed.
      pub hold: bool,
  }
  ```

  `Program` est défini dans la bibliothèque (exporté par `lib.rs`, comme `GridSize`)
  et traverse `graphene::run`, `app::run`, `run_app` et `App::first_program`.
  `session::participates` reçoit `first_program.is_some()` comme aujourd'hui.
- `HELP` ajoute `-E` dans la section « Running a program » : même règle que `-e`
  (en dernier, tout ce qui suit va au programme), la fenêtre reste ouverte jusqu'à
  l'appui sur une touche.

Alternative écartée : un drapeau `--hold` à placer avant `-e`. Décision produit : `-E`
seul.

### 2. Pane : état d'attente

Nouveau champ de `Pane` :

```rust
/// What the pane does once its program ends (see `-E`).
pub hold: Hold,

pub enum Hold {
    /// The pane closes with its program (shell, `-e`).
    Off,
    /// `-E`: the pane stays open once its program ends.
    Armed,
    /// The program ended: the message is shown, the next key closes the pane.
    Waiting,
}
```

`Pane::new` met `Hold::Off`. Au lancement du premier pane
([src/app/application.rs:412](../../../src/app/application.rs)), le pane passe à
`Hold::Armed` quand `program.hold` est vrai. Les autres panes (nouvel onglet, division)
restent à `Off`.

### 3. Fin du programme

[src/app/terminal_events.rs:103](../../../src/app/terminal_events.rs) devient :

```rust
TermEvent::ChildExit(status) => self.program_ended(id, Some(status), event_loop),
TermEvent::Exit => self.program_ended(id, None, event_loop),
```

`program_ended` :

- `Hold::Off` → `close_pane`, comme aujourd'hui.
- `Hold::Armed` → `pane.show_exit(status, lang)` puis `Hold::Waiting`, redessin.
- `Hold::Waiting` → rien. C'est le `Exit` qui suit un `ChildExit` : le message est
  déjà affiché, avec le code.

Si seul `Exit` arrive (statut inconnu), le message s'affiche sans code.

`Pane::show_exit` écrit le message dans la grille vivante avec un
`vte::ansi::Processor` neuf, sous le verrou `term`, comme le fait `pty::feed`
([src/pty.rs:69](../../../src/pty.rs)). La boucle PTY est arrêtée, donc rien d'autre
n'écrit dans la grille. Les octets écrits :

1. `ESC [0m` : remet à zéro les attributs laissés par le programme ;
2. `\r\n` si le curseur n'est pas en colonne 0, pour ne pas coller le message à la
   dernière ligne ;
3. le texte en vidéo inverse (`ESC [7m … ESC [0m`), pour le distinguer de la sortie ;
4. `ESC [?25l` : cache le curseur, puisqu'on ne tape plus rien.

Si le programme se termine sur l'écran alternatif (plantage d'un programme plein
écran), le message s'écrit sur cet écran : on garde ce que le programme montrait.

### 4. Texte du message (i18n)

Nouveau `Msg::ProgramExited(Exit)` dans [src/i18n.rs](../../../src/i18n.rs), avec un
identifiant pris au prochain numéro libre de la table des identifiants
([src/i18n.rs:1011](../../../src/i18n.rs) pour l'exemple de `CommandFailed`) :

```rust
pub enum Exit {
    /// The program returned this exit code.
    Code(i32),
    /// The program was killed by this signal.
    Signal(i32),
    /// The status could not be read.
    Unknown,
}
```

`Exit` se construit depuis `Option<ExitStatus>` : `code()`, sinon
`ExitStatusExt::signal()` (`std::os::unix::process`, graphene ne cible que Linux et
macOS), sinon `Unknown`.

| `Exit`      | en                                                       | fr                                                                  |
| ----------- | -------------------------------------------------------- | ------------------------------------------------------------------- |
| `Code(n)`   | `Program ended (code n). Press any key to close.`        | `Programme terminé (code n). Appuyez sur une touche pour fermer.`   |
| `Signal(n)` | `Program ended (signal n). Press any key to close.`      | `Programme terminé (signal n). Appuyez sur une touche pour fermer.` |
| `Unknown`   | `Program ended. Press any key to close.`                 | `Programme terminé. Appuyez sur une touche pour fermer.`            |

La langue est celle de la session (`Session::lang`), passée à `program_ended`.

### 5. Fermeture par une touche

Dans `Session::key_press`, après les overlays et le glisser de pane, avant la
résolution des raccourcis : si le pane actif est en `Hold::Waiting` et que la touche
n'est pas une touche de modification seule, `close_pane(id, event_loop)` puis retour.

- **Toute touche ferme, raccourcis de graphene compris** (décision : un raccourci
  comme `Ctrl+Shift+C` ne s'exécute pas, il ferme).
- Les touches de modification seules (`Shift`, `Control`, `Alt`, `Super`,
  `CapsLock`, les touches que `Shortcut::from_event` refuse déjà,
  [src/keybindings.rs:237](../../../src/keybindings.rs)) ne ferment pas : appuyer sur
  `Ctrl` pour commencer une combinaison ne doit pas fermer avant la seconde touche.
  La fonction `closes_hold(key: &Key) -> bool` est testée seule.
- Les overlays ouverts (palette, dialogue) gardent la main : la touche va à l'overlay,
  comme aujourd'hui.
- La souris n'est pas concernée : défilement et sélection restent possibles pour
  relire ou sélectionner la sortie.
- Seul le pane actif est concerné. Un pane en attente qui n'est pas actif (fenêtre
  divisée depuis la palette ou la souris) se ferme quand on y revient et qu'on appuie
  sur une touche. En diffusion de la saisie, un pane en attente qui n'est pas actif
  ignore la touche : la boucle PTY est arrêtée, `pty::Sender::send` la jette déjà
  ([src/pty.rs:60](../../../src/pty.rs)).

`close_pane` ferme la fenêtre si c'est le dernier pane, comme avant.

## Conséquences vérifiées

- `-e` ne change pas : `hold: false` donne `Hold::Off` et le chemin de fermeture
  actuel.
- Session sauvegardée : la fenêtre `-E` en est exclue comme celle de `-e`
  (`participates` reçoit `program.is_some()`).
- Fermer la fenêtre pendant l'attente (bouton, `Ctrl+Shift+Q`) : le pane n'a plus de
  processus au premier plan (`foreground_program` de son pid mort renvoie `None`), donc
  pas de confirmation.
- Terminal rapide (`--quick`) et restauration de session : `first_program` vaut `None`,
  rien ne change.

## Tests

- [src/main.rs](../../../src/main.rs) : `-E` prend le programme et ses arguments ;
  `-E` sans programme est une erreur ; les options après `-E` vont au programme ; le
  premier de `-e`/`-E` gagne (`-e prog -E` donne `prog -E`) ; `HELP` mentionne `-E`.
- [src/terminal.rs](../../../src/terminal.rs) / `Program` : `hold` n'influe pas sur ce
  que lance `launch`.
- `Exit` depuis `ExitStatus` : code, signal, inconnu.
- [src/i18n.rs](../../../src/i18n.rs) : textes en et fr des trois cas, identifiant
  unique.
- `Pane::show_exit` : sur une grille avec du texte et le curseur en milieu de ligne, le
  message arrive sur la ligne suivante, le curseur est caché, les attributs sont remis
  à zéro.
- `program_ended` : `Off` ferme, `Armed` → `Waiting` sans fermer, `ChildExit` puis
  `Exit` n'affichent le message qu'une fois.
- `closes_hold` : lettre, `Enter`, `Escape`, `Ctrl+Shift+C` ferment ; `Shift`,
  `Control`, `Alt`, `Super`, `CapsLock` seuls ne ferment pas.

## Documentation

- [docs/en/getting-started.md](../../../docs/en/getting-started.md) et
  [docs/fr/getting-started.md](../../../docs/fr/getting-started.md) : ligne `-E` dans
  le tableau des options, exemple `graphene -E ls -la`, différence avec `-e`.

## Découpage en tâches d'implémentation

Une seule tâche : l'option, l'attente, le message et la fermeture ne sont utilisables
qu'ensemble.

1. [#266](https://github.com/marmotz/graphene-term/issues/266) : option `-E`, attente
   du pane, message de fin traduit, fermeture par une touche, doc et tests.
