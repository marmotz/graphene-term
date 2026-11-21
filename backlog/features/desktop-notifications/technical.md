# Notifications de bureau : conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## Vue d'ensemble

```text
thread PTY                               thread UI (Session)                     thread « notifier »
signals::Scanner ── Chunk::Notify ──┐
timeline (fin de commande + durée) ─┴─> Timeline::notices ──(Wakeup)──> collecte, filtre,     ──> D-Bus (Linux)
TermEvent::Bell ─────────────────────────────────────────────────────>  compteurs, badges     ──> UNUserNotificationCenter (macOS)
                                          <── UserEvent::NotificationClicked(pane) ──────────────  clic
```

Trois étapes :

1. **Détection** (thread PTY) : OSC 9 et OSC 777 dans le scanner de signaux, et fin de
   commande dans la timeline. Les avis sont mis en file dans `Timeline`.
2. **Décision** (thread UI) : à chaque `Wakeup`, on vide la file. On applique les
   interrupteurs, le seuil et la règle « pane pas sous les yeux », puis on incrémente
   les compteurs et on demande l'affichage.
3. **Affichage** (thread dédié, un backend par OS) : notification système, badge de la
   barre des tâches et retour du clic.

## 1. Détection

### OSC 9 / OSC 777 dans le scanner

`vte` ignore les OSC inconnues sans appeler de handler. C'est pour ça que les signaux de
bloc sont repérés avant le parser, par [`signals::Scanner`](../../../src/signals.rs).
OSC 9 et OSC 777 y passent déjà : `State::Osc` collecte la charge utile jusqu'à
`MAX_OSC_LEN` (4096 octets), puis `finish` ([signals.rs:284](../../../src/signals.rs:284))
la jette quand ce n'est pas un signal `6973;<token>;`.

Ajouts :

- Une nouvelle variante `Chunk::Notify(Notice)` ([signals.rs:62](../../../src/signals.rs:62))
  et un `Event::Notify` interne, émis par `finish` quand la charge utile est :
  - `9;<texte>`, sauf si le premier champ est un sous-code ConEmu (`1` à `12` suivi de
    `;` ou de la fin) : `9;4;…` (progression, feature
    [osc-progress-indicator](../osc-progress-indicator/overview.md)), `9;9;…` (cwd), etc.
    Ces formes sont ignorées ici.
  - `777;notify;<titre>;<corps>`. Les autres sous-commandes 777 sont ignorées. Le corps
    reprend tout ce qui suit le 3e `;`, car un corps peut contenir des `;`.
- Le texte est décodé en UTF-8 avec perte (`from_utf8_lossy`). Les caractères de
  contrôle sont retirés, puis titre et corps sont coupés à 256 caractères. La limite de
  4096 octets de `MAX_OSC_LEN` borne déjà l'entrée.
- `Notice` (nouveau type, dans `signals.rs`) :
  `Program { title: Option<String>, body: String }`.

Le scanner reste un module pur, testé par octets découpés en lectures comme ses tests
actuels.

### Transport jusqu'à la timeline

[`pty::process`](../../../src/pty.rs:76) passe tout chunk autre que `Bytes` à
`Timeline::apply` ([timeline.rs:307](../../../src/timeline.rs:307)). `apply` traite
`Chunk::Notify` **sans** passer par `pending`. Les signaux de bloc sont retenus tant que
l'écran alternatif est actif, mais une notification d'un programme plein écran (éditeur,
agent CLI) doit partir tout de suite. L'avis est poussé dans un nouveau champ
`Timeline::notices: Vec<PaneNotice>`.

### Fin de commande et durée

Aujourd'hui les blocs ne mesurent **pas** de durée : `Current`
([timeline.rs:102](../../../src/timeline.rs:102)) et `Block` ne portent aucun instant.
Ajouts :

- `Current::started: Option<Instant>`, posé sur `Signal::Start`
  ([timeline.rs:325](../../../src/timeline.rs:325)).
- Sur `Signal::End` ([timeline.rs:331](../../../src/timeline.rs:331)), on pousse
  `PaneNotice::CommandFinished { command, status, duration }` dans `notices`, avant que
  `finish` ne remplace `current`.
- `Cancel` et la ligne vide (`Status::Cancelled`, `Status::Empty`) ne produisent rien :
  aucune commande n'a tourné.
- Un `End` retenu pendant l'écran alternatif est appliqué à la sortie de celui-ci. La
  durée est mesurée à ce moment-là, ce qui est juste à quelques millisecondes près.

On met toujours l'avis en file. Le seuil est appliqué côté UI, parce que la config vit
sur le thread UI et peut être rechargée à chaud. Le coût se limite à un petit objet par
commande.

