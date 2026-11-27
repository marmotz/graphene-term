# La navigation entre blocs ne peut pas sélectionner le bloc en cours : conception technique

Voir [overview.md](overview.md) pour le contexte et les décisions produit.

## Cause

Le bloc en cours est exclu volontairement : `block_actions` ne connaît que les blocs
terminés (doc du module, [src/block_actions.rs:1-3](../../../src/block_actions.rs)).

- `finished` ([src/block_actions.rs:27](../../../src/block_actions.rs)) garde la
  sélection seulement si `timeline.block(id)` la trouve. Or `Timeline::block` ne cherche
  que dans les blocs terminés ([src/timeline.rs:218](../../../src/timeline.rs)).
- `neighbour` ([src/block_actions.rs:43](../../../src/block_actions.rs)) parcourt
  `timeline.blocks()`, qui ne contient que les blocs terminés. En `Next` sur le dernier
  bloc terminé, il ne trouve rien et renvoie ce même bloc. Le test
  `navigation_moves_to_the_neighbours_and_stops_at_the_ends` fixe ce comportement
  (« The fresh prompt is not finished »).

Trois autres endroits écartent aussi le bloc en cours :

- `Frame::capture` ([src/frame.rs:234](../../../src/frame.rs)) filtre `selected_block`
  sur `timeline.block(id)`. Sa carte ne serait donc jamais dessinée comme sélectionnée.
  Le test `selected_block_no_longer_in_the_timeline_marks_no_card` le vérifie.
- `Pane::margin_press` ([src/pane.rs:713](../../../src/pane.rs), filtre ligne 721)
  ignore un clic sur la barre d'état du bloc en cours.
- `block_actions::text` ([src/block_actions.rs:73](../../../src/block_actions.rs)) ne lit
  que les blocs terminés.

## Ce qui gère déjà le bloc en cours

- Il a un identifiant (`Current::id`, [src/timeline.rs:104](../../../src/timeline.rs)).
  Quand il se termine, `Timeline::finish` ([src/timeline.rs:427](../../../src/timeline.rs))
  garde cet identifiant pour le bloc terminé (`id: current.id`). Une sélection posée sur
  le bloc en cours reste donc valide et désigne le bloc terminé, sans traitement
  particulier.
- `view::reveal` ([src/view.rs:564](../../../src/view.rs)) cherche l'identifiant dans
  `shapes`, qui inclut le bloc en cours (`shapes`, [src/view.rs:218](../../../src/view.rs)).
  Le défilement vers ce bloc marche déjà.
- `Timeline::toggle_fold` et `is_folded` ([src/timeline.rs:297-312](../../../src/timeline.rs))
  gèrent déjà `current.id`.
- `view::can_scroll` / `max_block_scroll` et `PaneContext::read`
  ([src/chrome/command_palette.rs:103](../../../src/chrome/command_palette.rs)) gèrent déjà
  le défilement du bloc en cours (`id == timeline.current().id`).
- `frame::marks` ([src/frame.rs:471](../../../src/frame.rs)) crée déjà une carte pour les
  lignes du bloc en cours (`row.block` vaut `current.id` pour les lignes `Live`).
- Taper au clavier efface déjà la sélection (`Pane::type_bytes`,
  [src/pane.rs:243](../../../src/pane.rs)). Sélectionner l'invite vide puis taper
  désélectionne, comme pour un autre bloc.

## Conception retenue

### 1. Un bloc « sélectionnable » couvre aussi le bloc en cours

Ajouter dans `Timeline` une méthode `contains(id)` : vrai pour un bloc terminé ou pour
`current.id`. Elle remplace `timeline.block(id).is_some()` partout où on valide une
sélection :

- `block_actions::finished`, renommé `selectable` ;
- `Frame::capture` ([src/frame.rs:234](../../../src/frame.rs)) ;
- `Pane::margin_press` ([src/pane.rs:721](../../../src/pane.rs)).

### 2. Navigation (`block_actions::neighbour`)

Les blocs navigables sont les blocs terminés, du plus ancien au plus récent, puis le bloc
en cours. Les identifiants restent croissants : le bloc en cours a toujours le plus grand.

