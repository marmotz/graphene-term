# Quick terminal autostart : conception technique

Voir [overview.md](overview.md) pour les décisions produit, et le
[quick terminal](../quick-terminal/technical.md) pour le processus résident.

## Constats sur le code actuel

- **Le raccourci vit dans le processus quick.** `register_hotkey` ne fait rien sans `quick_state()` ([src/app/hotkey.rs:41](../../../src/app/hotkey.rs)). Ce processus est démarré par `graphene --quick [--hidden]` ([src/main.rs:101](../../../src/main.rs)), via `spawn_quick` ([src/quick_terminal/control.rs:244](../../../src/quick_terminal/control.rs)).
- **Démarrage caché existant.** `start_hidden_quick` ([src/app/hotkey.rs:84](../../../src/app/hotkey.rs)) est appelé depuis `resumed` ([src/app/application.rs:590](../../../src/app/application.rs)) et depuis `reload_config` quand le hotkey passe de vide à défini ([src/app/application.rs:467](../../../src/app/application.rs)). Il tourne dans un thread, journalise les échecs avec `log::warn!`, et le verrou d'instance unique rend les démarrages simultanés inoffensifs.
- **Rechargement.** Un changement du réglage dans la fenêtre des réglages écrit `config.toml`, le watcher déclenche `reload_config` ([src/app/application.rs:459](../../../src/app/application.rs)) dans tous les processus ouverts, quick compris. Il n'y a donc qu'un point d'application à prévoir.
- **Wayland.** `quick_supported` ([src/app/quick.rs:583](../../../src/app/quick.rs)) vaut `false` sous Wayland. La section des réglages est déjà grisée par `Model::unavailable` ([src/settings/model.rs:605](../../../src/settings/model.rs)) : une nouvelle case dans cette section en hérite sans code supplémentaire.
- **Clés bool existantes.** `hide_on_focus_loss` montre le chemin complet : `QuickTerminalConfig` ([src/config/values.rs:278](../../../src/config/values.rs)), défaut ([src/config/mod.rs:132](../../../src/config/mod.rs)), lecture ([src/config/reader.rs:232](../../../src/config/reader.rs)), `Field` + `Widget::Toggle` ([src/settings/model.rs:193](../../../src/settings/model.rs), [:305](../../../src/settings/model.rs)), libellés ([src/i18n.rs:165](../../../src/i18n.rs)), valeur par défaut documentée ([config.default.toml:88](../../../config.default.toml)).
- **Chemins.** `config_dir()` ([src/config/paths.rs:6](../../../src/config/paths.rs)) renvoie le dossier de Graphene (`.../graphene`) et vaut `~/Library/Application Support/graphene` sous macOS : il ne convient pas pour les dossiers d'autostart, qui sont hors de ce dossier. Aucune dépendance supplémentaire n'est nécessaire (`libc`, `std::fs` suffisent).

## 1. Configuration

Nouvelle clé `autostart` dans `[quick_terminal]`, `false` par défaut (opt-in, décision produit).

