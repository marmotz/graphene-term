## Status: reviewed

# French. Same identifiers as en-US.ftl, which documents each message.

## Terminal actions

copy = Copier
paste = Coller
scroll-page-up = Remonter d'une page
scroll-page-down = Descendre d'une page
scroll-to-top = Aller tout en haut
scroll-to-bottom = Aller tout en bas
font-size-increase = Agrandir la police
font-size-decrease = Réduire la police
font-size-reset = Taille de police par défaut
font-size-reset-all = Taille de police par défaut dans tous les panes

## Command blocks actions

previous-block = Bloc de commande précédent
next-block = Bloc de commande suivant
copy-command = Copier la commande du bloc
copy-output = Copier la sortie du bloc
toggle-fold = Replier / déplier le bloc
toggle-fold-all = Replier / déplier tous les blocs
fold-block = Replier le bloc
unfold-block = Déplier le bloc
lines-hidden = { $rows ->
    [one] { $rows } ligne masquée
   *[other] { $rows } lignes masquées
}

## Search bar

open-search = Rechercher dans l'historique
search-placeholder = Rechercher
invalid-regex = Regex invalide
search-zone-both = Tout
search-zone-command = Commandes
search-zone-output = Sorties
search-status-all = Tous
search-status-success = Succès
search-status-failure = Échecs

## Tabs and panes actions

new-tab = Nouvel onglet
close-tab = Fermer l'onglet
close-pane = Fermer le panneau
next-tab = Onglet suivant
previous-tab = Onglet précédent
go-to-tab = Aller à l'onglet { $tab }
split-right = Diviser à droite
split-down = Diviser en bas
focus-pane = { $direction ->
    [left] Aller au panneau de gauche
    [right] Aller au panneau de droite
    [up] Aller au panneau du haut
   *[down] Aller au panneau du bas
}
resize-pane = { $direction ->
    [left] Redimensionner le panneau vers la gauche
    [right] Redimensionner le panneau vers la droite
    [up] Redimensionner le panneau vers le haut
   *[down] Redimensionner le panneau vers le bas
}
next-pane = Panneau suivant
previous-pane = Panneau précédent
toggle-zoom = Agrandir / réduire le panneau
toggle-broadcast-all = Diffuser la saisie à tous les panneaux / arrêter
zoom-pane = Agrandir le panneau
unzoom-pane = Réduire le panneau
split-vertical = Diviser verticalement
split-horizontal = Diviser horizontalement
equalise-panes = Égaliser les panneaux
detach-pane = Déplacer le panneau dans un nouvel onglet
rename-tab = Renommer l'onglet
themes = Thèmes
more-themes = Plus...
command-palette = Palette de commandes
open-settings = Paramètres
quit = Quitter Graphene

## Settings tab

