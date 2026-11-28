## Status: unreviewed

# German. Same identifiers as en-US.ftl, which documents each message. Translated by AI
# from en-US.ftl and fr.ftl: see i18n/CONTRIBUTING.md to review it.

## Terminal actions

copy = Kopieren
paste = Einfügen
scroll-page-up = Eine Seite nach oben blättern
scroll-page-down = Eine Seite nach unten blättern
scroll-to-top = Zum Anfang springen
scroll-to-bottom = Zum Ende springen
font-size-increase = Schrift vergrößern
font-size-decrease = Schrift verkleinern
font-size-reset = Schriftgröße zurücksetzen
font-size-reset-all = Schriftgröße in allen Bereichen zurücksetzen

## Command blocks actions

previous-block = Vorheriger Befehlsblock
next-block = Nächster Befehlsblock
copy-command = Befehl des Blocks kopieren
copy-output = Ausgabe des Blocks kopieren
toggle-fold = Block ein- / ausklappen
toggle-fold-all = Alle Blöcke ein- / ausklappen
fold-block = Block einklappen
unfold-block = Block ausklappen
lines-hidden = { $rows ->
    [one] { $rows } Zeile ausgeblendet
   *[other] { $rows } Zeilen ausgeblendet
}

## Search bar

open-search = Im Verlauf suchen
search-placeholder = Suchen
invalid-regex = Ungültiger regulärer Ausdruck
search-zone-both = Alles
search-zone-command = Befehle
search-zone-output = Ausgaben
search-status-all = Alle
search-status-success = Erfolge
search-status-failure = Fehlschläge

## Tabs and panes actions

new-tab = Neuer Tab
close-tab = Tab schließen
close-pane = Bereich schließen
next-tab = Nächster Tab
previous-tab = Vorheriger Tab
go-to-tab = Zu Tab { $tab } wechseln
split-right = Nach rechts teilen
split-down = Nach unten teilen
focus-pane = { $direction ->
    [left] Zum linken Bereich wechseln
    [right] Zum rechten Bereich wechseln
    [up] Zum oberen Bereich wechseln
   *[down] Zum unteren Bereich wechseln
}
resize-pane = { $direction ->
    [left] Größe des Bereichs nach links ändern
    [right] Größe des Bereichs nach rechts ändern
    [up] Größe des Bereichs nach oben ändern
   *[down] Größe des Bereichs nach unten ändern
}
next-pane = Nächster Bereich
previous-pane = Vorheriger Bereich
toggle-zoom = Bereich maximieren / wiederherstellen
toggle-broadcast-all = Eingabe an alle Bereiche senden / beenden
zoom-pane = Bereich maximieren
unzoom-pane = Bereich wiederherstellen
split-vertical = Vertikal teilen
split-horizontal = Horizontal teilen
equalise-panes = Bereiche angleichen
detach-pane = Bereich in einen neuen Tab verschieben
rename-tab = Tab umbenennen
themes = Designs
more-themes = Mehr...
command-palette = Befehlspalette
open-settings = Einstellungen
quit = Graphene beenden

## Settings tab

settings-section = { $section ->
    [language] Sprache
    [font] Schrift
    [window] Fenster
    [scrollback] Verlauf
    [cursor] Cursor
    [selection] Auswahl
    [tabs] Tabs
    [theme] Design
    [keyboard] Tastatur
    [keyboard-shortcuts] Tastenkürzel
    [notifications] Benachrichtigungen
    [quick-terminal] Quick terminal
   *[updates] Aktualisierungen
}
settings-field = { $field ->
    [language] Sprache
    [font-family] Familie
    [font-size] Größe
    [font-ligatures] Ligaturen
    [font-features] OpenType-Funktionen
    [startup-mode] Startmodus
    [window-width] Breite
    [window-height] Höhe
    [padding] Innenabstand
    [background-opacity] Deckkraft des Hintergrunds
    [window-progress] Fortschritt der Programme anzeigen
    [restore-session] Sitzung beim Start wiederherstellen
    [scrollback-lines] Behaltene Zeilen
    [cursor-shape] Form
    [cursor-blinking] Blinken
    [keyboard-ime] Eingabemethode (IME)
    [word-separators] Worttrennzeichen
    [tab-position] Position
    [theme-follow] Dem Erscheinungsbild des Systems folgen
    [theme-name] Design
    [theme-light] Helles Design
    [theme-dark] Dunkles Design
    [updates-check] Nach Aktualisierungen suchen
    [updates-interval] Suchintervall
    [notifications-program] Benachrichtigungen von Programmen
    [notifications-command-finished] Benachrichtigen, wenn ein langer Befehl endet
    [notifications-command-threshold] Schwelle für lange Befehle (Sekunden)
    [notifications-bell] Bei der Terminalglocke benachrichtigen
    [quick-terminal-enabled] Quick terminal aktivieren
    [quick-terminal-hotkey] Globales Tastenkürzel
    [quick-terminal-edge] Bildschirmrand
    [quick-terminal-size] Größe (% des Bildschirms)
    [quick-terminal-screen] Bildschirm
    [quick-terminal-animation] Dauer des Einblendens (ms)
    [quick-terminal-hide-on-focus-loss] Bei Fokusverlust ausblenden
   *[quick-terminal-autostart] Mit der Sitzung starten
}
system-monospace = Monospace-Schrift des Systems
active-screen = Bildschirm unter dem Mauszeiger
primary-screen = Hauptbildschirm
unknown-keys = Unbekannte Schlüssel
key-at-line = { $key } (Zeile { $line })
fix-file-by-hand = Die Datei kann nicht gelesen werden: Korrigieren Sie sie von Hand. Bis dahin bleiben die Einstellungen deaktiviert.
quick-terminal-unavailable-on-wayland = Das Quick terminal ist unter Wayland nicht verfügbar
quick-terminal-disabled = Aktivieren Sie es, um die Einstellungen unten zu ändern. Wirkt beim nächsten Start von Graphene.
cannot-save = Die Änderung kann nicht gespeichert werden: { $error }
not-a-number = Geben Sie eine Zahl ein
not-a-whole-number = Geben Sie eine ganze Zahl ein
at-least = Muss mindestens { $min } sein
at-most = Darf höchstens { $max } sein

