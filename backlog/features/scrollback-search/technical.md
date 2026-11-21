# Recherche dans l'historique : conception technique

Décisions produit : [overview.md](overview.md).

## Vue d'ensemble

| Élément | Où | Rôle |
|---|---|---|
| Moteur de recherche | nouveau `src/search.rs` (pur) | Requête, construction de la regex, collecte des occurrences par grille, filtres, occurrence courante |
| État de recherche du pane | `src/pane.rs` (champ `search`) | Requête, résultats, focus clavier de la barre |
| Barre de recherche | nouveau `src/chrome/search_bar.rs` (pur) + `src/chrome/scene.rs` | Mise en page, zones cliquables, dessin |
| Surlignage | `src/frame.rs` | Couleurs des occurrences appliquées après la capture |
| Navigation | `src/view.rs` | Rendre visible une ligne précise d'un bloc |
| Raccourcis | `src/keybindings.rs`, `src/input.rs`, `src/app/keyboard.rs` | Nouvelle action, pliage déplacé, routage des touches |
| Couleurs | `src/palette.rs`, `src/theme.rs`, `themes/*.toml`, `src/i18n.rs` | Deux nouvelles clés `[ui]` |

## Ce qui existe

- Chaque bloc terminé possède deux terminaux figés, `header` (prompt + commande) et
  `output`, plus `command` (texte envoyé par le shell) et `status`
  ([src/timeline.rs:66](../../../src/timeline.rs:66)). Le bloc courant a un `header`
  optionnel ; son contenu vivant est la grille de la session
  ([src/timeline.rs:102](../../../src/timeline.rs:102)).
- `Timeline::frozen` parcourt déjà tous les terminaux figés dans l'ordre
  ([src/timeline.rs:202](../../../src/timeline.rs:202)). `view::frozen_term` retrouve le
  terminal d'une `RowSource` ([src/view.rs:591](../../../src/view.rs:591)).
- Sans intégration shell, rien n'est détaché : tout reste dans la grille vivante du bloc
  courant ([src/timeline.rs:8](../../../src/timeline.rs:8)).
- Le surlignage d'une partie de ligne après capture existe pour les liens :
  `Frame::underline_link` ([src/frame.rs:570](../../../src/frame.rs:570)), appelé dans
  `redraw` ([src/app/redraw.rs:167](../../../src/app/redraw.rs:167)). Le cache de frame
  est indexé par `FrameKey` ([src/app/redraw.rs:139](../../../src/app/redraw.rs:139)).
- `view::reveal` fait défiler la timeline pour montrer un bloc
  ([src/view.rs:564](../../../src/view.rs:564)) ; un bloc plus haut que la vue a son propre
  défilement de sortie (`Block::scroll`, `scroll_output` dans
  [src/view.rs:472](../../../src/view.rs:472)).
- Les touches vont à l'overlay ouvert (palette, menu, dialogue) avant tout
  ([src/app/keyboard.rs:113](../../../src/app/keyboard.rs:113)). La palette a un champ de
  saisie minimal sans curseur ([src/chrome/command_palette.rs:349](../../../src/chrome/command_palette.rs:349)).
- `alacritty_terminal` 0.26 fournit `RegexSearch` (DFA paresseux de `regex-automata`)
  et `RegexIter`, qui gèrent les lignes repliées (`WRAPLINE`) et les caractères larges
  (`~/.cargo/registry/.../alacritty_terminal-0.26.0/src/term/search.rs`, l. 25 et 620).
  `RegexSearch::new` applique une casse « intelligente » : insensible sauf si le motif
  contient une majuscule (l. 38-39).

## Moteur : `src/search.rs`

### Requête

```rust
pub struct Query {
    pub text: String,
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    pub zone: Zone,          // Both | Command | Output
    pub status: StatusFilter, // All | Success | Failure
}
```

### Construction de la regex

- Mode littéral : le texte est échappé (fonction d'échappement locale des
  métacaractères, pour ne pas ajouter `regex-syntax` en dépendance directe).
- Casse : on préfixe `(?i)` ou `(?-i)`. Un drapeau en ligne l'emporte sur la casse
  intelligente d'alacritty, donc le comportement suit la bascule et non la présence de
  majuscules.
- Regex invalide : `RegexSearch::new` renvoie une erreur ; la barre l'affiche (bordure
  et compteur en couleur d'erreur, texte « Regex invalide »), aucun résultat.
- Texte vide : aucun résultat, pas de surlignage.

### Mot entier

Retenu : filtrer après coup les occurrences dont le caractère avant le début ou après
la fin (en suivant les lignes repliées) est alphanumérique ou `_`.

