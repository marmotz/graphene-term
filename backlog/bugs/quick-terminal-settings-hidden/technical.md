# Les réglages s'ouvrent sous le quick terminal : conception technique

Voir [overview.md](overview.md).

## Cause

Le quick terminal est créé au premier plan et en fenêtre « dock » :

- [src/app/application.rs:229](../../../src/app/application.rs) : `with_window_level(WindowLevel::AlwaysOnTop)` et, sous Linux, `with_x11_window_type(vec![WindowType::Dock])` ;
- [src/app/quick.rs:315](../../../src/app/quick.rs) `prepare_window` écrit avant le premier affichage les états `_NET_WM_STATE` de [`x11_window::QUICK`](../../../src/x11_window.rs) (dont `_NET_WM_STATE_ABOVE`).

La fenêtre des réglages est une fenêtre ordinaire, créée visible ([src/settings/mod.rs:81](../../../src/settings/mod.rs)) :

```rust
let attributes = Window::default_attributes()
    .with_title(i18n::text(lang, &Msg::OpenSettings))
    .with_inner_size(INITIAL_SIZE)
    .with_min_inner_size(MIN_SIZE);
```

Le gestionnaire de fenêtres la range dans la couche normale, sous la couche « au-dessus » du quick terminal.

Le processus du quick terminal n'a qu'une fenêtre de session (`App::session`) : une fenêtre de réglages ouverte dans ce processus l'est toujours depuis le quick terminal ([src/app/application.rs:527](../../../src/app/application.rs) `open_settings`).

Le quick terminal ne se masque déjà pas quand les réglages prennent le focus ([src/app/application.rs:702](../../../src/app/application.rs) → `QuickState::other_window_focused`, [src/app/quick.rs:110](../../../src/app/quick.rs)). En revanche, rien ne lui rend le focus à la fermeture des réglages. Comme les gestionnaires de fenêtres ne donnent jamais le focus à une fenêtre dock, le quick terminal reste affiché sans focus clavier.

## Approche retenue : réglages au premier plan en mode quick terminal

En mode quick terminal, la fenêtre des réglages reçoit le même niveau que le quick terminal :

1. `SettingsWindow::open` prend un paramètre `above: bool`. `App::open_settings` le passe à `self.quick_state().is_some()`.
2. Si `above` est vrai :
   - les attributs reçoivent `with_window_level(WindowLevel::AlwaysOnTop)` (repris par winit sous macOS, et sous X11 pour une fenêtre déjà affichée) ;
   - sous Linux, la fenêtre est créée masquée (`with_visible(false)`). Une nouvelle constante `x11_window::ABOVE = ["_NET_WM_STATE_ABOVE"]` est écrite par `x11_window::preset_states` avant le premier affichage, puis `window.set_visible(true)`. Le quick terminal applique déjà ce schéma : les gestionnaires de fenêtres ignorent un message client envoyé avant l'affichage, alors que la propriété écrite avant l'affichage est appliquée dès l'affichage ([src/x11_window.rs](../../../src/x11_window.rs), commentaire du module).
3. Le quick terminal et les réglages sont alors dans la même couche. La dernière fenêtre ouverte ou focalisée passe devant : les réglages s'affichent donc devant le quick terminal à leur ouverture, et `SettingsWindow::focus` ([src/settings/mod.rs:166](../../../src/settings/mod.rs)) les ramène devant si on les rouvre alors qu'ils sont déjà ouverts.

Hors mode quick terminal, rien ne change : `above` vaut faux et la fenêtre garde ses attributs actuels.

## Retour du focus à la fermeture

`QuickState` gagne une méthode `settings_closed(now)` : si le quick terminal est affiché, elle appelle `ask_focus(now)` ([src/app/quick.rs:74](../../../src/app/quick.rs)). Le quick terminal reprend alors le focus à chaque tick, pendant au plus `FOCUS_WAIT`, comme à son affichage.

Elle est appelée dans [src/app/application.rs:717](../../../src/app/application.rs) quand `outcome == settings::Outcome::Close`, juste après `self.settings = None`.

## Alternatives écartées

- **Rattacher les réglages au quick terminal (`WM_TRANSIENT_FOR`)** : le gestionnaire de fenêtres garde une fenêtre rattachée au-dessus de sa fenêtre parente, même après un clic sur le quick terminal. Écartée par choix produit en faveur de la solution la plus simple. Conséquence acceptée : un clic sur le quick terminal peut le repasser devant les réglages ; rouvrir les réglages les ramène devant.
- **Masquer le quick terminal pendant les réglages** : écartée dans l'overview (le quick terminal doit rester affiché).

## Conséquences vérifiées

- Les réglages restent au-dessus des autres applications tant qu'ils sont ouverts depuis le quick terminal. C'est cohérent avec le quick terminal, lui-même au premier plan.
- Sous Wayland, il n'y a pas de quick terminal ([src/app/application.rs:576](../../../src/app/application.rs) `quick_supported`) : le chemin n'est jamais emprunté.
- macOS : `WindowLevel::AlwaysOnTop` place les réglages au même niveau flottant que le quick terminal. À vérifier sur macOS pendant le correctif, comme prévu dans l'overview ; aucun code propre à macOS n'est prévu.

## Tests

- `x11_window` : le test `preset_states_writes_the_maximized_and_quick_states_before_mapping` couvre aussi `ABOVE` (skippé sans affichage X11, comme les tests existants).
- `quick.rs` : `settings_closed` demande le focus quand le quick terminal est affiché (`focus_due` vrai), et ne fait rien quand il est masqué.
- Vérification manuelle sous Cinnamon X11 : ouvrir les réglages depuis le quick terminal, puis les fermer ; le quick terminal reprend le focus clavier.

## Découpage en tâches d'implémentation

1. Réglages au premier plan en mode quick terminal et retour du focus à leur fermeture : [#255](https://github.com/marmotz/graphene-term/issues/255)