- `QuickTerminalConfig.autostart: bool` ([src/config/values.rs](../../../src/config/values.rs)), défaut `false` ([src/config/mod.rs](../../../src/config/mod.rs)), lecture comme `hide_on_focus_loss` avec `Checked::Valid` ([src/config/reader.rs](../../../src/config/reader.rs)).
- `config.default.toml` : `# autostart = false`, avec un commentaire (« Start the hidden quick terminal when your graphical session opens, so the global shortcut works at once. »).
- Fenêtre des réglages : `Field::QuickTerminalAutostart` en dernière position de `Section::QuickTerminal`, `Widget::Toggle`, clé `["quick_terminal", "autostart"]`, valeur `Value::Bool` ([src/settings/model.rs](../../../src/settings/model.rs)). Libellés « Start with the session » / « Démarrer avec la session » dans [src/i18n.rs](../../../src/i18n.rs) (variante `SettingsField`, liste de l'ordre, deux langues, et l'entrée de test d'exhaustivité).
- Sous Wayland, la section est grisée : rien à ajouter.

## 2. Module `quick_terminal::autostart`

Nouveau fichier [src/quick_terminal/autostart.rs](../../../src/quick_terminal/autostart.rs). Le fonctionnement est identique sur les deux plateformes : un fichier géré par Graphene, créé ou supprimé pour suivre le réglage.

```rust
/// Makes the session autostart entry follow `enabled`.
pub fn sync(enabled: bool) -> io::Result<()>
```

- **Idempotent** : calcule le contenu attendu, le compare au fichier existant, n'écrit que s'il diffère (pas d'écriture à chaque démarrage). `enabled == false` : supprime le fichier s'il existe, `NotFound` n'est pas une erreur.
- **Écriture atomique** : réutilise `atomic_file` ([src/atomic_file.rs](../../../src/atomic_file.rs)), le dossier parent est créé au besoin.
- **Exécutable** : la valeur de la variable `APPIMAGE` quand elle existe et pointe un fichier (chemin stable du `.AppImage`, alors que `current_exe()` est un point de montage temporaire), sinon `current_exe()`. Le chemin est réécrit à chaque `sync` si l'exécutable a bougé, ce qui suit un déplacement ou une mise à jour du binaire.
- **Arguments** : `--quick --hidden`, exactement ce que `spawn_quick(true)` lance. Le verrou d'instance unique rend inoffensive une session où un processus tourne déjà.

### Linux (X11)

- Fichier : `$XDG_CONFIG_HOME/autostart/graphene-quick-terminal.desktop`, `~/.config/autostart/...` sans `XDG_CONFIG_HOME` absolu (même règle que `config_dir_from` : variable vide ou relative ignorée). Une fonction `autostart_dir_from(xdg, home)` testable sans environnement.
- Contenu (spec Desktop Entry) :

```
[Desktop Entry]
Type=Application
Name=Graphene Quick Terminal
Exec="<exe>" --quick --hidden
Terminal=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
```

  `Exec` : le chemin est entre guillemets, `\`, `"`, `` ` `` et `$` y sont échappés par `\` comme l'exige la spec, `%` est doublé (`%%`).

### macOS

- Fichier : `~/Library/LaunchAgents/dev.marmotz.graphene.quick-terminal.plist` (`Label` identique), `RunAtLoad` à `true`, `ProgramArguments` = `[exe, "--quick", "--hidden"]`, `ProcessType` à `Interactive`. Pas de `KeepAlive` : le processus résident s'arrête de lui-même à la fin de la session et ne doit pas être relancé en boucle quand l'utilisateur le quitte.
- Les valeurs sont échappées en XML (`&`, `<`, `>`).
- `launchd` lit les LaunchAgents à l'ouverture de session suivante : l'entrée n'a aucun effet immédiat, ce qui convient (le quick terminal tourne déjà ou démarre à la demande).
- Alternative écartée : `SMAppService`, qui exige un bundle `.app` et ne marche pas pour l'archive `.tar.gz` ni pour `cargo run`.

### Alternatives écartées (toutes plateformes)

- Une commande `graphene --quick-autostart enable|disable` : écartée par décision produit (la clé de configuration est la seule interface).
- Modifier le script de session ou `.xprofile` : invasif, hors du périmètre de l'utilisateur.

## 3. Application du réglage

Une fonction `App::sync_autostart` (dans [src/app/hotkey.rs](../../../src/app/hotkey.rs), à côté de `start_hidden_quick`) :

1. Sortie immédiate si `!self.quick_supported` : sous Wayland, ni création ni suppression (un fichier laissé par une session X11 du même compte est inoffensif, `--quick` y sort avec le code 3).
2. Lance `autostart::sync(self.config.quick_terminal.autostart)` dans un thread nommé `quick-terminal-autostart`, comme `start_hidden_quick`. Un échec est journalisé par `log::warn!` (décision : pas de bannière, l'erreur n'est pas une clé invalide de configuration et l'utilisateur voit la case décochée ou cochée à tort uniquement après relecture du fichier).

Appels :

- `resumed`, juste après `start_hidden_quick` ([src/app/application.rs:590](../../../src/app/application.rs)), pour rattraper une entrée supprimée à la main, un exécutable déplacé, ou un `config.toml` édité hors de Graphene.
- `reload_config`, après `register_hotkey`, quand `old.quick_terminal.autostart != self.config.quick_terminal.autostart` : la case des réglages et l'édition du fichier prennent effet à chaud.

Pour tous les modes de fenêtre (normal, quick) : l'écriture étant idempotente, plusieurs processus qui appellent `sync` en même temps ne produisent qu'un état final identique.

Le démarrage caché d'une fenêtre normale (`wants_hidden_quick`) reste lié au hotkey : l'autostart ne le remplace pas, il le précède.

## 4. Documentation

- [docs/en/quick-terminal.md](../../../docs/en/quick-terminal.md) et [docs/fr/quick-terminal.md](../../../docs/fr/quick-terminal.md) : section « Start with the session », entre « Global shortcut » et « Slide, focus and closing », avec l'emplacement des fichiers créés par plateforme et le fait qu'un raccourci reste nécessaire pour l'utiliser.
- [docs/en/configuration.md](../../../docs/en/configuration.md) et sa version française : ligne `autostart` dans `[quick_terminal]`.

## Tests

- `autostart.rs` : contenu du `.desktop` (échappement de `"`, `$`, `\`, `%`, espaces dans le chemin) ; contenu du plist (échappement XML, `RunAtLoad`) ; `autostart_dir_from` (`XDG_CONFIG_HOME` absolu, vide, relatif, repli sur `HOME`) ; choix de l'exécutable (`APPIMAGE` valide, `APPIMAGE` pointant un fichier absent).
- `sync` sur un dossier temporaire : crée le fichier, ne réécrit pas quand le contenu est identique (date de modification inchangée), réécrit quand l'exécutable change, supprime quand `enabled` passe à `false`, supprimer un fichier absent n'est pas une erreur.
- Configuration : défaut `false`, `autostart = true` lu, valeur non booléenne signalée comme les autres clés.
- Réglages : champ présent dans la section, bascule écrit `["quick_terminal", "autostart"]`, section grisée sous Wayland (test existant étendu).
- `hotkey.rs` : la décision de synchroniser (supporté ou non, clé changée ou non) isolée dans une fonction pure testée comme `wants_hidden_quick`.
- `i18n` : test d'exhaustivité existant couvre le nouveau libellé.

## Découpage en tâches d'implémentation

1. [#250](https://github.com/marmotz/graphene-term/issues/250) : clé `autostart` et case des réglages (§1). Aucune dépendance.
2. [#251](https://github.com/marmotz/graphene-term/issues/251) : module d'entrée d'autostart Linux et macOS (§2). Aucune dépendance.
3. [#252](https://github.com/marmotz/graphene-term/issues/252) : application du réglage et documentation (§3, §4). Dépend de #250 et #251.
