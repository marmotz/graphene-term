# Protocole clavier kitty : conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## État actuel

### Côté terminal (séquences reçues)

- `alacritty_terminal` 0.26 gère déjà tout l'**état** du protocole kitty : pile de
  modes par écran (`keyboard_mode_stack`, et `inactive_keyboard_mode_stack` échangée
  au passage à l'écran alternatif), `CSI > u` / `CSI < u` / `CSI = u` / `CSI ? u`, et
  le reflet des 5 drapeaux dans `TermMode` (`DISAMBIGUATE_ESC_CODES` ...
  `REPORT_ASSOCIATED_TEXT`, regroupés dans `TermMode::KITTY_KEYBOARD_PROTOCOL`). Tout
  est conditionné par `term::Config::kitty_keyboard`, à `false` par défaut
  (`alacritty_terminal-0.26.0/src/term/mod.rs:349`, `:1275-1320`). Désactivé, la
  requête `CSI ? u` reste sans réponse : c'est pourquoi fish 4, Neovim et Helix ne
  l'activent pas aujourd'hui.
- Graphene construit cette config dans `Config::term_config`
  ([src/config/mod.rs:260](../../../src/config/mod.rs)) sans fixer `kitty_keyboard`.
- **modifyOtherKeys** : `vte` 0.15 analyse `CSI > 4 ; n m` et `CSI ? 4 m`
  (`vte-0.15.0/src/ansi.rs:1685-1700`) et appelle `Handler::set_modify_other_keys` /
  `report_modify_other_keys`, mais `Term` ne les implémente pas (méthodes par défaut
  vides). Graphene doit donc capter ces séquences lui-même.
- Le flux PTY passe déjà par deux pré-analyseurs avant `vte`
  ([src/pty.rs:76-130](../../../src/pty.rs)) : `graphics::sequence::Splitter` (APC/DCS
  des images, `ESC c`) puis `signals::Scanner`, qui collecte déjà les paramètres CSI
  pour détecter `CSI 2 J` / `CSI 3 J` ([src/signals.rs:62](../../../src/signals.rs),
  `fn csi` ligne 208) sans consommer d'octets.
- Les réponses au PTY émises par Graphene lui-même (graphiques kitty) sont accumulées
  dans le handler et écrites par la boucle PTY
  ([src/pty.rs:445](../../../src/pty.rs)).

### Côté clavier (séquences émises)

- `WindowEvent::KeyboardInput` ne passe à `Session::key_press` que les appuis réels :
  `is_real_key_press` ([src/app/application.rs:525](../../../src/app/application.rs))
  écarte les relâchements. Le drapeau `repeat` de winit n'est pas lu.
- `key_press` ([src/app/keyboard.rs:106](../../../src/app/keyboard.rs)) résout le
  raccourci, puis appelle `deliver_key` (ligne 177) pour chaque pane cible
  (`input_targets()`, diffusion comprise). `deliver_key` appelle
  `input::resolve_key` ([src/input.rs:228](../../../src/input.rs)) avec le
  `TermMode` du pane : commandes de l'éditeur, puis raccourcis, puis saisie. L'ordre
  de priorité voulu par l'overview est donc déjà en place.
- L'encodage est fait par `input::key_to_bytes` (ligne 316) : encodage legacy xterm
  seulement (`CSI 1;m X`, `CSI n;m ~`, codes de contrôle, préfixe `ESC` pour Alt).
  Les touches modificatrices seules et les touches non nommées renvoient `None`.
- L'éditeur de saisie émet des séquences privées `CSI 59136..59149 u`
  ([src/input_editor.rs:228](../../../src/input_editor.rs)), liées par les scripts
  d'intégration ; fish les lie en syntaxe brute
  ([shell-integration/fish/vendor_conf.d/graphene.fish:64-83](../../../shell-integration/fish/vendor_conf.d/graphene.fish)).
  Ces octets ne dépendent pas du mode clavier et ne changent pas.
- winit 0.30.13 fournit `repeat`, `location`, `physical_key` sur `KeyEvent`, et
  `KeyEventExtModifierSupplement::key_without_modifiers()` sur X11, Wayland et macOS
  (`winit-0.30.13/src/platform/modifier_supplement.rs`), nécessaire pour la touche de
  base et les touches alternatives.

## Conception retenue

### 1. Activer l'état kitty

`Config::term_config` fixe `kitty_keyboard: true`. Aucun réglage utilisateur
(décision produit). Pile, requête, écran alternatif et `ESC c` sont gérés par
`alacritty_terminal`.

Limite connue de la dépendance : quand la pile dépasse 4096 entrées,
`push_keyboard_mode` retire un élément de `title_stack` au lieu de la pile clavier
(`term/mod.rs:1295-1301`). Sans effet pour un programme normal ; non contourné.

### 2. Capter modifyOtherKeys dans `signals::Scanner`

- Le scanner reconnaît, dans son état CSI existant, `CSI > 4 ; n m` (n = 0, 1, 2 ;
  absent = 0) et `CSI ? 4 m`. Il émet deux nouveaux `Chunk` :
  `ModifyOtherKeys(level)` et `ModifyOtherKeysQuery`, après les `Bytes` de la
  séquence, comme `Clear`. Les octets continuent d'aller à `vte` (qui les ignore).
- Le niveau vit dans un `Arc<AtomicU8>` partagé entre la boucle PTY (écriture) et
  `Terminal` ([src/terminal.rs:125](../../../src/terminal.rs)) (lecture à
  l'encodage), sans prendre de verrou supplémentaire sur le chemin clavier.
- La requête reçoit `CSI > 4 ; n m`, poussée dans la file d'écriture de la boucle PTY
  comme les réponses graphiques.
- `Chunk::Reset` (`ESC c`) remet le niveau à 0. Comme xterm, le niveau est global au
  terminal, pas par écran.

Alternative écartée : envelopper `Term` dans un `vte::ansi::Handler` maison pour
implémenter `set_modify_other_keys`. Le trait compte des dizaines de méthodes à
déléguer, pour deux séquences ; le scanner analyse déjà les CSI.

### 3. Module d'encodage `src/keyboard_protocol.rs`

Module maison écrit d'après la spec kitty
(<https://sw.kovidgoyal.net/kitty/keyboard-protocol/>) et les ctlseqs xterm, sans
dépendance. Alternatives écartées : porter l'encodeur d'Alacritty (dans son binaire,
licence Apache seule, incompatible avec le double MIT/Apache de Graphene) ; `termwiz`
(grosse dépendance, types de touches à convertir).

- Entrée : un `KeyInput` construit depuis le `KeyEvent` winit : touche logique,
  `key_without_modifiers`, touche physique, `location`, texte, modificateurs (avec
  Super, Caps Lock, Num Lock pour kitty), type d'événement (appui, répétition,
  relâchement).
- Sortie : `Option<Vec<u8>>`. Choix du mode, dans l'ordre :
  1. un drapeau kitty actif dans `TermMode` : encodage `CSI code[:shifted[:base]] ;
     mods[:event] [; text] u` ou forme `CSI 1;mods X` / `CSI n;mods ~` pour les
     touches fonctionnelles, selon les drapeaux ;
  2. sinon modifyOtherKeys 1 ou 2 : `CSI 27 ; mods ; code ~` pour les combinaisons
     concernées par le niveau ;
  3. sinon l'encodage legacy actuel, inchangé (`key_to_bytes` devient la branche
     legacy du module).
- Le pavé numérique (`KeyLocation::Numpad`), les modificateurs gauche/droite et les
  touches multimédia reçoivent leurs codes kitty dédiés (zone privée 57344+), utiles
  avec « toutes les touches en séquences ».
- `resolve_key` garde sa signature de priorité ; il reçoit le `KeyInput` et le niveau
  modifyOtherKeys du pane au lieu de `(key, text, mods)`.

### 4. Relâchements et répétitions

- `is_real_key_press` laisse passer les relâchements non synthétiques vers un nouveau
  `Session::key_release` ; les répétitions sont déjà des appuis, marquées par
  `event.repeat`.
- Chaque `Pane` garde l'ensemble des touches physiques dont l'appui a été **envoyé**
  au programme. Un relâchement n'est encodé que pour ces touches, et seulement vers le
  pane qui a reçu l'appui, même si le focus a changé entre-temps. Une touche prise
  par un raccourci ou l'éditeur n'y entre pas : son relâchement n'est jamais envoyé.
- L'ensemble est vidé quand le programme quitte un mode kitty avec événements, et à
  la perte de focus de la fenêtre (kitty fait de même), pour ne pas garder de touche
  bloquée.
- Sans `REPORT_EVENT_TYPES`, les relâchements ne produisent rien et les répétitions
  sont encodées comme des appuis, comme aujourd'hui.

### 5. Éditeur de saisie et fish 4

`rich_mode` ([src/input_editor.rs:26](../../../src/input_editor.rs)) ne regarde pas
le mode clavier : l'éditeur garde la main à l'invite (décision produit). Ses
séquences privées restent identiques ; seules les touches qu'il laisse passer sont
encodées selon le mode actif.

Risque : fish 4 activera le protocole à son invite dès que Graphene répondra à
`CSI ? u`. Les liaisons brutes `bind \e\[59136u` doivent encore correspondre une fois
l'entrée décodée en mode kitty. La première tâche vérifie ce point avec un vrai
fish 4 ; si elles cassent, `graphene.fish` est adapté **avant** d'activer
`kitty_keyboard`.

### 6. Documentation

- README (en, fr) : une ligne dans les fonctionnalités.
- `docs/en/keybindings.md` et `docs/fr/keybindings.md` : section sur les protocoles
  clavier, la priorité des raccourcis Graphene et de l'éditeur de saisie, et les
  relâchements non transmis.
- `docs/dev/decisions.md` : modifyOtherKeys capté par le scanner, encodeur maison.

## Conséquences vérifiées

- Aucun changement pour un programme qui n'active rien : la branche legacy reprend
  `key_to_bytes` tel quel, et ses tests existants ([src/input.rs:542-630](../../../src/input.rs))
  restent valables.
- La diffusion (`input_targets`) encode déjà par pane avec son propre `TermMode` :
  deux panes dans des modes différents reçoivent chacun leur encodage.
- Les tests de `resolve_key` et `key_to_bytes` (`src/input.rs`, `src/input_editor.rs:794`)
  changent de signature et sont mis à jour.
- Bash et zsh n'activent pas le protocole : leurs liaisons ne sont pas touchées.

## Tests

- Tables d'encodage par mode (legacy, modifyOtherKeys 1 et 2, chaque drapeau kitty
  et leurs combinaisons), d'après les exemples de la spec kitty.
