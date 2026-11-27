## Status: unreviewed

# Spanish. Same identifiers as en-US.ftl, which documents each message. Translated by AI
# from en-US.ftl and fr.ftl: see i18n/CONTRIBUTING.md to review it.

## Terminal actions

copy = Copiar
paste = Pegar
scroll-page-up = Subir una página
scroll-page-down = Bajar una página
scroll-to-top = Ir al principio
scroll-to-bottom = Ir al final
font-size-increase = Aumentar el tamaño de fuente
font-size-decrease = Reducir el tamaño de fuente
font-size-reset = Restablecer el tamaño de fuente
font-size-reset-all = Restablecer el tamaño de fuente en todos los paneles

## Command blocks actions

previous-block = Bloque de comando anterior
next-block = Bloque de comando siguiente
copy-command = Copiar el comando del bloque
copy-output = Copiar la salida del bloque
toggle-fold = Plegar / desplegar el bloque
toggle-fold-all = Plegar / desplegar todos los bloques
fold-block = Plegar el bloque
unfold-block = Desplegar el bloque
lines-hidden = { $rows ->
    [one] { $rows } línea oculta
   *[other] { $rows } líneas ocultas
}

## Search bar

open-search = Buscar en el historial
search-placeholder = Buscar
invalid-regex = Expresión regular no válida
search-zone-both = Todo
search-zone-command = Comandos
search-zone-output = Salidas
search-status-all = Todos
search-status-success = Éxitos
search-status-failure = Fallos

## Tabs and panes actions

new-tab = Nueva pestaña
close-tab = Cerrar la pestaña
close-pane = Cerrar el panel
next-tab = Pestaña siguiente
previous-tab = Pestaña anterior
go-to-tab = Ir a la pestaña { $tab }
split-right = Dividir a la derecha
split-down = Dividir hacia abajo
focus-pane = { $direction ->
    [left] Ir al panel de la izquierda
    [right] Ir al panel de la derecha
    [up] Ir al panel de arriba
   *[down] Ir al panel de abajo
}
resize-pane = { $direction ->
    [left] Redimensionar el panel hacia la izquierda
    [right] Redimensionar el panel hacia la derecha
    [up] Redimensionar el panel hacia arriba
   *[down] Redimensionar el panel hacia abajo
}
next-pane = Panel siguiente
previous-pane = Panel anterior
toggle-zoom = Ampliar / reducir el panel
toggle-broadcast-all = Difundir la entrada a todos los paneles / detener
zoom-pane = Ampliar el panel
unzoom-pane = Reducir el panel
split-vertical = Dividir verticalmente
split-horizontal = Dividir horizontalmente
equalise-panes = Igualar los paneles
detach-pane = Mover el panel a una pestaña nueva
rename-tab = Renombrar la pestaña
themes = Temas
more-themes = Más...
command-palette = Paleta de comandos
open-settings = Ajustes
quit = Salir de Graphene

## Settings tab

settings-section = { $section ->
    [language] Idioma
    [font] Fuente
    [window] Ventana
    [scrollback] Historial
    [cursor] Cursor
    [selection] Selección
    [tabs] Pestañas
    [theme] Tema
    [keyboard-shortcuts] Atajos de teclado
    [notifications] Notificaciones
    [quick-terminal] Quick terminal
   *[updates] Actualizaciones
}
settings-field = { $field ->
    [language] Idioma
    [font-family] Familia
    [font-size] Tamaño
    [font-ligatures] Ligaduras
    [font-features] Funciones OpenType
    [startup-mode] Modo de inicio
    [window-width] Ancho
    [window-height] Alto
    [padding] Margen interior
    [background-opacity] Opacidad del fondo
    [window-progress] Mostrar el progreso de los programas
    [restore-session] Restaurar la sesión al iniciar
    [scrollback-lines] Líneas conservadas
    [cursor-shape] Forma
    [cursor-blinking] Parpadeo
    [word-separators] Separadores de palabras
    [tab-position] Posición
    [theme-follow] Seguir la apariencia del sistema
    [theme-name] Tema
    [theme-light] Tema claro
    [theme-dark] Tema oscuro
    [updates-check] Buscar actualizaciones
    [updates-interval] Intervalo de búsqueda
    [notifications-program] Notificaciones de los programas
    [notifications-command-finished] Avisar cuando termine un comando largo
    [notifications-command-threshold] Umbral de un comando largo (segundos)
    [notifications-bell] Avisar del timbre del terminal
    [quick-terminal-enabled] Activar el quick terminal
    [quick-terminal-hotkey] Atajo global
    [quick-terminal-edge] Borde de la pantalla
    [quick-terminal-size] Tamaño (% de la pantalla)
    [quick-terminal-screen] Pantalla
    [quick-terminal-animation] Duración del deslizamiento (ms)
    [quick-terminal-hide-on-focus-loss] Ocultar al perder el foco
   *[quick-terminal-autostart] Iniciar con la sesión
}
system-monospace = Monoespaciada del sistema
active-screen = Pantalla bajo el puntero del ratón
primary-screen = Pantalla principal
unknown-keys = Claves desconocidas
key-at-line = { $key } (línea { $line })
fix-file-by-hand = El archivo no se puede leer: corríjalo a mano. Los ajustes permanecen desactivados hasta entonces.
quick-terminal-unavailable-on-wayland = El quick terminal no está disponible en Wayland
quick-terminal-disabled = Actívelo para cambiar los ajustes de abajo. Se aplica la próxima vez que se inicie Graphene.
cannot-save = No se puede guardar el cambio: { $error }
not-a-number = Introduzca un número
not-a-whole-number = Introduzca un número entero
at-least = Debe ser al menos { $min }
at-most = Debe ser como máximo { $max }

