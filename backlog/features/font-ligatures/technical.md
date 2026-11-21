# Ligatures de police : conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## État actuel

- La grille est rendue caractère par caractère. [`scene::build`](../../../src/renderer/scene.rs)
  (scene.rs:381) émet un `GlyphInstance` par cellule non vide. Il porte une
  `GlyphKey { c, bold, italic, ui }` (glyphs.rs:59).
- [`Glyphs::rasterize`](../../../src/renderer/glyphs.rs) (glyphs.rs:327) met en forme ce
  caractère **seul** (`Buffer::set_text` d'un seul `char`, `Shaping::Advanced`), puis
  rastérise le premier glyphe. Aucune feature OpenType contextuelle (`calt`, `liga`) ne
  peut donc s'appliquer. Le cache `HashMap<GlyphKey, Option<Glyph>>` est indexé par
  caractère.
- `scene::build` est pur, sans accès au `FontSystem`. La résolution des glyphes se fait
  dans [`cells::resolve`](../../../src/renderer/cells.rs) (cells.rs:42), qui a
  `&mut Glyphs`.
- L'éditeur de saisie n'a pas de rendu propre ([input_editor.rs](../../../src/input_editor.rs)).
  C'est la ligne du shell dans la grille. Couvrir la grille couvre donc les deux zones
  retenues dans l'overview.
- Le curseur bloc est « cuit » dans les couleurs de la cellule (frame.rs:284). Les autres
  formes sont dessinées par-dessus (`Frame::cursor`, frame.rs:200). Pendant la phase
  cachée du clignotement, et pour un panneau sans focus, la forme est remplacée
  (frame.rs:274), et `Frame::cursor` vaut alors `None`.
- La sélection est dessinée comme un fond de cellule (`FrameCell::bg`).
- `FrameCell` ne contient qu'un `char`, sans caractères combinants. Un cluster de shaping
  correspond donc à une ou plusieurs cellules entières.
- cosmic-text 0.19 expose `Attrs::font_features(FontFeatures)`, avec
  `FontFeatures::set(FeatureTag::new(b"ss01"), value)`, transmis tel quel à harfrust
  (cosmic-text `src/shape.rs:156`).

## Configuration

Section `[font]` ([config/values.rs:107](../../../src/config/values.rs)) :

```toml
## Show the ligatures of fonts that have some (Fira Code, JetBrains Mono...).
# ligatures = true
## OpenType features, separated by spaces: "ss01" enables, "-calt" disables, "cv05=2" sets a value.
# features = ""
```

- `FontConfig` gagne `ligatures: bool` (défaut `true`) et `features: FontFeatureList`.
  `FontFeatureList` est un newtype sur `Vec<(FeatureTag, u32)>`, avec un `Display`
  canonique pour l'affichage dans les Réglages.
- Syntaxe d'un élément : `tag`, `+tag` → 1, `-tag` → 0, `tag=N` → N. Le tag fait 4
  caractères ASCII imprimables. Les séparateurs sont des espaces ou des virgules. Si le
  même tag apparaît plusieurs fois, la dernière occurrence gagne.
- Lecture dans [`read_section_key`](../../../src/config/reader.rs) (reader.rs:121) sur le
  modèle de `font.family`. Un élément invalide rend la valeur entière `Checked::Invalid`
  (message qui nomme l'élément fautif), et la valeur précédente est conservée, comme pour
  les autres clés.
- `config.default.toml` documente les deux clés. La complétion des fichiers existants
  ([config_edit/completion.rs](../../../src/config_edit/completion.rs)) les ajoute en
  commentaire, sans code spécifique.
- **Format chaîne plutôt que tableau TOML** : le modèle des Réglages ne connaît que
  `Bool/Integer/Float/Text` (settings/model.rs:70). Une chaîne s'édite dans un champ
  texte existant, comme `selection.word_separators`. Un tableau aurait demandé un type
  liste dans le modèle et dans l'édition du fichier.

## Réglages

Dans [settings/model.rs](../../../src/settings/model.rs) :

- `Field::FontLigatures` (`Widget::Toggle`, comme `CursorBlinking` à la ligne 237) et
  `Field::FontFeatures` (champ texte) sont ajoutés à `Section::Font` (ligne 146).
- Chemins `["font", "ligatures"]` et `["font", "features"]` (ligne 174). Les valeurs sont
  lues ligne 527.
- Libellés `SettingsField::FontLigatures` / `FontFeatures` dans
  [i18n.rs](../../../src/i18n.rs) (en, fr).

## Application à chaud

- `Glyphs::set_font(family, size)` (glyphs.rs:221) devient `set_font(&FontOptions, size)`,
  avec `FontOptions { family, ligatures, features }`. `Glyphs::new` prend le même
  `FontOptions` (application.rs:249).
- [refresh.rs:50](../../../src/app/refresh.rs) recharge la police quand `family`,
  `ligatures` ou `features` change. Chaque changement vide les caches
  (`Glyphs::clear`, nouvelle génération).
- Les attributs de la grille sont construits par une seule fonction
  `grid_attrs(bold, italic)`, qui ajoute `features`. Elle est utilisée par la
  mise en forme caractère par caractère comme par celle des runs : `ss01`, `zero`, etc.
  s'appliquent donc aussi quand les ligatures sont désactivées. Le texte de l'interface
  (`UiFont`, `shape_ui`) n'en reçoit pas.

## Mise en forme par runs

### Découpage (scène, pur)

Avec `ligatures = true`, `scene::build` regroupe les cellules d'une ligne en
`TextRun { origin, cells: Vec<RunCell { c, color }>, bold, italic, cursor: Option<usize> }`.
La scène gagne `runs: Vec<TextRun>` à côté de `glyphs`. Un run est une suite maximale de
cellules contiguës qui respectent toutes ces conditions :

- ni espace, ni spacer, ni cellule d'image ;
- pas de caractère large (cellule suivie d'un spacer) : ces caractères restent sur le
  chemin `GlyphInstance` actuel ;
- même style : `fg`, `bg`, `bold`, `italic`, `underline`, `strikeout`. Une sélection ou
  une coloration partielle coupe donc le run, conformément à l'overview.

Exception : la cellule du curseur n'entre pas dans la comparaison de style. Le curseur
bloc cuit ses couleurs dans la cellule, ce qui couperait sinon la ligature en deux
morceaux. Les morceaux voisins pourraient alors former leur propre ligature, par
exemple `-->` avec le curseur sur le premier `-` donnerait `-` + `->` ligaturé. La
couleur reste portée par cellule (`RunCell::color`).

Un run d'un seul caractère est émis directement en `GlyphInstance`, ce qui garde le
chemin rapide actuel. Avec `ligatures = false`, rien ne change : un `GlyphInstance` par
cellule. `scene::build` reçoit le booléen en paramètre (appel dans renderer/mod.rs:132).

Les runs s'arrêtent aux espaces : les polices de programmation ne forment pas de
ligature à travers un espace, et des runs de la taille d'un mot donnent un bon taux de
réussite du cache.

### Shaping (Glyphs)

`Glyphs::shape_run(text: &str, bold, italic) -> Rc<[RunGlyph]>`, avec
`RunGlyph { cell: u16, raster: CacheKey, ligated: bool }`.

- Le shaping est fait une fois par `(texte, bold, italic)` puis mis en cache dans
  `HashMap<(bool, bool), HashMap<Box<str>, Rc<[RunGlyph]>>>`, consulté avec un `&str`
  sans allocation. Le cache est vidé par `Glyphs::clear` et dès qu'il dépasse
  8192 entrées.
- Toutes les cellules d'un run font une colonne, donc l'indice de caractère est l'indice
  de cellule. Un cluster `start..end` est placé sur la cellule de `start`. Les cellules
  suivantes du cluster n'émettent rien : une ligature `liga` d'un seul glyphe déborde sur
  elles, comme un glyphe large aujourd'hui.
- Position : origine de la cellule du cluster, et non le `x` mis en forme. La grille
  reste alignée même avec une police de repli non monospace. Le `CacheKey` est calculé
  par `glyph.physical((-glyph.x, 0.0), 1.0)`, avec un bin sous-pixel nul.
- `ligated` vaut vrai si le cluster couvre plusieurs caractères, ou si `(font_id,
  glyph_id)` diffère de celui du caractère mis en forme seul. Ce deuxième cas couvre les
  ligatures `calt` (Fira Code, JetBrains Mono), où chaque cellule garde son propre glyphe
  substitué. Le glyphe isolé de chaque caractère est mémorisé avec le cache par
  caractère existant.

### Rastérisation

`rasterize` est scindé en deux : `char → CacheKey` (mise en forme isolée, chemin
actuel) et `CacheKey → Option<Glyph>` (bitmap + atlas). Un second cache
`raster: HashMap<CacheKey, Option<Glyph>>` est partagé par les deux chemins. La
gestion d'atlas plein, `generation` et `take_uploads` restent inchangées.
`cells::resolve_groups` résout les runs après les glyphes, avec la même reprise en cas
de reset d'atlas.

## Curseur

- `Frame` gagne `ligature_break: Option<(usize, usize)>` (ligne, colonne) : la position
  du curseur du terminal, renseignée dès qu'elle est dans la vue et que l'application ne
  l'a pas masquée (`content.cursor.shape != Hidden`). Elle est calculée **avant**
  l'override de clignotement et de focus (frame.rs:274). La ligature reste donc cassée
  pendant la phase cachée et dans un panneau sans focus.
- `scene::build` renseigne `TextRun::cursor` pour le run qui contient cette cellule.
- À la résolution, la portée cassée est la plus longue suite contiguë de `RunGlyph`
  `ligated` qui contient la cellule du curseur. Ces cellules sont dessinées avec leur
  glyphe isolé (`GlyphKey` par caractère). Si la cellule du curseur n'est pas ligaturée,
  rien ne change.

## Alternatives écartées

- **Shaper les lignes entières à chaque frame, sans cache** : coût de harfrust sur
  toute la grille à chaque redraw, alors que #168 vient d'optimiser le rendu.
- **Shaper dans `scene::build`** : la scène perdrait sa pureté et ses tests sans
  `FontSystem`.
- **Couper le run à la cellule du curseur** : laisse des ligatures partielles autour du
  curseur (`-` + `->`), contraire à la décision produit.
- **Désactiver `calt`/`liga` quand `ligatures = false`** : inutile, la mise en forme
  caractère par caractère ne peut pas former de ligature. Cela aurait aussi désactivé
  des alternances contextuelles que l'utilisateur peut vouloir via `features`.
- **Tableau TOML pour `features`** : voir Configuration.

## Conséquences vérifiées

- La bannière, le résumé des lignes masquées (scene.rs) et le texte de l'interface
  (`ui_text.rs`, `shape_ui`) gardent le chemin par caractère. La bannière reçoit
  `features`, mais pas de ligatures.
- Les tests de scène qui comptent les glyphes (`placed`, `glyphs_are_anchored_...`,
  `wide_char_is_drawn_once_...`) passent `ligatures = false`, ou lisent aussi `runs`.
- La taille de cellule (`measure`, glyphs.rs:173) ne change pas : elle est mesurée sur
  `M`, sans contexte.
- Pas de migration : les deux clés sont nouvelles, avec des valeurs par défaut.

## Tests

- Police : JetBrains Mono Regular (OFL, fichier de licence joint) dans
  `tests/fixtures/fonts/`, chargée dans le `fontdb` par un constructeur de test de
  `Glyphs`. Les tests sont déterministes sans dépendre des polices du système.
- Config : parsing de `ligatures` et `features` (syntaxes valides, dernière occurrence
  gagnante, élément invalide → valeur précédente conservée), `DEFAULT_CONFIG` toujours
  égal à `Config::default()`.
- Scène : découpage des runs (espaces, styles, caractère large, run d'un caractère),
  exception de style pour la cellule du curseur, `ligature_break` pendant la phase
  cachée.
- Glyphs : `->` / `===` donnent des `RunGlyph` `ligated` avec la police de test, `ab`
  non, mise en cache (pas de nouveau shaping au second appel), vidage au changement de
  features.
- Résolution : portée cassée autour du curseur, cellules hors portée inchangées.
- Réglages : nouveaux champs, chemins et widgets.

## Découpage en tâches d'implémentation

1. [#182](https://github.com/marmotz/graphene-term/issues/182) : options `[font] ligatures`
   et `features`, `FontOptions`, application à la police de la grille, police de test.
2. [#185](https://github.com/marmotz/graphene-term/issues/185) : interrupteur et features
   dans la fenêtre Réglages (dépend de #182).
3. [#186](https://github.com/marmotz/graphene-term/issues/186) : mise en forme de la grille
   par runs, avec cache (dépend de #182).
4. [#190](https://github.com/marmotz/graphene-term/issues/190) : ligature sous le curseur
   en caractères isolés (dépend de #186).
