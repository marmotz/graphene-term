<!-- backlog-sync 2026-11-26T20:02Z — GENERATED, do not hand-edit. Regenerate: skill backlog-sync -->

# Backlog

## Garder la fenêtre ouverte après `-e`  ·  [overview](features/execute-hold/overview.md)

_tâches créées — voir [technical.md](features/execute-hold/technical.md)_ — 0/1 tasks done

| Done | Issue                                                       | Title                                          | Blocked by |
|------|-------------------------------------------------------------|------------------------------------------------|------------|
| [ ]  | [#266](https://github.com/marmotz/graphene-term/issues/266) | Keep the window open after the program with -E | —          |

## Internationalisation (i18n)  ·  [overview](features/i18n/overview.md)

_tâches créées — voir [technical.md](features/i18n/technical.md)_ — 0/7 tasks done

| Done | Issue                                                       | Title                                                            | Blocked by       |
|------|-------------------------------------------------------------|------------------------------------------------------------------|------------------|
| [ ]  | [#243](https://github.com/marmotz/graphene-term/issues/243) | feat(i18n): move UI texts to Fluent files                        | —                |
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

## Per-pane font size ·  [overview](features/pane-font-size/overview.md)

_terminé — voir [technical.md](features/pane-font-size/technical.md)_ — 4/4 tasks done

| Done | Issue                                                       | Title                                                                 | Blocked by |
|------|-------------------------------------------------------------|-----------------------------------------------------------------------|------------|
| [x]  | [#256](https://github.com/marmotz/graphene-term/issues/256) | feat(pane-font-size): glyph cache keyed by font size                  | —          |
| [x]  | [#257](https://github.com/marmotz/graphene-term/issues/257) | feat(pane-font-size): per-pane font offset and keyboard actions       | #256       |
| [x]  | [#258](https://github.com/marmotz/graphene-term/issues/258) | feat(pane-font-size): Ctrl+wheel changes the hovered pane's font size | #257       |
| [x]  | [#259](https://github.com/marmotz/graphene-term/issues/259) | feat(pane-font-size): restore each pane's font size with the session  | #257       |

## Quick terminal (mode Quake)  ·  [overview](features/quick-terminal/overview.md)

_tâches créées — voir [technical.md](features/quick-terminal/technical.md)_ — 4/5 tasks done

| Done | Issue                                                       | Title                                                                        | Blocked by       |
|------|-------------------------------------------------------------|------------------------------------------------------------------------------|------------------|
| [ ]  | [#233](https://github.com/marmotz/graphene-term/issues/233) | feat(quick-terminal): restore the quick terminal with the session            | #230, #231, #214 |
| [x]  | [#229](https://github.com/marmotz/graphene-term/issues/229) | feat(quick-terminal): quick_terminal configuration and settings section      | —                |
| [x]  | [#230](https://github.com/marmotz/graphene-term/issues/230) | feat(quick-terminal): quick process, toggle command and window placement     | #229             |
| [x]  | [#231](https://github.com/marmotz/graphene-term/issues/231) | feat(quick-terminal): slide animation, hide on focus loss and reset on close | #230             |
| [x]  | [#232](https://github.com/marmotz/graphene-term/issues/232) | feat(quick-terminal): native global hotkey and hidden startup                | #229, #230       |

## Recherche dans l'historique ·  [overview](features/scrollback-search/overview.md)

_tâches créées — voir [technical.md](features/scrollback-search/technical.md)_ — 0/7 tasks done

| Done | Issue                                                       | Title                                                   | Blocked by       |
|------|-------------------------------------------------------------|---------------------------------------------------------|------------------|
| [ ]  | [#200](https://github.com/marmotz/graphene-term/issues/200) | [Recherche] Déplacer le pliage de bloc sur Ctrl+Shift+Z | —                |
| [ ]  | [#201](https://github.com/marmotz/graphene-term/issues/201) | [Recherche] Couleurs de thème des occurrences           | —                |
| [ ]  | [#202](https://github.com/marmotz/graphene-term/issues/202) | [Recherche] Moteur de recherche dans la timeline        | —                |
| [ ]  | [#203](https://github.com/marmotz/graphene-term/issues/203) | [Recherche] Rendre visible une ligne d'un bloc          | —                |
| [ ]  | [#212](https://github.com/marmotz/graphene-term/issues/212) | [Recherche] Barre de recherche du pane                  | #200, #202, #203 |
| [ ]  | [#216](https://github.com/marmotz/graphene-term/issues/216) | [Recherche] Surlignage des occurrences                  | #201, #202, #212 |
| [ ]  | [#218](https://github.com/marmotz/graphene-term/issues/218) | [Recherche] Documentation                               | #212, #216       |

## Bugs

### Quick terminal crashes when closed after changing the settings  ·  [overview](bugs/quick-terminal-crash-after-settings-change/overview.md)

_terminé — voir [technical.md](bugs/quick-terminal-crash-after-settings-change/technical.md)_ — 2/2 tasks done

| Done | Issue                                                       | Title                                                                  | Blocked by |
|------|-------------------------------------------------------------|------------------------------------------------------------------------|------------|
| [x]  | [#267](https://github.com/marmotz/graphene-term/issues/267) | fix(settings): paint the settings tab at the size of the render target | —          |
| [x]  | [#268](https://github.com/marmotz/graphene-term/issues/268) | fix(renderer): log uncaptured GPU errors instead of panicking          | —          |
