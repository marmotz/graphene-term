# Easter eggs : conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## Organisation

Un module `src/easter_eggs/` regroupe la logique pure, testable sans fenêtre ni GPU :

| Fichier | Rôle |
| ------- | ---- |
| `mod.rs` | déclarations, constantes partagées (`HIDDEN_THEME`) |
| `konami.rs` | détecteur de la séquence Konami |
| `honeycomb.rs` | animation des hexagones (temps, géométrie, rects) |
| `secrets.rs` | mots-clés cachés de la palette et leurs effets |
| `special_days.rs` | date locale et symbole du jour |
| `art.rs` | dessin ASCII du graphène et mise en page de sa fenêtre |

Le câblage (clavier, overlays, titre, renderer, réglages) reste dans les modules
existants, avec le minimum de code.

## 1. Konami code

### Détection

`konami.rs` : `KonamiDetector { progress: usize }`, avec
`fn feed(&mut self, key: &Key) -> bool`. La séquence attendue est
`ArrowUp ArrowUp ArrowDown ArrowDown ArrowLeft ArrowRight ArrowLeft ArrowRight b a`,
où `b` et `a` sont des `Key::Character`, comparés sans tenir compte de la casse.
Une touche hors séquence remet `progress` à 0. Si c'est `ArrowUp`, elle compte comme
première touche, ce qui gère `↑↑↑↓↓…`. `feed` rend `true` sur la dernière touche et
remet le détecteur à zéro.

Point d'appel : `key_press` ([keyboard.rs:106](../../../src/app/keyboard.rs)), après le
`match` des overlays (un overlay ouvert prend toutes les touches, ligne 113) et après
le test du glisser de pane (ligne 139). On n'alimente le détecteur que si le pane actif
est en mode riche (`Pane::rich_mode`, [pane.rs:169](../../../src/pane.rs)). Ce mode
n'est actif qu'au prompt, avec l'intégration shell, et jamais en écran alternatif : vim,
htop ou une commande en cours ne déclenchent donc rien. Une touche reçue hors du mode
riche remet le détecteur à zéro.

**Décidé : les touches sont transmises normalement.** Le détecteur observe sans rien
consommer. Les flèches parcourent l'historique et `ba` reste tapé dans le prompt.
Alternative écartée : restaurer l'état de l'éditeur après la séquence. Cela demande de
sauvegarder et de restaurer l'éditeur, et pourrait annuler une vraie saisie.

Le détecteur vit dans la session (`App`, [app/mod.rs](../../../src/app/mod.rs)), pas dans
le pane. Changer de pane au milieu de la séquence la remet donc à zéro (la touche de
focus ne fait pas partie de la séquence).

### Animation

`honeycomb.rs` : `Honeycomb { started: Instant, area: Rect }`, avec la zone du pane actif
au moment du déclenchement.

- Durée totale : 1,6 s. Les hexagones apparaissent en vague depuis le centre de la zone
  (délai proportionnel à la distance, 0,6 s pour la vague), restent 0,4 s, puis
  s'effacent ensemble en 0,6 s.
