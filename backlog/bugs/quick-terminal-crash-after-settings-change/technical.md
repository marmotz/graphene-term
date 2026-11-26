# Quick terminal crashes when closed after changing the settings: technical design

Voir [overview.md](overview.md).

## Diagnostic

### Le panic

```
In a set_scissor_rect command
  Scissor Rect { x: 0, y: 29, w: 2560, h: 1371 } is not contained in the render target (1280, 1400, 1)
```

wgpu 30 traite toute erreur de validation non capturée comme fatale par défaut : aucun
handler n'est installé sur le device créé dans
[src/renderer/mod.rs:469](../../../src/renderer/mod.rs) (`adapter.request_device(...)`),
donc le processus entier panique.

### Deux sources de taille qui divergent

1. **La cible de rendu** suit `Renderer::size`, mis à jour uniquement à l'événement
   `WindowEvent::Resized` ([src/app/application.rs:748](../../../src/app/application.rs)
   → `Renderer::resize`, [src/renderer/mod.rs:746](../../../src/renderer/mod.rs)).
   `Renderer::render_with` prépare les couches avec cette taille
   ([src/renderer/mod.rs:785](../../../src/renderer/mod.rs) : `let screen = self.size;`).
2. **La géométrie de l'interface** lit `self.window.inner_size()` en direct, qui reflète
   déjà la nouvelle taille demandée au serveur X avant que `Resized` n'arrive :
   `chrome_area` ([src/app/geometry.rs:20](../../../src/app/geometry.rs)), et surtout le
   dessin de l'onglet settings
   ([src/app/redraw.rs:262-267](../../../src/app/redraw.rs)) :

   ```rust
   let size = self.window.inner_size();
   ...
   settings.paint(&gpu, encoder, view, (size.width, size.height));
   ```

`Settings::paint` ([src/settings/mod.rs:327-339](../../../src/settings/mod.rs)) en fait le
`ScreenDescriptor::size_in_pixels` d'egui. egui-wgpu 0.37 borne ses scissor rects à
`size_in_pixels`, pas à la vraie cible : avec une fenêtre à 2560 px et une surface encore à
1280 px, le scissor de la zone de contenu (`y = 29`, sous la barre d'onglets) déborde.

Les panes, eux, sont protégés : `scissor()`
([src/renderer/mod.rs:63](../../../src/renderer/mod.rs)) borne chaque rectangle à
`prepared.screen`, c'est-à-dire à la taille de la cible.

### Pourquoi au moment de fermer / rouvrir le quick terminal

Le nouveau pourcentage n'est appliqué qu'au prochain affichage : `show_quick`
([src/app/quick.rs:399-404](../../../src/app/quick.rs)) compare `inner_size()` à la taille
voulue et appelle `request_inner_size`, puis `request_redraw()`. Si ce redraw passe avant
l'événement `Resized`, l'onglet settings (resté ouvert) est dessiné à la nouvelle taille
sur une surface à l'ancienne taille. D'où le caractère intermittent : tout dépend de
l'ordre d'arrivée des événements X11. Le cas observé (cible 1280, egui 2560) correspond au
passage de 50 % à 100 % sur un écran de 2560 px.

### Fenêtres normales

Le chemin de rendu est le même (`redraw.rs` ne distingue pas le quick terminal) : une
fenêtre normale avec l'onglet settings ouvert peut subir la même course pendant un
redimensionnement. Le correctif ci-dessous les couvre sans code dédié.

## Correctif retenu

### 1. Le peintre des settings reçoit la taille réelle de la cible

- `ContentPainter` ([src/renderer/mod.rs:49](../../../src/renderer/mod.rs)) prend en plus
  la taille de la cible :
  `&mut dyn FnMut(&mut wgpu::CommandEncoder, &wgpu::TextureView, (u32, u32))`.
- `Layers::draw` ([src/renderer/mod.rs:262-266](../../../src/renderer/mod.rs)) appelle
  `content(encoder, view, prepared.screen)`. `prepared.screen` vaut `Renderer::size`,
  identique à la surface et à la cible offscreen X11 (toutes deux reconfigurées ensemble
  dans `Renderer::resize`).
- `redraw.rs` ne lit plus `self.window.inner_size()` pour peindre : la closure transmet la
  taille reçue à `Settings::paint`.

Conséquence : egui borne ses scissors à la vraie cible, le panic devient impossible. Au
pire, une frame de l'onglet settings est rognée ou mal placée, et la frame suivante (après
`Resized` + `relayout`) est correcte.

### 2. Les erreurs wgpu non capturées ne sont plus fatales

Après `request_device` ([src/renderer/mod.rs:469](../../../src/renderer/mod.rs)), installer
`device.on_uncaptured_error(Arc::new(|error| log::error!("GPU error: {error}")))`
(API vérifiée dans wgpu 30.0.1 : `Device::on_uncaptured_error(Arc<dyn UncapturedErrorHandler>)`,
`UncapturedErrorHandler: Fn(Error) + Send + Sync`). La frame fautive est perdue,
l'application continue. Pas de notice visible pour l'utilisateur.

Le handler est posé dans une fonction dédiée (par exemple `install_error_handler(&Device)`)
pour être appelée aussi par les tests.

## Alternatives écartées

- **Source unique de taille** : faire passer toute la géométrie (14 appels à
  `inner_size()` dans `src/app`) sur `Renderer::size`. Frames toujours cohérentes, mais
  changement large qui touche aussi le hit-testing souris (`hover.rs`, `overlays.rs`) ;
  écarté au profit du correctif ciblé.
- **Logger et afficher une notice** sur erreur GPU : écarté, une erreur transitoire d'une
  frame n'a pas à être montrée.
- **Robustesse seule** (handler sans corriger la taille) : écarté, la frame serait perdue
  à chaque changement de taille.

## Tests

- Renderer headless (même mécanisme que les tests existants, ignoré sans adaptateur GPU,
  voir [src/renderer/mod.rs:954](../../../src/renderer/mod.rs)) : un `ContentPainter`
  reçoit exactement la taille de la cible dessinée.
- Device de test avec `install_error_handler` : un `set_scissor_rect` hors cible ne panique
  plus (l'erreur est loggée).
- Tests existants de `scissor()` ([src/renderer/mod.rs:1456](../../../src/renderer/mod.rs))
  et de `src/settings` inchangés et verts.

## Découpage en tâches d'implémentation

Les deux tâches sont indépendantes.

1. [#267](https://github.com/marmotz/graphene-term/issues/267) : peindre l'onglet settings
   à la taille de la cible de rendu (correctif §1).
2. [#268](https://github.com/marmotz/graphene-term/issues/268) : logger les erreurs GPU
   non capturées au lieu de paniquer (correctif §2).
