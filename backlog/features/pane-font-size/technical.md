# Per-pane font size — conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## État actuel

- **Une seule taille pour toute la fenêtre.** `Session::font_size` (taille logique) est
  appliquée par `apply_font_size` ([src/app/refresh.rs:78](../../../src/app/refresh.rs)) :
  `renderer.set_font_size(font_size * scale)` puis `relayout()`.
- **Actions** : `FontSizeIncrease` / `FontSizeDecrease` / `FontSizeReset`
  ([src/app/keyboard.rs:94-96](../../../src/app/keyboard.rs)) modifient `self.font_size`
  de ±1 ou la remettent à `default_font_size` (la taille configurée).
- **Rechargement de la config** : `reloaded_font_size`
  ([src/app/application.rs:81](../../../src/app/application.rs)) garde la taille courante
  tant que `font.size` ne change pas dans la config.
- **Glyphes** : `Glyphs` ([src/renderer/glyphs.rs](../../../src/renderer/glyphs.rs)) porte
  une seule paire `metrics` / `cell` pour la grille. Les caches de shaping `chars`
  (clé `GlyphKey { c, bold, italic, ui }`, l.72) et `runs` (clé `(bold, italic)` puis le
  texte, l.193) ne contiennent pas la taille. `set_font_size` (l.347) vide tout le cache.
  Le cache de rasterisation `raster` est indexé par `cosmic_text::CacheKey`, qui contient
  déjà la taille (`font_size_bits`).
- **Layout** : chaque pane a déjà son propre `GridLayout` avec son `cell`
  ([src/layout.rs:54](../../../src/layout.rs)), et `scene::build` dessine à partir de
  `layout.cell` ([src/renderer/scene.rs:454](../../../src/renderer/scene.rs)). Mais
  `relayout` ([src/app/refresh.rs:9](../../../src/app/refresh.rs)) donne à tous les panes
  la même cellule `geometry.cell = renderer.cell_size()`.
- **Molette** : `Session::mouse_wheel` ([src/app/pointer.rs:446](../../../src/app/pointer.rs))
  transmet la molette au pane survolé. `Pane::mouse_wheel`
  ([src/pane.rs:834](../../../src/pane.rs)) l'envoie au programme en mouse mode (avec
  les modificateurs, Ctrl compris), sinon fait défiler.
- **Session** : `NodeSnapshot::Pane { cwd }` ([src/session.rs:91](../../../src/session.rs)),
  écrit par `node_snapshot` ([src/workspace/mod.rs:906](../../../src/workspace/mod.rs)) via
  le trait `TitleSource`, relu par `Workspace::restore`
  ([src/workspace/mod.rs:307](../../../src/workspace/mod.rs)) avec un `spawn(id, cwd)`.

## Modèle

Chaque `Pane` porte un **écart** `font_offset: f32` (points logiques, `0.0` par défaut)
par rapport à la taille configurée `Session::font_size`. Sa taille effective est :

```rust
fn pane_font_size(base: f32, offset: f32) -> f32 {
    (base + offset).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
}
```

- Ctrl+= / Ctrl+- : `offset = pane_font_size(base, offset ± 1.0) - base`. L'écart reste
  donc toujours dans les bornes : à 72 pt, Ctrl+= ne fait rien.
- Ctrl+0 : `offset = 0.0`.
- Changement de `font.size` dans les réglages : `base` change, `offset` reste. C'est la
  décision "écart conservé". Un pane non ajusté (`offset == 0`) suit le réglage.
- `Session::font_size` devient exactement la taille configurée : plus aucun raccourci ne
  la modifie. `reloaded_font_size` disparaît : la nouvelle valeur est `new.font.size`.

**Alternative écartée : stocker une taille absolue par pane.** L'écart conservé
demanderait alors de recalculer toutes les tailles à chaque rechargement, et "non ajusté"
serait une notion à part (`Option<f32>`). L'écart rend les deux cas naturels.

## Rendu : plusieurs tailles dans un seul `Glyphs`

**Retenu** : un seul `Glyphs` (une `FontSystem`, un atlas), dont les caches de grille
sont indexés par taille.

- Nouveau type `GridSize` (taille physique en pixels, stockée en `u32` via `f32::to_bits`
  pour être `Eq + Hash`).