- Pavage en nid d'abeille (lignes décalées d'une demi-largeur), côté fixe de
  24 px logiques, contour seulement (graphène : liaisons, pas d'aplat).
- Couleur : `palette.cursor`, alpha modulé par la progression.
- `fn rects(&self, now: Instant, scale: f32) -> Vec<RectInstance>` (pure, testée) et
  `fn deadline(&self, now) -> Option<Instant>` (`now + 16 ms` tant qu'elle tourne), sur
  le modèle de `FoldAnimations::deadline` ([fold_animation.rs:57](../../../src/fold_animation.rs)).

Rendu : nouvelle forme `Shape::Hexagon = 2` dans
[rects.rs:33](../../../src/renderer/rects.rs), et une branche `SHAPE_HEXAGON` dans
[rect.wgsl](../../../src/renderer/rect.wgsl) : distance signée à un hexagone régulier
inscrit dans la boîte, en contour d'épaisseur `radius`, comme `sd_chevron`. Les rects vont
dans `UiLayer::rects` ([renderer/mod.rs:38](../../../src/renderer/mod.rs)) : au-dessus des
panes, sous la bannière et les overlays (ordre de `draw`,
[renderer/mod.rs:197](../../../src/renderer/mod.rs)).

Réveil : une méthode `honeycomb_tick(now)` est ajoutée à la chaîne des `earliest(...)`
de `about_to_wait` ([application.rs:432](../../../src/app/application.rs)) et demande un
redraw tant que l'animation tourne. Une fois finie, elle passe à `None`. Une nouvelle
séquence pendant l'animation la relance.

## 2. Entrées cachées de la palette

### Modèle

`secrets.rs` :

```rust
pub enum Secret { Answer, Author, Graphene }

/// The secret whose keyword is exactly `query`, trimmed and ignoring case.
pub fn find(query: &str) -> Option<Secret>
```

Mots-clés : `42`, `marmotz`, `graphene`. La correspondance est exacte : `graph` ou
`4` ne font rien apparaître.

Dans [command_palette.rs](../../../src/chrome/command_palette.rs) :

- `Command` (ligne 60) gagne `Secret(Secret)`.
- Les entrées secrètes ne sont **pas** dans `entries` : la recherche floue
  (`refilter`, ligne 416) ne les voit jamais. À la fin de `refilter`, si
  `secrets::find(&self.query)` trouve un secret, une entrée est ajoutée **en tête** des
  correspondances, avec des `indices` vides (pas de surlignage) ; elle est donc
  sélectionnée.
- Pour garder `Match::entry` comme indice dans `entries`, l'entrée secrète est poussée à
  la fin de `entries` au premier besoin et retirée quand la requête change. Variante
  plus simple, retenue si elle reste lisible : un champ `secret: Option<Entry>` rendu
  par `entry()` pour un indice sentinelle `usize::MAX`. Le choix final se fait à
  l'implémentation ; les tests portent sur le comportement observable.
- Libellés traduits (nouveaux `Msg` dans [i18n.rs](../../../src/i18n.rs)) :
  - `SecretAnswer` : « The Answer to the Ultimate Question » / « La grande question sur
    la vie, l'univers et le reste ».
  - `SecretAuthor` : « marmotz on GitHub » / « marmotz sur GitHub ».
  - `SecretGraphene` : « Graphene » (identique dans les deux langues, à ajouter à la liste
    d'exceptions du test qui vérifie que les traductions diffèrent,
    [i18n.rs:992](../../../src/i18n.rs)).
- Pas de raccourci affiché. Une entrée secrète n'est jamais enregistrée dans les
  actions récentes : `remember` ne reçoit que des `Command::Action`.

### Effets

`palette_outcome` ([overlays.rs:301](../../../src/app/overlays.rs)) gagne une branche
`Command::Secret(secret)` :

| Secret | Effet |
| ------ | ----- |
| `Answer` | `opener::open(url)` ([opener.rs:9](../../../src/opener.rs)), URL selon la langue de l'interface |
| `Author` | `opener::open("https://github.com/marmotz")` |
| `Graphene` | ouvre l'overlay du dessin ASCII (section 3) |

URLs (vérifiées le 2026-11-21) :

- `Lang::Fr` : `https://fr.wikipedia.org/wiki/La_grande_question_sur_la_vie,_l%27Univers_et_le_reste`
- `Lang::En` : `https://en.wikipedia.org/wiki/Phrases_from_The_Hitchhiker%27s_Guide_to_the_Galaxy#The_Answer_to_the_Ultimate_Question_of_Life,_the_Universe,_and_Everything_is_42`

`fn answer_url(lang: Lang) -> &'static str` dans `secrets.rs`, testée pour chaque langue
(`LANGS`, [i18n.rs:762](../../../src/i18n.rs)), pour qu'une future langue ne puisse pas
être oubliée.

## 3. Fenêtre du dessin ASCII

Nouvel overlay `Overlay::Art` dans l'enum `Overlay` ([app/mod.rs:151](../../../src/app/mod.rs)),
à côté de `Confirm`, `Palette` et `Menu`. Aucune fenêtre système : c'est une boîte
dessinée dans la fenêtre principale, comme le dialogue de confirmation.

- Contenu : une constante `GRAPHENE_ART` dans `art.rs`, avec un réseau hexagonal
  (`\__/`) d'environ 6 lignes et le mot `graphene` dessous. Lignes en ASCII pur, de
  même longueur (test).
- Mise en page : `ArtLayout::new(window, metrics)`, sur le modèle de `DialogLayout`
  ([dialog.rs:134](../../../src/chrome/dialog.rs)) : boîte centrée, marges du dialogue,
  couleurs `palette_background` / `palette_text`.
- Police : le dessin doit être en chasse fixe. `UiFont`
  ([glyphs.rs:35](../../../src/renderer/glyphs.rs)) n'a que `Chrome` et `Palette`, deux
  polices proportionnelles. On ajoute `UiFont::Mono` (taille `PALETTE_FONT_SIZE`), mis en
  forme avec la famille de la grille. Alternative écartée : dessiner le texte dans des
  cellules de grille, ce qui le coupe de la pile d'overlays.
- Fermeture : `Escape` ou `Enter` dans `key_press` (nouvelle branche du `match` ligne 113,
  toute autre touche est ignorée), ou un clic n'importe où (`overlay_press`,
  [overlays.rs:334](../../../src/app/overlays.rs)).

## 4. Version dans les réglages

`crate::VERSION` ([lib.rs:54](../../../src/lib.rs)) contient déjà la version de build
(`0.2.1`, ou `0.2.1-dev.<horodatage>` pour un build de développement,
[build_version.rs](../../../src/build_version.rs)). Elle n'est lue que par la vérification
des mises à jour ([update/mod.rs:67](../../../src/update/mod.rs)).

Affichage : en bas de la barre latérale des sections (`Panel::left("settings_sections")`,
[settings/view/mod.rs:179](../../../src/settings/view/mod.rs)), un libellé discret
« Graphene 0.2.1 », aligné en bas (`ui.with_layout(Layout::bottom_up(..))`), en couleur
faible (`weak`). Il est cliquable (`Sense::click()`) sans en avoir l'air : pas de
changement de curseur.

## 5. Thème caché `phosphor-crt`

### Thème

- Fichier `themes/phosphor-crt.toml` : fond presque noir teinté de vert, texte vert
  phosphore, les 16 couleurs ANSI en nuances de vert (le rouge et le jaune restent
  distincts en luminosité pour garder les erreurs lisibles), interface assortie.
- [theme.rs](../../../src/theme.rs) : une constante `HIDDEN: &[(&str, &str)]` à côté de
  `BUILT_IN` (ligne 26). `built_in()` (ligne 337) cherche dans les deux, donc
  `theme::load("phosphor-crt", …)` fonctionne toujours, débloqué ou pas. Écrire
  `theme.name = "phosphor-crt"` à la main revient à le trouver autrement, ce qui est
  acceptable.
- `available(config_dir, unlocked: &[String])` (ligne 348) ajoute les thèmes cachés
  présents dans `unlocked` après les intégrés. Les 5 appels
  ([settings/mod.rs:176](../../../src/settings/mod.rs), ligne 290,
  [settings/model.rs:344](../../../src/settings/model.rs), ligne 425,
  [overlays.rs:133](../../../src/app/overlays.rs), ligne 163) passent
  `&config.theme.unlocked`.

### Déblocage enregistré

**Décidé : clé `theme.unlocked`**, une liste de noms de thèmes cachés.

- `ThemeConfig` ([config/values.rs:174](../../../src/config/values.rs)) gagne
  `unlocked: Vec<String>` (vide par défaut).
- [reader.rs:165](../../../src/config/reader.rs) : nouvelle branche
  `("theme", "unlocked")`, qui accepte une liste de chaînes. Les noms inconnus sont
  ignorés sans erreur : on ne révèle rien.
- La clé n'est **pas** documentée dans `config.default.toml` : la complétion de la config
  ([completion.rs](../../../src/config_edit/completion.rs)) ne l'ajoute donc pas en
  commentaire. Le test `every_key_is_read` ([config/mod.rs:408](../../../src/config/mod.rs))
  vérifie que les clés du fichier par défaut sont lues, pas l'inverse, il reste donc vert.
  À vérifier à l'implémentation : `--prune-config`
  ([prune.rs](../../../src/config_edit/prune.rs)) doit classer la clé comme connue (elle
  l'est si le classement passe par le lecteur) ; un test le confirme.

Alternative écartée : un fichier d'état dans `$XDG_STATE_HOME/graphene`, comme
`palette-recent`. Il ne suit pas la config quand l'utilisateur la synchronise entre deux
machines.

### Clics sur la version

Dans `View` ([settings/view/mod.rs:45](../../../src/settings/view/mod.rs)) :
`version_clicks: Vec<Instant>`. Une fonction pure `easter_eggs::rapid_clicks(clicks, now)`
garde les clics des 2 dernières secondes et rend `true` à partir de 7. Sur le 7e :

- `edits.changes` reçoit `Change::Set(["theme", "unlocked"], <liste + "phosphor-crt">)`,
  sauf si le thème est déjà débloqué ;
- et `Change::Set(<clé active>, "phosphor-crt")`, avec la clé active donnée par
  `ThemeConfig::active_key` ([values.rs:198](../../../src/config/values.rs)) : quand le
  thème suit l'apparence système, c'est le thème clair ou sombre courant qui est remplacé ;
- la liste des clics est vidée.

L'application principale recharge la config via le watcher existant
([watcher.rs](../../../src/watcher.rs)) et applique le thème : rien de spécifique à faire
de ce côté.

### Effet scanlines

**Décidé : lié au thème caché par son nom**, le format des fichiers de thème ne change
pas. Alternative écartée : une clé `effects.scanlines` dans le format de thème, qui
toucherait au parseur, à l'éditeur de thème et à sa documentation.

- Le renderer gagne un champ `scanlines: bool`, mis à jour partout où la palette active
  change : chargement et rechargement (`with_theme`,
  [application.rs:606](../../../src/app/application.rs)), aperçu de thème du menu
  contextuel (`MenuTheme`, [app/mod.rs:163](../../../src/app/mod.rs)). Valeur :
  `name == easter_eggs::HIDDEN_THEME`.
- Rendu : nouvelle forme `Shape::Scanlines = 3`. Un rect couvrant chaque pane est ajouté à
  la fin de sa plage `over` (dessinée après les glyphes,
  [renderer/mod.rs:228](../../../src/renderer/mod.rs)). Dans `rect.wgsl`, la branche
  `SHAPE_SCANLINES` rend du noir à alpha 0,18 sur une ligne de pixels physiques sur 3
  (`u32(in.position.y) % 3u == 0u`), et un alpha nul ailleurs. Il n'y a ni passe
  supplémentaire ni nouveau pipeline. La barre d'onglets et les overlays ne sont pas
  touchés.
- Pas d'impact quand le thème n'est pas actif : aucun rect n'est ajouté.

## 6. Dates spéciales dans le titre

### Date locale

Aucune dépendance de date dans [Cargo.toml](../../../Cargo.toml) ; `libc` y est déjà.
`special_days.rs` :

```rust
/// Local (month, day) now, from `libc::localtime_r`; `None` if it fails.
pub fn local_today() -> Option<(u32, u32)>
/// Time left until the next local midnight.
pub fn until_midnight() -> Option<Duration>
/// How a special day decorates the window title.
pub enum Decoration {
    /// A symbol before the title: `"🎄 <title>"`.
    Prefix(&'static str),
    /// A text after the title: `"<title> - May the fourth..."`.
    Suffix(&'static str),
}

/// Decoration of a special day.
pub fn decoration(month: u32, day: u32) -> Option<Decoration>
```

`decoration` est pure et testée pour les 6 dates et un jour ordinaire :

| Date | Décoration |
| ---- | ---------- |
| 01/01 | préfixe `🎉` |
| 14/02 | préfixe `❤️` |
| 14/03 | préfixe `π` |
| 04/05 | suffixe `May the fourth...`, en anglais dans toutes les langues (le jeu de mots ne se traduit pas) |
| 31/10 | préfixe `🎃` |
| 25/12 | préfixe `🎄` |

Le titre est du texte brut affiché par le gestionnaire de fenêtres, il ne peut donc pas
contenir d'image. Aucun caractère Unicode ne représente un sabre laser : d'où le texte
pour le 4 mai.

L'anniversaire de la v1 s'ajoutera à cette table le moment venu.

### Titre

`refresh_title` ([refresh.rs:182](../../../src/app/refresh.rs)) construit le titre via une
fonction pure `easter_eggs::titled(decoration: Option<&Decoration>, title: &str) -> String`,
qui rend `"🎄 <titre>"`, `"<titre> - May the fourth..."` ou `<titre>` tel quel. La même
décoration s'applique au titre initial `DEFAULT_TITLE`
([application.rs:155](../../../src/app/application.rs)) et à
[geometry.rs:95](../../../src/app/geometry.rs). La décoration du jour est mise en cache
dans la session (`today: Option<Decoration>`), pour ne pas appeler `localtime_r` à chaque
changement de titre.

Changement de jour : la session garde `midnight: Instant` (maintenant +
`until_midnight()`), ajouté à la chaîne `earliest(...)` de `about_to_wait`
([application.rs:432](../../../src/app/application.rs)). À l'échéance, le symbole est
recalculé, `refresh_title` est appelé et l'échéance avancée au minuit suivant. Une seule
échéance par jour, aucun timer actif en dehors.

## Conséquences vérifiées

- Aucune modification du flux PTY ni de `Term` : toutes les surprises sont côté interface.
- Les fichiers de thème existants restent valides et le format de thème ne change pas.
- La liste des thèmes ne change pas tant que `theme.unlocked` est vide : les tests
  existants de `available` restent valides une fois leur appel mis à jour.
- `Command` de la palette gagne une variante : le seul `match` exhaustif sur ce type,
  `palette_outcome` ([overlays.rs:315](../../../src/app/overlays.rs)), doit la traiter.
  `recent_rank` utilise un `let … else` et l'ignore déjà.
- `Shape` gagne deux variantes : l'ajout est rétrocompatible (les instances existantes
  gardent `shape` 0 ou 1).

## Tests

- `konami.rs` : séquence complète, séquence interrompue, `↑↑↑↓↓…`, casse de `B`/`A`,
  remise à zéro après déclenchement.
- `honeycomb.rs` : nombre de rects pour une zone donnée, alpha nul au début et à la fin,
  `deadline` à `None` après la durée.
- `secrets.rs` : correspondance exacte, espaces autour et casse ignorés, `graph` et `4`
  ne trouvent rien, `answer_url` pour chaque langue.
- `command_palette.rs` : `42` place l'entrée secrète en tête et la sélectionne, une
  requête partielle ne la montre pas, une entrée secrète n'entre pas dans les récents.
- `art.rs` : lignes ASCII et de même longueur ; `ArtLayout` centré et dans la fenêtre.
- `theme.rs` : `phosphor-crt` absent de `available` sans déblocage, présent avec ;
  chargeable dans les deux cas ; le fichier se lit sans erreur.
- `reader.rs` : `theme.unlocked` est lu, les noms inconnus sont ignorés sans erreur ;
  `--prune-config` garde la clé.
- `rapid_clicks` : 6 clics ne débloquent rien, 7 en moins de 2 s débloquent, 7 clics
  trop étalés ne débloquent rien.
- `special_days.rs` : `decoration` pour les 6 dates et un jour ordinaire, `titled` avec
  préfixe, avec suffixe et sans décoration.
- Rendu (tests GPU existants de [rects.rs](../../../src/renderer/rects.rs), fonction
  `draw` ligne 184) : un hexagone couvre son centre et pas ses coins, les scanlines
  assombrissent une ligne sur trois.

## Découpage en tâches d'implémentation

| Issue | Tâche | Dépend de |
| ----- | ----- | --------- |
| [#219](https://github.com/marmotz/graphene-term/issues/219) | Afficher la version de Graphene dans les réglages (section 4) | — |
| [#220](https://github.com/marmotz/graphene-term/issues/220) | Entrées cachées 42 et marmotz dans la palette (section 2) | — |
| [#221](https://github.com/marmotz/graphene-term/issues/221) | Fenêtre du dessin ASCII et police UI à chasse fixe (section 3) | #220 |
| [#222](https://github.com/marmotz/graphene-term/issues/222) | Thème caché `phosphor-crt` et clé `theme.unlocked` (section 5) | — |
| [#223](https://github.com/marmotz/graphene-term/issues/223) | Déblocage par 7 clics sur la version (section 5) | #219, #222 |
| [#224](https://github.com/marmotz/graphene-term/issues/224) | Effet scanlines du thème caché (section 5) | #222 |
| [#225](https://github.com/marmotz/graphene-term/issues/225) | Konami code et animation en nid d'abeille (section 1) | — |
| [#226](https://github.com/marmotz/graphene-term/issues/226) | Décoration du titre les jours spéciaux (section 6) | — |
