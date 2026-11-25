# Les réglages s'ouvrent sous le quick terminal : conception technique

Voir [overview.md](overview.md).

## Cause

Le quick terminal est une fenêtre « dock » au premier plan
([src/app/application.rs](../../../src/app/application.rs) : `WindowLevel::AlwaysOnTop`
et, sous Linux, `WindowType::Dock` ; [src/app/quick.rs](../../../src/app/quick.rs)
`prepare_window` écrit `x11_window::QUICK`). Les réglages étaient une seconde fenêtre
winit, ordinaire, rangée dessous.

## Contournements X11 abandonnés

Essayés puis abandonnés sous Cinnamon X11 :

- réglages au même niveau (`_NET_WM_STATE_ABOVE` écrit avant l'affichage) : à la
  première ouverture, la protection contre le vol de focus les place juste sous la
  fenêtre focalisée, le quick terminal ;
- activation par `_NET_ACTIVE_WINDOW` (source « pager ») une fois la fenêtre affichée :
  corrige l'ouverture, mais Cinnamon remonte les fenêtres dock quand le pointeur entre
  dedans ;
- réglages rattachés au quick terminal (`WM_TRANSIENT_FOR`) : Cinnamon oublie le lien à
  chaque masquage du quick terminal, qui est retiré de l'écran, et ne le relit pas,
  même réécrit à son retour.

Tant que les réglages sont une seconde fenêtre, leur ordre dépend du gestionnaire de
fenêtres. Ils deviennent donc un onglet de la fenêtre du terminal.

## Approche retenue : un onglet des réglages

### Barre d'onglets

- [src/chrome/tab_bar.rs](../../../src/chrome/tab_bar.rs) : la barre est toujours
  affichée (`tab_bar::shown` disparaît). `TabBarLayout` gagne `settings` (case carrée
  avant les onglets, de la taille du bouton `+`), `settings_active`, `settings_hovered`,
  `set_settings_active` (aucun onglet actif quand les réglages sont affichés) et
  `TabBarHit::Settings`. Les onglets commencent après cette case : glisser un onglet ne
  peut jamais l'amener devant.
- [src/chrome/scene.rs](../../../src/chrome/scene.rs) `settings_tab` : une icône de
  curseurs (trois traits et leurs boutons, en rectangles), sur le fond de l'onglet
  actif quand les réglages sont affichés, survolée comme le bouton `+`.
- [src/workspace/drop.rs](../../../src/workspace/drop.rs) : un panneau lâché sur
  l'onglet des réglages n'a pas de cible.
- `Workspace::progress_on_screen` perd son paramètre `tab_bar_shown`.

### État

- `Session.settings: Option<SettingsTab>` : présent tant que l'onglet des réglages est
  affiché. L'onglet actif de l'espace de travail reste l'onglet de terminal
  précédent : c'est lui qui revient quand on quitte les réglages.
- `App::open_settings` crée le `SettingsTab` (l'`App` détient la configuration) et le
  donne à `Session::show_settings` ; rien de plus s'il est déjà affiché.
  `Action::OpenSettings`, le menu contextuel et `TabBarHit::Settings` passent par
  `Session.settings_requested`.
- `Session::leave_settings` ([src/app/settings_tab.rs](../../../src/app/settings_tab.rs)) :
  écrit une couleur en cours de choix (`SettingsTab::leave`), rend au terminal les
  couleurs du thème et libère l'onglet. Appelé sur `Escape` seul
  (`settings::Outcome::Leave`), par `Session::change_tab` et `Session::new_tab`. Fermer
  le dernier onglet de terminal ferme la fenêtre comme avant.

### Dessin

- [src/settings/mod.rs](../../../src/settings/mod.rs) : `SettingsWindow` devient
  `SettingsTab`, sans fenêtre ni surface. `prepare(gpu, content)` lance la frame egui
  dans la zone de contenu (`RawInput::screen_rect`, en points), écrit ses changements et
  envoie ses textures ; `paint(gpu, encoder, view, size)` la dessine par-dessus le
  terminal (`LoadOp::Load`).
- [src/renderer/mod.rs](../../../src/renderer/mod.rs) : `Renderer::render_with` prend un
  `ContentPainter`, appelé par `Layers::draw` après l'interface et le bandeau, dans une
  passe à lui, puis l'overlay (dialogue, menu) est dessiné dans une dernière passe.
  Même chemin sur la surface GPU et sur la cible hors écran de la présentation X11.
  `Renderer::format` donne le format dans lequel egui dessine.
- `Session::shown_layout` n'a aucun panneau tant que les réglages sont affichés :
  survol, clics et dessin des panneaux sont neutralisés sans autre cas particulier.

### Événements

`Session::settings_window_event` route chaque événement avec `settings_route` (pur,
testé) : le clavier va aux réglages (à la fenêtre si un overlay est ouvert) ; un clic ou
la molette au-dessus de la barre d'onglets ou d'un overlay va à la fenêtre, sinon aux
réglages ; déplacements, relâchements, focus, taille et modificateurs vont aux deux ;
le dessin et la fermeture à la fenêtre seule.

### Retiré

`SettingsWindow::open(…, above)`, `x11_window::ABOVE`, `QuickState::settings_closed` et
`QuickState::other_window_focused` (le focus ne quitte plus la fenêtre), la route
`Route::Settings` et le champ `App.gpu`.

## Tests

- `tab_bar` : case des réglages carrée avant les onglets, barre avec un seul onglet,
  aucun onglet actif quand les réglages sont affichés, positions et glisser décalés.
- `scene` : icône dans sa case, fond de l'onglet actif, survol.
- `drop` : pas de cible sur l'onglet des réglages.
- `settings_tab` : routage des événements.
- `settings` : zone de contenu en points ; `Escape` seul quitte l'onglet.
- Vérification manuelle (Cinnamon X11, quick terminal et fenêtre normale) : clic sur
  l'icône, `Ctrl+,`, menu contextuel ; `Escape` et clic sur un onglet ramènent
  l'onglet précédent ; choix de couleur, capture de raccourci, champs texte.

## Découpage en tâches d'implémentation

1. Réglages dans un onglet de la fenêtre : [#255](https://github.com/marmotz/graphene-term/issues/255)