- `GlyphKey` reçoit un champ `size: GridSize` pour les glyphes de grille. Il est ignoré
  pour les glyphes `ui`, à normaliser dans `GlyphKey::ui` pour ne pas dupliquer le cache.
- `runs` est indexé par `(GridSize, bold, italic)`. `MAX_CACHED_RUNS` reste un plafond
  global.
- `Glyphs` garde une table `grids: HashMap<GridSize, (Metrics, CellSize)>` remplie à la
  demande par `measure`. `cell_size()` / `metrics()` restent ceux de la taille de base,
  utilisés par le bandeau, la géométrie et la taille initiale de la fenêtre. Ajouter
  `cell_size_at(size)`.
- `set_font_size(base)` ne vide plus tout le cache : il change la taille de base, et les
  entrées des autres tailles restent valides. `set_font` (famille, ligatures, features)
  continue de tout vider.
- Le cache `raster` et l'atlas sont partagés sans changement, puisque `CacheKey` contient
  déjà la taille. Un atlas plein est déjà réinitialisé (`reset_atlas`). Plusieurs
  tailles en même temps le remplissent seulement plus vite.

**Alternative écartée : un `Glyphs` par taille.** Chaque instance a sa propre
`FontSystem` (des dizaines de Mo de polices chargées), son atlas et ses textures GPU,
pour un gain nul puisque la rasterisation est déjà indexée par taille.

Plomberie du rendu :
- `PaneFrame` ([src/renderer/mod.rs:48](../../../src/renderer/mod.rs)) reçoit
  `font_size: GridSize`, passé à `scene::build`. `build` le recopie dans les `GlyphKey`
  de grille ([src/renderer/scene.rs:356](../../../src/renderer/scene.rs)) et dans
  `TextRun` (nouveau champ `size`, l.48).
- `resolve_runs` ([src/renderer/cells.rs:128](../../../src/renderer/cells.rs)) appelle
  `shape_run(text, run.size, bold, italic)`.
- Le bandeau (`scene.rs:595`) reste à la taille de base.

## Layout

- `relayout` calcule pour chaque pane
  `renderer.cell_size_at(pane_font_size(base, pane.font_offset) * scale)` et la passe à
  `GridLayout::new`. `cell_size_at` mesure à la demande, ce qui demande `&mut renderer`.
  `relayout` a déjà `&mut self`.
- Le partage de l'espace entre panes (`tree.rs` : taille minimale l.53, arrondi des ratios
  l.478, pas du redimensionnement clavier l.575) reste en **cellules de base**
  (`geometry.cell`). Un pane à grande police reçoit le même rectangle : il a moins de
  colonnes, et `GridLayout` centre déjà les pixels restants. Sous le minimum
  (`MIN_COLUMNS`, `MIN_SCREEN_LINES`), `GridLayout` garde le minimum et le contenu est
  coupé par le scissor du pane, comme aujourd'hui pour une fenêtre trop petite.
- `apply_font_size(base)` (rechargement, changement d'échelle à
  [src/app/application.rs:774](../../../src/app/application.rs)) garde la même forme :
  base, échelle de l'UI, puis `relayout`, qui recalcule tous les panes.
- Nouvelle méthode `Session::set_pane_font_offset(id, offset)` : met à jour le pane puis
  appelle `relayout`. `pane.resize` envoie déjà la nouvelle taille au PTY (SIGWINCH) et
  aux images (`set_cell_size`).

## Entrées

- **Clavier** ([src/app/keyboard.rs:94-96](../../../src/app/keyboard.rs)) : les trois
  actions agissent sur `workspace.active_pane_id()` via `set_pane_font_offset`. Leurs
  libellés i18n "Agrandir la police" / "Taille de police par défaut" restent justes.
- **Palette** : `Context { font_size, default_font_size }`
  ([src/chrome/command_palette.rs:157](../../../src/chrome/command_palette.rs), rempli à
  [src/app/overlays.rs:48](../../../src/app/overlays.rs)) prend la taille effective du
  pane actif. Les règles d'affichage (l.192-198) ne changent pas.