| Sélection           | `Previous`                                   | `Next`                         |
|---------------------|----------------------------------------------|--------------------------------|
| Aucune              | Dernier bloc terminé (comme aujourd'hui), sinon le bloc en cours | Bloc en cours |
| Un bloc terminé     | Bloc terminé précédent, sinon lui-même       | Bloc suivant, le bloc en cours après le dernier terminé |
| Le bloc en cours    | Dernier bloc terminé, sinon lui-même         | Lui-même (rien ne change)      |

Sans intégration shell, il n'y a aucun bloc terminé : les deux sens sélectionnent le bloc
en cours.

`PaneContext::read` calcule `previous_block` / `next_block` avec `neighbour` : la
palette suit sans autre changement. `next_block` devient vrai sur le dernier bloc
terminé, et faux sur le bloc en cours.

### 3. Actions sur le bloc en cours sélectionné

`target` ([src/block_actions.rs:36](../../../src/block_actions.rs)) renvoie le bloc
sélectionné, y compris le bloc en cours. Sans sélection, il renvoie toujours le dernier
bloc terminé (comportement inchangé).

`text` traite le bloc en cours ainsi :

- `Part::Command` : `current.command` s'il n'est pas vide, sinon la dernière ligne de
  `current.header`, comme pour un bloc terminé. À l'invite (`Phase::Prompt`), il n'y a
  pas d'en-tête figé : `None`.
- `Part::Output` : toujours `None`, la sortie n'est figée qu'à la fin de la commande.

Conséquences :

- copier la commande marche pendant qu'elle tourne ; copier la sortie ne fait rien ;
- sur l'invite vide, aucune copie ne réagit ;
- `block_text` (texte mis dans la sélection primaire à la sélection) vaut la commande
  pendant l'exécution, `None` à l'invite : rien n'est écrit dans la sélection primaire ;
- le repli (`Pane::toggle_fold` → `Timeline::toggle_fold`) agit déjà sur le bloc en
  cours ;
- `PaneContext` : `copy_output` est faux et `toggle_fold` vrai sur le bloc en cours,
  sans autre changement.

## Alternatives écartées

- **Désélectionner sur `Ctrl+Shift+PageDown` au bout** : écarté par la discussion
  produit, la sélection reste sur le bloc en cours.
- **Garder le bloc en cours hors de `target`** (les actions visent le dernier bloc
  terminé) : écarté par la discussion produit. Un bloc sélectionné qui ne reçoit pas les
  actions serait incohérent.
- **Un variant de sélection dédié** (par exemple `Selection::Current` au lieu d'un
  `BlockId`) : inutile, l'identifiant du bloc en cours est stable et devient celui du bloc
  terminé.

## Tests

- `block_actions` :
  - `running_block_is_not_a_target` devient `selected_current_block_is_the_target` :
    sans sélection, la cible reste le dernier bloc terminé ; avec le bloc en cours
    sélectionné, la cible est le bloc en cours ;
  - `navigation_moves_to_the_neighbours_and_stops_at_the_ends` : `Next` depuis le dernier
    bloc terminé sélectionne le bloc en cours ; `Next` depuis le bloc en cours y reste ;
    `Previous` depuis le bloc en cours sélectionne le dernier bloc terminé ;
  - `first_navigation_selects_the_last_finished_block` : `Previous` sans sélection donne
    le dernier bloc terminé, `Next` donne le bloc en cours ;
  - nouveau test sans intégration shell : les deux sens sélectionnent le bloc en cours ;
  - `empty_parts_have_no_text` et un nouveau test : commande d'un bloc en cours
    d'exécution copiable, sortie `None`, invite vide sans texte.
- `frame` : `selected_block_no_longer_in_the_timeline_marks_no_card` ne vérifie plus que
  le bloc en cours n'est pas marqué ; un nouveau test vérifie que la carte du bloc en
  cours est `selected`.
- `command_palette` : le test de `PaneContext` ([src/chrome/command_palette.rs:1265](../../../src/chrome/command_palette.rs))
  attend `next_block` vrai sur le dernier bloc terminé, et un cas « bloc en cours
  sélectionné » avec `next_block` faux et `previous_block` vrai.
- `pane` : un test de `margin_press` sur une ligne du bloc en cours, s'il existe un
  harnais de test pour `Pane`. Sinon, la logique de filtrage passe par
  `Timeline::contains`, testé dans `timeline`.

## Documentation

`docs/fr/command-blocks.md` et `docs/en/command-blocks.md`, section « Sélectionner un
bloc » : préciser que le bloc en cours (commande en cours ou invite) se sélectionne aussi,
que `Ctrl+Shift+PageDown` y mène, et que sa sortie ne se copie qu'une fois la commande
terminée. Pas de changement dans `README`, la configuration ou `docs/dev/` : aucun
raccourci, réglage ni module nouveau.

## Découpage en tâches d'implémentation

1. [#279](https://github.com/marmotz/graphene-term/issues/279) : sélectionner le bloc en
   cours (`Timeline::contains`, navigation, rendu de la carte, clic dans la marge, docs).
2. [#280](https://github.com/marmotz/graphene-term/issues/280) : copier la commande du
   bloc en cours sélectionné (`block_actions::text`, docs). Dépend de #279.
