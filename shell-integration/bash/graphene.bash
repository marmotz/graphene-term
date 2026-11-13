# Graphene shell integration for bash.
#
# Graphene starts bash with --posix and ENV pointing to this file: a POSIX mode
# interactive shell reads $ENV and nothing else. This file turns POSIX mode off,
# sources the usual startup files like a regular interactive bash, then makes the shell
# tell Graphene when a command starts and ends. The prompt is never read, wrapped or
# rebuilt, and the user's dotfiles are never modified.

set +o posix
if [[ -n ${GRAPHENE_BASH_ENV+set} ]]; then
  export ENV=$GRAPHENE_BASH_ENV
  unset GRAPHENE_BASH_ENV
else
  unset ENV
fi

# The session token stays in a shell variable: commands run from the shell never see it.
_graphene_token=${GRAPHENE_BLOCK_TOKEN-}
unset GRAPHENE_BLOCK_TOKEN

if shopt -q login_shell; then
  if [[ -r /etc/profile ]]; then
    builtin source /etc/profile
  fi
  for _graphene_file in ~/.bash_profile ~/.bash_login ~/.profile; do
    if [[ -r $_graphene_file ]]; then
      builtin source "$_graphene_file"
      break
    fi
  done
  unset _graphene_file
else
  if [[ -r /etc/bash.bashrc ]]; then
    builtin source /etc/bash.bashrc
  fi
  if [[ -r ~/.bashrc ]]; then
    builtin source ~/.bashrc
  fi
fi

if [[ -n $_graphene_token ]]; then
  _graphene_command_ran=0
  # HISTCMD at the prompt: it only moves when the command line is added to the history.
  _graphene_histcmd=$HISTCMD

  # First PROMPT_COMMAND entry: reports the status of the command that just ended. Like
  # fish: no command end for an empty command line, nor before the first prompt.
  _graphene_prompt() {
    local ret=$?
    if ((_graphene_command_ran)); then
      _graphene_command_ran=0
      builtin printf '\e]6973;%s;end;%s\e\\' "$_graphene_token" "$ret"
    fi
    _graphene_histcmd=$HISTCMD
    return "$ret"
  }

  # Run by PS0 in a subshell: sends the command line, read back from the history. It is
  # empty when the line was not added (history off, ignorespace, ignoredups), since the
  # last entry is then an older command.
  _graphene_start() {
    local entry= encoded=
    if [[ $HISTCMD != "$_graphene_histcmd" ]]; then
      entry=$(HISTTIMEFORMAT= builtin history 1)
      # "  42  command", or "  42* command" for an edited entry.
      if [[ $entry =~ ^\ *[0-9]+\*?\ \ ?(.*)$ ]]; then
        entry=${BASH_REMATCH[1]}
      else
        entry=
      fi
      encoded=$(builtin printf '%s' "$entry" | command base64 -w0 2>/dev/null)
    fi
    builtin printf '\e]6973;%s;start;%s\e\\' "$_graphene_token" "$encoded"
  }

  if [[ ${PROMPT_COMMAND@a} == *a* ]]; then
    PROMPT_COMMAND=(_graphene_prompt "${PROMPT_COMMAND[@]}")
  else
    PROMPT_COMMAND="_graphene_prompt${PROMPT_COMMAND:+$'\n'$PROMPT_COMMAND}"
  fi
  # PS0 is shown when a command line is read, empty ones excepted. The subscript of the
  # unset array sets the flag as a side effect of the expansion and expands to nothing.
  PS0="${PS0-}"'${_graphene_unset[_graphene_command_ran=1]-}$(_graphene_start)'
fi
