# Activation du quick terminal : conception technique

Voir [overview.md](overview.md) pour les décisions produit, et [../quick-terminal/technical.md](../quick-terminal/technical.md) pour le fonctionnement du quick terminal.

## Constats sur le code actuel

Le code a dépassé le `technical.md` du quick terminal : une clé `autostart` existe déjà.

- **Config** : `QuickTerminalConfig` ([src/config/values.rs:265](../../../src/config/values.rs)) porte `hotkey`, `edge`, `size`, `screen`, `animation_ms`, `hide_on_focus_loss`, `autostart`. Défauts dans `Config::default` ([src/config/mod.rs:126](../../../src/config/mod.rs)), lecture clé par clé dans [src/config/reader.rs:201](../../../src/config/reader.rs) à `:233`, documentation dans [config.default.toml:76](../../../config.default.toml).
- **Trois chemins lancent le processus quick sans que l'utilisateur le demande** :
  1. le lancement caché depuis une fenêtre normale quand `hotkey` est renseigné : `wants_hidden_quick` et `hotkey_turned_on` ([src/app/hotkey.rs:10](../../../src/app/hotkey.rs), [src/app/hotkey.rs:15](../../../src/app/hotkey.rs)), appelés à `resumed` et au rechargement ([src/app/application.rs:582](../../../src/app/application.rs), [src/app/application.rs:459](../../../src/app/application.rs)) ;
  2. l'entrée d'autostart de session (`.desktop` ou LaunchAgent) qui exécute `graphene --quick --hidden` : `wants_autostart_sync` et `sync_autostart` ([src/app/hotkey.rs:22](../../../src/app/hotkey.rs), [src/app/hotkey.rs:112](../../../src/app/hotkey.rs)), appelés à `resumed` et au rechargement ([src/app/application.rs:583](../../../src/app/application.rs), [src/app/application.rs:465](../../../src/app/application.rs)) ;
  3. `graphene --quick-toggle`, qui lance `--quick` s'il ne tourne pas : `quick_toggle` ([src/lib.rs:89](../../../src/lib.rs)) puis `toggle_with` ([src/quick_terminal/control.rs:200](../../../src/quick_terminal/control.rs)).
- **Processus quick** : `run_quick` ([src/app/application.rs:851](../../../src/app/application.rs)) prend le verrou `quick.lock` avant de construire la boucle d'événements, puis `resumed` sort avec `WAYLAND_EXIT_CODE` (3) sous Wayland ([src/app/application.rs:569](../../../src/app/application.rs)). `toggle_with` traduit ce code en `ToggleError::Wayland` ([src/quick_terminal/control.rs:219](../../../src/quick_terminal/control.rs)). `main.rs` fait `process::exit(code)` pour un code non nul ([src/main.rs:35](../../../src/main.rs)).
- **Raccourci natif** : `register_hotkey` ne s'exécute que dans le processus quick (`quick_state().is_none()` retourne tôt, [src/app/hotkey.rs:54](../../../src/app/hotkey.rs)).
- **Réglages** : `Section::QuickTerminal` liste ses champs ([src/settings/model.rs:188](../../../src/settings/model.rs)). Le grisage est par section : `unavailable` ([src/settings/model.rs:629](../../../src/settings/model.rs)) renvoie le message Wayland, et la vue enveloppe tout le corps de section dans `add_enabled_ui` ([src/settings/view/mod.rs:239](../../../src/settings/view/mod.rs)). Le message est affiché par `file_problems` ([src/settings/view/mod.rs:627](../../../src/settings/view/mod.rs)). Il n'existe pas de grisage par champ.
- **Traductions** : `SettingsField` et `Msg` sont dans [src/i18n.rs](../../../src/i18n.rs), avec une table de numéros de message (`Msg::QuickTerminalUnavailableOnWayland => 98`, `:1058`) et une liste exhaustive de tests (`:1156`) à compléter pour chaque nouveau message.

## 1. Clé `quick_terminal.enabled`

- `QuickTerminalConfig::enabled: bool`, défaut `false` ([src/config/values.rs](../../../src/config/values.rs), [src/config/mod.rs:126](../../../src/config/mod.rs)).
- Lecture : une ligne dans [src/config/reader.rs](../../../src/config/reader.rs), sur le modèle de `autostart` (`Checked::Valid`). Une valeur invalide garde la valeur précédente et signale l'erreur, comme les autres clés.
- [config.default.toml](../../../config.default.toml) : `# enabled = false` en tête de `[quick_terminal]`, avec son commentaire.
- **Alternative écartée** : renommer ou fusionner avec `hotkey` (« hotkey non vide = activé »). Le défaut actuel est déjà ce comportement et ne permet pas d'utiliser la commande CLI seule sans lancement automatique.

## 2. Effets de `enabled = false`

`enabled` commande tout ce qui lance le processus quick de façon automatique, plus la commande CLI. Le processus déjà résident n'est pas touché (décision produit : effet au prochain démarrage).

