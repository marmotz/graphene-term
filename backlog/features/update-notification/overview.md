# Update notification

**Statut** : terminé, voir [technical.md](technical.md)

## Contexte

Des pré-releases (paquets deb, rpm et AppImage) sont désormais publiées automatiquement
sur GitHub depuis la CI. Les utilisateurs n'ont aucun moyen de savoir qu'une version plus
récente que celle installée est disponible.

## Objectif

Alerter l'utilisateur, depuis le terminal, qu'une nouvelle version est disponible, avec un
lien vers la release GitHub correspondante pour la télécharger.

## Décisions actées

- **Canal suivi selon la version installée** : une version de développement (pré-release
  `vX.Y.Z-dev.N`) est alertée des nouvelles pré-releases ; une version stable n'est alertée
  que des nouvelles releases stables.
- **Moment de la vérification** : au démarrage, puis périodiquement pendant la session.
- **Intervalle** configurable parmi : 6 h, 12 h, 24 h, 2 jours, 3 jours, 1 semaine. Par
  défaut : 6 h pour le canal dev, 24 h pour le canal stable.
- **Activée par défaut** ; une option de configuration désactive complètement la
  vérification (aucun appel réseau).
- **Affichage** : bannière non bloquante dans la fenêtre, avec la version disponible et un
  lien vers la page GitHub de la release (notes et tous les paquets).
- **Actions sur la bannière** :
  - fermer pour la session : la bannière revient au prochain lancement ;
  - ignorer cette version : plus d'alerte pour cette version, l'alerte revient à la
    version suivante.
- **Identité d'un build** : chaque binaire embarque son horodatage de compilation, qui
  devient le numéro des pré-releases (`vX.Y.Z-dev.<AAAAMMJJHHMMSS>`). Un build local suit
  les mêmes règles.
- **Cache** : le résultat d'une vérification est réutilisé pendant un intervalle, y
  compris au démarrage et entre plusieurs fenêtres.

## Dépend de

- [config-themes](../config-themes/overview.md) : fichier de configuration et bannière.
- [images-links](../images-links/overview.md) : ouverture des liens (partagée).