`Instant` est injecté dans `apply` (paramètre `now`) pour que les tests de timeline
restent déterministes.

### BEL

Déjà reçu sur le thread UI en `TermEvent::Bell`
([terminal_events.rs:91](../../../src/app/terminal_events.rs:91)), avec
`request_user_attention`. On garde cet appel et on ajoute un avis `Bell` qui suit le
même chemin de décision.

## 2. Décision (thread UI)

### Collecte

Dans `handle_terminal_event`, branche `TermEvent::Wakeup`
([terminal_events.rs:22](../../../src/app/terminal_events.rs:22)), on fait
`pane.terminal.timeline.lock()` puis `mem::take(&mut notices)`. Le `Wakeup` est toujours
envoyé après des octets traités ([pty.rs:459](../../../src/pty.rs:459)), donc un avis
ne reste jamais orphelin. Seul cas : un avis produit pendant une mise à jour
synchronisée part avec le `Wakeup` suivant.

Alternative écartée : un `UserEvent::Notice` envoyé depuis le thread PTY. `PtyLoop` est
générique sur `L: EventListener`, qui ne transporte que les `Event` d'alacritty. Il
faudrait un trait de plus sur tous les listeners, tests compris, pour un gain nul.

### Filtre

Nouveau module pur `src/notifications/mod.rs` :

- `fn wanted(notice, &NotificationsConfig) -> bool` : interrupteur de la source et, pour
  `CommandFinished`, `duration >= command_threshold`.
- `fn watched(focused, active_pane, pane) -> bool` : `focused && active_pane == Some(pane)`.
  Si le pane est regardé : ni notification, ni compteur.
- `fn content(notice, pane_title, lang) -> (String, String)` : titre et corps traduits.

| Source           | Titre                                                                   | Corps                                   |
| ---------------- | ----------------------------------------------------------------------- | --------------------------------------- |
| OSC 9            | titre du pane, sinon « Graphene »                                       | texte                                   |
| OSC 777          | titre fourni, sinon titre du pane                                       | corps                                   |
| Fin de commande  | « Commande terminée », « Commande échouée (code N) » ou « Commande terminée » si statut inconnu | `cargo build` · 2 min 13 s |
| BEL              | titre du pane                                                           | « Sonnerie »                            |

La durée est formatée par une fonction pure (`s`, `min s`, `h min`). Une commande vide
(hook sans ligne de commande) affiche seulement la durée.

### Compteurs et effacement

- `Pane::unseen: u32` (nouveau champ, [pane.rs](../../../src/pane.rs)). On l'incrémente
  à chaque avis retenu, via `workspace.pane_mut`. Le pane est alors « touché », donc
  son cadre se redessine.
- Total d'un onglet = somme de ses panes. Total de la fenêtre = somme de tous les panes.
  Les deux se calculent à la volée et ne sont pas stockés.
- Effacement : le compteur du pane qui devient « regardé » est remis à 0.
  - dans `focus_changed` ([panes.rs:17](../../../src/app/panes.rs:17)), pour le nouveau
    pane actif. Ce chemin couvre les changements d'onglet et de pane : 8 appels
    l'utilisent déjà ;
  - sur `WindowEvent::Focused(true)` ([application.rs:554](../../../src/app/application.rs:554)),
    pour le pane actif.
- Chaque remise à 0 ou incrément recalcule le total et l'envoie au backend s'il a
  changé (badge de la barre des tâches). Remettre un pane à 0 ferme aussi sa
  notification encore affichée.
- Fermer un pane retire son compteur, et le total est renvoyé.

### Rendu des badges

- Onglet : `TabSlot` ([tab_bar.rs:71](../../../src/chrome/tab_bar.rs:71)) gagne
  `badge: u32`. La largeur du titre (`title_width`) est réduite de celle de la pastille
  quand le badge est non nul. On la dessine dans `tabs`
  ([scene.rs:384](../../../src/chrome/scene.rs:384)) : pastille à la couleur d'accent du
  thème, chiffre en `readable_on`, et « 99+ » au-delà de 99.
- Pane : `PaneBarLayout` gagne `badge: u32`, dessiné dans `pane_bar`
  ([scene.rs:469](../../../src/chrome/scene.rs:469)) de la même façon.
- La barre d'onglets n'est affichée qu'à partir de 2 onglets
  ([tab_bar.rs:44](../../../src/chrome/tab_bar.rs:44)). Avec un seul onglet, seuls la
  barre du pane et la barre des tâches portent le compteur. C'est voulu : pas d'onglet,
  donc pas de badge d'onglet.