## Keyboard shortcuts

add-shortcut = Añadir
add-shortcut-hint = Añadir un atajo
change-shortcut = Cambiar
remove-shortcut = Quitar este atajo
no-shortcut = Sin atajo
press-shortcut = Pulse un atajo · Esc para cancelar
already-used-by = { $shortcut }: ya lo usa { $action }
replace = Reemplazar
filter-actions = Filtrar las acciones
no-action-matches = Ninguna acción coincide
search-shortcut = Buscar un atajo
no-action-for-shortcut = Ninguna acción usa este atajo

## Theme editor

theme-editor = Editor de temas
theme-editor-intro = Cambie los colores de sus propios temas. Los temas integrados no se pueden cambiar: duplique uno para obtener una copia editable.
theme-colors-hint = Haga clic en una muestra para elegir un color, o escriba o pegue su código #rrggbb. Cada cambio se guarda al instante en el archivo del tema.
invalid-hex-color = Introduzca un color con el formato #rrggbb
open-themes-folder = Abrir la carpeta de temas
edited-theme = Tema que editar
duplicate = Duplicar
rename = Renombrar
delete = Eliminar
new-theme-name = Nombre nuevo
confirm = Confirmar
confirm-delete-theme = ¿Eliminar el tema { $name }? Su archivo se borra.
built-in-theme = Este tema integrado no se puede cambiar. Haga clic en Duplicar para crear una copia editable.
keep-text-color = Conservar el color del texto
theme-color-group = { $group ->
    [terminal] Colores del terminal
    [normal] Colores ANSI normales
    [bright] Colores ANSI brillantes
   *[interface] Colores de la interfaz
}
theme-color = { $key ->
    [foreground] Texto
    [background] Fondo
    [cursor] Cursor
    [cursor-text] Texto bajo el cursor
    [selection-background] Fondo de la selección
    [selection-foreground] Texto seleccionado
    [normal-black] Negro normal
    [normal-red] Rojo normal
    [normal-green] Verde normal
    [normal-yellow] Amarillo normal
    [normal-blue] Azul normal
    [normal-magenta] Magenta normal
    [normal-cyan] Cian normal
    [normal-white] Blanco normal
    [bright-black] Negro brillante
    [bright-red] Rojo brillante
    [bright-green] Verde brillante
    [bright-yellow] Amarillo brillante
    [bright-blue] Azul brillante
    [bright-magenta] Magenta brillante
    [bright-cyan] Cian brillante
    [bright-white] Blanco brillante
    [tab-bar-background] Fondo de la barra de pestañas
    [tab-active-background] Fondo de la pestaña activa
    [tab-active-text] Texto de la pestaña activa
    [tab-inactive-text] Texto de las pestañas inactivas
    [pane-border] Borde de los paneles
    [pane-broadcast] Barra de título de los paneles difundidos
    [palette-background] Fondo de la paleta de comandos
    [palette-text] Texto de la paleta de comandos
    [palette-match] Coincidencias en la paleta de comandos
    [palette-selection-background] Selección en la paleta de comandos
    [banner-background] Fondo del banner
    [banner-text] Texto del banner
    [search-match-background] Coincidencias de la búsqueda
   *[search-current-background] Coincidencia actual de la búsqueda
}
theme-name-error = { $error ->
    [empty] Introduzca un nombre
    [invalid] Un nombre no puede contener / ni \ ni empezar por un punto
   *[taken] Ya existe un tema con este nombre
}

