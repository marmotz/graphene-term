## Status: unreviewed

# Italian. Same identifiers as en-US.ftl, which documents each message. Translated by AI
# from en-US.ftl and fr.ftl: see i18n/CONTRIBUTING.md to review it.

## Terminal actions

copy = Copia
paste = Incolla
scroll-page-up = Scorri su di una pagina
scroll-page-down = Scorri giù di una pagina
scroll-to-top = Vai all'inizio
scroll-to-bottom = Vai alla fine
font-size-increase = Ingrandisci il carattere
font-size-decrease = Riduci il carattere
font-size-reset = Ripristina la dimensione del carattere
font-size-reset-all = Ripristina la dimensione del carattere in tutti i riquadri

## Command blocks actions

previous-block = Blocco di comando precedente
next-block = Blocco di comando successivo
copy-command = Copia il comando del blocco
copy-output = Copia l'output del blocco
toggle-fold = Comprimi / espandi il blocco
toggle-fold-all = Comprimi / espandi tutti i blocchi
fold-block = Comprimi il blocco
unfold-block = Espandi il blocco
lines-hidden = { $rows ->
    [one] { $rows } riga nascosta
   *[other] { $rows } righe nascoste
}

## Search bar

open-search = Cerca nella cronologia
search-placeholder = Cerca
invalid-regex = Espressione regolare non valida
search-zone-both = Tutto
search-zone-command = Comandi
search-zone-output = Output
search-status-all = Tutti
search-status-success = Riusciti
search-status-failure = Falliti

## Tabs and panes actions

new-tab = Nuova scheda
close-tab = Chiudi la scheda
close-pane = Chiudi il riquadro
next-tab = Scheda successiva
previous-tab = Scheda precedente
go-to-tab = Vai alla scheda { $tab }
split-right = Dividi a destra
split-down = Dividi in basso
focus-pane = { $direction ->
    [left] Vai al riquadro a sinistra
    [right] Vai al riquadro a destra
    [up] Vai al riquadro in alto
   *[down] Vai al riquadro in basso
}
resize-pane = { $direction ->
    [left] Ridimensiona il riquadro verso sinistra
    [right] Ridimensiona il riquadro verso destra
    [up] Ridimensiona il riquadro verso l'alto
   *[down] Ridimensiona il riquadro verso il basso
}
next-pane = Riquadro successivo
previous-pane = Riquadro precedente
toggle-zoom = Ingrandisci / riduci il riquadro
toggle-broadcast-all = Trasmetti l'input a tutti i riquadri / interrompi
zoom-pane = Ingrandisci il riquadro
unzoom-pane = Riduci il riquadro
split-vertical = Dividi verticalmente
split-horizontal = Dividi orizzontalmente
equalise-panes = Uniforma i riquadri
detach-pane = Sposta il riquadro in una nuova scheda
rename-tab = Rinomina la scheda
themes = Temi
more-themes = Altro...
command-palette = Tavolozza dei comandi
open-settings = Impostazioni
quit = Esci da Graphene

## Settings tab

settings-section = { $section ->
    [language] Lingua
    [font] Carattere
    [window] Finestra
    [scrollback] Cronologia
    [cursor] Cursore
    [selection] Selezione
    [tabs] Schede
    [theme] Tema
    [keyboard] Tastiera
    [keyboard-shortcuts] Scorciatoie da tastiera
    [notifications] Notifiche
    [quick-terminal] Quick terminal
   *[updates] Aggiornamenti
}
settings-field = { $field ->
    [language] Lingua
    [font-family] Famiglia
    [font-size] Dimensione
    [font-ligatures] Legature
    [font-features] Funzionalità OpenType
    [startup-mode] Modalità di avvio
    [window-width] Larghezza
    [window-height] Altezza
    [padding] Margine interno
    [background-opacity] Opacità dello sfondo
    [window-progress] Mostra l'avanzamento dei programmi
    [restore-session] Ripristina la sessione all'avvio
    [scrollback-lines] Righe conservate
    [cursor-shape] Forma
    [cursor-blinking] Lampeggio
    [keyboard-ime] Metodo di input (IME)
    [word-separators] Separatori di parole
    [tab-position] Posizione
    [theme-follow] Segui l'aspetto del sistema
    [theme-name] Tema
    [theme-light] Tema chiaro
    [theme-dark] Tema scuro
    [updates-check] Cerca aggiornamenti
    [updates-interval] Intervallo di ricerca
    [notifications-program] Notifiche dei programmi
    [notifications-command-finished] Avvisa quando termina un comando lungo
    [notifications-command-threshold] Soglia di un comando lungo (secondi)
    [notifications-bell] Avvisa al campanello del terminale
    [quick-terminal-enabled] Attiva il quick terminal
    [quick-terminal-hotkey] Scorciatoia globale
    [quick-terminal-edge] Bordo dello schermo
    [quick-terminal-size] Dimensione (% dello schermo)
    [quick-terminal-screen] Schermo
    [quick-terminal-animation] Durata dello scorrimento (ms)
    [quick-terminal-hide-on-focus-loss] Nascondi alla perdita del focus
   *[quick-terminal-autostart] Avvia con la sessione
}
system-monospace = Monospazio di sistema
active-screen = Schermo sotto il puntatore del mouse
primary-screen = Schermo principale
unknown-keys = Chiavi sconosciute
key-at-line = { $key } (riga { $line })
fix-file-by-hand = Il file non è leggibile: correggilo a mano. Fino ad allora le impostazioni restano disattivate.
quick-terminal-unavailable-on-wayland = Il quick terminal non è disponibile su Wayland
quick-terminal-disabled = Attivalo per modificare le impostazioni qui sotto. Ha effetto al prossimo avvio di Graphene.
cannot-save = Impossibile salvare la modifica: { $error }
not-a-number = Inserisci un numero
not-a-whole-number = Inserisci un numero intero
at-least = Deve valere almeno { $min }
at-most = Deve valere al massimo { $max }

