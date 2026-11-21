# Saisie IME : conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## État actuel

- La fenêtre terminal n'appelle jamais `Window::set_ime_allowed` : winit 0.30 laisse
  l'IME désactivée par défaut, donc aucun `WindowEvent::Ime` n'arrive. Création de la
  fenêtre : [application.rs:154](../../../src/app/application.rs:154). Le `match` des
  événements ([application.rs:541-592](../../../src/app/application.rs:541)) ignore
  `WindowEvent::Ime`.
- La saisie passe uniquement par `KeyEvent::text` :
  [keyboard.rs:109](../../../src/app/keyboard.rs:109) (`key_press`) envoie la touche à
  l'overlay ouvert (palette : `CommandPalette::key`,
  [command_palette.rs:349](../../../src/chrome/command_palette.rs:349)), sinon à chaque
  pane cible via `deliver_key` ([keyboard.rs:181](../../../src/app/keyboard.rs:181)), qui
  passe par `input::resolve_key` ([input.rs:228](../../../src/input.rs:228)).
- L'éditeur de prompt n'a pas de tampon local : le texte de la ligne vit dans le shell
  (zle, readline, fish) et s'affiche dans la grille. Taper au prompt, c'est écrire des
  octets au PTY (`Pane::type_bytes`, [pane.rs:178](../../../src/pane.rs:178)), ou
  remplacer une sélection de l'entrée (`Pane::replace_selection`,
  [pane.rs:202](../../../src/pane.rs:202)).
- Le rendu d'un pane part d'une `Frame` capturée ([frame.rs:189](../../../src/frame.rs:189)),
  retouchée ensuite (`underline_link`, `dimmed`,
  [frame.rs:570](../../../src/frame.rs:570)), et mise en cache par `FrameKey`
  ([redraw.rs:11](../../../src/app/redraw.rs:11), [redraw.rs:139-175](../../../src/app/redraw.rs:139)).
  Le curseur est dessiné dans [scene.rs:403](../../../src/renderer/scene.rs:403), le
  soulignement des cellules dans [scene.rs:365](../../../src/renderer/scene.rs:365).
- La fenêtre de réglages (egui) gère déjà l'IME : `egui_winit::State::on_window_event`
  ([settings/mod.rs:257](../../../src/settings/mod.rs:257)) traduit `Ime::Preedit` et
  `Ime::Commit`, et `handle_platform_output_with_event_loop`
  ([settings/mod.rs:331](../../../src/settings/mod.rs:331)) appelle `set_ime_allowed`,
  `set_ime_purpose` et `set_ime_cursor_area` quand un champ texte a le focus (egui-winit
  0.37, `handle_platform_output_inner`). Rien à coder, seulement à vérifier.
- Recherche : aucun champ de recherche n'existe hors palette. La feature
  [scrollback-search](../scrollback-search/overview.md) devra réutiliser le mécanisme
  décrit ici (voir « Dépendances »).

## API winit utilisée (0.30.13)

- `Window::set_ime_allowed(bool)` : avec `true`, la fenêtre reçoit `WindowEvent::Ime`
  et, pendant une composition, ne reçoit plus de `KeyboardInput`. Sur macOS, l'IME doit
  être autorisée pour que les touches mortes se combinent.