- Scanner : `CSI > 4 ; n m`, `CSI ? 4 m`, séquence coupée entre deux lectures, `ESC c`.
- Relâchements : appui pris par un raccourci sans relâchement envoyé, relâchement vers
  le pane d'origine après changement de focus, vidage à la perte de focus.
- Intégration PTY : `CSI ? u` reçoit `CSI ? 0 u` puis le mode poussé ; `CSI ? 4 m`
  reçoit sa réponse.
- fish 4 réel en mode kitty : les séquences privées de l'éditeur déplacent le curseur.

## Découpage en tâches d'implémentation

1. [#191](https://github.com/marmotz/graphene-term/issues/191) : vérifier les liaisons
   fish 4 avec le protocole kitty actif (§5).
2. [#192](https://github.com/marmotz/graphene-term/issues/192) : module d'encodage
   kitty, modifyOtherKeys et legacy (§3).
3. [#193](https://github.com/marmotz/graphene-term/issues/193) : capter modifyOtherKeys
   et répondre à sa requête (§2).
4. [#194](https://github.com/marmotz/graphene-term/issues/194) : brancher l'encodeur et
   activer le protocole kitty (§1). Dépend de #191, #192, #193.
5. [#195](https://github.com/marmotz/graphene-term/issues/195) : transmettre les
   relâchements des touches envoyées au programme (§4). Dépend de #194.
6. [#196](https://github.com/marmotz/graphene-term/issues/196) : documentation et
   vérification des plateformes (§6). Dépend de #194, #195.