## Keyboard shortcuts

add-shortcut = Aggiungi
add-shortcut-hint = Aggiungi una scorciatoia
change-shortcut = Modifica
remove-shortcut = Rimuovi questa scorciatoia
no-shortcut = Nessuna scorciatoia
press-shortcut = Premi una scorciatoia · Esc per annullare
already-used-by = { $shortcut }: già usata da { $action }
replace = Sostituisci
filter-actions = Filtra le azioni
no-action-matches = Nessuna azione corrisponde
search-shortcut = Cerca una scorciatoia
no-action-for-shortcut = Nessuna azione usa questa scorciatoia

## Theme editor

theme-editor = Editor dei temi
theme-editor-intro = Modifica i colori dei tuoi temi. I temi integrati non sono modificabili: duplicane uno per ottenere una copia modificabile.
theme-colors-hint = Fai clic su un campione per scegliere un colore, oppure digita o incolla il suo codice #rrggbb. Ogni modifica viene salvata subito nel file del tema.
invalid-hex-color = Inserisci un colore nel formato #rrggbb
open-themes-folder = Apri la cartella dei temi
edited-theme = Tema da modificare
duplicate = Duplica
rename = Rinomina
delete = Elimina
new-theme-name = Nuovo nome
confirm = Conferma
confirm-delete-theme = Eliminare il tema { $name }? Il suo file viene cancellato.
built-in-theme = Questo tema integrato non è modificabile. Fai clic su Duplica per crearne una copia modificabile.
keep-text-color = Mantieni il colore del testo
theme-color-group = { $group ->
    [terminal] Colori del terminale
    [normal] Colori ANSI normali
    [bright] Colori ANSI brillanti
   *[interface] Colori dell'interfaccia
}
theme-color = { $key ->
    [foreground] Testo
    [background] Sfondo
    [cursor] Cursore
    [cursor-text] Testo sotto il cursore
    [selection-background] Sfondo della selezione
    [selection-foreground] Testo selezionato
    [normal-black] Nero normale
    [normal-red] Rosso normale
    [normal-green] Verde normale
    [normal-yellow] Giallo normale
    [normal-blue] Blu normale
    [normal-magenta] Magenta normale
    [normal-cyan] Ciano normale
    [normal-white] Bianco normale
    [bright-black] Nero brillante
    [bright-red] Rosso brillante
    [bright-green] Verde brillante
    [bright-yellow] Giallo brillante
    [bright-blue] Blu brillante
    [bright-magenta] Magenta brillante
    [bright-cyan] Ciano brillante
    [bright-white] Bianco brillante
    [tab-bar-background] Sfondo della barra delle schede
    [tab-active-background] Sfondo della scheda attiva
    [tab-active-text] Testo della scheda attiva
    [tab-inactive-text] Testo delle schede inattive
    [pane-border] Bordo dei riquadri
    [pane-broadcast] Barra del titolo dei riquadri in trasmissione
    [palette-background] Sfondo della tavolozza dei comandi
    [palette-text] Testo della tavolozza dei comandi
    [palette-match] Corrispondenze nella tavolozza dei comandi
    [palette-selection-background] Selezione nella tavolozza dei comandi
    [banner-background] Sfondo del banner
    [banner-text] Testo del banner
    [search-match-background] Corrispondenze della ricerca
   *[search-current-background] Corrispondenza corrente della ricerca
}
theme-name-error = { $error ->
    [empty] Inserisci un nome
    [invalid] Un nome non può contenere / o \ né iniziare con un punto
   *[taken] Esiste già un tema con questo nome
}