## Command palette

palette-placeholder = Escriba un comando, una pestaña o un panel
secret-answer = La respuesta a la pregunta fundamental
secret-author = marmotz en GitHub
secret-graphene = Graphene
tab-entry = Pestaña { $tab }: { $title }
pane-entry = Pestaña { $tab } > panel { $pane }: { $title }

## Close confirmation dialog

confirm-close-title = { $target ->
    [pane] ¿Cerrar este panel?
    [tab] ¿Cerrar esta pestaña?
   *[window] ¿Cerrar la ventana?
}
programs-running = { $count ->
    [one] { $programs } sigue en ejecución
   *[other] { $count } programas siguen en ejecución: { $programs }
}
close = Cerrar
cancel = Cancelar

## Update banner

update-available = Graphene { $tag } está disponible
open-release = Ver la versión
ignore-version = Ignorar esta versión

## Desktop notifications

command-finished = Comando terminado
command-failed = Comando fallido (código { $code })
bell = Timbre
show-pane = Mostrar el panel
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
    [code] Programa terminado (código { $value }). Pulse una tecla para cerrar.
    [signal] Programa terminado (señal { $value }). Pulse una tecla para cerrar.
   *[unknown] Programa terminado. Pulse una tecla para cerrar.
}

## Configuration errors

config-syntax = sintaxis no válida: { $detail }
config-cannot-read = no se puede leer el archivo: { $detail }
config-cannot-read-theme = no se puede leer el tema: { $detail }
config-cannot-write = no se puede escribir el archivo: { $detail }
config-no-config-dir = no se puede determinar la ruta del archivo de configuración
config-unknown-key = clave desconocida `{ $path }`
config-must-be-table = `{ $path }` debe ser una tabla
config-keybindings-moved = atajos de teclado trasladados a { $file }
config-invalid-value = valor no válido para `{ $path }`: { $reason ->
    [not-a-number] no es un número
    [not-negative] debe ser un número positivo o cero
    [at-least] debe ser al menos { $min }
    [greater-than-zero] debe ser mayor que 0
    [not-empty] no debe estar vacío
    [invalid-feature] función no válida "{ $item }", se espera "tag", "+tag", "-tag" o "tag=N" con un tag de 4 caracteres ASCII
    [empty-screen] no debe estar vacío, se espera "active", "primary" o un nombre de pantalla
    [unknown-modifier] modificador desconocido "{ $modifier }" en "{ $shortcut }"
    [missing-key] falta la tecla en "{ $shortcut }"
    [unknown-key-name] tecla desconocida "{ $key }" en "{ $shortcut }"
    [unknown-action] acción desconocida "{ $action }"
    [color-not-string] se espera un texto "#rrggbb"
    [not-a-color] `{ $text }` no es un color "#rrggbb"
   *[detail] { $detail }
}
config-out-of-range = `{ $path }`: { $value } está fuera del intervalo de { $min } a { $max }, se usa { $used }
config-above-maximum = `{ $path }`: { $value } supera el máximo, se usa { $max }
config-unknown-interval = `{ $path }`: valor no válido "{ $value }", valores posibles: { $expected }; se usa "auto"
config-unknown-theme = tema desconocido `{ $name }`, se usa `{ $fallback }`
config-theme-not-found = tema desconocido `{ $name }`
config-invalid-theme-name = nombre de tema no válido `{ $name }`
config-theme-exists = ya existe un tema llamado `{ $name }`
config-hotkey-no-key = el atajo global "{ $hotkey }" no tiene tecla en este sistema
config-hotkey-taken = el atajo global "{ $hotkey }" ya lo usa otra aplicación
config-hotkey-failed = no se puede registrar el atajo global: { $detail }
more-config-errors = { $first } (+{ $more } más)

## Settings lists

language-auto = Idioma del sistema
choice-cursor-shape = { $shape ->
   *[block] Bloque
    [beam] Barra
    [underline] Subrayado
}
choice-side = { $side ->
   *[top] Arriba
    [bottom] Abajo
    [left] Izquierda
    [right] Derecha
}
choice-startup-mode = { $mode ->
   *[default] Tamaño configurado
    [maximized] Maximizada
    [remember] Tamaño de la última ventana
}
choice-update-interval = { $interval ->
   *[auto] Automático
    [hours-6] Cada 6 horas
    [hours-12] Cada 12 horas
    [hours-24] Cada 24 horas
    [days-2] Cada 2 días
    [days-3] Cada 3 días
    [week-1] Cada semana
}