| Chemin | Comportement désactivé |
|--------|------------------------|
| Lancement caché depuis une fenêtre normale | Aucun. `wants_hidden_quick` prend la config et exige `enabled`. |
| Autostart de session | L'entrée est retirée. La valeur effective est `enabled && autostart`. |
| `graphene --quick-toggle` | Échec avec un message, code `1`, sans toucher au socket ni lancer de processus. |
| `graphene --quick` | Sort avec le code `4` avant de prendre le verrou. |
| Processus quick déjà résident | Continue jusqu'à la fin de la session, raccourci natif compris. |

### Lancement caché et autostart

Dans [src/app/hotkey.rs](../../../src/app/hotkey.rs) :

- `wants_hidden_quick(quick: &QuickTerminalConfig, supported: bool) -> bool` : `supported && quick.enabled && !quick.hotkey.is_empty()`.
- `hotkey_turned_on(old, new)` devient `hidden_start_turned_on(old, new)` sur deux configs : `!wants(old, true) && wants(new, true)`. Passer `enabled` de faux à vrai avec un `hotkey` déjà renseigné relance donc le démarrage caché au rechargement, comme renseigner le `hotkey`.
- `wants_autostart_sync(previous: Option<bool>, current: bool, supported)` garde sa signature : on lui passe la valeur effective `enabled && autostart`. `sync_autostart` prend `previous: Option<bool>` effectif et calcule `current` de la même façon, et [src/app/application.rs:465](../../../src/app/application.rs) passe `old.quick_terminal.enabled && old.quick_terminal.autostart`. Désactiver ou réactiver `enabled` retire ou recrée donc l'entrée, sans changer `autostart`.

### Commande CLI

`quick_toggle` ([src/lib.rs:89](../../../src/lib.rs)) lit la configuration avec `config::load()` (même fonction que `load_config`, sans le thème) avant tout. Désactivé : retourne `Err` avec le message ci-dessous. Il lit le fichier à chaque appel, donc le message suit la config sans redémarrage, même si un processus résident tourne encore.

Message (anglais, comme `ToggleError`) : `the quick terminal is disabled, set enabled = true in the [quick_terminal] section of config.toml, or turn it on in the settings`.

`ToggleError` ([src/quick_terminal/control.rs:157](../../../src/quick_terminal/control.rs)) gagne une variante `Disabled` avec ce même texte, utilisée quand l'enfant sort avec `DISABLED_EXIT_CODE` (cas de course : la config a changé entre la lecture et le lancement). Le texte est ainsi défini une seule fois.

### Processus `--quick`

`run_quick` ([src/app/application.rs:851](../../../src/app/application.rs)), avant `claim` : `config::load()` ; si `!enabled`, retourne `Ok(DISABLED_EXIT_CODE)` (nouvelle constante `4` à côté de `WAYLAND_EXIT_CODE`, [src/quick_terminal/control.rs:23](../../../src/quick_terminal/control.rs)). Cela couvre une entrée d'autostart périmée, ou un lancement manuel : le processus ne prend pas le verrou et ne crée aucune fenêtre. Aucune sortie console n'est écrite côté `--quick` (stdio vers `/dev/null` quand il est lancé par Graphene). Un lancement manuel en terminal affiche en plus le message sur stderr, via `main.rs`.

L'entrée d'autostart périmée est retirée au prochain démarrage d'une fenêtre normale par `sync_autostart(None)`.

**Ordre des contrôles** : désactivé (4) avant Wayland (3), donc un utilisateur sous Wayland qui n'a rien activé ne voit pas d'erreur de plateforme.

### Processus résident à la désactivation