settings-section = { $section ->
    [language] Langue
    [font] Police
    [window] Fenêtre
    [scrollback] Historique
    [cursor] Curseur
    [selection] Sélection
    [tabs] Onglets
    [theme] Thème
    [keyboard-shortcuts] Raccourcis clavier
    [notifications] Notifications
    [quick-terminal] Quick terminal
   *[updates] Mises à jour
}
settings-field = { $field ->
    [language] Langue
    [font-family] Famille
    [font-size] Taille
    [font-ligatures] Ligatures
    [font-features] Fonctionnalités OpenType
    [startup-mode] Mode au démarrage
    [window-width] Largeur
    [window-height] Hauteur
    [padding] Marge intérieure
    [background-opacity] Opacité du fond
    [window-progress] Afficher la progression des programmes
    [restore-session] Restaurer la session au lancement
    [scrollback-lines] Lignes conservées
    [cursor-shape] Forme
    [cursor-blinking] Clignotement
    [word-separators] Séparateurs de mots
    [tab-position] Emplacement
    [theme-follow] Suivre l'apparence du système
    [theme-name] Thème
    [theme-light] Thème clair
    [theme-dark] Thème sombre
    [updates-check] Rechercher les mises à jour
    [updates-interval] Intervalle de recherche
    [notifications-program] Notifications des programmes
    [notifications-command-finished] Signaler la fin d'une commande longue
    [notifications-command-threshold] Seuil d'une commande longue (secondes)
    [notifications-bell] Signaler la sonnerie du terminal
    [quick-terminal-enabled] Activer le quick terminal
    [quick-terminal-hotkey] Raccourci global
    [quick-terminal-edge] Bord de l'écran
    [quick-terminal-size] Taille (% de l'écran)
    [quick-terminal-screen] Écran
    [quick-terminal-animation] Durée du glissement (ms)
    [quick-terminal-hide-on-focus-loss] Masquer à la perte du focus
   *[quick-terminal-autostart] Démarrer avec la session
}
system-monospace = Monospace du système
active-screen = Écran sous le pointeur de la souris
primary-screen = Écran principal
unknown-keys = Clés inconnues
at-line = ligne { $line }
fix-file-by-hand = Le fichier est illisible : corrigez-le à la main. Les paramètres restent désactivés d'ici là.
quick-terminal-unavailable-on-wayland = Le quick terminal n'est pas disponible sous Wayland
quick-terminal-disabled = Activez-le pour modifier les réglages ci-dessous. Prend effet au prochain démarrage de Graphene.
cannot-save = Impossible d'enregistrer la modification : { $error }
not-a-number = Saisissez un nombre
not-a-whole-number = Saisissez un nombre entier
at-least = Doit valoir au moins { $min }
at-most = Doit valoir au plus { $max }

## Keyboard shortcuts

add-shortcut = Ajouter
add-shortcut-hint = Ajouter un raccourci
change-shortcut = Modifier
remove-shortcut = Supprimer ce raccourci
no-shortcut = Aucun raccourci
press-shortcut = Appuyez sur un raccourci · Échap pour annuler
already-used-by = Déjà utilisé par { $action }
replace = Remplacer
filter-actions = Filtrer les actions
no-action-matches = Aucune action ne correspond
search-shortcut = Rechercher un raccourci
no-action-for-shortcut = Aucune action n'utilise ce raccourci

## Theme editor

theme-editor = Éditeur de thème
theme-editor-intro = Modifiez les couleurs de vos propres thèmes. Les thèmes intégrés ne sont pas modifiables : dupliquez-en un pour obtenir une copie modifiable.
theme-colors-hint = Cliquez sur une pastille pour choisir une couleur, ou saisissez ou collez son code #rrggbb. Chaque changement est enregistré aussitôt dans le fichier du thème.
invalid-hex-color = Saisissez une couleur au format #rrggbb
open-themes-folder = Ouvrir le dossier des thèmes
edited-theme = Thème à modifier
duplicate = Dupliquer
rename = Renommer
delete = Supprimer
new-theme-name = Nouveau nom
confirm = Valider
confirm-delete-theme = Supprimer le thème { $name } ? Son fichier est effacé.
built-in-theme = Ce thème intégré n'est pas modifiable. Cliquez sur Dupliquer pour en créer une copie modifiable.
keep-text-color = Garder la couleur du texte
theme-color-group = { $group ->
    [terminal] Couleurs du terminal
    [normal] Couleurs ANSI normales
    [bright] Couleurs ANSI vives
   *[interface] Couleurs de l'interface
}
theme-color = { $key ->
    [foreground] Texte
    [background] Fond
    [cursor] Curseur
    [cursor-text] Texte sous le curseur
    [selection-background] Fond de la sélection
    [selection-foreground] Texte sélectionné
    [normal-black] Noir normal
    [normal-red] Rouge normal
    [normal-green] Vert normal
    [normal-yellow] Jaune normal
    [normal-blue] Bleu normal
    [normal-magenta] Magenta normal
    [normal-cyan] Cyan normal
    [normal-white] Blanc normal
    [bright-black] Noir vif
    [bright-red] Rouge vif
    [bright-green] Vert vif
    [bright-yellow] Jaune vif
    [bright-blue] Bleu vif
    [bright-magenta] Magenta vif
    [bright-cyan] Cyan vif
    [bright-white] Blanc vif
    [tab-bar-background] Fond de la barre d'onglets
    [tab-active-background] Fond de l'onglet actif
    [tab-active-text] Texte de l'onglet actif
    [tab-inactive-text] Texte des onglets inactifs
    [pane-border] Bordure des panneaux
    [pane-broadcast] Barre de titre des panneaux diffusés
    [palette-background] Fond de la palette de commandes
    [palette-text] Texte de la palette de commandes
    [palette-match] Lettres trouvées dans la palette
    [palette-selection-background] Sélection dans la palette
    [banner-background] Fond du bandeau
    [banner-text] Texte du bandeau
    [search-match-background] Occurrences de la recherche
   *[search-current-background] Occurrence courante de la recherche
}
theme-name-error = { $error ->
    [empty] Saisissez un nom
    [invalid] Un nom ne peut pas contenir / ou \ ni commencer par un point
   *[taken] Un thème porte déjà ce nom
}

## Command palette

palette-placeholder = Tapez une commande, un onglet ou un panneau
secret-answer = La grande question sur la vie, l'univers et le reste
secret-author = marmotz sur GitHub
secret-graphene = Graphene
tab-entry = Onglet { $tab } : { $title }
pane-entry = Onglet { $tab } > panneau { $pane } : { $title }

## Close confirmation dialog

confirm-close-title = { $target ->
    [pane] Fermer ce panneau ?
    [tab] Fermer cet onglet ?
   *[window] Fermer la fenêtre ?
}
programs-running = { $count ->
    [one] { $programs } est toujours en cours d'exécution
   *[other] { $count } programmes sont toujours en cours d'exécution : { $programs }
}
close = Fermer
cancel = Annuler

## Update banner

update-available = Graphene { $tag } est disponible
open-release = Voir la version
ignore-version = Ignorer cette version

## Desktop notifications

command-finished = Commande terminée
command-failed = Commande échouée (code { $code })
bell = Sonnerie
show-pane = Afficher le panneau
duration-seconds = { $s } s
duration-minutes = { $s ->
    [0] { $min } min
   *[other] { $min } min { $s } s
}
duration-hours = { $min ->
    [0] { $h } h
   *[other] { $h } h { $min } min
}

## Program run by -E

program-exited = { $exit ->
    [code] Programme terminé (code { $value }). Appuyez sur une touche pour fermer.
    [signal] Programme terminé (signal { $value }). Appuyez sur une touche pour fermer.
   *[unknown] Programme terminé. Appuyez sur une touche pour fermer.
}

## Configuration errors

config-syntax = syntaxe invalide : { $detail }
config-cannot-read = impossible de lire le fichier : { $detail }
config-cannot-read-theme = impossible de lire le thème : { $detail }
config-cannot-write = impossible d'écrire le fichier : { $detail }
config-no-config-dir = impossible de trouver le chemin du fichier de configuration
config-unknown-key = clé inconnue `{ $path }`
config-must-be-table = `{ $path }` doit être une table
config-keybindings-moved = raccourcis clavier déplacés dans { $file }
config-invalid-value = valeur invalide pour `{ $path }` : { $reason ->
    [not-a-number] ce n'est pas un nombre
    [not-negative] doit être un nombre positif ou nul
    [at-least] doit valoir au moins { $min }
    [greater-than-zero] doit être supérieur à 0
    [not-empty] ne doit pas être vide
    [invalid-feature] fonctionnalité invalide "{ $item }", attendu "tag", "+tag", "-tag" ou "tag=N" avec un tag de 4 caractères ASCII
    [empty-screen] ne doit pas être vide, attendu "active", "primary" ou un nom d'écran
    [unknown-modifier] modificateur inconnu "{ $modifier }" dans "{ $shortcut }"
    [missing-key] touche manquante dans "{ $shortcut }"
    [unknown-key-name] touche inconnue "{ $key }" dans "{ $shortcut }"
    [unknown-action] action inconnue "{ $action }"
    [color-not-string] texte "#rrggbb" attendu
    [not-a-color] `{ $text }` n'est pas une couleur "#rrggbb"
   *[detail] { $detail }
}
config-out-of-range = `{ $path }` : { $value } est hors de l'intervalle { $min } à { $max }, { $used } est utilisé
config-above-maximum = `{ $path }` : { $value } dépasse le maximum, { $max } est utilisé
config-unknown-interval = `{ $path }` : valeur invalide "{ $value }", valeurs possibles : { $expected } ; "auto" est utilisé
config-unknown-theme = thème inconnu `{ $name }`, `{ $fallback }` est utilisé
config-theme-not-found = thème inconnu `{ $name }`
config-invalid-theme-name = nom de thème invalide `{ $name }`
config-theme-exists = un thème nommé `{ $name }` existe déjà
config-hotkey-no-key = le raccourci global "{ $hotkey }" n'a pas de touche sur ce système
config-hotkey-taken = le raccourci global "{ $hotkey }" est déjà utilisé par une autre application
config-hotkey-failed = impossible d'enregistrer le raccourci global : { $detail }
more-config-errors = { $more ->
    [one] { $first } (+{ $more } autre)
   *[other] { $first } (+{ $more } autres)
}
