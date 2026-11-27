## Status: reviewed

# English (United States): the reference. Every message of the application is defined
# here, and every other language defines a subset of these identifiers.

## Terminal actions

copy = Copy
paste = Paste
scroll-page-up = Scroll up one page
scroll-page-down = Scroll down one page
scroll-to-top = Scroll to top
scroll-to-bottom = Scroll to bottom
font-size-increase = Increase font size
font-size-decrease = Decrease font size
font-size-reset = Reset font size
font-size-reset-all = Reset font size in all panes

## Command blocks actions

previous-block = Previous command block
next-block = Next command block
copy-command = Copy block command
copy-output = Copy block output
toggle-fold = Fold / unfold block
toggle-fold-all = Fold / unfold all blocks
fold-block = Fold block
unfold-block = Unfold block
# Summary of a folded block: its hidden output rows.
lines-hidden = { $rows ->
    [one] { $rows } line hidden
   *[other] { $rows } lines hidden
}

## Search bar

open-search = Search the history
search-placeholder = Search
invalid-regex = Invalid regex
search-zone-both = All
search-zone-command = Commands
search-zone-output = Output
search-status-all = Any
search-status-success = Success
search-status-failure = Failure

## Tabs and panes actions

new-tab = New tab
close-tab = Close the tab
close-pane = Close pane
next-tab = Next tab
previous-tab = Previous tab
# $tab: 1-based tab number.
go-to-tab = Go to tab { $tab }
split-right = Split right
split-down = Split down
focus-pane = { $direction ->
    [left] Focus pane on the left
    [right] Focus pane on the right
    [up] Focus pane above
   *[down] Focus pane below
}
resize-pane = { $direction ->
    [left] Resize pane to the left
    [right] Resize pane to the right
    [up] Resize pane upwards
   *[down] Resize pane downwards
}
next-pane = Next pane
previous-pane = Previous pane
toggle-zoom = Zoom / unzoom pane
toggle-broadcast-all = Broadcast input to all panes / stop
zoom-pane = Zoom the pane
unzoom-pane = Unzoom the pane
split-vertical = Split vertically
split-horizontal = Split horizontally
equalise-panes = Equalise panes
detach-pane = Move pane to a new tab
rename-tab = Rename the tab
themes = Themes
more-themes = More...
command-palette = Command palette
open-settings = Settings
quit = Quit Graphene

## Settings tab

settings-section = { $section ->
    [language] Language
    [font] Font
    [window] Window
    [scrollback] Scrollback
    [cursor] Cursor
    [selection] Selection
    [tabs] Tabs
    [theme] Theme
    [keyboard-shortcuts] Keyboard shortcuts
    [notifications] Notifications
    [quick-terminal] Quick terminal
   *[updates] Updates
}
settings-field = { $field ->
    [language] Language
    [font-family] Family
    [font-size] Size
    [font-ligatures] Ligatures
    [font-features] OpenType features
    [startup-mode] Startup mode
    [window-width] Width
    [window-height] Height
    [padding] Padding
    [background-opacity] Background opacity
    [window-progress] Show program progress
    [restore-session] Restore the session at launch
    [scrollback-lines] Lines kept
    [cursor-shape] Shape
    [cursor-blinking] Blinking
    [word-separators] Word separators
    [tab-position] Position
    [theme-follow] Follow the system appearance
    [theme-name] Theme
    [theme-light] Light theme
    [theme-dark] Dark theme
    [updates-check] Check for updates
    [updates-interval] Check interval
    [notifications-program] Notifications from programs
    [notifications-command-finished] Notify when a long command finishes
    [notifications-command-threshold] Long command threshold (seconds)
    [notifications-bell] Notify on terminal bell
    [quick-terminal-enabled] Enable the quick terminal
    [quick-terminal-hotkey] Global shortcut
    [quick-terminal-edge] Screen edge
    [quick-terminal-size] Size (% of the screen)
    [quick-terminal-screen] Screen
    [quick-terminal-animation] Slide duration (ms)
    [quick-terminal-hide-on-focus-loss] Hide when the focus is lost
   *[quick-terminal-autostart] Start with the session
}
system-monospace = System monospace
active-screen = Screen under the mouse cursor
primary-screen = Primary screen
unknown-keys = Unknown keys
# $line: 1-based line of the file.
at-line = line { $line }
fix-file-by-hand = The file cannot be read: fix it by hand. The settings stay disabled until then.
quick-terminal-unavailable-on-wayland = The quick terminal is not available on Wayland
quick-terminal-disabled = Turn it on to change the settings below. Takes effect the next time Graphene starts.
cannot-save = Cannot save the change
not-a-number = Enter a number
not-a-whole-number = Enter a whole number
at-least = Must be at least { $min }
at-most = Must be at most { $max }

