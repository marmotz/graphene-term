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
  _graphene_precmd() {
    local ret=$?
    if (( _graphene_command_ran )); then
      _graphene_command_ran=0
      builtin print -rn -- $'\e]6973;'"$_graphene_token;end;$ret"$'\e\\'
    fi
    return $ret
  }

  # $1 is the command line as typed, empty when the history is off.
  _graphene_preexec() {
    _graphene_command_ran=1
    local encoded
    encoded=$(builtin print -rn -- "$1" | command base64 -w0 2>/dev/null)
    builtin print -rn -- $'\e]6973;'"$_graphene_token;start;$encoded"$'\e\\'
  }

  typeset -ga precmd_functions preexec_functions
  precmd_functions+=(_graphene_precmd)
  preexec_functions+=(_graphene_preexec)
fi
