# Internationalisation (i18n)

**Statut** : terminé — voir [technical.md](technical.md)

## Contexte

L'interface est déjà traduite en anglais et en français : les textes vivent dans le code
(`src/i18n.rs`, un `match` exhaustif par langue) et le réglage `language` choisit entre
la langue détectée et une langue forcée. Ajouter une langue impose d'écrire du Rust, les
variantes régionales n'existent pas, des textes visibles restent peut-être en dur, et
rien ne gère les pluriels, les dates ou les nombres selon la locale.

## Objectif

Ouvrir l'interface à d'autres langues et variantes régionales, permettre à des
contributeurs de traduire sans écrire de Rust, couvrir tous les textes visibles par
l'utilisateur, et adapter pluriels, dates et nombres à la locale.

## Décisions actées

### Langues livrées

- Langues de base : anglais (en-US), français, espagnol, allemand, italien.
- Variantes régionales : en-GB et fr-CA.
- L'anglais de référence, et repli final, est en-US. L'anglais actuel devient en-US.
- Une variante ne définit que ses différences : un texte absent vient de la langue
  mère (fr-CA vers fr, en-GB vers en-US), puis de en-US.
- La détection de la locale reconnaît les nouvelles langues et les variantes
  (`fr_CA.UTF-8` donne fr-CA). Une langue inconnue donne en-US.
- Le réglage `language` propose toutes les langues et variantes livrées.

### Traductions

- Les textes sortent du code vers des fichiers de traduction dans le dépôt, éditables
  sans connaître Rust.
- Ces fichiers sont embarqués dans le binaire à la compilation. Un utilisateur ne
  charge pas ses propres fichiers de langue.
- Les nouvelles langues sont d'abord traduites par IA, puis relues et corrigées par la
  communauté.
- Chaque langue porte un statut de relecture (relue ou non) visible dans le dépôt
  seulement. L'interface n'affiche rien à ce sujet.

### Couverture

- Tous les textes de l'interface graphique sont traduits : menus, dialogues, palette,
  bannières, notifications.
- Les erreurs de configuration montrées à l'utilisateur sont traduites. Le détail
  d'une erreur de syntaxe TOML, produit par la bibliothèque, reste en anglais après un
  préfixe traduit.
- Les listes de la fenêtre Réglages affichent des libellés traduits, pas les valeurs
  du fichier. Les langues y portent leur nom dans leur propre langue.
- Hors périmètre : aide et messages de la ligne de commande, journaux techniques.
- La documentation et le README restent en anglais et en français.

### Formats régionaux

- Pluriels selon les règles de chaque langue.
- Dates et heures au format de la locale, là où l'interface en affiche.
- Nombres avec les séparateurs de la locale. Les champs numériques des Réglages
  acceptent le point et le séparateur décimal de la locale. Le fichier garde le point.
- Les unités des durées sont traduites. Le format de date est prêt, mais aucun écran
  n'affiche de date pour l'instant.

### Garde-fou

- La CI échoue si une langue de base manque un texte, ou si un texte visible est écrit
  en dur hors des fichiers de traduction.

### Hors périmètre

- Langues écrites de droite à gauche (arabe, hébreu) : chantier de rendu et de mise en
  page à traiter dans une feature à part.
