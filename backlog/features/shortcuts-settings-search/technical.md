# Recherche dans les réglages des raccourcis : conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## État actuel

- [src/settings/view/shortcuts.rs:48](../../../src/settings/view/shortcuts.rs) `View::shortcuts`
  dessine une ligne par action à partir de `model.shortcut_rows()`, sans aucun filtre.
- [src/settings/model.rs:712](../../../src/settings/model.rs) `SettingsModel::shortcut_rows`
  renvoie `Vec<(Action, Vec<Shortcut>)>` : les actions par défaut, puis celles des
  fichiers.
- [src/settings/view/mod.rs:45](../../../src/settings/view/mod.rs) `View` garde l'état
  entre les frames. Le champ `capturing: Option<Action>` désigne l'action dont la
  prochaine touche devient un raccourci.
- [src/settings/mod.rs:225](../../../src/settings/mod.rs) : quand `view.capturing()` vaut
  vrai, la touche est lue depuis l'événement winit et passée à `View::captured_key`, sans
  passer par egui. La touche est ainsi reconnue comme dans la fenêtre du terminal.
- [src/settings/mod.rs:243](../../../src/settings/mod.rs) : Échap ferme la fenêtre, sauf si
  egui l'utilise (`egui_takes_escape` : popup ouverte ou **champ qui a le focus**) ou si
  `View::escape` annule quelque chose.
- [src/settings/view/mod.rs:184](../../../src/settings/view/mod.rs) : changer de section
  remet à zéro `capturing`, `conflict` et l'état du thème.
- La fenêtre s'ouvre toujours sur `SettingsSection::default()` (`Language`,
  [src/i18n.rs:94](../../../src/i18n.rs)). Seul un clic dans la barre latérale ouvre
  donc la section Raccourcis.
- `nucleo-matcher` est déjà une dépendance (palette de commandes,
  [src/chrome/command_palette.rs:13](../../../src/chrome/command_palette.rs)). Il expose
  `nucleo_matcher::chars::normalize(char) -> char` (« ä » donne « a ») et
  `chars::to_lower_case`. Ces fonctions dépendent des features `unicode-normalization` et
  `unicode-casefold`, actives par défaut et non désactivées dans
  [Cargo.toml](../../../Cargo.toml).

## Conception

### État de la vue

Dans `View` ([src/settings/view/mod.rs](../../../src/settings/view/mod.rs)) :

```rust
/// What the next key press is captured for.
enum Capture {
    /// A new shortcut of this action.
    Bind(Action),
    /// The shortcut whose actions the list shows.
    Search,
}

/// What the shortcuts list is narrowed to.
enum ShortcutFilter {
    None,
    /// Actions whose label contains this text.
    Text(String),
    /// The actions bound to `shortcut` when it was captured, frozen.
    Shortcut { shortcut: Shortcut, actions: Vec<Action> },
}
```

- `capturing: Option<Action>` devient `capturing: Option<Capture>`. `View::capturing()`
  garde sa signature : le routage winit de `settings/mod.rs` reste inchangé.
- Nouveaux champs : `filter: ShortcutFilter` et `focus_filter: bool` (donner le focus au
  champ à la prochaine frame).

### Barre de filtre

En tête de `View::shortcuts`, une ligne horizontale contient :

1. un `TextEdit::singleline` lié au texte de `ShortcutFilter::Text`, avec
   `hint_text(Msg::FilterActions)`. Quand le filtre est un raccourci, le champ est
   affiché vide ;
2. un bouton dont le libellé dépend de l'état :
   - pas de recherche de raccourci : `Msg::SearchShortcut`. Un clic passe à
     `capturing = Some(Capture::Search)` ;
   - capture en cours : `Msg::Cancel`, avec le texte `Msg::PressShortcut` en dessous
     (même texte que la capture « Ajouter ») ;
   - raccourci trouvé : un bouton `chip_text(shortcut)` (« Ctrl+T × »). Un clic vide le
     filtre.

Règles :

- Modifier le texte (`response.changed()`) donne `filter = Text(text)`, ou `None` si le
  texte est vide. Cela remplace un éventuel filtre par raccourci.
- Toute modification du filtre (texte modifié, bouton cliqué, raccourci capturé, filtre
  vidé) annule l'édition en cours : `capturing = None` (si c'était un `Bind`) et
  `conflict = None`. Une méthode `View::set_filter` regroupe ces effets.
- Focus : `focus_filter` passe à vrai quand on entre dans la section Raccourcis
  ([src/settings/view/mod.rs:194](../../../src/settings/view/mod.rs)). La frame suivante
  appelle `response.request_focus()`, puis remet `focus_filter` à faux.
- Échap : quand le champ a le focus, egui le reçoit et retire le focus. Si
  `response.lost_focus()` et `ui.input(|i| i.key_pressed(Key::Escape))`, et qu'un
  filtre est actif, la vue vide le filtre et redonne le focus. Si aucun filtre n'est
  actif, le champ perd simplement le focus, et un second Échap ferme la fenêtre comme
  aujourd'hui.
- Quitter la section ([src/settings/view/mod.rs:194](../../../src/settings/view/mod.rs))
  remet `filter = None`. Rouvrir la fenêtre crée un nouveau `View`, donc le filtre est
  déjà vide.

### Capture du raccourci recherché

`View::captured_key` ([src/settings/view/shortcuts.rs:9](../../../src/settings/view/shortcuts.rs))
traite les deux cas :

- les cas communs restent les mêmes : Échap seul annule, un modificateur seul continue
  d'attendre, et un fichier `keybindings.toml` verrouillé annule ;