- Les deux layouts sont purs : on teste le retrait de la largeur et le texte « 99+ ».

## 3. Affichage : backend par OS

Interface commune, `src/notifications/backend.rs` :

```rust
pub enum Command {
    Show { pane: PaneId, title: String, body: String },
    Close { pane: PaneId },
    Badge(u32),
}
```

`spawn(proxy) -> Notifier`. `Notifier::send(Command)` ne bloque jamais. Les clics
reviennent par `UserEvent::NotificationClicked(PaneId)`, une nouvelle variante de
[`UserEvent`](../../../src/terminal.rs:31). Si le backend est indisponible (pas de
serveur de notifications, binaire macOS hors bundle), un `log::info!` est émis une fois
et les commandes sont ignorées. Les badges internes continuent de fonctionner.

**Remplacement (une notif par pane)** : le backend garde `pane -> id de la notification
affichée`. Un nouveau `Show` pour le même pane remplace la précédente : `replaces_id`
sur Linux, même `identifier` de requête sur macOS.

### Linux : D-Bus avec zbus

On reprend le modèle d'[appearance.rs](../../../src/appearance.rs:47) : un thread nommé
avec `zbus::block_on`. Il reçoit les commandes par canal et écoute les signaux D-Bus.

- `org.freedesktop.Notifications.Notify(app_name="Graphene", replaces_id, app_icon="graphene",
  summary, body, actions=["default", <libellé>], hints={"desktop-entry": "graphene"}, expire_timeout=-1)`.
  Le `desktop-entry` correspond à
  [packaging/graphene.desktop](../../../packaging/graphene.desktop) : le bureau regroupe
  et attribue ainsi les notifications.
- Le signal `ActionInvoked(id, "default")` cherche le pane associé à `id` et renvoie
  `NotificationClicked`. `NotificationClosed(id)` retire l'entrée.
- `Close` appelle `CloseNotification(id)`.
- **Badge** : signal `com.canonical.Unity.LauncherEntry.Update("application://graphene.desktop",
  {"count": x, "count-visible": x > 0})`, émis sur un objet exporté par la connexion.
  Ce protocole est pris en charge par KDE Plasma, le dock d'Ubuntu et Dash to Dock.
  GNOME sans extension n'affiche rien, et il n'existe pas de standard plus large. Avec
  `request_user_attention`, qu'on garde pour le BEL, c'est ce que le bureau permet.

Alternative écartée : la crate `notify-rust`. zbus est déjà une dépendance et le besoin
tient en deux méthodes et deux signaux.

### macOS : UNUserNotificationCenter et Dock

- Le bundle existe déjà : `Graphene.app`, `CFBundleIdentifier = dev.marmotz.graphene`,
  signature ad hoc ([scripts/package-macos.sh:73](../../../scripts/package-macos.sh:73)).
  `UNUserNotificationCenter` lève une exception hors bundle (`cargo run`). On vérifie
  donc `NSBundle.mainBundle.bundleIdentifier` avant tout appel. Sans bundle, le backend
  est indisponible (voir plus haut).
- Dépendances `cfg(target_os = "macos")` à ajouter : `objc2-user-notifications`,
  `objc2-foundation` et `objc2-app-kit`, dans les versions de la branche `objc2` 0.6
  déjà tirée par winit (le lockfile contient `objc2` 0.6.5 et `objc2-app-kit` 0.3.2).
  Versions exactes et features à vérifier à l'ajout.
- Autorisation : `requestAuthorizationWithOptions(alert | badge)` au premier `Show`, pas
  au démarrage, pour éviter une demande d'autorisation sans raison. Si l'utilisateur
  refuse, l'information est notée et les `Show` suivants sont ignorés.
- Délégué `UNUserNotificationCenterDelegate`, défini avec `objc2::define_class!` :
  - `willPresentNotification` renvoie `banner | list`. Sans ce délégué, macOS n'affiche
    rien quand l'app est au premier plan, alors qu'on notifie aussi pour un pane non
    actif d'une fenêtre active ;
  - `didReceiveNotificationResponse` lit le `PaneId` dans `userInfo` et envoie
    `NotificationClicked`.
- Remplacement : identifiant de requête `pane-<id>`. Pour `Close`, on utilise
  `removeDeliveredNotificationsWithIdentifiers`.
- Badge : `NSApplication.sharedApplication.dockTile.setBadgeLabel("5")`, ou `nil` à 0.
  Appel sur le thread principal, c'est-à-dire directement depuis le thread UI, sans
  passer par le thread du backend.

### Clic

