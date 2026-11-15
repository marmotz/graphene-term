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
  # fish: no command end for an empty command line, nor before the first prompt. Then
  # `prompt`: readline is about to read a command line, the rich input editor of
  # Graphene may send its key sequences.
  _graphene_prompt() {
    local ret=$?
    if [[ -n ${_graphene_in_prompt+set} ]]; then
      _graphene_in_prompt=1
    fi
    if ((_graphene_command_ran)); then
      _graphene_command_ran=0
      builtin printf '\e]6973;%s;end;%s\e\\' "$_graphene_token" "$ret"
    fi
    builtin printf '\e]6973;%s;prompt\e\\' "$_graphene_token"
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
      encoded=$(builtin printf '%s' "$entry" | command base64 2>/dev/null | command tr -d '\r\n')
    fi
    builtin printf '\e]6973;%s;start;%s\e\\' "$_graphene_token" "$encoded"
  }
  _graphene_start_text() {
    local encoded=
    encoded=$(builtin printf '%s' "$1" | command base64 2>/dev/null | command tr -d '\r\n')
    builtin printf '\e]6973;%s;start;%s\e\\' "$_graphene_token" "$encoded"
  }

  _graphene_user_prompt_command=${PROMPT_COMMAND-}
  _graphene_prompt_command_decl=$(declare -p PROMPT_COMMAND 2>/dev/null || :)
  if [[ $_graphene_prompt_command_decl == "declare -a PROMPT_COMMAND="* ]]; then
    PROMPT_COMMAND=(_graphene_prompt "${PROMPT_COMMAND[@]}")
  else
    PROMPT_COMMAND="_graphene_prompt${PROMPT_COMMAND:+$'\n'$PROMPT_COMMAND}"
  fi
  unset _graphene_prompt_command_decl

  # PS0 (Bash 4.4+) is shown when a command line is read, empty ones excepted. Older
  # system Bash versions, including macOS's Bash 3.2, use DEBUG to report the first
  # command after each prompt instead.
  if [[ ${GRAPHENE_BASH_DEBUG_FALLBACK-} != 1 ]] \
    && ((BASH_VERSINFO[0] > 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 4))); then
    PS0="${PS0-}"'${_graphene_unset[_graphene_command_ran=1]-}$(_graphene_start)'
  else
    _graphene_in_prompt=0
    _graphene_debug() {
      ((BASH_SUBSHELL == 0)) || return 0
      [[ $_graphene_in_prompt == 1 || $_graphene_command_ran == 1 ]] && return 0
      [[ $BASH_COMMAND == _graphene_prompt* || $BASH_COMMAND == _graphene_prompt_ready* ]] && return 0
      [[ -n $_graphene_user_prompt_command
        && $_graphene_user_prompt_command == *"$BASH_COMMAND"* ]] && return 0
      _graphene_command_ran=1
      _graphene_start_text "$BASH_COMMAND"
    }
    _graphene_prompt_ready() {
      _graphene_in_prompt=0
    }
    PROMPT_COMMAND=$PROMPT_COMMAND$'\n'_graphene_prompt_ready
    _graphene_install_debug_trap=1
  fi

  # Rich input editor: Graphene sends private sequences (CSI <code> u, code points from
  # the Unicode private use area) for the editor moves, mirrored from
  # src/input_editor.rs. Every move is passive and clamped at the buffer ends.
  # READLINE_POINT counts characters like ${#READLINE_LINE} (see docs/decisions.md).
  _graphene_char_left() {
    ((READLINE_POINT > 0)) && READLINE_POINT=$((READLINE_POINT - 1))
    return 0
  }
  _graphene_char_right() {
    ((READLINE_POINT < ${#READLINE_LINE})) && READLINE_POINT=$((READLINE_POINT + 1))
    return 0
  }
  # Start of the next word, as zsh's forward-word (readline's stops at the word end):
  # skip the rest of the word, then the separators. Readline words are alphanumeric.
  _graphene_word_right() {
    local p=$READLINE_POINT n=${#READLINE_LINE}
    while ((p < n)) && [[ ${READLINE_LINE:p:1} == [[:alnum:]] ]]; do ((p++)); done
    while ((p < n)) && [[ ${READLINE_LINE:p:1} != [[:alnum:]] ]]; do ((p++)); done
    READLINE_POINT=$p
    return 0
  }
  # Column of the cursor in its line: characters since the previous newline.
  _graphene_column() {
    local before=${READLINE_LINE:0:READLINE_POINT}
    local line=${before##*$'\n'}
    _graphene_col=${#line}
  }
  _graphene_line_start() {
    _graphene_column
    READLINE_POINT=$((READLINE_POINT - _graphene_col))
  }
  _graphene_line_end() {
    local after=${READLINE_LINE:READLINE_POINT}
    local line=${after%%$'\n'*}
    READLINE_POINT=$((READLINE_POINT + ${#line}))
  }
  # Same column on the previous line, clamped to its length; nothing on the first line.
  _graphene_line_up() {
    _graphene_column
    local start=$((READLINE_POINT - _graphene_col))
    ((start > 0)) || return 0
    local before=${READLINE_LINE:0:start-1}
    local line=${before##*$'\n'}
    local col=$((_graphene_col < ${#line} ? _graphene_col : ${#line}))
    READLINE_POINT=$((start - 1 - ${#line} + col))
  }
  # Same column on the next line, clamped to its length; nothing on the last line.
  _graphene_line_down() {
    _graphene_column
    local after=${READLINE_LINE:READLINE_POINT}
    [[ $after == *$'\n'* ]] || return 0
    local rest=${after%%$'\n'*}
    local next=$((READLINE_POINT + ${#rest} + 1))
    after=${READLINE_LINE:next}
    local line=${after%%$'\n'*}
    READLINE_POINT=$((next + (_graphene_col < ${#line} ? _graphene_col : ${#line})))
  }
  _graphene_buffer_start() {
    READLINE_POINT=0
  }
  _graphene_buffer_end() {
    READLINE_POINT=${#READLINE_LINE}
  }
  _graphene_insert_newline() {
    READLINE_LINE=${READLINE_LINE:0:READLINE_POINT}$'\n'${READLINE_LINE:READLINE_POINT}
    READLINE_POINT=$((READLINE_POINT + 1))
  }

  for _graphene_keymap in emacs vi-insert; do
    {
      builtin bind -m "$_graphene_keymap" -x '"\e[59136u": _graphene_char_left'
      builtin bind -m "$_graphene_keymap" -x '"\e[59137u": _graphene_char_right'
      builtin bind -m "$_graphene_keymap" '"\e[59138u": backward-word'
      builtin bind -m "$_graphene_keymap" -x '"\e[59139u": _graphene_word_right'
      builtin bind -m "$_graphene_keymap" '"\e[59140u": backward-kill-word'
      builtin bind -m "$_graphene_keymap" '"\e[59141u": kill-word'
      builtin bind -m "$_graphene_keymap" -x '"\e[59142u": _graphene_line_start'
      builtin bind -m "$_graphene_keymap" -x '"\e[59143u": _graphene_line_end'
      builtin bind -m "$_graphene_keymap" -x '"\e[59144u": _graphene_line_up'
      builtin bind -m "$_graphene_keymap" -x '"\e[59145u": _graphene_line_down'
      builtin bind -m "$_graphene_keymap" -x '"\e[59146u": _graphene_buffer_start'
      builtin bind -m "$_graphene_keymap" -x '"\e[59147u": _graphene_buffer_end'
      builtin bind -m "$_graphene_keymap" -x '"\e[59148u": _graphene_insert_newline'
      # Readline's forward-word already stops at the end of the word.
      builtin bind -m "$_graphene_keymap" '"\e[59149u": forward-word'
    } 2>/dev/null
  done
  unset _graphene_keymap
  if [[ -n ${_graphene_install_debug_trap-} ]]; then
    unset GRAPHENE_BASH_DEBUG_FALLBACK _graphene_install_debug_trap
    trap '_graphene_debug' DEBUG
  fi
fi