- **Ctrl+molette** : dans `Session::mouse_wheel`, après les retours anticipés des overlays
  (menu, palette, autre overlay) et avant `pane.mouse_wheel`, si
  `modifiers.control_key()` : changer l'écart du pane sous la souris (`pane_under_mouse`)
  et ne rien transmettre au pane. Le programme ne reçoit donc jamais Ctrl+molette, même
  en mouse mode.
  - Pas : 1 pt par cran. `LineDelta(_, y)` donne `y` crans. `PixelDelta` est accumulé
    dans un `font_wheel_accumulator` de la session, avec un cran par hauteur de cellule
    du pane, comme le `scroll_accumulator` du pane.
  - Molette vers le haut (`y > 0`) agrandit.

## Création de panes

- **Split** : `new_pane_start` ([src/app/panes.rs:47](../../../src/app/panes.rs)) renvoie
  aussi le `font_offset` du pane actif. `pane_spawner` le donne à `Pane::new`. Le premier
  layout (celui du pane actif) a déjà la bonne cellule.
- **Nouvel onglet** : même chemin, avec un écart forcé à `0.0`. Le premier layout du
  pane actif peut avoir une autre cellule : il est corrigé par le `relayout` qui suit
  (`focus_changed`), avant tout rendu. Le shell démarre avec une taille de PTY
  provisoire, ce qui arrive déjà aujourd'hui lors d'un split.
- **Détacher / glisser un pane** vers un autre onglet : l'objet `Pane` est déplacé avec
  son écart, rien à faire.

## Session

- `NodeSnapshot::Pane { cwd, #[serde(default, skip_serializing_if = "is_zero")] font_offset: f32 }`.
  Rétrocompatible dans les deux sens : un ancien fichier donne `0.0`, et un ancien binaire
  ignore le champ (pas de `deny_unknown_fields`). `SESSION_VERSION` reste à 1.
- `TitleSource` (ou un trait voisin) expose `font_offset()`, lu par `node_snapshot`.
- `Workspace::restore` passe le `font_offset` à `spawn`, qui devient
  `spawn(id, cwd, font_offset)`. Appelant :
  [src/app/application.rs:437](../../../src/app/application.rs).

## Conséquences vérifiées

- **Ctrl+= ne change plus toute l'app** : voulu. Aucun autre appelant de
  `apply_font_size` n'est lié aux raccourcis.
- **Images (kitty, sixel)** : `graphics.set_cell_size` est par pane
  ([src/pty.rs:276](../../../src/pty.rs)). Une image s'affiche à la cellule de son pane.
- **Mouse mode** : Ctrl+molette n'est plus transmis. Un programme qui l'utilisait (rare)
  ne le reçoit plus. C'est la décision produit.
- **Quick terminal** : il utilise la même `Session`, donc il a le même comportement sans
  code dédié.
- **Mémoire** : quelques tailles en même temps, c'est une table de métriques et des
  glyphes en plus dans l'atlas partagé. Pas de nouvelle texture.

## Tests

- `glyphs.rs` : deux tailles donnent deux `cell_size_at` différents, le même caractère à
  deux tailles donne deux glyphes distincts, `set_font_size` ne vide plus les autres
  tailles.
- `cells.rs` / `scene.rs` : `build` met la taille du `PaneFrame` dans les clés et les
  runs, et le bandeau garde la taille de base.
- Fonction pure `pane_font_size` + calcul d'écart : bornes, Ctrl+0, base qui change.
- `session.rs` : aller-retour avec `font_offset`, lecture d'un fichier sans le champ,
  champ absent à l'écriture quand il vaut 0.
- `workspace/mod.rs` : `restore` transmet l'écart à `spawn`, `snapshot` le relit.
- Molette : décision Ctrl+molette (police) contre molette seule (défilement), extraite en
  fonction pure testable (crans à partir d'un `MouseScrollDelta` et d'un accumulateur).

## Découpage en tâches d'implémentation

1. [#256](https://github.com/marmotz/graphene-term/issues/256) : cache de glyphes indexé
   par taille (`GridSize`, `cell_size_at`, `PaneFrame::font_size`). Aucun changement
   visible.
2. [#257](https://github.com/marmotz/graphene-term/issues/257) : écart de police par
   pane, relayout, raccourcis clavier, palette, split et nouvel onglet. Dépend de #256.
3. [#258](https://github.com/marmotz/graphene-term/issues/258) : Ctrl+molette sur le
   pane survolé. Dépend de #257.
4. [#259](https://github.com/marmotz/graphene-term/issues/259) : restauration de l'écart
   avec la session. Dépend de #257.