`UserEvent::NotificationClicked(pane)` est traité dans `user_event`
([application.rs:451](../../../src/app/application.rs:451)) : `focus_pane(pane)`, qui
affiche l'onglet et efface le compteur, puis `window.focus_window()`. Si le pane a été
fermé entre-temps, on ramène seulement la fenêtre.

Limite connue : sous Wayland, ramener la fenêtre au premier plan dépend du compositeur
(xdg-activation). Certains se contentent de signaler la fenêtre comme demandant
l'attention. On ne peut pas l'éviter côté application. À vérifier sur KDE et GNOME
pendant l'implémentation.

## 4. Configuration

Nouvelle section `[notifications]`, déclarée à tous les endroits où passe une option
existante (le modèle est `[updates]`) :

```toml
# [notifications]
## Notifications requested by programs (OSC 9, OSC 777).
# program = true
## Notify when a command that ran at least `command_threshold` seconds finishes.
# command_finished = true
# command_threshold = 10
## Turn the terminal bell into a notification.
# bell = false
```

| Endroit                     | Changement                                                                                            |
| --------------------------- | ----------------------------------------------------------------------------------------------------- |
| [config.default.toml](../../../config.default.toml) | le bloc ci-dessus (source de la complétion des défauts, [config/mod.rs:33](../../../src/config/mod.rs:33)) |
| [config/mod.rs](../../../src/config/mod.rs:59) | `Config::notifications: NotificationsConfig`, `Default`, section reconnue dans `parse_with_unknown_keys` |
| [config/reader.rs](../../../src/config/reader.rs:168) | lecture des 4 clés ; `command_threshold` est un entier ≥ 1, sinon erreur `InvalidValue` et valeur précédente conservée |
| [settings/model.rs](../../../src/settings/model.rs:131) | `Section::Notifications` et 4 `Field` (3 `Widget::Toggle`, 1 numérique) |
| [i18n.rs](../../../src/i18n.rs) | libellés des réglages, textes des notifications (tableau ci-dessus), libellé de l'action |
| `docs/en/configuration.md`, `docs/fr/configuration.md` | documentation de la section |

Le seuil est en secondes entières, pour suivre le format des autres nombres de la
config. Le rechargement à chaud s'applique au prochain avis, sans autre travail.

## 5. Conséquences vérifiées

- **Feature OSC 9;4** : le scanner reconnaît le préfixe `9;`. La règle « sous-code
  ConEmu ignoré » lui laisse le champ libre. Le jour où la progression sera implémentée,
  `9;4` deviendra un autre `Chunk` émis au même endroit.
- **Contexte de overview.md** : l'overview affirmait que les blocs connaissent déjà la
  durée des commandes, ce qui est faux. Il est corrigé, et la mesure fait partie de
  cette feature.
- **Sécurité** : un fichier affiché par `cat` peut contenir une OSC 9. C'est le
  comportement de tous les terminaux qui la gèrent, et l'interrupteur `program` permet
  de la couper. Le texte est assaini et borné. Aucune action autre que l'affichage n'est
  déclenchée.
- **Performance** : le scanner fait déjà tourner chaque OSC dans sa machine à états. On
  ajoute seulement un `starts_with` sur 2 préfixes à la fin de l'OSC. Rien sur le chemin
  des octets ordinaires.
- **Tests existants** : `Timeline::apply` change de signature (`now`), il faut adapter
  les helpers de test de [timeline.rs](../../../src/timeline.rs) et [pty.rs](../../../src/pty.rs).
  Le test `every_key_is_read` et les tests de complétion de config doivent couvrir la
  nouvelle section.

## Découpage en tâches d'implémentation

Ordre conseillé : #175 et #177 en parallèle, puis #183, puis #184, puis #187, #188 et #189
en parallèle.

| Issue | Tâche | Dépend de |
| ----- | ----- | --------- |
| [#175](https://github.com/marmotz/graphene-term/issues/175) | Détecter OSC 9 et OSC 777 dans le flux PTY | aucune |
| [#177](https://github.com/marmotz/graphene-term/issues/177) | Section de configuration `[notifications]` | aucune |
| [#183](https://github.com/marmotz/graphene-term/issues/183) | Mesurer la durée des commandes et signaler leur fin | #175 |
| [#184](https://github.com/marmotz/graphene-term/issues/184) | Décider, compter et effacer les notifications | #175, #177, #183 |
| [#187](https://github.com/marmotz/graphene-term/issues/187) | Badges compteurs sur les onglets et les barres de pane | #184 |
| [#188](https://github.com/marmotz/graphene-term/issues/188) | Backend Linux : D-Bus et badge LauncherEntry | #184 |
| [#189](https://github.com/marmotz/graphene-term/issues/189) | Backend macOS : UNUserNotificationCenter et badge du Dock | #184 |
