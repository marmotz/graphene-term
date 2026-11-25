# Internationalisation (i18n) : conception technique

Voir [overview.md](overview.md) pour les décisions produit.

## État actuel

- [`src/i18n.rs`](../../../src/i18n.rs) porte tout : `enum Lang { En, Fr }` (i18n.rs:13),
  `enum Msg` de ~110 variantes, dont certaines portent des valeurs (i18n.rs:209), et
  `text(lang, &Msg) -> Cow<'static, str>` (i18n.rs:395). Cette fonction aiguille vers
  `english` (i18n.rs:402) ou `french` (i18n.rs:602), deux `match` exhaustifs : une
  variante sans traduction ne compile pas.
- Les pluriels sont codés à la main par langue : `Msg::LinesHidden(1)` contre
  `LinesHidden(rows)` (i18n.rs:591), et `ProgramsRunning`, qui teste la longueur de la
  liste (i18n.rs:576).
- `Lang::from_locale_or_system` (i18n.rs:50) lit `LC_ALL`, puis `LC_MESSAGES`, puis
  `LANG`, puis `sys_locale` sur macOS. Il garde la langue (avant `_`, `-`, `.`, `@`) :
  `fr` donne `Fr`, tout le reste donne `En`. La région est ignorée.
- Le réglage `language` est l'enum serde `Language { Auto, En, Fr }`
  ([config/values.rs:7](../../../src/config/values.rs)), avec `VALUES` (values.rs:17) et
  `resolve` (values.rs:27). Il est appliqué à chaud dans
  [app/refresh.rs:69](../../../src/app/refresh.rs).
- `Lang` est `Copy` et passe en paramètre dans tout le chrome : `context_menu`,
  `command_palette`, `dialog`, `banner`, `settings`, `frame`. Les libellés y sont déjà des
  `Msg`, convertis en `String` au dernier moment (context_menu.rs:80,
  command_palette.rs:230).
- Les tests de [i18n.rs:886](../../../src/i18n.rs) (`variant_index`, `all_messages`,
  `every_message_has_a_text_in_every_language`, détection de locale) bouclent sur
  `LANGS` (i18n.rs:392).

### Textes visibles encore hors i18n

- **Erreurs de configuration** : `ConfigError.message: String`
  ([config/error.rs:15](../../../src/config/error.rs)) est un texte anglais libre. Il est
  construit dans [config/reader.rs](../../../src/config/reader.rs) (~20 sites : « not a
  number », « {size} is out of … », « unknown key … »), dans
  [config/files.rs](../../../src/config/files.rs), [theme.rs](../../../src/theme.rs)
  (399, 420), [theme_edit.rs](../../../src/theme_edit.rs) (83, 108, 150) et
  [config_edit/mod.rs](../../../src/config_edit/mod.rs) (174, 303, 312). Le texte est
  affiché par `Banner::config_errors` ([banner.rs:28](../../../src/banner.rs)), avec un
  « (+N more) » lui aussi en dur. Les erreurs de syntaxe recopient le message de la
  crate `toml` (reader.rs:82).
- **Listes des Réglages** : `names(&X::VALUES)`
  ([settings/model.rs:318](../../../src/settings/model.rs)) affiche les valeurs brutes du
  fichier (`auto`, `block`, `beam`, `en`). `Choice.label: Option<Msg>` (model.rs:59)
  existe déjà, mais vaut `None` pour ces listes.
- **Durées** : `notifications::format_duration`
  ([notifications/mod.rs:83](../../../src/notifications/mod.rs)) écrit les unités `s`,
  `min` et `h` en dur.
- **Nombres** : les champs numériques affichent `n.to_string()` (model.rs:676) et
  parsent avec `str::parse::<f64>` (model.rs:341), donc toujours avec un point.
  `AtLeast` et `AtMost` reçoivent un nombre déjà formaté en `String`.
- **Dates** : aucune date n'est affichée aujourd'hui.

## Choix retenus

### Format : Fluent

Un fichier `.ftl` par langue, lu par `fluent-bundle` 0.16 (avec `unic-langid` 0.9).

- **Retenu** : Fluent. Les pluriels CLDR et les sélecteurs (`Direction`, `CloseTarget`)
  s'écrivent dans le fichier. La syntaxe se lit sans connaître Rust, et Weblate la gère.
- **Écarté** : gettext `.po`, qui n'a qu'une formule de pluriel et pas de sélecteurs.
- **Écarté** : un TOML maison, qui obligerait à réécrire pluriels, sélecteurs et repli.

Arborescence :

```
i18n/
  languages.ftl   # names of the languages in their own language, shared by every bundle
  en-US.ftl       # reference, complete
  en-GB.ftl       # differences only
  fr.ftl          # complete
  fr-CA.ftl       # differences only
  es.ftl de.ftl it.ftl   # complete
```

Les fichiers sont embarqués par `include_str!`. Chaque fichier commence par une ligne
`## Status: reviewed` ou `## Status: unreviewed`, c'est le statut de relecture de
l'overview. Un test vérifie que la ligne est présente.