## Keyboard shortcuts

add-shortcut = Hinzufügen
add-shortcut-hint = Tastenkürzel hinzufügen
change-shortcut = Ändern
remove-shortcut = Dieses Tastenkürzel entfernen
no-shortcut = Kein Tastenkürzel
press-shortcut = Drücken Sie ein Tastenkürzel · Esc zum Abbrechen
already-used-by = { $shortcut }: Bereits verwendet von { $action }
replace = Ersetzen
filter-actions = Aktionen filtern
no-action-matches = Keine Aktion passt
search-shortcut = Tastenkürzel suchen
no-action-for-shortcut = Keine Aktion verwendet dieses Tastenkürzel

## Theme editor

theme-editor = Design-Editor
theme-editor-intro = Ändern Sie die Farben Ihrer eigenen Designs. Integrierte Designs können nicht geändert werden: Duplizieren Sie eines, um eine bearbeitbare Kopie zu erhalten.
theme-colors-hint = Klicken Sie auf ein Farbfeld, um eine Farbe zu wählen, oder geben Sie ihren #rrggbb-Code ein oder fügen Sie ihn ein. Jede Änderung wird sofort in der Design-Datei gespeichert.
invalid-hex-color = Geben Sie eine Farbe im Format #rrggbb ein
open-themes-folder = Design-Ordner öffnen
edited-theme = Zu bearbeitendes Design
duplicate = Duplizieren
rename = Umbenennen
delete = Löschen
new-theme-name = Neuer Name
confirm = Bestätigen
confirm-delete-theme = Das Design { $name } löschen? Seine Datei wird entfernt.
built-in-theme = Dieses integrierte Design kann nicht geändert werden. Klicken Sie auf Duplizieren, um eine bearbeitbare Kopie zu erstellen.
keep-text-color = Textfarbe beibehalten
theme-color-group = { $group ->
    [terminal] Terminalfarben
    [normal] Normale ANSI-Farben
    [bright] Helle ANSI-Farben
   *[interface] Oberflächenfarben
}
theme-color = { $key ->
    [foreground] Text
    [background] Hintergrund
    [cursor] Cursor
    [cursor-text] Text unter dem Cursor
    [selection-background] Hintergrund der Auswahl
    [selection-foreground] Ausgewählter Text
    [normal-black] Normales Schwarz
    [normal-red] Normales Rot
    [normal-green] Normales Grün
    [normal-yellow] Normales Gelb
    [normal-blue] Normales Blau
    [normal-magenta] Normales Magenta
    [normal-cyan] Normales Cyan
    [normal-white] Normales Weiß
    [bright-black] Helles Schwarz
    [bright-red] Helles Rot
    [bright-green] Helles Grün
    [bright-yellow] Helles Gelb
    [bright-blue] Helles Blau
    [bright-magenta] Helles Magenta
    [bright-cyan] Helles Cyan
    [bright-white] Helles Weiß
    [tab-bar-background] Hintergrund der Tableiste
    [tab-active-background] Hintergrund des aktiven Tabs
    [tab-active-text] Text des aktiven Tabs
    [tab-inactive-text] Text der inaktiven Tabs
    [pane-border] Rahmen der Bereiche
    [pane-broadcast] Titelleiste der Bereiche mit gesendeter Eingabe
    [palette-background] Hintergrund der Befehlspalette
    [palette-text] Text der Befehlspalette
    [palette-match] Treffer in der Befehlspalette
    [palette-selection-background] Auswahl in der Befehlspalette
    [banner-background] Hintergrund des Banners
    [banner-text] Text des Banners
    [search-match-background] Suchtreffer
   *[search-current-background] Aktueller Suchtreffer
}
theme-name-error = { $error ->
    [empty] Geben Sie einen Namen ein
    [invalid] Ein Name darf weder / noch \ enthalten noch mit einem Punkt beginnen
   *[taken] Ein Design mit diesem Namen existiert bereits
}