## Keyboard shortcuts

add-shortcut = Add
add-shortcut-hint = Add shortcut
change-shortcut = Change
remove-shortcut = Remove this shortcut
no-shortcut = No shortcut
press-shortcut = Press a shortcut · Esc to cancel
# $action: label of the other action.
already-used-by = Already used by { $action }
replace = Replace
filter-actions = Filter actions
no-action-matches = No action matches
search-shortcut = Find a shortcut
no-action-for-shortcut = No action uses this shortcut

## Theme editor

theme-editor = Theme editor
theme-editor-intro = Change the colors of your own themes. Built-in themes cannot be changed: duplicate one to get a copy you can edit.
theme-colors-hint = Click a swatch to pick a color, or type or paste its #rrggbb code. Each change is saved to the theme file right away.
invalid-hex-color = Enter a color as #rrggbb
open-themes-folder = Open the themes folder
edited-theme = Theme to edit
duplicate = Duplicate
rename = Rename
delete = Delete
new-theme-name = New name
confirm = Confirm
confirm-delete-theme = Delete the theme { $name }? Its file is removed.
built-in-theme = This built-in theme cannot be changed. Click Duplicate to create a copy you can edit.
keep-text-color = Keep the text color
theme-color-group = { $group ->
    [terminal] Terminal colors
    [normal] Normal ANSI colors
    [bright] Bright ANSI colors
   *[interface] Interface colors
}
theme-color = { $key ->
    [foreground] Text
    [background] Background
    [cursor] Cursor
    [cursor-text] Text under the cursor
    [selection-background] Selection background
    [selection-foreground] Selected text
    [normal-black] Normal black
    [normal-red] Normal red
    [normal-green] Normal green
    [normal-yellow] Normal yellow
    [normal-blue] Normal blue
    [normal-magenta] Normal magenta
    [normal-cyan] Normal cyan
    [normal-white] Normal white
    [bright-black] Bright black
    [bright-red] Bright red
    [bright-green] Bright green
    [bright-yellow] Bright yellow
    [bright-blue] Bright blue
    [bright-magenta] Bright magenta
    [bright-cyan] Bright cyan
    [bright-white] Bright white
    [tab-bar-background] Tab bar background
    [tab-active-background] Active tab background
    [tab-active-text] Active tab text
    [tab-inactive-text] Inactive tab text
    [pane-border] Pane border
    [pane-broadcast] Broadcast pane title bar
    [palette-background] Command palette background
    [palette-text] Command palette text
    [palette-match] Command palette matches
    [palette-selection-background] Command palette selection
    [banner-background] Banner background
    [banner-text] Banner text
    [search-match-background] Search matches
   *[search-current-background] Current search match
}
theme-name-error = { $error ->
    [empty] Enter a name
    [invalid] A name cannot contain / or \ nor start with a dot
   *[taken] A theme with this name already exists
}

## Command palette

palette-placeholder = Type a command, a tab or a pane
secret-answer = The Answer to the Ultimate Question
secret-author = marmotz on GitHub
secret-graphene = Graphene
# $tab: 1-based tab number, $title: the tab title.
tab-entry = Tab { $tab }: { $title }
# $tab, $pane: 1-based numbers, $title: the pane title.
pane-entry = Tab { $tab } > pane { $pane }: { $title }

## Close confirmation dialog

confirm-close-title = { $target ->
    [pane] Close this pane?
    [tab] Close this tab?
   *[window] Close the window?
}
# Foreground programs still running, at least one. $programs: their names, joined.
programs-running = { $count ->
    [one] { $programs } is still running
   *[other] { $count } programs are still running: { $programs }
}
close = Close
cancel = Cancel

## Update banner

# $tag: the tag of the newer release.
update-available = Graphene { $tag } is available
open-release = Open release
ignore-version = Ignore this version

## Desktop notifications

command-finished = Command finished
command-failed = Command failed (code { $code })
bell = Bell
show-pane = Show pane
# Duration of a command, in its two largest units; a zero last unit is left out.
# $s: seconds.
duration-seconds = { $s } s
# $min: minutes, $s: seconds below a minute.
duration-minutes = { $s ->
    [0] { $min } min
   *[other] { $min } min { $s } s
}
# $h: hours, $min: minutes below an hour.
duration-hours = { $min ->
    [0] { $h } h
   *[other] { $h } h { $min } min
}

## Program run by -E

# $exit: how it ended, $value: its exit code or signal.
program-exited = { $exit ->
    [code] Program ended (code { $value }). Press any key to close.
    [signal] Program ended (signal { $value }). Press any key to close.
   *[unknown] Program ended. Press any key to close.
}