- `Capture::Bind(action)` : comportement actuel ;
- `Capture::Search` : `capturing = None`, puis
  `set_filter(Shortcut { shortcut, actions })`. `actions` est calculé une fois à partir
  de `model.shortcut_rows()` : ce sont les actions dont la liste contient `shortcut`. La
  méthode ne renvoie aucun changement à écrire.

### Filtrage des lignes

Une fonction pure dans `shortcuts.rs`, testable sans egui :

```rust
fn filtered(rows: Vec<(Action, Vec<Shortcut>)>, filter: &ShortcutFilter, lang: Lang)
    -> Vec<(Action, Vec<Shortcut>)>
```

- `None` : toutes les lignes.
- `Text(text)` : les lignes dont `fold(label(action))` contient `fold(text)`.
  `fold` applique `chars::normalize` puis `chars::to_lower_case` à chaque caractère.
- `Shortcut { actions, .. }` : les lignes dont l'action est dans `actions`, dans l'ordre
  de `shortcut_rows`. **La liste des actions est figée, mais pas leur contenu** : chaque
  ligne affiche toujours ses raccourcis actuels. Si l'on remplace un raccourci en
  conflit, l'action qui le perd reste affichée, et l'action qui le gagne n'apparaît
  qu'au prochain changement du filtre.

Le filtre texte est recalculé à chaque frame. C'est équivalent à une liste figée, car
modifier un raccourci ne change pas le nom des actions.

Les largeurs de colonnes (`label_width`, `slot_width`) restent calculées sur **toutes**
les lignes. Les colonnes ne bougent donc pas quand on tape.

Quand le résultat est vide, un texte discret remplace la liste :
`Msg::NoActionForShortcut` (« Aucune action n'utilise ce raccourci ») pour un
raccourci, et `Msg::NoActionMatches` (« Aucune action ne correspond ») pour un texte.

### Textes

Nouveaux `Msg` dans [src/i18n.rs](../../../src/i18n.rs), en anglais et en français, avec
les index suivants (87 à 90) et ajoutés à la liste de test qui vérifie que chaque message
a un texte :

| Msg                    | en                               | fr                                     |
|------------------------|----------------------------------|----------------------------------------|
| `FilterActions`        | Filter actions                   | Filtrer les actions                    |
| `SearchShortcut`       | Find a shortcut                  | Rechercher un raccourci                |
| `NoActionForShortcut`  | No action uses this shortcut     | Aucune action n'utilise ce raccourci   |
| `NoActionMatches`      | No action matches                | Aucune action ne correspond            |

### Documentation

Le paragraphe « Changing shortcuts » de
[docs/en/keybindings.md:90](../../../docs/en/keybindings.md) et son équivalent dans
[docs/fr/keybindings.md:98](../../../docs/fr/keybindings.md) mentionnent le filtre texte
et la recherche par raccourci.

## Alternatives écartées

- **Correspondance approximative avec `nucleo-matcher`**, comme la palette : écartée par
  la décision produit (correspondance par sous-chaîne, résultats prévisibles). On
  utilise seulement sa normalisation des accents.
- **`Pattern` de nucleo avec `AtomKind::Substring` et `Normalization::Smart`** : en mode
  `Smart`, la normalisation est désactivée dès que le texte tapé contient un caractère
  accentué (« é » ne trouverait pas « e »). Le `fold` écrit à la main donne le même
  résultat dans les deux sens.
- **Nouvelle dépendance `unicode-normalization`** : inutile, `chars::normalize` couvre
  déjà les lettres latines accentuées des deux langues de l'interface.
- **Capture du raccourci recherché par egui** (événements `Key` d'egui) : écartée. Le
  raccourci ne serait pas reconnu comme dans le terminal, contrairement à la capture
  « Ajouter », qui passe par winit.
- **Recalculer le filtre par raccourci à chaque frame** : contraire à la décision
  « liste figée ».

## Conséquences vérifiées

- Les tests qui construisent `View { capturing: Some(Action::…) }`
  ([src/settings/view/mod.rs:527](../../../src/settings/view/mod.rs) et suivants)
  deviennent `Some(Capture::Bind(Action::…))`.
- Le routage winit et `egui_takes_escape` ne changent pas.
- Aucun changement de format de fichier ni de configuration.

## Tests

- `filtered` : sous-chaîne au milieu du nom, insensible à la casse et aux accents
  (« ONGLET », « onglét »), texte vide, aucune correspondance, filtre par raccourci
  figé après un `replace_shortcut`.
- `captured_key` en `Capture::Search` : renseigne le filtre avec les actions du
  raccourci, n'écrit rien, Échap annule, un modificateur seul continue d'attendre, un
  raccourci libre donne une liste d'actions vide.
- `set_filter` annule une capture `Bind` et un conflit en cours. Le texte et le
  raccourci s'excluent.
- Changer de section vide le filtre et, en entrant dans Raccourcis, active
  `focus_filter`.
- i18n : les quatre nouveaux messages ont un texte dans chaque langue.

## Découpage en tâches d'implémentation

1. [#227](https://github.com/marmotz/graphene-term/issues/227) Filtrer les actions par
   texte : `ShortcutFilter`, `set_filter`, `filtered`, barre de filtre, focus, Échap,
   messages `FilterActions` et `NoActionMatches`.
2. [#228](https://github.com/marmotz/graphene-term/issues/228) Retrouver une action en
   tapant son raccourci : `Capture`, `captured_key` en recherche, liste figée, bouton,
   messages `SearchShortcut` et `NoActionForShortcut`, documentation. Dépend de #227.
