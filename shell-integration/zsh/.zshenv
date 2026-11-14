# Graphene shell integration for zsh.
#
# Graphene starts zsh with ZDOTDIR pointing to this folder, so this file runs first.
# It restores the user's ZDOTDIR, sources the user's .zshenv, then makes the
# interactive shell tell Graphene when a command starts and ends. The prompt is never
# read, wrapped or rebuilt, and the user's dotfiles are never modified.

if [[ -n ${GRAPHENE_ZDOTDIR+set} ]]; then
  ZDOTDIR=$GRAPHENE_ZDOTDIR
  unset GRAPHENE_ZDOTDIR
else
  unset ZDOTDIR
fi

# The session token stays in a shell variable: commands run from the shell never see it.
typeset -g _graphene_token=${GRAPHENE_BLOCK_TOKEN-}
unset GRAPHENE_BLOCK_TOKEN

if [[ -f ${ZDOTDIR:-$HOME}/.zshenv ]]; then
  builtin source -- "${ZDOTDIR:-$HOME}/.zshenv"
fi

if [[ -o interactive && -n $_graphene_token ]]; then
  # Set by preexec: a command ran since the last prompt.
  typeset -gi _graphene_command_ran=0

  # Like fish: no command end for an empty command line, nor before the first prompt.
  # Then `prompt`: the line editor is about to read a command line, the rich input
  # editor of Graphene may send its key sequences.
  _graphene_precmd() {
    local ret=$?
    if (( _graphene_command_ran )); then
      _graphene_command_ran=0
      builtin print -rn -- $'\e]6973;'"$_graphene_token;end;$ret"$'\e\\'
    fi
    builtin print -rn -- $'\e]6973;'"$_graphene_token;prompt"$'\e\\'
    return $ret
  }

  # $1 is the command line as typed, empty when the history is off.
  _graphene_preexec() {
    _graphene_command_ran=1
    local encoded
    encoded=$(builtin print -rn -- "$1" | command base64 -w0 2>/dev/null)
    builtin print -rn -- $'\e]6973;'"$_graphene_token;start;$encoded"$'\e\\'
  }

  # Rich input editor: Graphene sends private sequences (CSI <code> u, code points from
  # the Unicode private use area) for the editor moves, mirrored from
  # src/input_editor.rs. Every move is passive: clamped at the buffer ends and never
  # accepting an autosuggestion. The widgets call the builtin (dot) widgets, which
  # plugins such as zsh-autosuggestions do not wrap.
  _graphene_char_left() { (( CURSOR > 0 )) && CURSOR=$(( CURSOR - 1 )); return 0 }
  _graphene_char_right() { (( CURSOR < $#BUFFER )) && CURSOR=$(( CURSOR + 1 )); return 0 }
  _graphene_word_left() { zle .backward-word; return 0 }
  _graphene_word_right() { zle .forward-word; return 0 }
  _graphene_delete_word_left() { zle .backward-kill-word; return 0 }
  _graphene_delete_word_right() { zle .kill-word; return 0 }
  _graphene_line_start() { zle .beginning-of-line; return 0 }
  _graphene_line_end() { zle .end-of-line; return 0 }
  # up-line / down-line stay in the buffer: no history at the first or last line.
  _graphene_line_up() { zle .up-line; return 0 }
  _graphene_line_down() { zle .down-line; return 0 }
  _graphene_buffer_start() { CURSOR=0 }
  _graphene_buffer_end() { CURSOR=$#BUFFER }
  _graphene_insert_newline() { LBUFFER+=$'\n' }

  typeset -ga _graphene_bindings=(
    $'\e[59136u' _graphene_char_left
    $'\e[59137u' _graphene_char_right
    $'\e[59138u' _graphene_word_left
    $'\e[59139u' _graphene_word_right
    $'\e[59140u' _graphene_delete_word_left
    $'\e[59141u' _graphene_delete_word_right
    $'\e[59142u' _graphene_line_start
    $'\e[59143u' _graphene_line_end
    $'\e[59144u' _graphene_line_up
    $'\e[59145u' _graphene_line_down
    $'\e[59146u' _graphene_buffer_start
    $'\e[59147u' _graphene_buffer_end
    $'\e[59148u' _graphene_insert_newline
  )

  # Run before each prompt, after the user's files: a keymap reset by the user's
  # configuration or a plugin gets the bindings back.
  _graphene_bind() {
    local seq widget keymap
    for seq widget in "${_graphene_bindings[@]}"; do
      builtin zle -N $widget
      for keymap in emacs viins; do
        builtin bindkey -M $keymap $seq $widget
      done
    done
  }

  typeset -ga precmd_functions preexec_functions
  precmd_functions+=(_graphene_precmd _graphene_bind)
  preexec_functions+=(_graphene_preexec)
fi
