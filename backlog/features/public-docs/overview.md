# Public documentation

**Statut** : tâches créées — voir [technical.md](technical.md)

## Contexte

Graphene va être présenté publiquement sur X. Le README actuel (environ 230 lignes) a
grossi au fil des features : il mélange lancement, configuration détaillée et
comportements internes, sans parcours clair pour un nouveau visiteur. La documentation
existante sous `docs/` (architecture, décisions, développement, vision) vise les
contributeurs, pas les utilisateurs.

## Objectif

Qu'un visiteur arrivant depuis X comprenne en quelques minutes ce qu'est Graphene, ce
qu'il sait faire, en quoi il se distingue des autres terminaux, comment l'installer ou le
compiler, comment le configurer et comment contribuer ; et qu'un utilisateur trouve
ensuite une documentation détaillée de chaque fonctionnalité.

## Décisions actées

### Documentation

- Toute la documentation est en Markdown, versionnée dans le dépôt (pas de site
  généré, pas d'hébergement externe). La publication sur X se contente de pointer vers
  le dépôt.
- Le README devient une page d'entrée claire : présentation, fonctionnalités,
  configuration, build, contribution.
- Une documentation utilisateur plus détaillée complète le README, avec un fichier par
  grand thème (installation, configuration, thèmes, raccourcis, panneaux...).
- La documentation utilisateur est bilingue, anglais et français, dans des répertoires
  séparés (`docs/en/` et `docs/fr/`).
- Le README existe en anglais (`README.md`, affiché par GitHub) et en français
  (`README.fr.md`), chacun avec un lien vers l'autre en tête.
- Les documents contributeur existants (`architecture.md`, `decisions.md`,
  `development.md`, `vision.md`) sont déplacés dans `docs/dev/`, mis à jour et remis en
  forme pour être lisibles. Ils restent en anglais uniquement.
- Un `CONTRIBUTING.md` à la racine explique comment contribuer et renvoie vers la
  documentation contributeur.
- Le README s'ouvre sur une capture d'écran ou un GIF de Graphene.
- Installation documentée de deux façons : paquets générés à chaque release (deb, rpm,
  AppImage, dmg) ou build depuis les sources, avec les prérequis par OS. Le paquet AUR,
  prévu prochainement, sera ajouté quand il existera.

### Comparatif avec d'autres terminaux

- Le comparatif figure uniquement dans le README.
- Terminaux comparés (Linux) : Alacritty, Kitty, WezTerm, foot, Terminator,
  GNOME Terminal et Konsole. La version testée de chaque terminal est indiquée.
- Benchmark de performance, mesuré dans les mêmes conditions pour chaque terminal :
  - débit d'affichage d'un gros flux ;
  - taux de rafraîchissement (images par seconde) sous charge ;
  - temps de démarrage ;
  - mémoire utilisée, au repos et sous charge.
  La latence à la frappe n'est pas mesurée.
- Le benchmark est produit par un script versionné dans le dépôt, qui génère
  directement le tableau Markdown du README. Les benchmarks criterion existants
  (`benches/render.rs`) mesurent des étapes internes et ne servent pas à ce comparatif.
- Un tableau des fonctionnalités (qui a quoi) accompagne le benchmark. Ses lignes
  couvrent toutes les fonctionnalités de Graphene. Il est rempli à la main, d'après des
  tests faits sur la version indiquée de chaque terminal.
- Les résultats sont présentés en tableaux Markdown ; pas de graphique dans un premier
  temps.
