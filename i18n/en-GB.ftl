## Status: unreviewed

# English (United Kingdom). Only the messages whose text differs from en-US.ftl: every
# other message falls back to en-US.

## Theme editor

theme-editor-intro = Change the colours of your own themes. Built-in themes cannot be changed: duplicate one to get a copy you can edit.
theme-colors-hint = Click a swatch to pick a colour, or type or paste its #rrggbb code. Each change is saved to the theme file right away.
invalid-hex-color = Enter a colour as #rrggbb
keep-text-color = Keep the text colour
theme-color-group = { $group ->
    [terminal] Terminal colours
    [normal] Normal ANSI colours
    [bright] Bright ANSI colours
   *[interface] Interface colours
}

## Configuration errors

config-invalid-value = invalid value for `{ $path }`: { $reason ->
    [not-a-number] not a number
    [not-negative] must be a non-negative number
    [at-least] must be at least { $min }
    [greater-than-zero] must be greater than 0
    [not-empty] must not be empty
    [invalid-feature] invalid feature "{ $item }", expected "tag", "+tag", "-tag" or "tag=N" with a tag of 4 ASCII characters
    [empty-screen] must not be empty, expected "active", "primary" or a screen name
    [unknown-modifier] unknown modifier "{ $modifier }" in "{ $shortcut }"
    [missing-key] missing key in "{ $shortcut }"
    [unknown-key-name] unknown key "{ $key }" in "{ $shortcut }"
    [unknown-action] unknown action "{ $action }"
    [color-not-string] expected a "#rrggbb" string
    [not-a-color] `{ $text }` is not a "#rrggbb" colour
   *[detail] { $detail }
}

## Settings lists

choice-startup-mode = { $mode ->
   *[default] Configured size
    [maximized] Maximised
    [remember] Size of the last window
}
