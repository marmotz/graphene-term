# Graphene shell integration for bash.
#
# Graphene starts bash with --posix and ENV pointing to this file: a POSIX mode
# interactive shell reads $ENV and nothing else. This file turns POSIX mode off,
# sources the usual startup files like a regular interactive bash, then makes the shell
# emit OSC 133 marks around the prompt and the command. The user's dotfiles are never
# modified.

set +o posix
if [[ -n ${GRAPHENE_BASH_ENV+set} ]]; then
  export ENV=$GRAPHENE_BASH_ENV
  unset GRAPHENE_BASH_ENV
else
  unset ENV
fi

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

_graphene_command_ran=0
_graphene_ps1=

# First PROMPT_COMMAND entry: reports the status of the command that just ended. Like
# fish: no command end for an empty command line, nor before the first prompt.
_graphene_prompt_start() {
  local ret=$?
  if ((_graphene_command_ran)); then
    _graphene_command_ran=0
    builtin printf '\e]133;D;%s\e\\' "$ret"
  fi
  return "$ret"
}

# Last PROMPT_COMMAND entry: wraps PS1 again when a prompt command rebuilt it.
_graphene_prompt_end() {
  local ret=$?
  if [[ $PS1 != "$_graphene_ps1" ]]; then
    _graphene_ps1='\[\e]133;A\e\\\]'"$PS1"'\[\e]133;B\e\\\]'
    PS1=$_graphene_ps1
  fi
  return "$ret"
}

if [[ ${PROMPT_COMMAND@a} == *a* ]]; then
  PROMPT_COMMAND=(_graphene_prompt_start "${PROMPT_COMMAND[@]}" _graphene_prompt_end)
else
  PROMPT_COMMAND="_graphene_prompt_start${PROMPT_COMMAND:+$'\n'$PROMPT_COMMAND}"$'\n_graphene_prompt_end'
fi
# PS0 is shown when a command line is read, empty ones excepted. The subscript of the
# unset array sets the flag as a side effect of the expansion and expands to nothing.
PS0="${PS0-}"'${_graphene_unset[_graphene_command_ran=1]-}\e]133;C\e\\'
