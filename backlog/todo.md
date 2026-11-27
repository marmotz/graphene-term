<!-- backlog-sync 2026-11-27T19:29Z — GENERATED, do not hand-edit. Regenerate: skill backlog-sync -->

# Backlog

## Internationalisation (i18n)  ·  [overview](features/i18n/overview.md)

_tâches créées — voir [technical.md](features/i18n/technical.md)_ — 1/7 tasks done

| Done | Issue                                                       | Title                                                            | Blocked by       |
|------|-------------------------------------------------------------|------------------------------------------------------------------|------------------|
| [x]  | [#243](https://github.com/marmotz/graphene-term/issues/243) | feat(i18n): move UI texts to Fluent files                        | —                |
| [ ]  | [#244](https://github.com/marmotz/graphene-term/issues/244) | feat(i18n): regional variants en-US, en-GB, fr-CA with fallback  | #243             |
| [ ]  | [#245](https://github.com/marmotz/graphene-term/issues/245) | feat(i18n): locale-aware numbers, durations and dates            | #244             |
| [ ]  | [#246](https://github.com/marmotz/graphene-term/issues/246) | feat(i18n): translate configuration errors                       | #243             |
| [ ]  | [#247](https://github.com/marmotz/graphene-term/issues/247) | feat(i18n): translated labels in settings choice lists           | #244             |
| [ ]  | [#248](https://github.com/marmotz/graphene-term/issues/248) | feat(i18n): UiText type and clippy guard against hardcoded texts | #245, #246, #247 |
| [ ]  | [#249](https://github.com/marmotz/graphene-term/issues/249) | feat(i18n): Spanish, German and Italian                          | #245, #246, #247 |

## Saisie IME ·  [overview](features/ime-input/overview.md)

_tâches créées — voir [technical.md](features/ime-input/technical.md)_ — 0/5 tasks done

| Done | Issue                                                       | Title                                                                    | Blocked by |
|------|-------------------------------------------------------------|--------------------------------------------------------------------------|------------|
| [ ]  | [#169](https://github.com/marmotz/graphene-term/issues/169) | [Config] Option [keyboard] ime                                           | —          |
| [ ]  | [#170](https://github.com/marmotz/graphene-term/issues/170) | [Saisie] Activation de l'IME, état de composition et texte validé        | #169       |
| [ ]  | [#171](https://github.com/marmotz/graphene-term/issues/171) | [Rendu] Affichage de la composition au curseur et fenêtre de suggestions | #170       |
| [ ]  | [#172](https://github.com/marmotz/graphene-term/issues/172) | [Palette] Saisie IME dans la palette de commandes                        | #171       |
| [ ]  | [#173](https://github.com/marmotz/graphene-term/issues/173) | [Docs] Documenter la saisie IME et vérifier les plateformes              | #171, #172 |

## Protocole clavier kitty ·  [overview](features/kitty-keyboard-protocol/overview.md)

_tâches créées — voir [technical.md](features/kitty-keyboard-protocol/technical.md)_ — 0/6 tasks done

| Done | Issue                                                       | Title                                                                      | Blocked by       |
|------|-------------------------------------------------------------|----------------------------------------------------------------------------|------------------|
| [ ]  | [#191](https://github.com/marmotz/graphene-term/issues/191) | [Shell] Vérifier les liaisons fish 4 avec le protocole clavier kitty actif | —                |
| [ ]  | [#192](https://github.com/marmotz/graphene-term/issues/192) | [Clavier] Module d'encodage kitty, modifyOtherKeys et legacy               | —                |
| [ ]  | [#193](https://github.com/marmotz/graphene-term/issues/193) | [Terminal] Capter modifyOtherKeys et répondre à sa requête                 | —                |
| [ ]  | [#194](https://github.com/marmotz/graphene-term/issues/194) | [Clavier] Brancher l'encodeur et activer le protocole kitty                | #191, #192, #193 |
| [ ]  | [#195](https://github.com/marmotz/graphene-term/issues/195) | [Clavier] Transmettre les relâchements des touches envoyées au programme   | #194             |
| [ ]  | [#196](https://github.com/marmotz/graphene-term/issues/196) | [Docs] Documenter les protocoles clavier et vérifier les plateformes       | #194, #195       |

## Multiplexage ·  [overview](features/multiplexing/overview.md)

_tâches créées — voir [technical.md](features/multiplexing/technical.md)_ — 0/10 tasks done

| Done | Issue                                                       | Title                                                                  | Blocked by |
|------|-------------------------------------------------------------|------------------------------------------------------------------------|------------|
| [ ]  | [#197](https://github.com/marmotz/graphene-term/issues/197) | Multiplexage : protocole client/serveur                                | —          |
| [ ]  | [#205](https://github.com/marmotz/graphene-term/issues/205) | Multiplexage : journal d'octets par pane                               | —          |
| [ ]  | [#206](https://github.com/marmotz/graphene-term/issues/206) | Multiplexage : serveur graphene --mux                                  | #197, #205 |
| [ ]  | [#207](https://github.com/marmotz/graphene-term/issues/207) | Multiplexage : backend distant de Terminal et repli local              | #197, #206 |
| [ ]  | [#208](https://github.com/marmotz/graphene-term/issues/208) | Multiplexage : rejeu et rattachement des fenêtres détachées            | #207       |
| [ ]  | [#209](https://github.com/marmotz/graphene-term/issues/209) | Multiplexage : dialogue de fermeture et window.on_close                | #208       |
| [ ]  | [#210](https://github.com/marmotz/graphene-term/issues/210) | Multiplexage : passation du serveur au nouveau binaire                 | #206, #207 |
| [ ]  | [#211](https://github.com/marmotz/graphene-term/issues/211) | Multiplexage : icône et notification sous Linux                        | #206       |
| [ ]  | [#213](https://github.com/marmotz/graphene-term/issues/213) | Multiplexage : icône de barre de menus sous macOS                      | #206       |
| [ ]  | [#215](https://github.com/marmotz/graphene-term/issues/215) | Multiplexage : images kitty par fichier temporaire ou mémoire partagée | #205, #208 |

## Activation du quick terminal ·  [overview](features/quick-terminal-toggle/overview.md)

_terminé — voir [technical.md](features/quick-terminal-toggle/technical.md)_ — 3/3 tasks done

| Done | Issue                                                       | Title                                                      | Blocked by |
|------|-------------------------------------------------------------|------------------------------------------------------------|------------|
| [x]  | [#275](https://github.com/marmotz/graphene-term/issues/275) | Add the quick_terminal.enabled setting                     | —          |
| [x]  | [#276](https://github.com/marmotz/graphene-term/issues/276) | Gate the quick terminal launches on quick_terminal.enabled | #275       |
| [x]  | [#277](https://github.com/marmotz/graphene-term/issues/277) | Add the quick terminal switch to the settings window       | #275       |

## Bugs

### La navigation entre blocs ne peut pas sélectionner le bloc en cours ·  [overview](bugs/block-navigation-skips-current-block/overview.md)

_terminé — voir [technical.md](bugs/block-navigation-skips-current-block/technical.md)_ — 2/2 tasks done

| Done | Issue                                                       | Title                                                | Blocked by |
|------|-------------------------------------------------------------|------------------------------------------------------|------------|
| [x]  | [#279](https://github.com/marmotz/graphene-term/issues/279) | Block navigation: select the current block           | —          |
| [x]  | [#280](https://github.com/marmotz/graphene-term/issues/280) | Block actions: copy the command of the current block | #279       |