- `WindowEvent::Ime(Ime)` : `Enabled`, `Preedit(String, Option<(usize, usize)>)` (curseur
  en octets, `None` = curseur caché, chaîne vide = composition effacée),
  `Commit(String)` (précédé d'un `Preedit` vide), `Disabled`.
- `Window::set_ime_cursor_area(position, size)` : position de la fenêtre de suggestions.
  X11 n'utilise que la position.
- `Window::set_ime_purpose(ImePurpose::Terminal)`.

## Conception

### 1. Configuration `[keyboard] ime`

- `KeyboardConfig { ime: bool }` dans [config/values.rs](../../../src/config/values.rs),
  champ `keyboard` dans `Config` ([config/mod.rs:59](../../../src/config/mod.rs:59)),
  défaut `true`.
- Lecture dans [config/reader.rs](../../../src/config/reader.rs) (`("keyboard", "ime")`,
  sur le modèle de `("cursor", "blinking")`, [reader.rs:162](../../../src/config/reader.rs:162)),
  commentaire du modèle de fichier dans [config/files.rs](../../../src/config/files.rs).
- Fenêtre de réglages : `Section::Keyboard` (`"keyboard"`) dans la liste des sections
  ([settings/model.rs:131](../../../src/settings/model.rs:131)), `Field::KeyboardIme`
  (case à cocher, chemin `["keyboard", "ime"]`), libellés en et fr dans
  [i18n.rs](../../../src/i18n.rs).
- L'option ne concerne que la fenêtre terminal. La fenêtre de réglages garde la gestion
  d'egui, qui n'active l'IME que dans un champ texte.

### 2. Activation dans la fenêtre terminal

- À la création de la fenêtre ([application.rs:154](../../../src/app/application.rs:154)) :
  `set_ime_allowed(config.keyboard.ime)` puis `set_ime_purpose(ImePurpose::Terminal)`.
- Au rechargement (`Session::apply_config`, [refresh.rs:33](../../../src/app/refresh.rs:33)) :
  si `old.keyboard.ime != new.keyboard.ime`, rappeler `set_ime_allowed` ; en passant à
  `false`, effacer la composition en cours.

### 3. État de composition

Nouveau module `src/ime.rs`, logique pure et testable :

```rust
/// Text being composed by the input method, shown at the cursor until committed.
pub struct Preedit {
    pub text: String,
    /// Cursor inside `text`, in bytes; `None` hides the cursor.
    pub cursor: Option<(usize, usize)>,
}
```

`Session` porte `preedit: Option<Preedit>`. Gestion de `WindowEvent::Ime` dans
`session_window_event` :

- `Preedit(text, cursor)` : `Some` si `text` n'est pas vide, `None` sinon ; redessin.
- `Commit(text)` : efface la composition, puis livre `text` comme du texte tapé (§4).
- `Enabled` : rien. `Disabled` : efface la composition.
- La composition est aussi effacée quand la fenêtre perd le focus
  (`WindowEvent::Focused(false)`), quand le pane actif change et quand un overlay
  s'ouvre ou se ferme : elle appartient toujours à la cible courante de la saisie.

Alternative écartée : garder une composition par pane. L'IME système n'a qu'une
composition par fenêtre ; la rattacher à la cible courante suffit.

### 4. Livraison du texte validé

Nouvelle méthode `Session::commit_text(&str)` dans [keyboard.rs](../../../src/app/keyboard.rs) :

- Overlay `Palette` ouvert : nouvelle méthode `CommandPalette::insert(&str)`, extraite
  de la branche « texte » de `key` ([command_palette.rs:360-370](../../../src/chrome/command_palette.rs:360)),
  que `key` appelle aussi. Autres overlays (dialogue, menu) : texte ignoré.
- Sinon, pour chaque cible de `workspace.input_targets()` (même règle que les touches,
  donc le broadcast s'applique) : en mode riche, `Pane::replace_selection(bytes)` d'abord,
  puis `Pane::type_bytes(bytes)` si rien n'est remplacé ; `bytes` est le texte en UTF-8.
- Pas de passage par `resolve_key` ni par les raccourcis : un texte validé n'est jamais
  une commande. Quand le protocole clavier kitty arrivera
  ([kitty-keyboard-protocol](../kitty-keyboard-protocol/overview.md)), le texte validé
  reste envoyé en UTF-8 brut, comme dans kitty.

Doublons : winit ne livre pas de `KeyboardInput` pendant une composition. Par sécurité,
`key_press` ignore une touche reçue alors que `preedit` est `Some`.

### 5. Affichage dans le terminal

`Frame::with_preedit(&mut self, preedit: &Preedit, shift: bool, colors)` dans
[frame.rs](../../../src/frame.rs), appelée dans la closure de `frames.update`
([redraw.rs:152](../../../src/app/redraw.rs:152)) pour le pane actif seulement :

- Les caractères de la composition deviennent des `FrameCell` soulignées, posées à partir
  de la cellule du curseur, avec les couleurs de la cellule d'origine. Un caractère
  large occupe deux cellules (la seconde en `spacer`) : largeur calculée avec
  `unicode-width` 0.2.3 (déjà dans `Cargo.lock` via `alacritty_terminal`), ajoutée en
  dépendance directe.
- `shift = pane.rich_mode(..)` (éditeur de prompt) : les cellules à droite du curseur
  sont décalées de la largeur de la composition. Sinon (plein écran, ou programme hors
  prompt) : la composition recouvre les cellules, la grille n'est pas modifiée.
- Dans les deux cas, ce qui dépasse le bord droit de la grille est coupé, sans retour à
  la ligne.
- Le curseur de la `Frame` est déplacé à la position du curseur de composition
  (`cursor.0` converti en colonnes), caché si `cursor` est `None`.
- `FrameKey` gagne un champ `preedit: Option<Preedit>` pour invalider le cache.

Alternative écartée : écrire la composition dans la grille du terminal. Cela mélangerait
un état d'affichage avec l'état du programme et casserait la sélection et la copie.

### 6. Position de la fenêtre de suggestions

Après chaque redessin (`Session::redraw`, [redraw.rs](../../../src/app/redraw.rs)), si
l'IME est autorisée, calculer le rectangle de la cellule de la composition (ou du
curseur sans composition) en pixels physiques de la fenêtre, avec `layout.cell_x` et
`layout.row_top` ([layout.rs:215](../../../src/layout.rs:215)) du pane actif ; palette
ouverte : le rectangle `PaletteLayout::caret`
([command_palette.rs:557](../../../src/chrome/command_palette.rs:557)). Appeler
`set_ime_cursor_area` seulement quand ce rectangle change (dernier rectangle gardé dans
`Session`).

### 7. Palette de commandes

- `CommandPalette` reçoit la composition à afficher : `PaletteLayout::new` affiche
  `query` suivi du texte composé souligné, et place le caret après lui.
- Le soulignement se fait dans [chrome/scene.rs](../../../src/chrome/scene.rs) (rectangle
  sous le texte composé, autour de [scene.rs:270](../../../src/chrome/scene.rs:270)).
- Le filtre ne tient compte que de `query` : la composition ne filtre pas tant qu'elle
  n'est pas validée.

## Tests

- `ime.rs` / `frame.rs` : composition décalée au prompt, superposée en plein écran, coupée
  au bord, caractères larges, curseur placé ou caché.
- `keyboard.rs` : `commit_text` va à la palette quand elle est ouverte, sinon à toutes les
  cibles de saisie ; un remplacement de sélection a lieu en mode riche.
- `command_palette.rs` : `insert` filtre ; la composition s'affiche sans filtrer.
- Config : lecture de `[keyboard] ime`, défaut `true`, erreur sur une valeur non booléenne ;
  champ présent dans le modèle des réglages.
- Manuel : fcitx5 et ibus sous X11 et Wayland, IME japonaise et chinoise sous macOS ;
  champs texte de la fenêtre de réglages (vérification seule).

## Documentation

- `docs/en/configuration.md` et `docs/fr/configuration.md` : section `[keyboard]`, avec la
  note que sur macOS la désactivation empêche aussi les touches mortes de se combiner.
- `docs/en|fr/input-editor.md` : paragraphe sur la saisie IME.
- README (en et fr) : ligne dans les fonctionnalités.

## Dépendances

- [scrollback-search](../scrollback-search/overview.md) : son champ de recherche devra
  recevoir le texte validé et afficher la composition comme la palette (§4, §7).
- [kitty-keyboard-protocol](../kitty-keyboard-protocol/overview.md) : le texte validé
  n'est pas encodé par ce protocole (§4).

## Découpage en tâches d'implémentation

1. [#169](https://github.com/marmotz/graphene-term/issues/169) : option `[keyboard] ime`
   (config, fenêtre de réglages, doc de configuration) (§1).
2. [#170](https://github.com/marmotz/graphene-term/issues/170) : activation de l'IME,
   état de composition et livraison du texte validé aux panes (§2, §3, §4). Dépend de #169.
3. [#171](https://github.com/marmotz/graphene-term/issues/171) : affichage de la
   composition au curseur et position de la fenêtre de suggestions (§5, §6). Dépend de #170.
4. [#172](https://github.com/marmotz/graphene-term/issues/172) : saisie IME dans la
   palette de commandes (§4, §7). Dépend de #171.
5. [#173](https://github.com/marmotz/graphene-term/issues/173) : documentation et
   vérification manuelle des plateformes. Dépend de #171 et #172.