Écarté : entourer le motif de `\b`. Le `\b` Unicode n'est pas supporté par le DFA
paresseux (la recherche échoue sur un octet non ASCII), et le `\b` ASCII
(`(?-u:\b)`) coupe les mots accentués (« déjà »).

### Grilles parcourues

Dans l'ordre du haut vers le bas :

1. Pour chaque bloc terminé : `header` puis `output`.
2. Le `header` du bloc courant, quand il existe.
3. La grille vivante, sauf quand elle contient la ligne de commande en cours d'édition
   (`Current::editing`, phase `Prompt`) : c'est la sortie de la commande en cours, ou
   tout le contenu quand il n'y a pas d'intégration shell.

Plein écran (`TermMode::ALT_SCREEN`, [src/view.rs:305](../../../src/view.rs:305)) :
`Ctrl+Shift+F` ne fait rien, et une barre déjà ouverte se ferme à l'entrée en plein
écran.

### Filtres

- **Zone sortie** : `output` des blocs et grille vivante en phase `Running`.
- **Zone commande** : seulement la partie commande du `header` (voir ci-dessous).
- **Statut** : `Success` = `Status::Success` ; `Failure` = `Status::Failure(_)`. Avec un
  filtre de statut actif, les blocs `Unknown`, `Cancelled`, `Empty` et le bloc en cours
  sont exclus.
- Sans intégration shell (aucun bloc), les filtres de zone et de statut sont grisés.

### Partie commande d'un en-tête

Le shell ne signale pas la fin du prompt : seul le début est connu
(`Current::prompt_start`, [src/timeline.rs:139](../../../src/timeline.rs:139)).

Retenu : retrouver `Block::command` à la fin du `header`. On cherche (en littéral) la
première ligne de `command` en partant de la fin du `header` ; la partie commande va du
début de cette occurrence à la fin du `header`. Les lignes suivantes d'une commande
multiligne sont couvertes car elles suivent. Le point de départ est calculé une fois par
bloc et mis en cache avec les résultats.