Exemple de pluriel, qui remplace le `match` codé à la main :

```ftl
lines-hidden = { $rows ->
    [one] { $rows } line hidden
   *[other] { $rows } lines hidden
}
```

### API : `Msg` reste, un test remplace la garantie de compilation

- **Retenu** : `Msg` reste l'API du code, et les appelants ne changent pas.
  `english` et `french` sont remplacés par un seul `match` exhaustif
  `fn fluent(msg: &Msg) -> (&'static str, FluentArgs)`, qui donne l'identifiant et les
  arguments du message. Une nouvelle variante ne compile toujours pas tant qu'elle n'a
  pas d'identifiant.
- **Écarté** : générer `Msg` depuis `en-US.ftl` dans un `build.rs`. L'erreur arriverait
  à la compilation, mais le code généré est plus dur à lire et à relire.
- Tests, lancés par `cargo test` en CI
  ([ci.yml:42](../../../.github/workflows/ci.yml)) :
  - pour chaque `Msg` de `all_messages()` et chaque langue de base, l'identifiant existe
    **dans le fichier de cette langue**, sans passer par le repli, et le formatage ne
    renvoie aucune erreur ;
  - aucun fichier ne définit un identifiant inconnu de `en-US.ftl` (pas d'orphelin) ;
  - les variantes (`en-GB`, `fr-CA`) peuvent omettre n'importe quel identifiant ;
  - chaque fichier porte sa ligne de statut.

### Langues et repli

`Lang` reste un enum `Copy`, puisqu'il est passé partout par valeur :

```rust
pub enum Lang { #[default] EnUs, EnGb, Fr, FrCa, Es, De, It }
```

- `Lang::parent()` donne la langue de repli : `EnGb` vers `EnUs`, `FrCa` vers `Fr`, les
  autres vers `None`. `Lang::tag()` donne l'étiquette BCP 47 (`fr-CA`).
- `text(lang, msg)` cherche le message dans le bundle de `lang`, puis dans celui de
  `parent()`, puis dans `EnUs`. Les bundles sont des
  `fluent_bundle::concurrent::FluentBundle`, construits une fois dans un `LazyLock`. La
  version concurrente est `Sync`, et les notifications partent d'un autre thread.
- `set_use_isolating(false)` : l'interface n'a pas de RTL, et les marques FSI/PDI
  s'afficheraient avec la police de la grille.
- `text` renvoie désormais un `String` : Fluent formate à chaque appel, et le
  `Cow<'static>` actuel n'a plus d'intérêt. Les appelants font déjà `.into_owned()`.

**Détection** : `from_locale_or_system` lit aussi la région. `fr_CA.UTF-8` et `fr-CA`
donnent `FrCa`, `en_GB` donne `EnGb`, `fr_BE` donne `Fr`, `es_MX` donne `Es`, `en_AU`
donne `EnUs`. Une langue inconnue donne `EnUs`.

**Réglage `language`** : les valeurs deviennent `auto`, `en-US`, `en-GB`, `fr`,
`fr-CA`, `es`, `de`, `it`. `en` reste accepté comme alias de `en-US` (`#[serde(alias)]`),
pour que les fichiers existants restent valides. La casse est ignorée.

### Formats régionaux : ICU4X

- **Pluriels** : gérés par Fluent, qui embarque ses propres règles CLDR.
- **Nombres** : `icu_decimal` 2.4. Le hook `FluentBundle::set_formatter` formate chaque
  `FluentValue::Number` avec le formateur de la locale du bundle, mémoïsé par le
  memoizer d'intl du bundle. La sélection de pluriel reçoit toujours le nombre brut.
  `AtLeast` et `AtMost` passent un nombre à la place d'une `String`.
- **Saisie** : `parse_number` (model.rs:341) accepte le point et le séparateur décimal
  de la locale, et ignore le séparateur de milliers. Le champ affiche la valeur au
  format de la locale. Le fichier garde toujours le point.
- **Durées** : `format_duration` passe par des messages Fluent
  (`duration-seconds = { $s } s`, etc.) et prend un `Lang`.
- **Dates** : `icu_datetime` 2.4 dans une fonction `i18n::format_datetime(lang, …)`,
  testée mais sans appelant pour l'instant. La dépendance est acceptée dès maintenant.
- L'étiquette `unic-langid` de Fluent est convertie en `icu_locale::Locale` depuis
  `Lang::tag()`.

### Erreurs de configuration traduites

- `ConfigError.message: String` est remplacé par `detail: ConfigMsg`, un enum dans
  `i18n` (`NotANumber`, `OutOfRange { value, min, max, used }`, `UnknownKey(path)`,
  `MustBeTable(name)`, `UnknownTheme { name, fallback }`, `CannotRead(io)`,
  `CannotWrite(io)`, `Syntax(toml_detail)`, …). Il est exposé par
  `Msg::Config(ConfigMsg)`.
- `Display for ConfigError` (error.rs:69) disparaît au profit de
  `ConfigError::text(lang)`. `Banner::config_errors` prend un `Lang`, et le
  « (+N more) » devient un message avec un pluriel.
- Les textes que nous ne produisons pas restent en anglais et sont insérés comme
  détail : message de la crate `toml` (« Syntaxe invalide, ligne 12 : … ») et texte
  d'une `io::Error`.

### Listes des Réglages traduites

- Chaque enum de choix (`Language`, `CursorShape`, `TabPosition`, `StartupMode`,
  `UpdateInterval`, `QuickTerminalEdge`, …) gagne un `Msg::Choice(...)`.
  `names()` (model.rs:318) remplit `Choice.label`, ce qui est déjà affiché quand il
  existe. La valeur écrite dans le fichier ne change pas.
- Les langues s'affichent par leur nom dans leur propre langue, tiré de
  `languages.ftl` (« Deutsch », « Français (Canada) »), et non traduit. `auto` est
  traduit.

### Garde-fou contre les textes en dur

- **Retenu** : par les types. Un `pub struct UiText(String)` ne se construit que par
  `i18n::text`, ou par `UiText::user(String)` pour un contenu qui n'est pas un texte de
  l'application : titre d'onglet, nom de programme, nom de thème, commande. Les champs
  d'affichage prennent `UiText` à la place de `String` : `Banner.text` et `actions`,
  `context_menu::Item.label`, `command_palette::Entry.label`, le titre et le message de
  `dialog`, le titre et le corps des notifications. Un littéral ne compile plus.
- Fenêtre Réglages (egui) : `ui.label`, `ui.button`, `ui.heading` et
  `ui.selectable_label` acceptent un `&str`. Un `clippy.toml` les met en
  `disallowed-methods`, et la vue passe par des enveloppes qui prennent `UiText`.
  `cargo clippy -- -D warnings` tourne déjà en CI (ci.yml:39).
- **Écarté** : un test qui scanne le code à la recherche de littéraux. Il donne des faux
  positifs et se contourne facilement.
- Les symboles (`×` de banner.rs:15 et chrome/scene.rs:46) passent par
  `UiText::symbol`.
- Les noms de touches des raccourcis (`Ctrl+Shift+E`) restent en anglais : ce sont ceux
  du fichier `keybindings`.

### Traduction des nouvelles langues

`es.ftl`, `de.ftl`, `it.ftl`, `en-GB.ftl` et `fr-CA.ftl` sont produits par IA à partir de
`en-US.ftl` et `fr.ftl`, avec la ligne `## Status: unreviewed`. Un
`i18n/CONTRIBUTING.md` explique comment relire une langue et passer son statut à
`reviewed`.

## Conséquences vérifiées

- Appelants de `text` : la signature `text(Lang, &Msg)` est conservée. Seul le type de
  retour change (`String`), et les sites qui font `.into_owned()` le perdent.
- `Lang::En` et `Lang::Fr` sont renommés (`EnUs`, `Fr`). Les usages hors i18n sont
  `frame_cache.rs:78` et des tests : à mettre à jour.
- `LANGS` (test) contient les 7 langues. Les tests de détection existants
  (`detect_*`) sont adaptés aux régions.
- Configuration existante : `language = "en"` reste valide grâce à l'alias. Le test
  config/mod.rs:385, qui boucle sur `Language::VALUES`, couvre les nouvelles valeurs.
- `ConfigError` est construit dans les tests de banner.rs:288, settings/model.rs:1507
  et suivants, et config/mod.rs:1021 : à passer à `ConfigMsg`.
- Les tests de chrome comparent des libellés anglais (`entry_at(&items, "Close pane")`) :
  ils restent valides, puisque le texte de `en-US.ftl` est identique.
- Documentation : `docs/en/configuration.md:118` et `docs/fr/configuration.md:123`
  listent les nouvelles valeurs de `language`.
- Nouvelles dépendances : `fluent-bundle`, `unic-langid`, `icu_decimal`,
  `icu_datetime`, `icu_locale`. Les données compilées d'ICU4X grossissent le binaire :
  à mesurer dans la première tâche qui ajoute ces dépendances.

## Découpage en tâches d'implémentation

1. [#243](https://github.com/marmotz/graphene-term/issues/243) : textes en-US et fr dans
   des fichiers Fluent, bundles, tests de complétude.
2. [#244](https://github.com/marmotz/graphene-term/issues/244) : variantes en-GB et
   fr-CA, repli, détection avec région, réglage `language` (dépend de #243).
3. [#245](https://github.com/marmotz/graphene-term/issues/245) : nombres, durées et dates
   selon la locale, avec ICU4X (dépend de #244).
4. [#246](https://github.com/marmotz/graphene-term/issues/246) : erreurs de configuration
   traduites (dépend de #243).
5. [#247](https://github.com/marmotz/graphene-term/issues/247) : libellés traduits des
   listes des Réglages, noms des langues (dépend de #244).
6. [#248](https://github.com/marmotz/graphene-term/issues/248) : type `UiText` et
   garde-fou clippy (dépend de #245, #246, #247).
7. [#249](https://github.com/marmotz/graphene-term/issues/249) : espagnol, allemand,
   italien, guide de relecture (dépend de #245, #246, #247).