## Command palette

palette-placeholder = Befehl, Tab oder Bereich eingeben
secret-answer = Die Antwort auf die endgültige Frage
secret-author = marmotz auf GitHub
secret-graphene = Graphene
tab-entry = Tab { $tab }: { $title }
pane-entry = Tab { $tab } > Bereich { $pane }: { $title }

## Close confirmation dialog

confirm-close-title = { $target ->
    [pane] Diesen Bereich schließen?
    [tab] Diesen Tab schließen?
   *[window] Das Fenster schließen?
}
programs-running = { $count ->
    [one] { $programs } läuft noch
   *[other] { $count } Programme laufen noch: { $programs }
}
close = Schließen
cancel = Abbrechen

## Update banner

update-available = Graphene { $tag } ist verfügbar
open-release = Version ansehen
ignore-version = Diese Version ignorieren

## Desktop notifications

command-finished = Befehl beendet
command-failed = Befehl fehlgeschlagen (Code { $code })
bell = Glocke
show-pane = Bereich anzeigen
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
    [code] Programm beendet (Code { $value }). Drücken Sie eine Taste zum Schließen.
    [signal] Programm beendet (Signal { $value }). Drücken Sie eine Taste zum Schließen.
   *[unknown] Programm beendet. Drücken Sie eine Taste zum Schließen.
}

## Configuration errors

config-syntax = ungültige Syntax: { $detail }
config-cannot-read = Datei kann nicht gelesen werden: { $detail }
config-cannot-read-theme = Design kann nicht gelesen werden: { $detail }
config-cannot-write = Datei kann nicht geschrieben werden: { $detail }
config-no-config-dir = Pfad der Konfigurationsdatei kann nicht ermittelt werden
config-unknown-key = unbekannter Schlüssel `{ $path }`
config-must-be-table = `{ $path }` muss eine Tabelle sein
config-keybindings-moved = Tastenkürzel nach { $file } verschoben
config-invalid-value = ungültiger Wert für `{ $path }`: { $reason ->
    [not-a-number] keine Zahl
    [not-negative] muss eine nicht negative Zahl sein
    [at-least] muss mindestens { $min } sein
    [greater-than-zero] muss größer als 0 sein
    [not-empty] darf nicht leer sein
    [invalid-feature] ungültige Funktion "{ $item }", erwartet "tag", "+tag", "-tag" oder "tag=N" mit einem Tag aus 4 ASCII-Zeichen
    [empty-screen] darf nicht leer sein, erwartet "active", "primary" oder einen Bildschirmnamen
    [unknown-modifier] unbekannte Modifikatortaste "{ $modifier }" in "{ $shortcut }"
    [missing-key] fehlende Taste in "{ $shortcut }"
    [unknown-key-name] unbekannte Taste "{ $key }" in "{ $shortcut }"
    [unknown-action] unbekannte Aktion "{ $action }"
    [color-not-string] Text "#rrggbb" erwartet
    [not-a-color] `{ $text }` ist keine Farbe "#rrggbb"
   *[detail] { $detail }
}
config-out-of-range = `{ $path }`: { $value } liegt außerhalb von { $min } bis { $max }, { $used } wird verwendet
config-above-maximum = `{ $path }`: { $value } überschreitet das Maximum, { $max } wird verwendet
config-unknown-interval = `{ $path }`: ungültiger Wert "{ $value }", mögliche Werte: { $expected }; "auto" wird verwendet
config-unknown-theme = unbekanntes Design `{ $name }`, `{ $fallback }` wird verwendet
config-theme-not-found = unbekanntes Design `{ $name }`
config-invalid-theme-name = ungültiger Designname `{ $name }`
config-theme-exists = ein Design namens `{ $name }` existiert bereits
config-hotkey-no-key = das globale Tastenkürzel "{ $hotkey }" hat auf diesem System keine Taste
config-hotkey-taken = das globale Tastenkürzel "{ $hotkey }" wird bereits von einer anderen Anwendung verwendet
config-hotkey-failed = globales Tastenkürzel kann nicht registriert werden: { $detail }
more-config-errors = { $more ->
    [one] { $first } (+{ $more } weiterer)
   *[other] { $first } (+{ $more } weitere)
}

## Settings lists

language-auto = Systemsprache
choice-cursor-shape = { $shape ->
   *[block] Block
    [beam] Balken
    [underline] Unterstrich
}
choice-side = { $side ->
   *[top] Oben
    [bottom] Unten
    [left] Links
    [right] Rechts
}
choice-startup-mode = { $mode ->
   *[default] Konfigurierte Größe
    [maximized] Maximiert
    [remember] Größe des letzten Fensters
}
choice-update-interval = { $interval ->
   *[auto] Automatisch
    [hours-6] Alle 6 Stunden
    [hours-12] Alle 12 Stunden
    [hours-24] Alle 24 Stunden
    [days-2] Alle 2 Tage
    [days-3] Alle 3 Tage
    [week-1] Jede Woche
}