Repli : quand `command` est vide (le hook ne l'a pas envoyée) ou introuvable, la partie
commande est tout le `header`.

Écartés :
- Enregistrer la position du curseur à la première frappe : faux avec un collage, un
  rappel d'historique ou un prompt à droite.
- Ajouter un signal « fin du prompt » aux scripts d'intégration : modifie les trois
  scripts shell pour un seul usage ; à reconsidérer si un autre besoin apparaît.

### Résultats

```rust
pub struct Match {
    pub block: Option<BlockId>,
    pub source: RowSource,           // Header(id) | Output(id) | Live
    pub range: RangeInclusive<Point>, // lignes de la grille vivante comptées depuis le haut de son historique
}
```

- Lignes de la grille vivante en coordonnées absolues (depuis le haut de l'historique),
  comme `TimelineSelection` ([src/timeline_selection.rs:32](../../../src/timeline_selection.rs:32)),
  pour qu'une occurrence reste sur son texte quand de la sortie arrive.
- Plafond de 10 000 occurrences ; au-delà le compteur affiche « 10000+ » et la
  collecte s'arrête.
- Cache par grille figée, clé `(BlockId, header|output, colonnes, requête)`. Un bloc
  terminé ne change plus sauf au reflow (changement de colonnes,
  [src/timeline.rs:280](../../../src/timeline.rs:280)), donc seule la grille vivante est
  re-parcourue quand de la sortie arrive. Un bloc retiré par la limite d'historique
  disparaît du cache.

### Occurrence courante et numérotation

- Numérotation depuis le bas : 1 = occurrence la plus récente. `Entrée` va vers la
  plus ancienne (indice + 1), `Maj+Entrée` vers la plus récente, avec bouclage.
- À la frappe : l'occurrence courante devient la plus récente qui commence au-dessus du
  bas de la vue, sinon la plus récente.
- Après un nouveau calcul (sortie live, reflow), l'occurrence courante est retrouvée
  par `(source, début)` ; si elle a disparu, on prend la suivante vers le plus ancien.
  La vue ne bouge pas.

## Navigation : rendre visible une occurrence

Nouveau `view::reveal_line(timeline, live, lines, source, line)` :

1. Si le bloc est plié, le déplier (`Timeline::toggle_fold`).
2. Si l'occurrence est dans la sortie d'un bloc plus haut que la vue, régler
   `Block::scroll` (ou `Current::scroll`) pour que la ligne soit dans la fenêtre de
   sortie montrée, en réutilisant le calcul de `body_window`
   ([src/view.rs:130](../../../src/view.rs:130)).
3. Appeler `reveal` pour le bloc, puis ajuster `Timeline::scroll` si la ligne reste
   hors de la vue.

Pour la grille vivante, même logique avec le défilement du bloc courant.

À la fermeture (`Échap`), rien n'est restauré : la vue reste où elle est.

## État dans le pane

`Pane` ([src/pane.rs:43](../../../src/pane.rs:43)) reçoit :

```rust
pub search: Option<SearchState>, // None = barre fermée
```

`SearchState` contient la requête, le champ de saisie (texte + curseur), les résultats,
l'indice courant, l'erreur éventuelle, `focused: bool` (la barre a le clavier) et la
révision de contenu vue au dernier calcul.

Recalcul : à chaque modification de la requête, et à chaque frame où la révision du
pane (`workspace.revision(id)`) a changé alors que la barre est ouverte.

## Barre de recherche

### Mise en page : `src/chrome/search_bar.rs`

Module pur, sur le modèle de `pane_bar.rs` : `SearchBarLayout::new(content_rect,
state, metrics)` place une barre en haut à droite de la zone de contenu du pane (sous
sa barre de titre), largeur bornée (max ~420 px logiques, réduite si le pane est
étroit). Contenu : champ, compteur « 3/12 », trois bascules (`Aa`, `.*`, `ab`), sélecteur
de zone, sélecteur de statut, boutons précédent/suivant, fermer. `hit(x, y)` renvoie la
zone cliquée.

Dessin dans `src/chrome/scene.rs`, avec les couleurs de la palette de commandes
(`palette_background`, `palette_text`, `palette_match` pour une bascule active). La
barre est dessinée dans la couche UI au-dessus des cellules, comme la palette.

### Champ de saisie

Petit éditeur de ligne propre à la barre : insertion au curseur, `Gauche`/`Droite`,
`Origine`/`Fin`, `Retour arrière`, `Suppr`, `Ctrl+Retour arrière` (mot précédent),
collage `Ctrl+Shift+V` (sans retours à la ligne). Pas d'historique.

IME : voir [Ordre des features](overview.md#ordre-des-features). Le champ reçoit
le texte validé comme une frappe ; le texte en composition est affiché au curseur du
champ si ime-input est livrée.

### Clavier et focus

Dans `key_press` ([src/app/keyboard.rs:106](../../../src/app/keyboard.rs:106)), après les
overlays et avant les raccourcis : si le pane actif a une barre ouverte et `focused`,
la touche va à la barre.

| Touche | Effet |
|---|---|
| `Entrée` / `Maj+Entrée` | Occurrence plus ancienne / plus récente |
| `Échap` | Ferme la barre |
| `Ctrl+Shift+V` | Colle dans le champ |
| Touches d'édition | Champ de saisie |
| `Ctrl+Shift+F` | Sélectionne tout le texte du champ |
| Autres raccourcis (onglets, panes, palette...) | Inchangés |

Le reste des touches ne va pas au shell tant que la barre a le focus.

Souris :
- Clic dans le contenu du pane : `focused = false`, le clavier revient au shell, barre
  et surlignage restent.
- Clic sur la barre ou `Ctrl+Shift+F` : `focused = true`.
- La barre suit son pane : changer de pane actif ne la ferme pas.

## Surlignage

Nouveau `Frame::highlight_matches(&mut self, view, matches, current, colors)` sur le
modèle de `underline_link` : change le fond des cellules couvertes, `search_current_background`
pour l'occurrence courante, `search_match_background` pour les autres. Le texte garde sa
couleur. Appelé dans `redraw` après la capture, uniquement pour les occurrences des
sources présentes dans la vue.

`FrameKey` ([src/app/redraw.rs:11](../../../src/app/redraw.rs:11)) reçoit une révision de
l'état de recherche du pane, incrémentée à chaque recalcul ou changement d'occurrence
courante, pour invalider le cache.

## Couleurs du thème

- `UiColors` ([src/palette.rs:29](../../../src/palette.rs:29)) : `search_match_background`,
  `search_current_background`.
- `ColorKey` ([src/theme.rs:135](../../../src/theme.rs:135)) : deux variantes, chemins
  `["ui", "search_match_background"]` et `["ui", "search_current_background"]`, visibles
  dans l'éditeur de thème.
- Libellés en/fr dans `src/i18n.rs` (comme `PaletteMatch`,
  [src/i18n.rs:711](../../../src/i18n.rs:711)).
- Valeurs dans les 8 fichiers `themes/*.toml`. Un thème utilisateur sans ces clés garde
  la valeur du thème de base (comportement existant, [src/theme.rs:5](../../../src/theme.rs:5)).
- Contraste : les tests de thème vérifient déjà la lisibilité de couleurs d'UI
  ([src/theme.rs:683](../../../src/theme.rs:683)) ; on ajoute la même vérification texte
  au premier plan / fond d'occurrence.

## Raccourcis

| Action | Avant | Après |
|---|---|---|
| `open_search` (nouvelle) | - | `Ctrl+Shift+F` |
| `toggle_fold` | `Ctrl+Shift+F` | `Ctrl+Shift+Z` |
| `toggle_fold_all` | `Ctrl+Shift+Alt+F` | `Ctrl+Shift+Alt+Z` |

Fichiers :
- `DEFAULTS` ([src/keybindings.rs:114](../../../src/keybindings.rs:114)) et les tests
  ([src/keybindings.rs:665](../../../src/keybindings.rs:665)).
- `Action::OpenSearch` dans `src/input.rs` : enum, `ALL` (38 → 39,
  [src/input.rs:63](../../../src/input.rs:63)), nom `open_search`, `Msg` associé.
- `Msg::OpenSearch` dans `src/i18n.rs` : libellés en/fr, `variant_index`, `all_messages`.
- Liste des actions autorisées en tête de
  [keybindings.default.toml](../../../keybindings.default.toml).
- `run_action` ([src/app/keyboard.rs:12](../../../src/app/keyboard.rs:12)) : ouvre la barre
  du pane actif, ou lui redonne le focus.

Migration : `keybindings.default.toml` ne fixe aucun raccourci, donc les configurations
existantes ne contiennent pas l'ancien raccourci. Une configuration qui associe
explicitement `"Ctrl+Shift+F" = "toggle_fold"` garde ce choix : la recherche reste alors
accessible par la palette de commandes. Aucune migration automatique.

Documentation à mettre à jour (en et fr) :
- `docs/*/keybindings.md` : `docs/en/keybindings.md:46-47`, `docs/fr/keybindings.md:51-52`.
- `docs/*/command-blocks.md` : `docs/en/command-blocks.md:80-82`, `docs/fr/command-blocks.md:82-84`.
- `docs/*/getting-started.md` : `docs/en/getting-started.md:23`, `docs/fr/getting-started.md:25`.
- [docs/dev/architecture.md:474](../../../docs/dev/architecture.md:474).
- Nouvelle section « Recherche » dans `docs/*/command-blocks.md`, ligne au tableau des
  fonctionnalités du README (en et fr).
- Notes de version : le changement de raccourci est signalé dans le message de commit
  (`BREAKING CHANGE:`), repris par `scripts/release-notes.sh`.

## Tests

- `src/search.rs` : échappement littéral, casse forcée dans les deux sens, regex
  invalide, mot entier avec accents, filtres zone et statut, partie commande (commande
  trouvée, vide, multiligne, introuvable), grille vivante exclue pendant l'édition,
  plafond, ordre et bouclage de la navigation, occurrence courante conservée après
  ajout de sortie. Sessions de test avec `timeline::testing::Session`
  ([src/timeline.rs:557](../../../src/timeline.rs:557)).
- `src/view.rs` : `reveal_line` sur un bloc plié, un bloc plus haut que la vue, la
  grille vivante.
- `src/chrome/search_bar.rs` : mise en page large et étroite, zones cliquables.
- `src/frame.rs` : couleurs des occurrences et de l'occurrence courante.
- `src/keybindings.rs` : nouveaux raccourcis par défaut.
- `src/theme.rs` : nouvelles clés lues et contraste dans les 8 thèmes.
- Benchmark dans `benches/render.rs` : recherche sur 10 000 lignes d'historique, pour
  vérifier que la recherche pendant la frappe reste fluide.

## Découpage en tâches d'implémentation

| # | Tâche | Dépend de |
|---|---|---|
| [#200](https://github.com/marmotz/graphene-term/issues/200) | Déplacer le pliage de bloc sur `Ctrl+Shift+Z` | - |
| [#201](https://github.com/marmotz/graphene-term/issues/201) | Couleurs de thème des occurrences | - |
| [#202](https://github.com/marmotz/graphene-term/issues/202) | Moteur de recherche dans la timeline | - |
| [#203](https://github.com/marmotz/graphene-term/issues/203) | Rendre visible une ligne d'un bloc | - |
| [#212](https://github.com/marmotz/graphene-term/issues/212) | Barre de recherche du pane | #200, #202, #203 |
| [#216](https://github.com/marmotz/graphene-term/issues/216) | Surlignage des occurrences | #201, #202, #212 |
| [#218](https://github.com/marmotz/graphene-term/issues/218) | Documentation | #212, #216 |
