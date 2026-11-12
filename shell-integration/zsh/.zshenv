# Graphene shell integration for zsh.
#
# Graphene starts zsh with ZDOTDIR pointing to this folder, so this file runs first.
# It restores the user's ZDOTDIR, sources the user's .zshenv, then makes the
# interactive shell emit OSC 133 marks around the prompt and the command. The user's
# dotfiles are never modified.

if [[ -n ${GRAPHENE_ZDOTDIR+set} ]]; then
  ZDOTDIR=$GRAPHENE_ZDOTDIR
  unset GRAPHENE_ZDOTDIR
else
  unset ZDOTDIR
fi

if [[ -f ${ZDOTDIR:-$HOME}/.zshenv ]]; then
  builtin source -- "${ZDOTDIR:-$HOME}/.zshenv"
fi

if [[ -o interactive ]]; then
  # Set by preexec: a command ran since the last prompt.
  typeset -gi _graphene_command_ran=0
  typeset -g _graphene_ps1= _graphene_rps1=

  # Wraps the prompts in marks. Prompt themes may rebuild them on every precmd: wrap
  # them again when they changed since the last wrap. Returns 1 when nothing changed.
  _graphene_wrap_prompts() {
    local changed=1
    if [[ $PS1 != "$_graphene_ps1" ]]; then
      _graphene_ps1=$'%{\e]133;A\e\\%}'"$PS1"$'%{\e]133;B\e\\%}'
      PS1=$_graphene_ps1
      changed=0
    fi
    if [[ -n $RPS1 && $RPS1 != "$_graphene_rps1" ]]; then
      _graphene_rps1=$'%{\e]133;P;k=r\e\\%}'"$RPS1"$'%{\e]133;B\e\\%}'
      RPS1=$_graphene_rps1
      changed=0
    fi
    return $changed
  }

  _graphene_precmd() {
    local ret=$?
    # Like fish: no command end for an empty command line, nor before the first prompt.
    if (( _graphene_command_ran )); then
      _graphene_command_ran=0
      builtin print -rn -- $'\e]133;D;'"$ret"$'\e\\'
    fi
    _graphene_wrap_prompts
    # Run after the hooks added later by the user's configuration, so their prompt
    # changes are wrapped too. zsh calls a copy of the array: this takes effect from
    # the next prompt.
    precmd_functions=(${precmd_functions:#_graphene_precmd} _graphene_precmd)
    return $ret
  }

  _graphene_preexec() {
    _graphene_command_ran=1
    builtin print -rn -- $'\e]133;C\e\\'
  }

  # On the first prompt, a precmd hook added after this one may have rebuilt the prompt
  # after it was wrapped: wrap it and redraw it once the line editor starts. A widget
  # defined later by the user's configuration replaces this one.
  _graphene_line_init() {
    if _graphene_wrap_prompts; then
      zle reset-prompt
    fi
  }

  typeset -ga precmd_functions preexec_functions
  precmd_functions+=(_graphene_precmd)
  preexec_functions+=(_graphene_preexec)
  zle -N zle-line-init _graphene_line_init
fi