Aucun changement : `register_hotkey` et `listen_quick` ne lisent pas `enabled`. Le rechargement dans le processus quick lui-même met seulement à jour l'entrée d'autostart (qui s'exécute dans tous les modes).

**Alternative écartée** : fermer le processus résident au rechargement quand `enabled` passe à faux. Contredit la décision produit (« au prochain démarrage ») et ajoute un chemin d'arrêt que le processus n'a pas aujourd'hui.

## 3. Fenêtre de réglages

- Nouveau `Field::QuickTerminalEnabled` en tête de `Section::QuickTerminal` ([src/settings/model.rs:188](../../../src/settings/model.rs)), chemin `["quick_terminal", "enabled"]`, `Widget::Toggle`. Libellé `SettingsField::QuickTerminalEnabled` : « Enable the quick terminal » / « Activer le quick terminal ».
- Grisage par champ : `SettingsModel::field_enabled(field) -> bool`, vrai sauf pour un champ de `Section::QuickTerminal` autre que `QuickTerminalEnabled` quand `!config.quick_terminal.enabled`. Dans `section_body` ([src/settings/view/mod.rs:282](../../../src/settings/view/mod.rs)), le widget de chaque champ est enveloppé dans `ui.add_enabled_ui(model.field_enabled(field), ...)`. Le grisage de section existant (Wayland, fichier verrouillé) est inchangé et l'emporte, interrupteur compris.
- Explication : sous le titre de section, quand le quick terminal est désactivé et que la section n'est ni verrouillée ni indisponible, un texte informatif `Msg::QuickTerminalDisabled` : « The quick terminal is disabled: turn it on to change these settings. The change applies the next time Graphene starts. » (français équivalent). Il sert aussi à annoncer le moment d'application.
- Modèle : `field_enabled` lit `self.config`, que `written` met à jour dès l'écriture du fichier ([src/settings/model.rs:454](../../../src/settings/model.rs)). Les champs se dégrisent donc dès le clic.
- Enregistrement dans [src/i18n.rs](../../../src/i18n.rs) : `SettingsField::QuickTerminalEnabled` (variantes anglais et français, liste des champs `:205`), `Msg::QuickTerminalDisabled` (anglais, français, table des numéros, liste de test).

**Alternative écartée** : griser tous les champs de la section sauf l'interrupteur via `unavailable`. Ce mécanisme sert à des raisons de plateforme et à un seul message, et grise aussi l'interrupteur ; il faudrait le distinguer, ce que le grisage par champ fait plus simplement.

## Conséquences vérifiées

- **Rupture assumée** : un utilisateur qui a `hotkey` ou `autostart = true` sans `enabled` perd le lancement automatique, et son entrée d'autostart est retirée à son prochain démarrage de fenêtre. Un processus résident déjà lancé continue jusqu'à la fin de sa session.
- `--quick-toggle` désactivé ne touche ni le socket ni le verrou : le comportement d'un processus résident n'est pas modifié par une commande qui échoue.
- `hotkey` vide et `enabled = false` : aucun processus lancé, aucune entrée d'autostart, comme avant pour le défaut.
- Sous Wayland, `enabled` n'a aucun effet : l'ordre des contrôles ci-dessus, et `quick_supported` garde `wants_hidden_quick` et `sync_autostart` inertes.
- `docs/dev/architecture.md` : la ligne de `control.rs` ([docs/dev/architecture.md:85](../../../docs/dev/architecture.md)) cite les codes de sortie ; à compléter.

## Documentation

Selon `AGENTS.md`, dans le même changement :

- `docs/en/quick-terminal.md` et `docs/fr/quick-terminal.md` : nouvelle section « Enable the quick terminal » en tête, rappel dans « Show and hide it », « Global shortcut » et « Start with the session » que `enabled = true` est requis.
- `docs/en/configuration.md` et `docs/fr/configuration.md` : ligne `enabled` dans le tableau de `[quick_terminal]` (autour de `:244`).
- `README.md` et `README.fr.md` : ligne « Features » du quick terminal ; le tableau de comparaison n'est pas concerné.
- `config.default.toml` : voir section 1.
- Pas de nouvelle page ni de nouveau sujet : les index `docs/*/README.md` ne changent pas.

## Tests

- Config : `quick_terminal_defaults` vérifie `!enabled` ; `enabled = true` est lu ; une valeur invalide garde la précédente et signale `quick_terminal.enabled` (tests existants de [src/config/mod.rs:568](../../../src/config/mod.rs) à `:700`, sur le modèle de `autostart`).
- [src/app/hotkey.rs](../../../src/app/hotkey.rs) : `wants_hidden_quick` faux sans `enabled` ou sans `hotkey` ou non supporté ; `hidden_start_turned_on` vrai quand `enabled` passe à vrai avec un `hotkey`, ou quand `hotkey` est renseigné avec `enabled`, faux sinon ; l'autostart effectif suit `enabled && autostart` (la valeur calculée à passer à `wants_autostart_sync`, dans une fonction pure testée).
- `control.rs` : `toggle_with` traduit `DISABLED_EXIT_CODE` en `ToggleError::Disabled` (modèle du test Wayland, [src/quick_terminal/control.rs:337](../../../src/quick_terminal/control.rs)) ; le texte du message est celui attendu.
- `lib.rs` / commande : une fonction pure `toggle_gate(&QuickTerminalConfig) -> Result<(), String>` testée pour désactivé et activé.
- `run_quick` : un contrôle pur `quick_allowed(&Config) -> Option<i32>` testé (désactivé donne `Some(4)`).
- Réglages : `field_enabled` faux pour les champs de la section quand désactivé, vrai pour `QuickTerminalEnabled`, vrai partout quand activé, et vrai pour les autres sections ; test de la vue sur l'écriture de `["quick_terminal", "enabled"]` (modèle de [src/settings/view/mod.rs:1117](../../../src/settings/view/mod.rs)) ; table et liste exhaustive de l'i18n complétées.
- Documentation : les tests de cohérence existants des docs ou du `config.default.toml` s'il y en a ; sinon contrôle manuel EN/FR.

## Découpage en tâches d'implémentation

1. [#275](https://github.com/marmotz/graphene-term/issues/275) : clé `quick_terminal.enabled`, `config.default.toml` et pages de configuration (§1). Aucune dépendance.
2. [#276](https://github.com/marmotz/graphene-term/issues/276) : lancement caché, autostart, commande CLI et `--quick` conditionnés par `enabled` (§2). Dépend de #275.
3. [#277](https://github.com/marmotz/graphene-term/issues/277) : interrupteur et grisage par champ dans les réglages (§3). Dépend de #275.