## Command palette

palette-placeholder = Digita un comando, una scheda o un riquadro
secret-answer = La risposta alla domanda fondamentale
secret-author = marmotz su GitHub
secret-graphene = Graphene
tab-entry = Scheda { $tab }: { $title }
pane-entry = Scheda { $tab } > riquadro { $pane }: { $title }

## Close confirmation dialog

confirm-close-title = { $target ->
    [pane] Chiudere questo riquadro?
    [tab] Chiudere questa scheda?
   *[window] Chiudere la finestra?
}
programs-running = { $count ->
    [one] { $programs } è ancora in esecuzione
   *[other] { $count } programmi sono ancora in esecuzione: { $programs }
}
close = Chiudi
cancel = Annulla

## Update banner

update-available = Graphene { $tag } è disponibile
open-release = Vedi la versione
ignore-version = Ignora questa versione

## Desktop notifications

command-finished = Comando terminato
command-failed = Comando non riuscito (codice { $code })
bell = Campanello
show-pane = Mostra il riquadro
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
    [code] Programma terminato (codice { $value }). Premi un tasto per chiudere.
    [signal] Programma terminato (segnale { $value }). Premi un tasto per chiudere.
   *[unknown] Programma terminato. Premi un tasto per chiudere.
}

## Configuration errors

config-syntax = sintassi non valida: { $detail }
config-cannot-read = impossibile leggere il file: { $detail }
config-cannot-read-theme = impossibile leggere il tema: { $detail }
config-cannot-write = impossibile scrivere il file: { $detail }
config-no-config-dir = impossibile determinare il percorso del file di configurazione
config-unknown-key = chiave sconosciuta `{ $path }`
config-must-be-table = `{ $path }` deve essere una tabella
config-keybindings-moved = scorciatoie da tastiera spostate in { $file }
config-invalid-value = valore non valido per `{ $path }`: { $reason ->
    [not-a-number] non è un numero
    [not-negative] deve essere un numero positivo o zero
    [at-least] deve valere almeno { $min }
    [greater-than-zero] deve essere maggiore di 0
    [not-empty] non deve essere vuoto
    [invalid-feature] funzionalità non valida "{ $item }", atteso "tag", "+tag", "-tag" o "tag=N" con un tag di 4 caratteri ASCII
    [empty-screen] non deve essere vuoto, atteso "active", "primary" o il nome di uno schermo
    [unknown-modifier] modificatore sconosciuto "{ $modifier }" in "{ $shortcut }"
    [missing-key] tasto mancante in "{ $shortcut }"
    [unknown-key-name] tasto sconosciuto "{ $key }" in "{ $shortcut }"
    [unknown-action] azione sconosciuta "{ $action }"
    [color-not-string] atteso un testo "#rrggbb"
    [not-a-color] `{ $text }` non è un colore "#rrggbb"
   *[detail] { $detail }
}
config-out-of-range = `{ $path }`: { $value } è fuori dall'intervallo da { $min } a { $max }, si usa { $used }
config-above-maximum = `{ $path }`: { $value } supera il massimo, si usa { $max }
config-unknown-interval = `{ $path }`: valore non valido "{ $value }", valori possibili: { $expected }; si usa "auto"
config-unknown-theme = tema sconosciuto `{ $name }`, si usa `{ $fallback }`
config-theme-not-found = tema sconosciuto `{ $name }`
config-invalid-theme-name = nome di tema non valido `{ $name }`
config-theme-exists = esiste già un tema chiamato `{ $name }`
config-hotkey-no-key = la scorciatoia globale "{ $hotkey }" non ha un tasto su questo sistema
config-hotkey-taken = la scorciatoia globale "{ $hotkey }" è già usata da un'altra applicazione
config-hotkey-failed = impossibile registrare la scorciatoia globale: { $detail }
more-config-errors = { $more ->
    [one] { $first } (+{ $more } altro)
   *[other] { $first } (+{ $more } altri)
}

## Settings lists

language-auto = Lingua del sistema
choice-cursor-shape = { $shape ->
   *[block] Blocco
    [beam] Barra
    [underline] Sottolineato
}
choice-side = { $side ->
   *[top] In alto
    [bottom] In basso
    [left] A sinistra
    [right] A destra
}
choice-startup-mode = { $mode ->
   *[default] Dimensione configurata
    [maximized] Massimizzata
    [remember] Dimensione dell'ultima finestra
}
choice-update-interval = { $interval ->
   *[auto] Automatico
    [hours-6] Ogni 6 ore
    [hours-12] Ogni 12 ore
    [hours-24] Ogni 24 ore
    [days-2] Ogni 2 giorni
    [days-3] Ogni 3 giorni
    [week-1] Ogni settimana
}
